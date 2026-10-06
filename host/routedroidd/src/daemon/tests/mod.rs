//! The daemon's components against a fake phone (adb script plus a scripted
//! app) and a fake helper: real sockets, real adb code paths, no device.

mod fake_adb;
mod fake_app;
mod fake_helper;
mod fake_replies;

mod connections;
mod doctor;
mod leased;
mod reconnect;

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use routedroid_ipc::{ConnectionState, DnsChoice, Event, StartRequest};
use tokio::sync::broadcast;

use super::{AttachedDevices, DeviceConnections, EventBus};
use fake_adb::FakeAdb;
use fake_helper::FakeHelper;

const WAIT: Duration = Duration::from_secs(10);

/// A private directory, removed at the end of the test.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("rdd-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Lab {
    adb: FakeAdb,
    devices: AttachedDevices,
    helper: FakeHelper,
    connections: DeviceConnections,
    events: broadcast::Receiver<Event>,
    _scratch: Scratch,
}

impl Lab {
    async fn new(serials: &[&str]) -> Self {
        let scratch = Scratch::new();
        let adb = FakeAdb::new(scratch.path(), serials);
        let helper = FakeHelper::spawn(scratch.path());
        let events = EventBus::new();
        let devices = AttachedDevices::start(adb.adb()).await;
        let connections = DeviceConnections::new(
            adb.adb(),
            helper.socket.clone(),
            events.clone(),
            devices.clone(),
        );
        Self {
            adb,
            devices,
            helper,
            connections,
            events: events.subscribe(),
            _scratch: scratch,
        }
    }

    /// The next state `serial` reaches that `wanted` accepts.
    async fn until(
        &mut self,
        serial: &str,
        wanted: impl Fn(&ConnectionState) -> bool,
    ) -> ConnectionState {
        let waiting = async {
            loop {
                match self.events.recv().await.unwrap() {
                    Event::Connection { serial: s, state } if s == serial && wanted(&state) => {
                        return state;
                    }
                    _ => {}
                }
            }
        };
        tokio::time::timeout(WAIT, waiting)
            .await
            .unwrap_or_else(|_| panic!("{serial} never reached the state"))
    }

    async fn ended(&mut self, serial: &str) -> routedroid_ipc::Outcome {
        match self
            .until(serial, |s| matches!(s, ConnectionState::Ended { .. }))
            .await
        {
            ConnectionState::Ended { outcome } => outcome,
            _ => unreachable!(),
        }
    }
}

fn request(serial: &str, phone_ip: [u8; 4]) -> StartRequest {
    StartRequest {
        serial: serial.into(),
        lan_if: "lan0".into(),
        phone_ip: Some(Ipv4Addr::from(phone_ip)),
        tun: None,
        mtu: None,
        dns: DnsChoice::None,
        connect_timeout_secs: Some(10),
        reconnect_secs: None,
        allow_network_adb: false,
    }
}
