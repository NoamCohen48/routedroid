//! Pure host state machine (`protocol/version-1.md` §5). Fed one validated
//! frame at a time; returns what to emit or why to close. No I/O, no time.

use routedroid_proto::auth::{self, Nonce, Secret};
use routedroid_proto::frame::{Frame, MessageType};
use routedroid_proto::messages::{
    self, Auth, BodyError, ErrorBody, ErrorCode, Hello, HelloAck, VpnReady,
};
use routedroid_proto::state::{self, State};
use routedroid_proto::Role;
use routedroid_proto::PROTOCOL_VERSION;

use super::{Close, SessionConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outbound {
    ToPeer(Frame),
    ToHelper(Vec<u8>),
}

#[derive(Debug)]
pub struct Machine {
    state: State,
    cfg: SessionConfig,
    host_nonce: Nonce,
    /// Fixed by HELLO/HELLO_ACK; verified against AUTH.
    transcript: Option<Vec<u8>>,
    /// `Some` until AUTH is decided, then dropped (zeroized).
    secret: Option<Secret>,
}

impl Machine {
    /// `secret` is the session's only copy; the machine drops it once AUTH
    /// is decided (§7.3), whatever the outcome.
    pub fn new(cfg: SessionConfig, secret: Secret, host_nonce: Nonce) -> Self {
        Self {
            state: State::Connected,
            cfg,
            host_nonce,
            transcript: None,
            secret: Some(secret),
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn mtu(&self) -> u32 {
        self.cfg.mtu
    }

    #[cfg(test)]
    pub fn secret_consumed(&self) -> bool {
        self.secret.is_none()
    }

    fn refuse(&mut self, code: ErrorCode, message: impl Into<String>) -> Close {
        self.state = State::Closed;
        self.secret = None;
        Close::Refuse(ErrorBody::new(code, message))
    }

    fn body<T: messages::Body>(&mut self, what: &str, bytes: &[u8]) -> Result<T, Close> {
        messages::parse(bytes)
            .map_err(|e: BodyError| self.refuse(ErrorCode::ProtocolError, format!("{what}: {e}")))
    }

    /// Feed one frame that already passed header validation.
    pub fn handle(&mut self, frame: Frame) -> Result<Vec<Outbound>, Close> {
        let t = frame.message_type;
        if !state::is_allowed(Role::Host, self.state, t) {
            return Err(self.refuse(
                ErrorCode::ProtocolError,
                format!("{t} not allowed in {}", self.state.name()),
            ));
        }
        match t {
            MessageType::Stop => {
                self.state = State::Closed;
                self.secret = None;
                Err(Close::PeerStop)
            }
            MessageType::Ping => Ok(vec![Outbound::ToPeer(Frame::empty(MessageType::Pong))]),
            MessageType::Pong => Ok(Vec::new()),
            MessageType::Hello => self.on_hello(&frame.body),
            MessageType::Auth => self.on_auth(&frame.body),
            MessageType::VpnReady => self.on_vpn_ready(&frame.body),
            MessageType::VpnError => {
                let body: ErrorBody = self.body("VPN_ERROR", &frame.body)?;
                self.state = State::Closed;
                Err(Close::VpnError(body))
            }
            // The allowlist admitted it, so the session is Active; the
            // §6 packet checks belong to the uplink.
            MessageType::IpPacket => Ok(vec![Outbound::ToHelper(frame.body)]),
            // Never in the host allowlist; kept exhaustive on purpose.
            MessageType::HelloAck | MessageType::ConfigureVpn | MessageType::Error => Err(self
                .refuse(
                    ErrorCode::ProtocolError,
                    format!("{t} is host-to-android only"),
                )),
        }
    }

    fn on_hello(&mut self, bytes: &[u8]) -> Result<Vec<Outbound>, Close> {
        let hello: Hello = self.body("HELLO", bytes)?;
        if hello.protocol != u32::from(PROTOCOL_VERSION) {
            self.state = State::Closed;
            self.secret = None;
            return Err(Close::Refuse(ErrorBody::protocol_unsupported(format!(
                "app speaks protocol {}, host speaks {PROTOCOL_VERSION}",
                hello.protocol
            ))));
        }
        if hello.session != self.cfg.expected_session
            || hello.device_port != self.cfg.expected_device_port
        {
            return Err(self.refuse(
                ErrorCode::SessionMismatch,
                "HELLO session/port is not the one launched",
            ));
        }
        let transcript = auth::transcript(
            &hello.session,
            hello.device_port,
            &hello.client_nonce,
            &self.host_nonce,
        );
        let Some(secret) = self.secret.as_ref() else {
            return Err(self.refuse(ErrorCode::Internal, "session secret already consumed"));
        };
        let host_proof = auth::proof(secret, Role::Host, &transcript);
        self.transcript = Some(transcript);
        self.state = State::Authenticating;
        let ack = HelloAck {
            protocol: PROTOCOL_VERSION,
            mtu: self.cfg.mtu,
            host_nonce: self.host_nonce,
            host_proof,
        };
        Ok(vec![Outbound::ToPeer(Frame::json(
            MessageType::HelloAck,
            &ack,
        ))])
    }

    fn on_auth(&mut self, bytes: &[u8]) -> Result<Vec<Outbound>, Close> {
        let msg: Auth = self.body("AUTH", bytes)?;
        // Single use: the secret is dropped whatever the outcome.
        let secret = self
            .secret
            .take()
            .expect("secret present while Authenticating");
        let transcript = self.transcript.take().expect("transcript fixed by HELLO");
        if !auth::verify(&secret, Role::Android, &transcript, &msg.android_proof) {
            return Err(self.refuse(ErrorCode::AuthFailed, "android_proof does not verify"));
        }
        drop(secret);
        self.state = State::Configuring;
        Ok(vec![Outbound::ToPeer(Frame::json(
            MessageType::ConfigureVpn,
            &self.cfg.configure_vpn(),
        ))])
    }

    fn on_vpn_ready(&mut self, bytes: &[u8]) -> Result<Vec<Outbound>, Close> {
        let ready: VpnReady = self.body("VPN_READY", bytes)?;
        if ready.mtu != self.cfg.mtu {
            return Err(self.refuse(
                ErrorCode::ProtocolError,
                format!("VPN_READY mtu {} != {}", ready.mtu, self.cfg.mtu),
            ));
        }
        // Exactly what was sent, compared as values: nothing missing, nothing extra.
        let missing = self
            .cfg
            .addresses
            .iter()
            .find(|a| !ready.addresses.contains(a));
        let extra = ready
            .addresses
            .iter()
            .find(|a| !self.cfg.addresses.contains(a));
        if let Some(a) = missing {
            return Err(self.refuse(
                ErrorCode::ProtocolError,
                format!("VPN_READY lacks {}/{}", a.address, a.prefix),
            ));
        }
        if let Some(a) = extra {
            return Err(self.refuse(
                ErrorCode::ProtocolError,
                format!("VPN_READY has extra {}/{}", a.address, a.prefix),
            ));
        }
        self.state = State::Active;
        Ok(Vec::new())
    }
}
