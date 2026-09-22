//! One session as the daemon sees it: a task running the connect sequence
//! and the protocol driver, a state it publishes, and a stop switch.

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use routedroid_ipc::{Event, Outcome, SessionInfo, SessionState, StartRequest};
use tokio::sync::watch;

use super::Daemon;
use crate::session::Counters;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Longer than the worst orderly teardown (adb timeout 15 s + helper ack 10 s).
const STOP_WAIT: Duration = Duration::from_secs(30);

pub struct SessionHandle {
    /// Distinguishes this handle from a later session on the same serial.
    id: u64,
    pub serial: String,
    pub lan_if: String,
    pub phone_ip: Ipv4Addr,
    pub tun: String,
    pub started_at: u64,
    pub counters: Arc<Counters>,
    state: watch::Receiver<SessionState>,
    stop: Arc<watch::Sender<bool>>,
}

/// Publishes state changes to the handle's watch and to the event bus.
pub(super) struct StateSink {
    serial: String,
    daemon: Arc<Daemon>,
    tx: watch::Sender<SessionState>,
}

impl StateSink {
    pub fn set(&self, state: SessionState) {
        tracing::info!(serial = %self.serial, ?state, "session state");
        let _ = self.tx.send(state.clone());
        self.daemon.publish(Event::Session { serial: self.serial.clone(), state });
    }
}

impl SessionHandle {
    /// Spawn the session task. The handle is live immediately in state
    /// `Starting`; failures surface as `Ended` with a non-ok outcome. The task
    /// is never aborted: teardown (adb, helper) must always run to the end.
    pub fn spawn(daemon: Arc<Daemon>, req: StartRequest, tun: String) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let (state_tx, state) = watch::channel(SessionState::Starting);
        let (stop, stop_rx) = watch::channel(false);
        let stop = Arc::new(stop);
        let counters = Arc::new(Counters::default());
        let sink = StateSink { serial: req.serial.clone(), daemon: daemon.clone(), tx: state_tx };
        let serial = req.serial.clone();
        let handle_serial = serial.clone();
        let lan_if = req.lan_if.clone();
        let phone_ip = req.phone_ip;
        let task_counters = counters.clone();
        let task_tun = tun.clone();
        tokio::spawn(async move {
            sink.set(SessionState::Starting);
            let run = super::run::SessionRun::new(&daemon, req, task_tun, task_counters, &sink);
            let outcome = run.run(stop_rx).await;
            // Leave the map before announcing the end, so a client reacting
            // to `Ended` with a new `start` finds the serial free. Only our
            // own entry: daemon shutdown may have drained the map already.
            {
                let mut sessions = daemon.sessions().await;
                if sessions.get(&serial).is_some_and(|handle| handle.id == id) {
                    sessions.remove(&serial);
                }
            }
            sink.set(SessionState::Ended(outcome));
        });
        let started_at = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        Self { id, serial: handle_serial, lan_if, phone_ip, tun, started_at, counters, state, stop }
    }

    pub fn state(&self) -> SessionState {
        self.state.borrow().clone()
    }

    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            serial: self.serial.clone(),
            lan_if: self.lan_if.clone(),
            phone_ip: self.phone_ip,
            tun: self.tun.clone(),
            state: self.state(),
            started_at: self.started_at,
            packets_to_phone: self.counters.packets_to_phone(),
            packets_from_phone: self.counters.packets_from_phone(),
        }
    }

    /// The stop switch and state watch, so a caller can wait for the end
    /// without holding the session map locked (the task needs that lock).
    pub fn stop_switch(&self) -> Arc<watch::Sender<bool>> {
        self.stop.clone()
    }

    pub fn state_watch(&self) -> watch::Receiver<SessionState> {
        self.state.clone()
    }

    /// Resolves with the final outcome once the task has ended.
    pub async fn wait_ended(mut state: watch::Receiver<SessionState>) -> Outcome {
        loop {
            if let SessionState::Ended(outcome) = &*state.borrow() {
                return outcome.clone();
            }
            if state.changed().await.is_err() {
                return Outcome { ok: false, kind: None, message: "session task vanished".into() };
            }
        }
    }

    /// Ask the session to stop and wait for it; `None` if it is still tearing
    /// down after `STOP_WAIT` (it keeps going; a later `Ended` event tells).
    pub async fn stop_and_wait_on(stop: &watch::Sender<bool>, state: watch::Receiver<SessionState>) -> Option<Outcome> {
        let _ = stop.send(true);
        tokio::time::timeout(STOP_WAIT, Self::wait_ended(state)).await.ok()
    }

    pub async fn stop_and_wait(&self) -> Option<Outcome> {
        Self::stop_and_wait_on(&self.stop, self.state.clone()).await
    }
}
