//! Device inventory: `adb devices` merged with live sessions, plus the two
//! background tickers (device changes, traffic counters).

use std::sync::Arc;
use std::time::Duration;

use routedroid_ipc::fault::Result;
use routedroid_ipc::{DeviceInfo, Event, SessionState};

use super::Daemon;
use crate::adb::DeviceState;
use crate::device::Transport;

const DEVICE_POLL: Duration = Duration::from_secs(2);
const TRAFFIC_TICK: Duration = Duration::from_secs(1);

impl Daemon {
    /// Every device adb knows about, with this daemon's session state on it.
    pub async fn devices(&self) -> Result<Vec<DeviceInfo>> {
        let devices = self.adb.devices().await?;
        let sessions = self.sessions.states().await;
        Ok(devices
            .into_iter()
            .map(|device| DeviceInfo {
                unusable_reason: unusable(&device.state, &device.serial).map(str::to_string),
                session: sessions.get(&device.serial).cloned(),
                state: match &device.state {
                    DeviceState::Other(other) => other.clone(),
                    state => format!("{state:?}").to_lowercase(),
                },
                serial: device.serial,
                model: device.model,
            })
            .collect())
    }

    /// Poll adb and publish `Event::Devices` whenever the inventory changes.
    pub async fn watch_devices(self: Arc<Self>) {
        let mut last: Option<Vec<DeviceInfo>> = None;
        loop {
            if let Ok(now) = self.devices().await {
                if last.as_ref() != Some(&now) {
                    self.events.publish(Event::Devices { devices: now.clone() });
                    last = Some(now);
                }
            }
            tokio::time::sleep(DEVICE_POLL).await;
        }
    }

    /// Publish counters for every Active session once a second.
    pub async fn watch_traffic(self: Arc<Self>) {
        loop {
            tokio::time::sleep(TRAFFIC_TICK).await;
            for session in self.sessions.info().await.into_iter().filter(|s| s.state == SessionState::Active) {
                self.events.publish(Event::Traffic {
                    serial: session.serial,
                    packets_to_phone: session.packets_to_phone,
                    packets_from_phone: session.packets_from_phone,
                });
            }
        }
    }
}

/// None when a session can be started on this device; otherwise the reason.
fn unusable(state: &DeviceState, serial: &str) -> Option<&'static str> {
    match state {
        DeviceState::Unauthorized => Some("USB debugging not authorized on the phone"),
        DeviceState::Offline => Some("device is offline"),
        DeviceState::Other(_) => Some("device is not ready"),
        DeviceState::Device => Transport::classify(serial).refusal(false),
    }
}
