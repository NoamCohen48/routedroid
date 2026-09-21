//! The connect sequence for one session, from `start` accepted to everything
//! undone. Order matters for safety: host network first (so a failure leaves
//! nothing on the phone), then the reverse mapping, then the secret, then
//! the launch.

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use routedroid_ipc::fault::{Fault, Kind, Result};
use routedroid_ipc::{Outcome, SessionState, StartRequest};
use routedroid_proto::frame::DEFAULT_MTU;
use routedroid_proto::messages::Prefix;
use tokio::sync::watch;
use tracing::info;

use super::session::StateSink;
use super::Daemon;
use crate::app_listener::AppListener;
use crate::device::{DeviceSession, Transport};
use crate::host_network::HostNetwork;
use crate::session::{Counters, Machine, Progress, SessionConfig, SessionDriver, SessionEnd};

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(90);
const HELPER_START_TIMEOUT: Duration = Duration::from_secs(15);

pub async fn run_session(
    daemon: &Daemon,
    req: StartRequest,
    tun: String,
    stop_rx: watch::Receiver<bool>,
    counters: Arc<Counters>,
    sink: &StateSink,
) -> Outcome {
    match run(daemon, req, tun, stop_rx, counters, sink).await {
        Ok(message) => Outcome { ok: true, kind: None, message: message.into() },
        Err(e) => {
            tracing::warn!(kind = e.kind().as_str(), "session failed: {e}");
            Outcome { ok: false, kind: Some(e.kind()), message: e.to_string() }
        }
    }
}

async fn run(
    daemon: &Daemon,
    req: StartRequest,
    tun: String,
    mut stop_rx: watch::Receiver<bool>,
    counters: Arc<Counters>,
    sink: &StateSink,
) -> Result<&'static str> {
    Transport::check(&req.serial, req.allow_network_adb)?;
    let mtu = req.mtu.unwrap_or(DEFAULT_MTU);
    if !(576..=65535).contains(&mtu) {
        return Err(Fault::msg(Kind::Usage, format!("mtu {mtu} outside 576..=65535")));
    }
    let adb = daemon.adb.device(&req.serial);
    let listener = AppListener::bind().await?;
    let host_port = listener.port();

    let mut network = tokio::time::timeout(
        HELPER_START_TIMEOUT,
        HostNetwork::start(&daemon.helper_socket, &req.lan_if, req.phone_ip, &tun, mtu),
    )
    .await
    .map_err(|_| Fault::msg(Kind::Helper, format!("helper did not answer Start within {HELPER_START_TIMEOUT:?}")))??;
    info!(serial = adb.serial(), tun = %network.tun, host_ip = %network.host_ip, lan_prefix = network.lan_prefix,
          phone_ip = %req.phone_ip, helper_session = %network.session, "host network ready");
    let mut device_session = match DeviceSession::open(adb, host_port).await {
        Ok(device_session) => device_session,
        Err(e) => {
            network.stop().await;
            return Err(e);
        }
    };

    let outcome = async {
        device_session.bootstrap().await?;
        sink.set(SessionState::WaitingForApp);
        // The app dials in (over adb reverse, not over the VPN): the phone
        // never listens, so nothing on it can be reached before AUTH.
        let connect_timeout = req.connect_timeout_secs.map(Duration::from_secs).unwrap_or(DEFAULT_CONNECT_TIMEOUT);
        let stream = tokio::select! {
            r = listener.accept(connect_timeout) => r?,
            _ = stop_rx.changed() => return Ok("stopped before the app connected"),
        };
        info!(host_port, device_port = device_session.device_port(), "app connected");
        sink.set(SessionState::Handshaking);

        let cfg = SessionConfig {
            mtu,
            addresses: vec![Prefix::new(req.phone_ip, 32)],
            routes: vec![Prefix::new(Ipv4Addr::UNSPECIFIED, 0)],
            dns: req.dns.iter().map(ToString::to_string).collect(),
            session_name: "Routedroid".into(),
            expected_session: device_session.session.clone(),
            expected_device_port: device_session.device_port(),
            secret: device_session.take_secret(),
        };
        let (progress, mut active_rx) = Progress::new(counters);
        let machine = Machine::new(cfg, device_session.host_nonce);
        let driver = SessionDriver::run(stream, machine, network.relay(), stop_rx, progress);
        tokio::pin!(driver);
        // Publish Active the moment the driver flips it; then wait for the end.
        let mut watch_active = true;
        let summary = loop {
            tokio::select! {
                summary = &mut driver => break summary,
                changed = active_rx.changed(), if watch_active => match changed {
                    Ok(()) if *active_rx.borrow() => { sink.set(SessionState::Active); watch_active = false; }
                    Ok(()) => {}
                    Err(_) => watch_active = false,
                },
            }
        };
        info!(
            to_phone = summary.packets_to_phone,
            from_phone = summary.packets_from_phone,
            dropped = summary.bad_packets,
            "traffic"
        );
        sink.set(SessionState::Stopping);
        match summary.end {
            // A stop we asked for is a success whatever phase it interrupted.
            SessionEnd::LocalStop if !summary.reached_active => Ok("stopped before the session was active"),
            SessionEnd::LocalStop | SessionEnd::PeerStop | SessionEnd::PeerClosed if summary.reached_active => {
                Ok("session ended cleanly")
            }
            // Peer-supplied text: `{:?}` escapes control characters before it reaches a terminal.
            SessionEnd::VpnError(e) => Err(Fault::msg(Kind::Vpn, format!("{}: {:?}", e.code, e.message))),
            SessionEnd::Refused(e) if e.code == "auth_failed" => Err(Fault::msg(Kind::Auth, e.message)),
            SessionEnd::Refused(e) => Err(Fault::msg(Kind::Protocol, format!("{}: {:?}", e.code, e.message))),
            other => Err(Fault::msg(Kind::Vpn, other.to_string())),
        }
    }
    .await;

    // Concurrent: a hung adb must not delay releasing the host network.
    tokio::join!(device_session.close(), network.stop());
    outcome
}
