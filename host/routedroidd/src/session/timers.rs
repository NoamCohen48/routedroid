//! The driver's two clocks: keepalive (§5.1) and the pre-Active phase deadline.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use routedroid_proto::state::State;
use tokio::time::Instant;

/// §5.1: PING after this much silence, dead after `KEEPALIVE_DEAD`.
pub const KEEPALIVE_IDLE: Duration = Duration::from_secs(10);
pub const KEEPALIVE_DEAD: Duration = Duration::from_secs(30);
/// Connected → Configuring must complete within this (anyone on the phone can
/// connect to the reverse port; a silent peer must not hold the TUN).
pub const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(15);
/// Configuring → Active includes the user answering the VPN consent dialog.
pub const CONSENT_DEADLINE: Duration = Duration::from_secs(120);

/// When the reader last received a frame; written per frame by the reader
/// task, read by the driver when its keepalive timer fires.
pub struct LastRx {
    base: Instant,
    nanos: AtomicU64,
}

impl LastRx {
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            nanos: AtomicU64::new(0),
        }
    }

    pub fn touch(&self) {
        let nanos = u64::try_from(self.base.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.nanos.fetch_max(nanos, Ordering::Relaxed);
    }

    pub fn get(&self) -> Instant {
        self.base + Duration::from_nanos(self.nanos.load(Ordering::Relaxed))
    }
}

/// Silence detector: one PING after `KEEPALIVE_IDLE` of silence, dead at
/// `KEEPALIVE_DEAD`. Anything received counts as life, packets included.
pub struct Keepalive {
    last_rx: Arc<LastRx>,
    pinged_at: Option<Instant>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Idle {
    /// Something arrived since the deadline was computed.
    Wait,
    SendPing,
    Dead,
}

impl Keepalive {
    pub fn new(last_rx: Arc<LastRx>) -> Self {
        Self {
            last_rx,
            pinged_at: None,
        }
    }

    fn pinged_since_rx(&self, last: Instant) -> bool {
        self.pinged_at.is_some_and(|pinged| pinged >= last)
    }

    /// When to act next if nothing arrives.
    pub fn deadline(&self) -> Instant {
        let last = self.last_rx.get();
        last + if self.pinged_since_rx(last) {
            KEEPALIVE_DEAD
        } else {
            KEEPALIVE_IDLE
        }
    }

    /// The timer fired; decide against the latest receive time.
    pub fn on_idle(&mut self) -> Idle {
        let now = Instant::now();
        if now < self.deadline() {
            Idle::Wait
        } else if self.pinged_since_rx(self.last_rx.get()) {
            Idle::Dead
        } else {
            self.pinged_at = Some(now);
            Idle::SendPing
        }
    }
}

/// How long the peer may take to leave the current pre-Active phase. The
/// handshake states share one budget; the driver restarts the timer only
/// when the budget changes (on entering Configuring).
pub struct PhaseTimer {
    started: Instant,
}

impl PhaseTimer {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }

    pub fn reset(&mut self) {
        self.started = Instant::now();
    }

    pub fn budget(state: State) -> Option<Duration> {
        match state {
            State::Connected | State::Authenticating | State::Negotiated => {
                Some(HANDSHAKE_DEADLINE)
            }
            State::Configuring => Some(CONSENT_DEADLINE),
            State::Active | State::Closed => None,
        }
    }

    /// `None` once the session is Active: nothing to wait for any more.
    pub fn deadline(&self, state: State) -> Option<Instant> {
        Self::budget(state).map(|d| self.started + d)
    }
}

#[cfg(test)]
mod tests;
