//! One protocol session of a device connection, between "both sides are up"
//! and "the driver stopped": wait for the app, hand the socket to the
//! protocol driver, and publish `Active` the moment the driver reaches it.
//! A connection has one session, and one more each time the phone comes
//! back after going away (`resume`).

use std::net::Ipv4Addr;

use routedroid_ipc::{ConnectionState, EndReason, NetworkInfo};
use routedroid_proto::messages::Prefix;
use tokio::sync::watch;
use tracing::info;

use super::resume::{Driven, Shared};
use super::run::ConnectionRun;
use super::{end, run};
use crate::app_listener::{AppListener, Expected};
use crate::device::AdbBridge;
use crate::fault::{Fault, Kind, Result};
use crate::host_network::HelperEvent;
use crate::session::SessionEnd;
use crate::session::{Machine, Progress, SessionConfig, SessionDriver};

impl ConnectionRun {
    /// `Err` is a failure before the app connected (adb, the listener).
    pub(super) async fn drive(
        &self,
        listener: AppListener,
        bridge: &mut AdbBridge,
        shared: &mut Shared<'_>,
        mut stop_rx: watch::Receiver<bool>,
    ) -> Result<Driven> {
        let mtu = self.spec.mtu;
        let secret = bridge.bootstrap().await?;
        self.sink
            .set(ConnectionState::WaitingForApp { screen: None });
        let screen = super::screen::watch(bridge.adb().clone(), &self.sink);
        tokio::pin!(screen);
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
            _ = stop_rx.changed() => return Ok(Driven {
                result: Ok(if shared.was_active { EndReason::Stopped } else { EndReason::StoppedEarly }),
                lost: false,
                reached_active: false,
            }),
            never = &mut screen => match never {},
        };
        info!(
            host_port,
            device_port = bridge.device_port(),
            "app connected"
        );
        self.sink.set(ConnectionState::Handshaking { screen: None });

        let config = SessionConfig {
            mtu,
            addresses: vec![Prefix::new(shared.placed.phone_ip, 32)],
            routes: vec![Prefix::new(Ipv4Addr::UNSPECIFIED, 0)],
            dns: shared.placed.dns.clone(),
            session_name: "Routedroid".into(),
            expected_session: bridge.session.clone(),
            expected_device_port: bridge.device_port(),
        };
        let (progress, mut active_rx) = Progress::new(self.counters.clone());
        let machine = Machine::new(config, secret, bridge.host_nonce);
        let packets = shared.network.packets();
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
                never = &mut screen, if watch_active => match never {},
                event = shared.events.recv(), if watch_events => match event {
                    Some(event) => self.on_helper(event, shared.placed, &mut ended),
                    None => watch_events = false,
                },
            }
        };
        // The helper's word on why it ended comes just before it closes.
        while let Ok(event) = shared.events.try_recv() {
            self.on_helper(event, shared.placed, &mut ended);
        }
        info!(
            to_phone = summary.packets_to_phone,
            from_phone = summary.packets_from_phone,
            malformed = summary.malformed,
            congested = summary.congested,
            "traffic"
        );
        if ended.is_some() {
            shared.network.ended_by_helper();
        }
        let lost = ended.is_none()
            && matches!(
                summary.end,
                SessionEnd::PeerClosed | SessionEnd::Transport(_) | SessionEnd::KeepaliveTimeout
            );
        let reached_active = summary.reached_active;
        let result = match ended {
            Some(why) if matches!(summary.end, SessionEnd::HelperClosed) => Err(Fault::msg(
                Kind::Helper,
                format!("the helper ended the session: {why}"),
            )),
            _ => end::reason(summary.end, reached_active || shared.was_active),
        };
        Ok(Driven {
            result,
            lost,
            reached_active,
        })
    }

    pub(super) fn on_helper(
        &self,
        event: HelperEvent,
        placed: &mut NetworkInfo,
        ended: &mut Option<String>,
    ) {
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
