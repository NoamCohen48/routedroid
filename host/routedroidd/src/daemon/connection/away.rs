//! While the phone is away: telling an unplug apart from the app closing
//! the connection, and holding the host side until the phone is back.

use std::time::Duration;

use routedroid_ipc::{ConnectionState, NetworkInfo};
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, sleep, sleep_until};
use tracing::info;

use super::resume::spoken;
use super::run::ConnectionRun;
use crate::adb::DeviceState;
use crate::fault::{Fault, Kind, Result};
use crate::host_network::{HelperEvent, HostNetwork};

/// adb may learn of an unplug a moment after the phone's socket closed.
const SETTLE: Duration = Duration::from_secs(3);
const SETTLE_STEP: Duration = Duration::from_millis(500);

impl ConnectionRun {
    /// Whether adb no longer has the phone as a usable device. One that
    /// stays listed for `SETTLE` is there: its app closed the connection.
    pub(super) async fn phone_gone(&self) -> bool {
        let settled = Instant::now() + SETTLE;
        loop {
            // adb itself failing is the phone being out of reach too.
            if self.devices.refresh().await.is_err() || !self.attached() {
                return true;
            }
            if Instant::now() >= settled {
                return false;
            }
            sleep(SETTLE_STEP).await;
        }
    }

    fn attached(&self) -> bool {
        let device = self.devices.get(&self.spec.serial);
        device.is_some_and(|d| d.state == DeviceState::Device)
    }

    /// Hold the host side until the phone is back (`Ok(true)`) or `stop`
    /// (`Ok(false)`). The helper ending the session, or `until` passing,
    /// ends the connection.
    pub(super) async fn await_return(
        &self,
        until: Instant,
        wait: Duration,
        network: &mut HostNetwork,
        placed: &mut NetworkInfo,
        events: &mut mpsc::Receiver<HelperEvent>,
        stop_rx: &mut watch::Receiver<bool>,
    ) -> Result<bool> {
        let left = until.saturating_duration_since(Instant::now());
        info!(serial = %self.spec.serial, ?left, "the phone went away; holding its address");
        // Whole seconds, rounded up: `--reconnect-wait 20s` reads as 20 s.
        let wait_secs = left.as_secs() + u64::from(left.subsec_nanos() > 0);
        self.sink.set(ConnectionState::Reconnecting { wait_secs });
        let mut changes = self.devices.changes();
        let mut ended = None;
        loop {
            if self.attached() {
                info!(serial = %self.spec.serial, "the phone is back");
                return Ok(true);
            }
            tokio::select! {
                changed = changes.changed() => if changed.is_err() {
                    return Err(Fault::msg(Kind::Internal, "the device list is gone"));
                },
                _ = stop_rx.changed() => return Ok(false),
                Some(event) = events.recv() => {
                    self.on_helper(event, placed, &mut ended);
                    if let Some(why) = ended.take() {
                        network.ended_by_helper();
                        let message = format!("the helper ended the session: {why}");
                        return Err(Fault::msg(Kind::Helper, message));
                    }
                }
                () = sleep_until(until) => {
                    let message = format!("the phone went away and was not back within {}", spoken(wait));
                    return Err(Fault::msg(Kind::Adb, message));
                }
            }
        }
    }
}
