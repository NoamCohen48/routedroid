//! What the driver reports while it runs, for whoever owns the session
//! (the daemon publishes it as status and events).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::watch;

/// Shared, lock-free counters; read by the daemon's status/traffic ticker.
#[derive(Default, Debug)]
pub struct Counters {
    pub reached_active: AtomicBool,
    pub to_phone: AtomicU64,
    pub from_phone: AtomicU64,
    pub bytes_to_phone: AtomicU64,
    pub bytes_from_phone: AtomicU64,
    /// Phone packets that failed the §6 checks (dropped, not violations).
    pub malformed: AtomicU64,
    /// Phone packets dropped because the helper's queue was full.
    pub congested: AtomicU64,
}

impl Counters {
    pub fn packets_to_phone(&self) -> u64 {
        self.to_phone.load(Ordering::Relaxed)
    }

    pub fn packets_from_phone(&self) -> u64 {
        self.from_phone.load(Ordering::Relaxed)
    }

    pub fn malformed(&self) -> u64 {
        self.malformed.load(Ordering::Relaxed)
    }

    pub fn congested(&self) -> u64 {
        self.congested.load(Ordering::Relaxed)
    }

    pub fn reached_active(&self) -> bool {
        self.reached_active.load(Ordering::Relaxed)
    }

    pub(super) fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// One packet of `len` bytes delivered in one direction.
    pub(super) fn carried(packets: &AtomicU64, bytes: &AtomicU64, len: usize) {
        packets.fetch_add(1, Ordering::Relaxed);
        bytes.fetch_add(len as u64, Ordering::Relaxed);
    }
}

pub struct Progress {
    pub counters: Arc<Counters>,
    /// Flips to `true` once, when the session reaches Active.
    pub active: watch::Sender<bool>,
}

impl Progress {
    pub fn new(counters: Arc<Counters>) -> (Self, watch::Receiver<bool>) {
        let (active, rx) = watch::channel(false);
        (Self { counters, active }, rx)
    }

    /// For callers that do not care.
    #[cfg(test)]
    pub fn detached() -> Self {
        Self::new(Arc::default()).0
    }

    pub(super) fn set_active(&self) {
        self.counters.reached_active.store(true, Ordering::Relaxed);
        let _ = self.active.send(true);
    }
}
