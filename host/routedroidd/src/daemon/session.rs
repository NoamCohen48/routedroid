//! One session as the daemon sees it: a task running the connect sequence
//! and the protocol driver, a state it publishes, and a stop switch.

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use routedroid_ipc::{Event, Outcome, SessionInfo, SessionState, StartRequest};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::Daemon;
use crate::session::Counters;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

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
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
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
    /// `Starting`; failures surface as `Ended` with a non-ok outcome.
    pub fn spawn(daemon: Arc<Daemon>, req: StartRequest, tun: String) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let (state_tx, state) = watch::channel(SessionState::Starting);
        let (stop, stop_rx) = watch::channel(false);
        let counters = Arc::new(Counters::default());
        let sink = StateSink { serial: req.serial.clone(), daemon: daemon.clone(), tx: state_tx };
        let serial = req.serial.clone();
        let handle_serial = serial.clone();
        let lan_if = req.lan_if.clone();
        let phone_ip = req.phone_ip;
        let task_counters = counters.clone();
        let task_tun = tun.clone();
        let task = tokio::spawn(async move {
            sink.set(SessionState::Starting);
            let outcome = super::api::run_session(&daemon, req, task_tun, stop_rx, task_counters, &sink).await;
            sink.set(SessionState::Ended(outcome));
            // Only our own entry: `stop` may have removed it and `start` replaced it.
            let mut sessions = daemon.sessions.lock().await;
            if sessions.get(&serial).is_some_and(|h| h.id == id) {
                sessions.remove(&serial);
            }
        });
        let started_at = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        Self { id, serial: handle_serial, lan_if, phone_ip, tun, started_at, counters, state, stop, task: Some(task) }
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

    pub fn request_stop(&self) {
        let _ = self.stop.send(true);
    }

    /// Resolves with the final outcome once the task has ended.
    pub async fn wait_ended(&self) -> Outcome {
        let mut rx = self.state.clone();
        loop {
            if let SessionState::Ended(o) = &*rx.borrow() {
                return o.clone();
            }
            if rx.changed().await.is_err() {
                return Outcome { ok: false, kind: None, message: "session task vanished".into() };
            }
        }
    }

    pub async fn stop_and_wait(mut self) -> Outcome {
        self.request_stop();
        let outcome = tokio::time::timeout(Duration::from_secs(20), self.wait_ended()).await;
        if let Some(task) = self.task.take() {
            if outcome.is_err() {
                task.abort();
            }
        }
        outcome.unwrap_or_else(|_| Outcome { ok: false, kind: None, message: "session did not stop in time".into() })
    }
}
