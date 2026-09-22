//! One device connection as the daemon sees it: a task running the connect
//! sequence and the protocol driver, a state it publishes, and a stop
//! switch. The handle is what the daemon keeps; the task outlives any
//! client that asked for it.

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use routedroid_ipc::{ConnectionInfo, ConnectionState, Event, Outcome, StartRequest};
use tokio::sync::watch;

mod drive;
pub(super) mod run;

use super::context::ConnectionContext;
use crate::session::Counters;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Longer than the worst orderly teardown (adb timeout 15 s + helper ack 10 s).
const STOP_WAIT: Duration = Duration::from_secs(30);

pub struct DeviceConnection {
    /// Distinguishes this handle from a later connection on the same serial.
    id: u64,
    pub serial: String,
    pub lan_if: String,
    pub phone_ip: Ipv4Addr,
    pub tun: String,
    pub started_at: u64,
    pub counters: Arc<Counters>,
    state: watch::Receiver<ConnectionState>,
    stop: Arc<watch::Sender<bool>>,
}

/// Publishes state changes to the handle's watch and to the event bus.
pub(super) struct StateSink {
    serial: String,
    events: super::events::EventBus,
    tx: watch::Sender<ConnectionState>,
}

impl StateSink {
    pub fn set(&self, state: ConnectionState) {
        tracing::info!(serial = %self.serial, ?state, "connection state");
        let _ = self.tx.send(state.clone());
        self.events.publish(Event::Connection { serial: self.serial.clone(), state });
    }
}

impl DeviceConnection {
    /// Spawn the connection's task. The handle is live immediately in state
    /// `Starting`; failures surface as `Ended` with a non-ok outcome. The task
    /// is never aborted: teardown (adb, helper) must always run to the end.
    pub fn spawn(context: ConnectionContext, req: StartRequest, tun: String) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let (state_tx, state) = watch::channel(ConnectionState::Starting);
        let (stop, stop_rx) = watch::channel(false);
        let stop = Arc::new(stop);
        let counters = Arc::new(Counters::default());
        let sink = StateSink { serial: req.serial.clone(), events: context.events.clone(), tx: state_tx };
        let serial = req.serial.clone();
        let handle_serial = serial.clone();
        let lan_if = req.lan_if.clone();
        let phone_ip = req.phone_ip;
        let task_counters = counters.clone();
        let task_tun = tun.clone();
        tokio::spawn(async move {
            sink.set(ConnectionState::Starting);
            let run = run::ConnectionRun::new(&context, req, task_tun, task_counters, &sink);
            let outcome = run.run(stop_rx).await;
            // Leave the table before announcing the end, so a client reacting
            // to `Ended` with a new `start` finds the serial free.
            context.connections.remove(&serial, id).await;
            sink.set(ConnectionState::Ended(outcome));
        });
        let started_at = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        Self { id, serial: handle_serial, lan_if, phone_ip, tun, started_at, counters, state, stop }
    }

    /// Distinguishes this handle from a later connection on the same serial.
    pub(super) fn id(&self) -> u64 {
        self.id
    }

    pub fn state(&self) -> ConnectionState {
        self.state.borrow().clone()
    }

    pub fn info(&self) -> ConnectionInfo {
        ConnectionInfo {
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
    /// without holding the connection table locked (the task needs that lock).
    pub fn stop_switch(&self) -> Arc<watch::Sender<bool>> {
        self.stop.clone()
    }

    pub fn state_watch(&self) -> watch::Receiver<ConnectionState> {
        self.state.clone()
    }

    /// Resolves with the final outcome once the task has ended.
    pub async fn wait_ended(mut state: watch::Receiver<ConnectionState>) -> Outcome {
        loop {
            if let ConnectionState::Ended(outcome) = &*state.borrow() {
                return outcome.clone();
            }
            if state.changed().await.is_err() {
                return Outcome { ok: false, kind: None, message: "connection task vanished".into() };
            }
        }
    }

    /// Ask the connection to stop and wait for it; `None` if it is still tearing
    /// down after `STOP_WAIT` (it keeps going; a later `Ended` event tells).
    pub async fn stop_and_wait_on(
        stop: &watch::Sender<bool>,
        state: watch::Receiver<ConnectionState>,
    ) -> Option<Outcome> {
        let _ = stop.send(true);
        tokio::time::timeout(STOP_WAIT, Self::wait_ended(state)).await.ok()
    }

    pub async fn stop_and_wait(&self) -> Option<Outcome> {
        Self::stop_and_wait_on(&self.stop, self.state.clone()).await
    }
}
