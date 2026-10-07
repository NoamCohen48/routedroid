//! One device connection as the daemon sees it: a task running the connect
//! sequence and the protocol driver, the state and network it publishes, and
//! a stop switch. The handle is what the daemon keeps; the task outlives any
//! client that asked for it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use routedroid_ipc::{ConnectionInfo, ConnectionState, NetworkInfo, Outcome, Traffic};
use tokio::sync::watch;

mod app;
mod away;
mod drive;
mod end;
mod resume;
mod run;
mod sink;

use super::connections::DeviceConnections;
use super::spec::ConnectionSpec;
use crate::fault::{Fault, Kind, Result};
use crate::session::Counters;
use sink::StateSink;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Longer than the worst orderly teardown (adb timeout 15 s + helper ack 10 s).
const STOP_WAIT: Duration = Duration::from_secs(30);

pub struct DeviceConnection {
    /// Distinguishes this handle from a later connection on the same serial.
    id: u64,
    spec: Arc<ConnectionSpec>,
    counters: Arc<Counters>,
    state: watch::Receiver<ConnectionState>,
    network: watch::Receiver<Option<NetworkInfo>>,
    stop: Arc<watch::Sender<bool>>,
}

/// What a caller needs to stop a connection and wait for its end, without
/// holding the connection table (the task needs it to leave).
pub struct Stopper {
    stop: Arc<watch::Sender<bool>>,
    state: watch::Receiver<ConnectionState>,
    serial: String,
}

impl DeviceConnection {
    /// Spawn the connection's task. The handle is live immediately in state
    /// `Starting`; failures surface as `Ended` with a failed outcome. The task
    /// is never aborted: teardown (adb, helper) must always run to the end.
    pub fn spawn(owner: &DeviceConnections, spec: ConnectionSpec) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let spec = Arc::new(spec);
        let (sink, (state, network)) = StateSink::new(spec.serial.clone(), owner.events.clone());
        let (stop, stop_rx) = watch::channel(false);
        let counters = Arc::new(Counters::default());
        let run = run::ConnectionRun::new(owner, spec.clone(), counters.clone(), sink);
        let connections = owner.clone();
        tokio::spawn(async move {
            let (outcome, sink) = run.run(stop_rx).await;
            // Leave the table before announcing the end, so a client reacting
            // to `Ended` with a new `start` finds the serial free.
            connections.remove(&sink.serial, id);
            sink.set(ConnectionState::Ended { outcome });
        });
        let stop = Arc::new(stop);
        Self {
            id,
            spec,
            counters,
            state,
            network,
            stop,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn spec(&self) -> &ConnectionSpec {
        &self.spec
    }

    /// The phone's address: requested, or leased once the helper has one.
    pub fn phone_ip(&self) -> Option<std::net::Ipv4Addr> {
        self.spec
            .phone_ip
            .or_else(|| self.network.borrow().as_ref().map(|n| n.phone_ip))
    }

    pub fn state(&self) -> ConnectionState {
        self.state.borrow().clone()
    }

    pub fn traffic(&self) -> Traffic {
        traffic(&self.counters)
    }

    pub fn info(&self) -> ConnectionInfo {
        let spec = &self.spec;
        ConnectionInfo {
            serial: spec.serial.clone(),
            lan_if: spec.lan_if.to_string(),
            tun: spec.tun.to_string(),
            mtu: spec.mtu,
            state: self.state(),
            started_at: spec.started_at,
            network: self.network.borrow().clone(),
            traffic: self.traffic(),
        }
    }

    pub fn stopper(&self) -> Stopper {
        Stopper {
            stop: self.stop.clone(),
            state: self.state.clone(),
            serial: self.spec.serial.clone(),
        }
    }
}

impl Stopper {
    /// Ask the connection to stop and wait for how it ended. A teardown still
    /// running after `STOP_WAIT` is a timeout; it keeps going, and its
    /// `ended` event tells the rest.
    pub async fn stop(mut self) -> Result<Outcome> {
        let _ = self.stop.send(true);
        let ended = async {
            loop {
                if let ConnectionState::Ended { outcome } = &*self.state.borrow_and_update() {
                    return outcome.clone();
                }
                if self.state.changed().await.is_err() {
                    return Outcome::failed(Kind::Internal, "connection task vanished");
                }
            }
        };
        tokio::time::timeout(STOP_WAIT, ended).await.map_err(|_| {
            let serial = &self.serial;
            Fault::msg(
                Kind::Timeout,
                format!("{serial} is still disconnecting; watch for its ended event"),
            )
        })
    }
}

fn traffic(counters: &Counters) -> Traffic {
    let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
    Traffic {
        packets_to_phone: read(&counters.to_phone),
        packets_from_phone: read(&counters.from_phone),
        bytes_to_phone: read(&counters.bytes_to_phone),
        bytes_from_phone: read(&counters.bytes_from_phone),
        dropped_malformed: read(&counters.malformed),
        dropped_congested: read(&counters.congested),
    }
}
