//! A connection's protocol sessions on its one host network. The first
//! starts with the connection. When the phone goes away after being active
//! (unplugged, or adb lost it) and the connection's `reconnect_wait` allows,
//! the host side stays up: the next session starts once the phone is back,
//! on the same TUN, with the same address and lease, and with a fresh adb
//! reverse mapping, secret and app launch, as any start has.

use std::time::Duration;

use routedroid_ipc::{EndReason, NetworkInfo};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use super::run::ConnectionRun;
use crate::app_listener::AppListener;
use crate::device::AdbBridge;
use crate::fault::{Kind, Result};
use crate::host_network::{HelperEvent, HostNetwork};

/// How one session ended, for the connection to decide what comes next.
pub(super) struct Driven {
    pub result: Result<EndReason>,
    /// The transport closed, broke or went silent: the phone itself may be
    /// gone, which is worth waiting out if adb no longer sees it.
    pub lost: bool,
    pub reached_active: bool,
}

/// Everything a session shares with the rest of its connection.
pub(super) struct Shared<'a> {
    pub network: &'a mut HostNetwork,
    /// Where the phone is on the LAN; renewals update it.
    pub placed: &'a mut NetworkInfo,
    pub events: &'a mut mpsc::Receiver<HelperEvent>,
    /// An earlier session was active: ends read like an active connection's.
    pub was_active: bool,
}

/// The last session's bridge, still to be closed, comes back with the end.
pub(super) type Ended = (Result<EndReason>, Option<AdbBridge>);

/// One session's run: how it went (`Err` before the app connected), and
/// its bridge if one was opened.
struct Attempt {
    driven: Result<Driven>,
    bridge: Option<AdbBridge>,
}

impl ConnectionRun {
    pub(super) async fn sessions(
        &self,
        network: &mut HostNetwork,
        placed: &mut NetworkInfo,
        events: &mut mpsc::Receiver<HelperEvent>,
        stop_rx: &mut watch::Receiver<bool>,
    ) -> Ended {
        let wait = self.spec.reconnect_wait;
        let mut was_active = false;
        // Set while the phone is away: when to give up on it.
        let mut deadline: Option<Instant> = None;
        loop {
            let mut shared = Shared {
                network,
                placed,
                events,
                was_active,
            };
            let Attempt { driven, bridge } = self.session(&mut shared, stop_rx.clone()).await;
            let (result, lost) = match driven {
                Ok(driven) => {
                    let Driven {
                        result,
                        lost,
                        reached_active,
                    } = driven;
                    if reached_active {
                        was_active = true;
                        deadline = None;
                    }
                    (result, lost && was_active)
                }
                // adb failing while the phone comes back is the phone not back yet.
                Err(fault) => {
                    let lost = deadline.is_some() && fault.kind() == Kind::Adb;
                    (Err(fault), lost)
                }
            };
            let (network, placed, events) = (shared.network, shared.placed, shared.events);
            if wait.is_zero() || !lost || !self.phone_gone().await {
                return (result, bridge);
            }
            if let Some(bridge) = bridge {
                bridge.close().await; // its transport is gone with the phone
            }
            let until = *deadline.get_or_insert_with(|| Instant::now() + wait);
            match self
                .await_return(until, wait, network, placed, events, stop_rx)
                .await
            {
                Ok(true) => {}
                Ok(false) => return (Ok(EndReason::Stopped), None),
                Err(fault) => return (Err(fault), None),
            }
        }
    }

    /// One session: a listener and an adb bridge of its own, then the protocol.
    async fn session(&self, shared: &mut Shared<'_>, stop_rx: watch::Receiver<bool>) -> Attempt {
        let failed = |fault| Attempt {
            driven: Err(fault),
            bridge: None,
        };
        let listener = match AppListener::bind().await {
            Ok(listener) => listener,
            Err(fault) => return failed(fault),
        };
        let adb = self.adb.device(&self.spec.serial);
        let mut bridge = match AdbBridge::open(adb, listener.port()).await {
            Ok(bridge) => bridge,
            Err(fault) => return failed(fault),
        };
        let driven = self.drive(listener, &mut bridge, shared, stop_rx).await;
        Attempt {
            driven,
            bridge: Some(bridge),
        }
    }
}

/// A wait as people say it: `2 min`, `90 s`.
pub(super) fn spoken(wait: Duration) -> String {
    match wait.as_secs() {
        secs if secs >= 60 && secs % 60 == 0 => format!("{} min", secs / 60),
        secs => format!("{secs} s"),
    }
}
