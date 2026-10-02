//! Keeps the daemon's picture of adb current: follow `adb track-devices -l`
//! (polling `adb devices -l` while that is unavailable), hold the latest
//! list, and say when it changed. It speaks adb's vocabulary only —
//! serial, state, model — and knows nothing about device connections, so
//! nothing here can depend on the rest of the daemon.

use std::sync::Arc;
use std::time::Duration;

use crate::fault::Result;
use tokio::sync::watch;

use super::background::Background;
use crate::adb::{Adb, Device};

const POLL: Duration = Duration::from_secs(2);

/// The attached devices as of one poll. Shared, so handing it out is a
/// refcount bump rather than a copy of the list.
pub type Snapshot = Arc<Vec<Device>>;

#[derive(Clone)]
pub struct AttachedDevices {
    adb: Adb,
    current: Arc<watch::Sender<Snapshot>>,
    /// Dropped with the last handle, which stops the polling.
    _poll: Arc<Background>,
}

impl AttachedDevices {
    /// Take a first reading, then keep polling in the background. A first
    /// reading that fails is not fatal: the list starts empty and fills in.
    pub async fn start(adb: Adb) -> Self {
        let current = Arc::new(watch::channel(Snapshot::default()).0);
        publish(&adb, &current).await;
        let poll = Background::spawn(follow(adb.clone(), Arc::clone(&current)));
        Self {
            adb,
            current,
            _poll: Arc::new(poll),
        }
    }

    /// The latest reading; costs nothing, so a client cannot make the daemon
    /// run adb by asking often.
    pub fn current(&self) -> Snapshot {
        self.current.borrow().clone()
    }

    /// Fires whenever the list changes. `watch` keeps only the newest value,
    /// which is all a device list needs: a slow reader never misses a device,
    /// it just skips the states in between.
    pub fn changes(&self) -> watch::Receiver<Snapshot> {
        self.current.subscribe()
    }

    /// Read adb now. For the moments where a push still in flight would be
    /// wrong — refusing a start on a phone that was just plugged in.
    pub async fn refresh(&self) -> Result<Snapshot> {
        let devices = self.adb.devices().await?;
        Ok(store(&self.current, devices))
    }

    pub fn get(&self, serial: &str) -> Option<Device> {
        self.current
            .borrow()
            .iter()
            .find(|device| device.serial == serial)
            .cloned()
    }
}

/// Follow adb's pushes for as long as it sends them. When tracking cannot
/// start or stops (the adb server restarted, say), fall back to one poll per
/// `POLL` and try tracking again, so the list is never more than that stale.
async fn follow(adb: Adb, current: Arc<watch::Sender<Snapshot>>) {
    loop {
        match adb.track() {
            Ok(mut tracker) => loop {
                match tracker.next().await {
                    Ok(Some(devices)) => {
                        store(&current, devices);
                    }
                    Ok(None) => break,
                    Err(fault) => {
                        tracing::debug!("tracking devices failed: {fault}");
                        break;
                    }
                }
            },
            Err(fault) => tracing::debug!("cannot track devices: {fault}"),
        }
        tokio::time::sleep(POLL).await;
        publish(&adb, &current).await;
    }
}

/// One reading, published if it differs. Failure keeps the last list: adb
/// being briefly unavailable is not the same as no devices being attached.
async fn publish(adb: &Adb, current: &watch::Sender<Snapshot>) {
    match adb.devices().await {
        Ok(devices) => {
            store(current, devices);
        }
        Err(fault) => tracing::debug!(
            kind = fault.kind().as_str(),
            "listing devices failed: {fault}"
        ),
    }
}

fn store(current: &watch::Sender<Snapshot>, devices: Vec<Device>) -> Snapshot {
    current.send_if_modified(|held| {
        if **held == devices {
            return false;
        }
        *held = Arc::new(devices);
        true
    });
    current.borrow().clone()
}
