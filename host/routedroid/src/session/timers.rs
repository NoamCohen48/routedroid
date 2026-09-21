//! The driver's two clocks: keepalive (§5.1) and the pre-Active phase deadline.

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

/// Silence detector: one PING after `KEEPALIVE_IDLE`, dead at `KEEPALIVE_DEAD`.
pub struct Keepalive {
    last_rx: Instant,
    pinged: bool,
}

pub enum Idle {
    SendPing,
    Dead,
}

impl Keepalive {
    pub fn new() -> Self {
        Self { last_rx: Instant::now(), pinged: false }
    }

    /// When to act next if nothing arrives.
    pub fn deadline(&self) -> Instant {
        self.last_rx + if self.pinged { KEEPALIVE_DEAD } else { KEEPALIVE_IDLE }
    }

    /// Anything from the peer counts as life, including PONG and packets.
    pub fn on_rx(&mut self) {
        self.last_rx = Instant::now();
        self.pinged = false;
    }

    /// The deadline passed with nothing received.
    pub fn on_idle(&mut self) -> Idle {
        if self.pinged {
            Idle::Dead
        } else {
            self.pinged = true;
            Idle::SendPing
        }
    }
}

/// How long the peer may stay in the current pre-Active state; restarted on
/// every state change.
pub struct PhaseTimer {
    started: Instant,
}

impl PhaseTimer {
    pub fn new() -> Self {
        Self { started: Instant::now() }
    }

    pub fn reset(&mut self) {
        self.started = Instant::now();
    }

    pub fn budget(state: State) -> Option<Duration> {
        match state {
            State::Connected | State::Authenticating | State::Negotiated => Some(HANDSHAKE_DEADLINE),
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
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn keepalive_pings_once_then_declares_dead() {
        let mut k = Keepalive::new();
        assert_eq!(k.deadline() - Instant::now(), KEEPALIVE_IDLE);
        assert!(matches!(k.on_idle(), Idle::SendPing));
        assert_eq!(k.deadline() - Instant::now(), KEEPALIVE_DEAD);
        assert!(matches!(k.on_idle(), Idle::Dead));
        k.on_rx();
        assert!(matches!(k.on_idle(), Idle::SendPing));
    }

    #[tokio::test(start_paused = true)]
    async fn phase_deadline_follows_state_and_reset() {
        let mut p = PhaseTimer::new();
        assert_eq!(p.deadline(State::Connected), Some(Instant::now() + HANDSHAKE_DEADLINE));
        assert_eq!(p.deadline(State::Configuring), Some(Instant::now() + CONSENT_DEADLINE));
        assert_eq!(p.deadline(State::Active), None);
        tokio::time::advance(Duration::from_secs(5)).await;
        p.reset();
        assert_eq!(p.deadline(State::Negotiated), Some(Instant::now() + HANDSHAKE_DEADLINE));
    }
}
