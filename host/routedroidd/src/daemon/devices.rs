//! Device inventory: `adb devices` merged with live sessions, plus the two
//! background tickers (device changes, traffic counters).

use std::sync::Arc;
use std::time::Duration;

use routedroid_ipc::fault::Result;
use routedroid_ipc::{DeviceInfo, Event, SessionState};

use super::Daemon;
use crate::adb::DeviceState;
use crate::device::Transport;

pub async fn list(daemon: &Daemon) -> Result<Vec<DeviceInfo>> {
    let devices = daemon.adb.devices().await?;
    let sessions = daemon.sessions.lock().await;
    Ok(devices
        .into_iter()
        .map(|d| DeviceInfo {
            unusable_reason: unusable(&d.state, &d.serial).map(str::to_string),
            session: sessions.get(&d.serial).map(|s| s.state()),
            state: match &d.state {
                DeviceState::Other(s) => s.clone(),
                s => format!("{s:?}").to_lowercase(),
            },
            serial: d.serial,
            model: d.model,
        })
        .collect())
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

/// Poll adb and publish `Event::Devices` whenever the inventory changes.
pub async fn watch_devices(daemon: Arc<Daemon>) {
    let mut last: Option<Vec<DeviceInfo>> = None;
    loop {
        if let Ok(now) = list(&daemon).await {
            if last.as_ref() != Some(&now) {
                daemon.publish(Event::Devices { devices: now.clone() });
                last = Some(now);
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Publish counters for every Active session once a second.
pub async fn traffic_ticker(daemon: Arc<Daemon>) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let sessions = daemon.sessions.lock().await;
        for s in sessions.values().filter(|s| s.state() == SessionState::Active) {
            daemon.publish(Event::Traffic {
                serial: s.serial.clone(),
                packets_to_phone: s.counters.packets_to_phone(),
                packets_from_phone: s.counters.packets_from_phone(),
            });
        }
    }
}
