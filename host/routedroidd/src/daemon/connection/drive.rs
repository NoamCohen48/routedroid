//! The part of a device connection between "both sides are up" and "the
//! driver stopped": wait for the app, hand the socket to the protocol
//! driver, and publish `Active` the moment the driver reaches it. The
//! protocol's own word for what runs here is a *session*.

use std::net::Ipv4Addr;

use routedroid_ipc::{ConnectionState, EndReason, NetworkInfo};
use routedroid_proto::messages::Prefix;
use tokio::sync::watch;
use tracing::info;

use super::run::ConnectionRun;
use super::{end, run};
use crate::app_listener::{AppListener, Expected};
use crate::device::AdbBridge;
use crate::fault::{Fault, Kind, Result};
use crate::host_network::{HelperEvent, HostNetwork};
use crate::session::SessionEnd;
use crate::session::{Machine, Progress, SessionConfig, SessionDriver};

impl ConnectionRun {
    pub(super) async fn drive(
        &self,
        listener: AppListener,
        placed: &NetworkInfo,
        bridge: &mut AdbBridge,
        network: &mut HostNetwork,
        mut stop_rx: watch::Receiver<bool>,
    ) -> Result<EndReason> {
        let mtu = self.spec.mtu;
        let secret = bridge.bootstrap().await?;
        self.sink.set(ConnectionState::WaitingForApp);
        // The app dials in (over adb reverse, not over the VPN): the phone
        // never listens, so nothing on it can be reached before AUTH.
        let connect_timeout = self.spec.connect_timeout;
        let host_port = listener.port();
        let expected = Expected {
            session: bridge.session.clone(),
            device_port: bridge.device_port(),
            mtu,
        };
        let app = tokio::select! {
            accepted = listener.accept(connect_timeout, &expected) => accepted?,
            _ = stop_rx.changed() => return Ok(EndReason::StoppedEarly),
        };
        info!(
            host_port,
            device_port = bridge.device_port(),
            "app connected"
        );
        self.sink.set(ConnectionState::Handshaking);

        let config = SessionConfig {
            mtu,
            addresses: vec![Prefix::new(placed.phone_ip, 32)],
            routes: vec![Prefix::new(Ipv4Addr::UNSPECIFIED, 0)],
            dns: placed.dns.clone(),
            session_name: "Routedroid".into(),
            expected_session: bridge.session.clone(),
            expected_device_port: bridge.device_port(),
        };
        let (progress, mut active_rx) = Progress::new(self.counters.clone());
        let machine = Machine::new(config, secret, bridge.host_nonce);
        let packets = network.relay();
        let mut events = network.take_events().expect("relay() was just called");
        let driver = SessionDriver::run(
            app.stream,
            Some(app.hello),
            machine,
            packets,
            stop_rx,
            progress,
        );
        tokio::pin!(driver);
        // Publish Active the moment the driver flips it, and renewals as
        // they come; then wait for the end.
        let mut placed = placed.clone();
        let mut ended = None;
        let mut watch_active = true;
        let mut watch_events = true;
        let summary = loop {
            tokio::select! {
                summary = &mut driver => break summary,
                changed = active_rx.changed(), if watch_active => match changed {
                    Ok(()) if *active_rx.borrow() => { self.sink.set(ConnectionState::Active); watch_active = false; }
                    Ok(()) => {}
                    Err(_) => watch_active = false,
                },
                event = events.recv(), if watch_events => match event {
                    Some(event) => self.on_helper(event, &mut placed, &mut ended),
                    None => watch_events = false,
                },
            }
        };
        // The helper's word on why it ended comes just before it closes.
        while let Ok(event) = events.try_recv() {
            self.on_helper(event, &mut placed, &mut ended);
        }
        info!(
            to_phone = summary.packets_to_phone,
            from_phone = summary.packets_from_phone,
            malformed = summary.malformed,
            congested = summary.congested,
            "traffic"
        );
        self.sink.set(ConnectionState::Stopping);
        if ended.is_some() {
            network.ended_by_helper();
        }
        match ended {
            Some(why) if matches!(summary.end, SessionEnd::HelperClosed) => Err(Fault::msg(
                Kind::Helper,
                format!("the helper ended the session: {why}"),
            )),
            _ => end::reason(summary.end, summary.reached_active),
        }
    }

    fn on_helper(&self, event: HelperEvent, placed: &mut NetworkInfo, ended: &mut Option<String>) {
        match event {
            HelperEvent::Renewed(lease) => {
                info!(expires_at = lease.expires_at, "lease renewed");
                placed.lease = Some(run::lease(&lease));
                self.sink.set_network(placed.clone());
            }
            HelperEvent::Ended(why) => *ended = Some(why),
        }
    }
}
