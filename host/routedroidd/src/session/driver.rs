//! Async driver: one accepted TCP stream, the helper's packet endpoints,
//! keepalive (§5.1) and an orderly close. Packets flow through two pumps
//! of their own (`uplink`, `downlink`); this loop wakes only for control
//! frames, timers, shutdown and the end of either pump. The protocol itself
//! is `Machine`.

use std::sync::Arc;
use std::time::Duration;

use routedroid_proto::frame::Frame;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{sleep_until, Instant};
use tracing::{info, warn};

use super::downlink;
use super::progress::Progress;
use super::timers::{Keepalive, LastRx, PhaseTimer};
use super::uplink::{reader_task, Inbound, Inject, Uplink};
use super::writer::{writer_task, QUEUE_DEPTH};
use super::{Machine, SessionEnd};

const WRITER_FLUSH: Duration = Duration::from_millis(500);

/// How packets reach the helper (`inject`) and come back from the TUN.
pub struct PacketEndpoints {
    pub inject: Inject,
    pub from_helper: mpsc::Receiver<Vec<u8>>,
}

#[derive(Debug)]
pub struct SessionSummary {
    pub end: SessionEnd,
    pub reached_active: bool,
    pub packets_to_phone: u64,
    pub packets_from_phone: u64,
    pub malformed: u64,
    pub congested: u64,
}

pub struct SessionDriver {
    pub(super) machine: Machine,
    /// Frames for the writer task.
    pub(super) out_tx: mpsc::Sender<Frame>,
    pub(super) uplink: Uplink,
    pub(super) keepalive: Keepalive,
    pub(super) phase: PhaseTimer,
    pub(super) progress: Progress,
    writer: JoinHandle<std::io::Result<()>>,
}

impl SessionDriver {
    /// Drive `machine` over `stream` until either side ends the session;
    /// `first` is a frame already read from it (the listener's HELLO).
    pub async fn run(
        stream: TcpStream,
        first: Option<Frame>,
        machine: Machine,
        packets: PacketEndpoints,
        mut shutdown: watch::Receiver<bool>,
        progress: Progress,
    ) -> SessionSummary {
        let (rd, wr) = stream.into_split();
        let (out_tx, out_rx) = mpsc::channel::<Frame>(QUEUE_DEPTH);
        let (in_tx, mut in_rx) = mpsc::channel(QUEUE_DEPTH);
        let last_rx = Arc::new(LastRx::new());
        if let Some(first) = first {
            in_tx
                .try_send(Inbound::Frame(first))
                .expect("an empty queue has room");
        }
        let uplink = Uplink {
            inject: packets.inject,
            counters: progress.counters.clone(),
        };
        let reader = tokio::spawn(reader_task(
            rd,
            machine.mtu(),
            uplink.clone(),
            last_rx.clone(),
            in_tx,
        ));
        let active_rx = progress.active.subscribe();
        let mut downlink = tokio::spawn(downlink::pump(
            packets.from_helper,
            out_tx.clone(),
            active_rx,
            progress.counters.clone(),
        ));
        let mut driver = Self {
            machine,
            out_tx,
            uplink,
            keepalive: Keepalive::new(last_rx),
            phase: PhaseTimer::new(),
            progress,
            writer: tokio::spawn(writer_task(wr, out_rx)),
        };
        let (idle, phase) = (sleep_until(Instant::now()), sleep_until(Instant::now()));
        tokio::pin!(idle, phase);
        let mut watch_shutdown = true;

        let end = loop {
            let active = driver.progress.counters.reached_active();
            idle.as_mut().reset(driver.keepalive.deadline());
            let phase_deadline = driver.phase.deadline(driver.machine.state());
            if let Some(deadline) = phase_deadline {
                phase.as_mut().reset(deadline);
            }
            let end = tokio::select! {
                r = shutdown.changed(), if watch_shutdown => match r {
                    Ok(()) if *shutdown.borrow() => Some(driver.on_shutdown().await),
                    Ok(()) => None,
                    // Sender gone (no Ctrl-C handler): stop polling this branch.
                    Err(_) => { watch_shutdown = false; None }
                },
                inbound = in_rx.recv() => driver.on_inbound(inbound).await,
                end = &mut downlink => Some(end.unwrap_or_else(|e| SessionEnd::Transport(format!("downlink: {e}")))),
                () = &mut idle, if active => driver.on_idle().await,
                () = &mut phase, if phase_deadline.is_some() => Some(driver.on_phase_deadline().await),
            };
            if let Some(end) = end {
                break end;
            }
        };
        reader.abort();
        downlink.abort();
        driver.finish(end).await
    }

    pub(super) fn writer_ended(&self) -> bool {
        self.writer.is_finished()
    }

    /// Close the writer (flushing queued frames) and report.
    async fn finish(self, end: SessionEnd) -> SessionSummary {
        let Self {
            out_tx,
            mut writer,
            progress,
            ..
        } = self;
        drop(out_tx);
        match tokio::time::timeout(WRITER_FLUSH, &mut writer).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(e))) => warn!(error = %e, "TCP writer failed"),
            Ok(Err(e)) => warn!(error = %e, "TCP writer task panicked"),
            Err(_) => {
                warn!("TCP writer did not flush within {WRITER_FLUSH:?}; abandoning it");
                writer.abort();
            }
        }
        info!(end = %end, "session ended");
        let counters = &progress.counters;
        SessionSummary {
            end,
            reached_active: counters.reached_active(),
            packets_to_phone: counters.packets_to_phone(),
            packets_from_phone: counters.packets_from_phone(),
            malformed: counters.malformed(),
            congested: counters.congested(),
        }
    }
}
