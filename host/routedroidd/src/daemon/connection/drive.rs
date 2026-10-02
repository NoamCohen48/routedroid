//! The part of a device connection between "both sides are up" and "the
//! driver stopped": wait for the app, hand the socket to the protocol
//! driver, and publish `Active` the moment the driver reaches it. The
//! protocol's own word for what runs here is a *session*.

use std::net::Ipv4Addr;
use std::time::Duration;

use routedroid_ipc::fault::{Fault, Kind, Result};
use routedroid_ipc::ConnectionState;
use routedroid_proto::messages::{ErrorCode, Prefix};
use tokio::sync::watch;
use tracing::info;

use super::run::ConnectionRun;
use crate::app_listener::{AppListener, Expected};
use crate::device::AdbBridge;
use crate::host_network::HostNetwork;
use crate::session::{Machine, Progress, SessionConfig, SessionDriver, SessionEnd};

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(90);

impl ConnectionRun<'_> {
    pub(super) async fn drive(
        &self,
        mtu: u32,
        listener: AppListener,
        bridge: &mut AdbBridge,
        network: &mut HostNetwork,
        mut stop_rx: watch::Receiver<bool>,
    ) -> Result<&'static str> {
        let secret = bridge.bootstrap().await?;
        self.sink.set(ConnectionState::WaitingForApp);
        // The app dials in (over adb reverse, not over the VPN): the phone
        // never listens, so nothing on it can be reached before AUTH.
        let connect_timeout = self
            .request
            .connect_timeout_secs
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_CONNECT_TIMEOUT);
        let host_port = listener.port();
        let expected = Expected {
            session: bridge.session.clone(),
            device_port: bridge.device_port(),
            mtu,
        };
        let app = tokio::select! {
            accepted = listener.accept(connect_timeout, &expected) => accepted?,
            _ = stop_rx.changed() => return Ok("stopped before the app connected"),
        };
        info!(
            host_port,
            device_port = bridge.device_port(),
            "app connected"
        );
        self.sink.set(ConnectionState::Handshaking);

        let config = SessionConfig {
            mtu,
            addresses: vec![Prefix::new(self.request.phone_ip, 32)],
            routes: vec![Prefix::new(Ipv4Addr::UNSPECIFIED, 0)],
            dns: self.request.dns.clone(),
            session_name: "Routedroid".into(),
            expected_session: bridge.session.clone(),
            expected_device_port: bridge.device_port(),
        };
        let (progress, mut active_rx) = Progress::new(self.counters.clone());
        let machine = Machine::new(config, secret, bridge.host_nonce);
        let driver = SessionDriver::run(
            app.stream,
            Some(app.hello),
            machine,
            network.relay(),
            stop_rx,
            progress,
        );
        tokio::pin!(driver);
        // Publish Active the moment the driver flips it; then wait for the end.
        let mut watch_active = true;
        let summary = loop {
            tokio::select! {
                summary = &mut driver => break summary,
                changed = active_rx.changed(), if watch_active => match changed {
                    Ok(()) if *active_rx.borrow() => { self.sink.set(ConnectionState::Active); watch_active = false; }
                    Ok(()) => {}
                    Err(_) => watch_active = false,
                },
            }
        };
        info!(
            to_phone = summary.packets_to_phone,
            from_phone = summary.packets_from_phone,
            malformed = summary.malformed,
            congested = summary.congested,
            "traffic"
        );
        self.sink.set(ConnectionState::Stopping);
        match summary.end {
            // A stop we asked for is a success whatever phase it interrupted.
            SessionEnd::LocalStop if !summary.reached_active => {
                Ok("stopped before the connection was active")
            }
            SessionEnd::LocalStop | SessionEnd::PeerStop | SessionEnd::PeerClosed
                if summary.reached_active =>
            {
                Ok("session ended cleanly")
            }
            SessionEnd::VpnError(e) | SessionEnd::Refused(e)
                if e.known_code() == Some(ErrorCode::ConsentTimeout) =>
            {
                Err(Fault::msg(
                    Kind::Vpn,
                    "VPN permission was not granted on the phone within 2 minutes",
                ))
            }
            // Peer-supplied text: `{:?}` escapes control characters before it reaches a terminal.
            SessionEnd::VpnError(e) => Err(Fault::msg(
                Kind::Vpn,
                format!("{}: {:?}", e.code, e.message),
            )),
            SessionEnd::Refused(e) if e.code == "auth_failed" => {
                Err(Fault::msg(Kind::Auth, e.message))
            }
            SessionEnd::Refused(e) => Err(Fault::msg(
                Kind::Protocol,
                format!("{}: {:?}", e.code, e.message),
            )),
            other => Err(Fault::msg(Kind::Vpn, other.to_string())),
        }
    }
}
