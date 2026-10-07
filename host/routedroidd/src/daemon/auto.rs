//! Connecting remembered phones the moment they are ready: plugged in (or
//! attached when the daemon starts) and authorized. Only that moment counts,
//! so a phone the user disconnects stays disconnected until it is plugged in
//! again, and a phone whose connection fails is not retried in a loop.

use std::collections::HashSet;

use routedroid_ipc::StartRequest;
use tokio::sync::watch;
use tracing::{info, warn};

use super::{DeviceConnections, Snapshot};
use crate::adb::DeviceState;

pub async fn run(connections: DeviceConnections, mut changes: watch::Receiver<Snapshot>) {
    let mut ready = HashSet::new();
    loop {
        let now: HashSet<String> = changes
            .borrow_and_update()
            .iter()
            .filter(|d| d.state == DeviceState::Device)
            .map(|d| d.serial.clone())
            .collect();
        for serial in now.difference(&ready) {
            connect(&connections, serial).await;
        }
        ready = now;
        if changes.changed().await.is_err() {
            return;
        }
    }
}

async fn connect(connections: &DeviceConnections, serial: &str) {
    let Some(phone) = connections.phones().find(serial) else {
        return;
    };
    // An unplugged phone's connection waits for it and resumes by itself.
    if !phone.auto || connections.states().contains_key(serial) {
        return;
    }
    let request = StartRequest {
        serial: Some(serial.to_string()),
        lan_if: None,
        phone_ip: None,
        tun: None,
        mtu: None,
        dns: None,
        connect_timeout_secs: None,
        reconnect_secs: None,
        allow_network_adb: false,
    };
    match connections.start(request).await {
        Ok(accepted) => {
            info!(%serial, lan_if = %accepted.lan_if, "plugged in: connecting it, as remembered");
        }
        Err(error) => warn!(%serial, %error, "plugged in, but could not connect it"),
    }
}
