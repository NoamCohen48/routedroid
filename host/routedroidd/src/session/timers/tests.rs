use super::*;

#[tokio::test(start_paused = true)]
async fn keepalive_pings_once_then_declares_dead() {
    let last_rx = Arc::new(LastRx::new());
    let mut k = Keepalive::new(last_rx.clone());
    assert_eq!(k.on_idle(), Idle::Wait, "fired early");
    tokio::time::advance(KEEPALIVE_IDLE).await;
    assert_eq!(k.on_idle(), Idle::SendPing);
    assert_eq!(k.deadline(), last_rx.get() + KEEPALIVE_DEAD);
    tokio::time::advance(KEEPALIVE_DEAD - KEEPALIVE_IDLE).await;
    assert_eq!(k.on_idle(), Idle::Dead);
}

#[tokio::test(start_paused = true)]
async fn any_receive_after_the_ping_revives() {
    let last_rx = Arc::new(LastRx::new());
    let mut k = Keepalive::new(last_rx.clone());
    tokio::time::advance(KEEPALIVE_IDLE).await;
    assert_eq!(k.on_idle(), Idle::SendPing);
    tokio::time::advance(Duration::from_secs(1)).await;
    last_rx.touch();
    // Short of a full idle period since that receive.
    tokio::time::advance(Duration::from_secs(5)).await;
    assert_eq!(k.on_idle(), Idle::Wait);
    assert_eq!(k.deadline(), last_rx.get() + KEEPALIVE_IDLE);
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
