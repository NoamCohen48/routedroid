//! Device inventory: `adb devices` merged with the live connections, plus
//! the two background tickers (device changes, traffic counters).

use std::sync::Arc;
use std::time::Duration;

use routedroid_ipc::fault::Result;
use routedroid_ipc::{ConnectionState, DeviceInfo, Event};

use super::Daemon;
use crate::adb::DeviceState;
use crate::device::Transport;

const DEVICE_POLL: Duration = Duration::from_secs(2);
const TRAFFIC_TICK: Duration = Duration::from_secs(1);

impl Daemon {
    /// Every device adb knows about, with its connection's state on it.
    pub async fn devices(&self) -> Result<Vec<DeviceInfo>> {
        let devices = self.adb.devices().await?;
        let connections = self.connections.states().await;
        Ok(devices
            .into_iter()
            .map(|device| DeviceInfo {
                unusable_reason: unusable(&device.state, &device.serial).map(str::to_string),
                connection: connections.get(&device.serial).cloned(),
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

    /// Publish counters for every Active connection once a second.
    pub async fn watch_traffic(self: Arc<Self>) {
        loop {
            tokio::time::sleep(TRAFFIC_TICK).await;
            let active = self.connections.info().await.into_iter().filter(|c| c.state == ConnectionState::Active);
            for connection in active {
                self.events.publish(Event::Traffic {
                    serial: connection.serial,
                    packets_to_phone: connection.packets_to_phone,
                    packets_from_phone: connection.packets_from_phone,
                });
            }
        }
    }
}

/// None when this device can be connected; otherwise the reason.
fn unusable(state: &DeviceState, serial: &str) -> Option<&'static str> {
    match state {
        DeviceState::Unauthorized => Some("USB debugging not authorized on the phone"),
        DeviceState::Offline => Some("device is offline"),
        DeviceState::Other(_) => Some("device is not ready"),
        DeviceState::Device => Transport::classify(serial).refusal(false),
    }
}
