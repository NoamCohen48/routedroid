//! Host-side session state machine (pure) and its async driver.
//!
//! ```text
//! Connected --HELLO/HELLO_ACK(host_proof)--> Authenticating
//! Authenticating --AUTH(android_proof ok)--> Negotiated --CONFIGURE_VPN--> Configuring
//! Configuring --VPN_READY--> Active (IP_PACKET allowed both ways)
//! Configuring --VPN_ERROR--> Closed
//! any --STOP--> Closed
//! ```
//!
//! The host sends CONFIGURE_VPN as soon as AUTH verifies, so `Negotiated` is
//! only ever observed between verifying AUTH and emitting CONFIGURE_VPN. The
//! session secret is zeroized the moment AUTH is decided either way.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, trace, warn};

use crate::auth::{self, Secret, NONCE_LEN};
use crate::frame::{self, Frame, FrameError, MessageType};
use crate::ipv4;
use crate::messages::{Auth, ConfigureVpn, ErrorBody, Hello, HelloAck, Prefix, VpnReady};
use crate::stats::Stats;

/// Bounded queue depth for every packet/frame channel.
pub const QUEUE_DEPTH: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    pub mtu: u32,
    pub addresses: Vec<Prefix>,
    pub routes: Vec<Prefix>,
    pub dns: Vec<String>,
    pub session_name: String,
    /// When set, the HELLO `session` string must match exactly.
    pub expected_session: Option<String>,
    /// Single-use session secret delivered to the phone out of band (§3.4).
    pub secret: Secret,
}

impl SessionConfig {
    pub fn configure_vpn(&self) -> ConfigureVpn {
        ConfigureVpn {
            mtu: self.mtu,
            addresses: self.addresses.clone(),
            routes: self.routes.clone(),
            dns: self.dns.clone(),
            session_name: self.session_name.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Connected,
    Authenticating,
    Negotiated,
    Configuring,
    Active,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outbound {
    ToPeer(Frame),
    ToTun(Vec<u8>),
}

/// Why the machine wants the connection closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Close {
    /// Peer sent STOP.
    PeerStop,
    /// Peer sent ERROR.
    PeerError(ErrorBody),
    /// Peer sent VPN_ERROR while Configuring.
    VpnError(ErrorBody),
    /// We detected a violation; send this ERROR then close.
    Protocol(ErrorBody),
}

impl fmt::Display for Close {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PeerStop => write!(f, "peer sent STOP"),
            Self::PeerError(e) => write!(f, "peer sent ERROR {}: {}", e.code, e.message),
            Self::VpnError(e) => write!(f, "peer sent VPN_ERROR {}: {}", e.code, e.message),
            Self::Protocol(e) => write!(f, "protocol violation {}: {}", e.code, e.message),
        }
    }
}

#[derive(Debug)]
pub struct Machine {
    state: State,
    cfg: SessionConfig,
    hello: Option<Hello>,
    host_nonce: [u8; NONCE_LEN],
    /// Transcript fixed by HELLO/HELLO_ACK; verified against AUTH.
    transcript: Option<Vec<u8>>,
    /// `Some` until AUTH is decided, then zeroized (single use).
    secret: Option<Secret>,
}

impl Machine {
    pub fn new(mut cfg: SessionConfig, host_nonce: [u8; NONCE_LEN]) -> Self {
        // Move the secret out of the config so exactly one copy lives here.
        let secret = Some(std::mem::replace(&mut cfg.secret, Secret::new([0; auth::SECRET_LEN])));
        Self { state: State::Connected, cfg, hello: None, host_nonce, transcript: None, secret }
    }

    /// Whether the secret has been consumed (AUTH decided, either way).
    #[cfg(test)]
    pub fn secret_consumed(&self) -> bool {
        self.secret.is_none()
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn hello(&self) -> Option<&Hello> {
        self.hello.as_ref()
    }

    pub fn mtu(&self) -> u32 {
        self.cfg.mtu
    }

    fn violation(&mut self, code: &str, message: impl Into<String>) -> Close {
        self.state = State::Closed;
        self.secret = None;
        Close::Protocol(ErrorBody::new(code, message))
    }

    fn parse_json<T: serde::de::DeserializeOwned>(&mut self, what: &str, body: &[u8]) -> Result<T, Close> {
        serde_json::from_slice(body).map_err(|e| self.violation("bad_json", format!("{what}: {e}")))
    }

    /// Feed one validated inbound frame; get the frames/packets to emit.
    pub fn handle(&mut self, frame: Frame) -> Result<Vec<Outbound>, Close> {
        if self.state == State::Closed {
            return Err(self.violation("closed", "frame after close"));
        }
        match frame.message_type {
            MessageType::Stop => {
                self.state = State::Closed;
                Err(Close::PeerStop)
            }
            MessageType::Error => {
                let body: ErrorBody = self.parse_json("ERROR", &frame.body)?;
                self.state = State::Closed;
                Err(Close::PeerError(body))
            }
            MessageType::Ping => Ok(vec![Outbound::ToPeer(Frame::empty(MessageType::Pong))]),
            MessageType::Pong => Ok(Vec::new()),
            MessageType::Hello => {
                if self.state != State::Connected {
                    return Err(self.violation("out_of_state", format!("HELLO in {:?}", self.state)));
                }
                let hello: Hello = self.parse_json("HELLO", &frame.body)?;
                if hello.protocol != frame::PROTOCOL_VERSION {
                    return Err(self.violation(
                        "unsupported_protocol",
                        format!("HELLO protocol {} != {}", hello.protocol, frame::PROTOCOL_VERSION),
                    ));
                }
                if let Some(expected) = &self.cfg.expected_session {
                    if &hello.session != expected {
                        return Err(self.violation("bad_session", "HELLO session does not match launch extra"));
                    }
                }
                let Some(client_nonce) = auth::nonce_from_hex(&hello.client_nonce) else {
                    return Err(self.violation("bad_nonce", "HELLO client_nonce must be 32 bytes hex"));
                };
                let transcript = auth::transcript(
                    hello.protocol,
                    &hello.session,
                    hello.device_port,
                    &client_nonce,
                    &self.host_nonce,
                );
                let Some(secret) = self.secret.as_ref() else {
                    return Err(self.violation("secret_consumed", "session secret already used"));
                };
                let host_proof = hex::encode(auth::proof(secret, "host", &transcript));
                self.transcript = Some(transcript);
                self.hello = Some(hello);
                self.state = State::Authenticating;
                let ack = Frame::json(
                    MessageType::HelloAck,
                    &HelloAck { protocol: 0, mtu: self.cfg.mtu, host_nonce: hex::encode(self.host_nonce), host_proof },
                );
                Ok(vec![Outbound::ToPeer(ack)])
            }
            MessageType::Auth => {
                if self.state != State::Authenticating {
                    return Err(self.violation("out_of_state", format!("AUTH in {:?}", self.state)));
                }
                let auth_msg: Auth = self.parse_json("AUTH", &frame.body)?;
                // Single use: the secret is dropped (zeroized) whatever the outcome.
                let secret = self.secret.take().expect("secret present while Authenticating");
                let transcript = self.transcript.take().expect("transcript fixed by HELLO");
                if !auth::verify(&secret, "android", &transcript, &auth_msg.android_proof) {
                    return Err(self.violation("auth_failed", "android_proof does not verify"));
                }
                drop(secret);
                self.state = State::Negotiated;
                let cfg = Frame::json(MessageType::ConfigureVpn, &self.cfg.configure_vpn());
                self.state = State::Configuring;
                Ok(vec![Outbound::ToPeer(cfg)])
            }
            MessageType::VpnReady => {
                if self.state != State::Configuring {
                    return Err(self.violation("out_of_state", format!("VPN_READY in {:?}", self.state)));
                }
                let ready: VpnReady = self.parse_json("VPN_READY", &frame.body)?;
                if ready.mtu != self.cfg.mtu {
                    return Err(self.violation(
                        "mtu_mismatch",
                        format!("VPN_READY mtu {} != configured {}", ready.mtu, self.cfg.mtu),
                    ));
                }
                for want in &self.cfg.addresses {
                    let want_s = format!("{}/{}", want.address, want.prefix);
                    if !ready.addresses.contains(&want_s) {
                        return Err(self.violation(
                            "address_mismatch",
                            format!("VPN_READY addresses {:?} do not include {want_s}", ready.addresses),
                        ));
                    }
                }
                self.state = State::Active;
                Ok(Vec::new())
            }
            MessageType::VpnError => {
                if self.state != State::Configuring {
                    return Err(self.violation("out_of_state", format!("VPN_ERROR in {:?}", self.state)));
                }
                let body: ErrorBody = self.parse_json("VPN_ERROR", &frame.body)?;
                self.state = State::Closed;
                Err(Close::VpnError(body))
            }
            MessageType::IpPacket => {
                if self.state != State::Active {
                    return Err(self.violation("out_of_state", format!("IP_PACKET in {:?}", self.state)));
                }
                if let Err(e) = ipv4::validate(&frame.body) {
                    return Err(self.violation("bad_ipv4", e.to_string()));
                }
                Ok(vec![Outbound::ToTun(frame.body)])
            }
            MessageType::HelloAck | MessageType::ConfigureVpn => {
                Err(self.violation("wrong_direction", format!("{:?} is host->android only", frame.message_type)))
            }
        }
    }
}

/// How a driven session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnd {
    /// We were asked to stop (Ctrl-C); STOP was sent to the peer.
    LocalStop,
    /// Peer closed the TCP stream at a frame boundary.
    PeerClosed,
    PeerStop,
    PeerError(ErrorBody),
    VpnError(ErrorBody),
    /// Peer violated the protocol (framing or state); ERROR was sent when possible.
    ProtocolViolation(String),
    /// TCP transport failure.
    Transport(String),
    /// TUN side of the pipeline went away (writer/reader task ended).
    TunClosed,
}

impl fmt::Display for SessionEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LocalStop => write!(f, "local stop"),
            Self::PeerClosed => write!(f, "peer closed connection"),
            Self::PeerStop => write!(f, "peer sent STOP"),
            Self::PeerError(e) => write!(f, "peer ERROR {}: {}", e.code, e.message),
            Self::VpnError(e) => write!(f, "peer VPN_ERROR {}: {}", e.code, e.message),
            Self::ProtocolViolation(s) => write!(f, "protocol violation: {s}"),
            Self::Transport(s) => write!(f, "transport error: {s}"),
            Self::TunClosed => write!(f, "TUN pipeline closed"),
        }
    }
}

#[derive(Debug)]
pub struct SessionSummary {
    pub end: SessionEnd,
    pub reached_active: bool,
}

/// The packet-side endpoints of a session. `to_tun` receives validated
/// packets to inject into the TUN; `from_tun` yields packets read from it.
pub struct TunEndpoints {
    pub to_tun: mpsc::Sender<Vec<u8>>,
    pub from_tun: mpsc::Receiver<Vec<u8>>,
}

/// TCP writer: drains frames from a bounded queue with `write_all`.
/// A partial stream write continues until the whole frame is sent.
async fn writer_task<W: AsyncWrite + Unpin>(
    mut wr: W,
    mut rx: mpsc::Receiver<Frame>,
    stats: Arc<Stats>,
) -> Result<(), std::io::Error> {
    let mut buf = Vec::with_capacity(frame::HEADER_LEN + 65536);
    while let Some(f) = rx.recv().await {
        buf.clear();
        f.encode_into(&mut buf);
        wr.write_all(&buf).await?;
        if f.message_type == MessageType::IpPacket {
            stats.add_tun_to_peer(f.body.len());
        } else {
            stats.control_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
    wr.shutdown().await.ok();
    Ok(())
}

/// Drive one accepted connection through the state machine until it ends.
pub async fn run_session(
    stream: TcpStream,
    mut machine: Machine,
    tun: TunEndpoints,
    stats: Arc<Stats>,
    mut shutdown: watch::Receiver<bool>,
) -> SessionSummary {
    let mtu = machine.mtu();
    let TunEndpoints { to_tun, mut from_tun } = tun;
    let (mut rd, wr) = stream.into_split();
    let (out_tx, out_rx) = mpsc::channel::<Frame>(QUEUE_DEPTH);
    let (active_tx, active_rx) = watch::channel(false);

    let writer = tokio::spawn(writer_task(wr, out_rx, stats.clone()));

    // TUN -> peer pump. Packets that arrive before Active are dropped (counted).
    let pump_out = out_tx.clone();
    let pump_stats = stats.clone();
    let pump = tokio::spawn(async move {
        while let Some(pkt) = from_tun.recv().await {
            if !*active_rx.borrow() {
                pump_stats.drop_tun_not_active.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                continue;
            }
            if pump_out.send(Frame::ip_packet(pkt)).await.is_err() {
                break;
            }
        }
    });
    tokio::pin!(pump);

    let mut reached_active = false;
    let end = loop {
        let frame = tokio::select! {
            biased;
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    let _ = out_tx.send(Frame::empty(MessageType::Stop)).await;
                    break SessionEnd::LocalStop;
                }
                continue;
            }
            _ = &mut pump => break SessionEnd::TunClosed,
            r = frame::read_frame(&mut rd, mtu) => match r {
                Ok(f) => f,
                Err(FrameError::Truncated { clean: true }) => break SessionEnd::PeerClosed,
                Err(FrameError::Io(e)) => break SessionEnd::Transport(e),
                Err(e @ FrameError::Truncated { clean: false }) => break SessionEnd::Transport(e.to_string()),
                Err(e) => {
                    let body = ErrorBody::new("bad_frame", e.to_string());
                    let _ = out_tx.send(Frame::json(MessageType::Error, &body)).await;
                    break SessionEnd::ProtocolViolation(e.to_string());
                }
            },
        };
        let is_packet = frame.message_type == MessageType::IpPacket;
        if !is_packet {
            stats.control_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            debug!(?frame.message_type, state = ?machine.state(), "control frame");
        }
        match machine.handle(frame) {
            Ok(outbound) => {
                for o in outbound {
                    match o {
                        Outbound::ToPeer(f) => {
                            if out_tx.send(f).await.is_err() {
                                break;
                            }
                        }
                        Outbound::ToTun(pkt) => {
                            let len = pkt.len();
                            trace!(src = %ipv4::source(&pkt), dst = %ipv4::destination(&pkt), len, "android -> tun");
                            // Bounded: this suspends the TCP reader when the TUN writer lags.
                            if to_tun.send(pkt).await.is_err() {
                                break;
                            }
                            stats.add_peer_to_tun(len);
                        }
                    }
                }
                if machine.state() == State::Active && !reached_active {
                    reached_active = true;
                    let _ = active_tx.send(true);
                    if let Some(h) = machine.hello() {
                        info!(session = %h.session, device_port = h.device_port, "session Active");
                    }
                }
                if writer.is_finished() {
                    break SessionEnd::TunClosed;
                }
            }
            Err(close) => {
                break match close {
                    Close::PeerStop => SessionEnd::PeerStop,
                    Close::PeerError(e) => SessionEnd::PeerError(e),
                    Close::VpnError(e) => SessionEnd::VpnError(e),
                    Close::Protocol(e) => {
                        let _ = out_tx.send(Frame::json(MessageType::Error, &e)).await;
                        SessionEnd::ProtocolViolation(format!("{}: {}", e.code, e.message))
                    }
                };
            }
        }
    };

    // Tear down: stop the pump, let the writer flush what is queued, then drop.
    pump.abort();
    drop(out_tx);
    match tokio::time::timeout(Duration::from_millis(500), writer).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => warn!(error = %e, "TCP writer failed"),
        Ok(Err(e)) => warn!(error = %e, "TCP writer task panicked"),
        Err(_) => warn!("TCP writer did not flush within 500ms"),
    }
    info!(end = %end, "session ended");
    SessionSummary { end, reached_active }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SessionConfig {
        SessionConfig {
            mtu: 1400,
            addresses: vec![Prefix { address: "192.168.10.74".into(), prefix: 32 }],
            routes: vec![Prefix { address: "0.0.0.0".into(), prefix: 0 }],
            dns: vec!["192.168.10.1".into()],
            session_name: "Routedroid Phase 0".into(),
            expected_session: Some("s1".into()),
            secret: Secret::new(TV_SECRET),
        }
    }

    const TV_SECRET: [u8; 32] = [7u8; 32];
    const CLIENT_NONCE: [u8; 32] = [0xaa; 32];
    const HOST_NONCE: [u8; 32] = [0xbb; 32];

    fn machine() -> Machine {
        Machine::new(cfg(), HOST_NONCE)
    }

    fn hello(session: &str) -> Frame {
        Frame::json(
            MessageType::Hello,
            &Hello { protocol: 0, session: session.into(), device_port: 9000, client_nonce: hex::encode(CLIENT_NONCE) },
        )
    }

    fn auth_frame(secret: &[u8; 32], role: &str, session: &str, port: u16) -> Frame {
        let t = auth::transcript(0, session, port, &CLIENT_NONCE, &HOST_NONCE);
        Frame::json(MessageType::Auth, &Auth { android_proof: hex::encode(auth::proof(secret, role, &t)) })
    }

    /// HELLO + valid AUTH; returns the CONFIGURE_VPN outbound.
    fn authenticate(m: &mut Machine) -> Vec<Outbound> {
        let out = m.handle(hello("s1")).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(m.state(), State::Authenticating);
        m.handle(auth_frame(&TV_SECRET, "android", "s1", 9000)).unwrap()
    }

    fn ready() -> Frame {
        Frame::json(MessageType::VpnReady, &VpnReady { addresses: vec!["192.168.10.74/32".into()], mtu: 1400 })
    }

    fn packet() -> Vec<u8> {
        let mut p = vec![0u8; 28];
        p[0] = 0x45;
        p[2..4].copy_from_slice(&28u16.to_be_bytes());
        p
    }

    #[test]
    fn happy_path_reaches_active_and_forwards_packets() {
        let mut m = machine();
        assert_eq!(m.state(), State::Connected);
        let out = m.handle(hello("s1")).unwrap();
        assert_eq!(out.len(), 1);
        match &out[0] {
            Outbound::ToPeer(f) => {
                assert_eq!(f.message_type, MessageType::HelloAck);
                let ack: HelloAck = serde_json::from_slice(&f.body).unwrap();
                assert_eq!(ack.mtu, 1400);
                assert_eq!(ack.host_nonce, hex::encode(HOST_NONCE));
                let t = auth::transcript(0, "s1", 9000, &CLIENT_NONCE, &HOST_NONCE);
                assert!(auth::verify(&TV_SECRET, "host", &t, &ack.host_proof));
            }
            o => panic!("{o:?}"),
        }
        assert!(!m.secret_consumed());
        let out = m.handle(auth_frame(&TV_SECRET, "android", "s1", 9000)).unwrap();
        assert!(m.secret_consumed());
        assert_eq!(out.len(), 1);
        match &out[0] {
            Outbound::ToPeer(f) => {
                assert_eq!(f.message_type, MessageType::ConfigureVpn);
                let c: ConfigureVpn = serde_json::from_slice(&f.body).unwrap();
                assert_eq!(c, cfg().configure_vpn());
            }
            o => panic!("{o:?}"),
        }
        assert_eq!(m.state(), State::Configuring);
        assert!(m.handle(ready()).unwrap().is_empty());
        assert_eq!(m.state(), State::Active);
        let out = m.handle(Frame::ip_packet(packet())).unwrap();
        assert_eq!(out, vec![Outbound::ToTun(packet())]);
        assert_eq!(
            m.handle(Frame::empty(MessageType::Ping)).unwrap(),
            vec![Outbound::ToPeer(Frame::empty(MessageType::Pong))]
        );
        assert_eq!(m.handle(Frame::empty(MessageType::Stop)).unwrap_err(), Close::PeerStop);
        assert_eq!(m.state(), State::Closed);
    }

    #[test]
    fn ip_packet_before_active_closes() {
        let mut m = machine();
        let err = m.handle(Frame::ip_packet(packet())).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "out_of_state"), "{err:?}");
        assert_eq!(m.state(), State::Closed);

        let mut m = machine();
        m.handle(hello("s1")).unwrap();
        let err = m.handle(Frame::ip_packet(packet())).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "out_of_state"), "{err:?}");

        let mut m = machine();
        authenticate(&mut m);
        let err = m.handle(Frame::ip_packet(packet())).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "out_of_state"), "{err:?}");
    }

    #[test]
    fn auth_rejects_wrong_secret_reflection_replay_and_out_of_order() {
        // Wrong secret.
        let mut m = machine();
        m.handle(hello("s1")).unwrap();
        let err = m.handle(auth_frame(&[9u8; 32], "android", "s1", 9000)).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "auth_failed"), "{err:?}");
        assert_eq!(m.state(), State::Closed);
        assert!(m.secret_consumed());

        // Reflection: the host's own proof sent back.
        let mut m = machine();
        m.handle(hello("s1")).unwrap();
        let err = m.handle(auth_frame(&TV_SECRET, "host", "s1", 9000)).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "auth_failed"));

        // Transcript bound to the reverse port: proof for another port fails.
        let mut m = machine();
        m.handle(hello("s1")).unwrap();
        let err = m.handle(auth_frame(&TV_SECRET, "android", "s1", 9001)).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "auth_failed"));

        // AUTH before HELLO, and a second AUTH after success.
        let mut m = machine();
        let err = m.handle(auth_frame(&TV_SECRET, "android", "s1", 9000)).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "out_of_state"));
        let mut m = machine();
        authenticate(&mut m);
        let err = m.handle(auth_frame(&TV_SECRET, "android", "s1", 9000)).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "out_of_state"));

        // Malformed nonce and malformed proof.
        let mut m = machine();
        let bad = Frame::json(
            MessageType::Hello,
            &Hello { protocol: 0, session: "s1".into(), device_port: 9000, client_nonce: "zz".into() },
        );
        let err = m.handle(bad).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "bad_nonce"));
        let mut m = machine();
        m.handle(hello("s1")).unwrap();
        let err = m.handle(Frame::json(MessageType::Auth, &Auth { android_proof: "nothex".into() })).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "auth_failed"));

        // Any violation before AUTH also discards the secret.
        let mut m = machine();
        let _ = m.handle(hello("other")).unwrap_err();
        assert!(m.secret_consumed());
    }

    #[test]
    fn invalid_ipv4_in_active_closes() {
        let mut m = machine();
        authenticate(&mut m);
        m.handle(ready()).unwrap();
        let mut p = packet();
        p[3] = 29; // total_length mismatch
        let err = m.handle(Frame::ip_packet(p)).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "bad_ipv4"), "{err:?}");
    }

    #[test]
    fn hello_checks_protocol_and_session() {
        let mut m = machine();
        let err = m.handle(hello("other")).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "bad_session"));

        let mut m = machine();
        let bad = Frame::json(
            MessageType::Hello,
            &Hello { protocol: 1, session: "s1".into(), device_port: 1, client_nonce: hex::encode(CLIENT_NONCE) },
        );
        let err = m.handle(bad).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "unsupported_protocol"));

        let mut m = Machine::new(SessionConfig { expected_session: None, ..cfg() }, HOST_NONCE);
        assert!(m.handle(hello("anything")).is_ok());
    }

    #[test]
    fn duplicate_hello_and_wrong_direction_close() {
        let mut m = machine();
        m.handle(hello("s1")).unwrap();
        let err = m.handle(hello("s1")).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "out_of_state"));

        let mut m = machine();
        let ack = HelloAck { protocol: 0, mtu: 1, host_nonce: String::new(), host_proof: String::new() };
        let err = m.handle(Frame::json(MessageType::HelloAck, &ack)).unwrap_err();
        assert!(matches!(err, Close::Protocol(ref e) if e.code == "wrong_direction"));
    }

    #[test]
    fn vpn_ready_validates_mtu_and_addresses() {
        let mut m = machine();
        authenticate(&mut m);
        let bad =
            Frame::json(MessageType::VpnReady, &VpnReady { addresses: vec!["192.168.10.74/32".into()], mtu: 1500 });
        assert!(matches!(m.handle(bad).unwrap_err(), Close::Protocol(ref e) if e.code == "mtu_mismatch"));

        let mut m = machine();
        authenticate(&mut m);
        let bad = Frame::json(MessageType::VpnReady, &VpnReady { addresses: vec!["10.0.0.1/32".into()], mtu: 1400 });
        assert!(matches!(m.handle(bad).unwrap_err(), Close::Protocol(ref e) if e.code == "address_mismatch"));

        let mut m = machine();
        authenticate(&mut m);
        let err =
            m.handle(Frame::json(MessageType::VpnError, &ErrorBody::new("vpn_permission_denied", "no"))).unwrap_err();
        assert_eq!(err, Close::VpnError(ErrorBody::new("vpn_permission_denied", "no")));
        assert_eq!(m.state(), State::Closed);
    }
}
