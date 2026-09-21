//! Async driver: one accepted TCP stream in, the helper's packet channel on
//! the other side, keepalive (§5.1), and an orderly close. The protocol
//! itself is `Machine`; this is the event loop around it.

use std::time::Duration;

use routedroid_proto::frame::{Frame, FrameError};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{sleep_until, Instant};
use tracing::{info, warn};

use super::tasks::{reader_task, writer_task, QUEUE_DEPTH};
use super::timers::{Keepalive, PhaseTimer};
use super::{Machine, SessionEnd};

/// Packets to inject (`to_helper`) and packets read from the TUN (`from_helper`).
pub struct PacketEndpoints {
    pub to_helper: mpsc::Sender<Vec<u8>>,
    pub from_helper: mpsc::Receiver<Vec<u8>>,
}

#[derive(Debug)]
pub struct SessionSummary {
    pub end: SessionEnd,
    pub reached_active: bool,
    pub packets_to_phone: u64,
    pub packets_from_phone: u64,
    pub bad_packets: u64,
}

#[derive(Default)]
pub(super) struct Stats {
    pub reached_active: bool,
    pub to_phone: u64,
    pub from_phone: u64,
}

pub struct SessionDriver {
    pub(super) machine: Machine,
    /// Frames for the writer task.
    pub(super) out_tx: mpsc::Sender<Frame>,
    pub(super) to_helper: mpsc::Sender<Vec<u8>>,
    pub(super) keepalive: Keepalive,
    pub(super) phase: PhaseTimer,
    pub(super) stats: Stats,
    writer: JoinHandle<std::io::Result<()>>,
}

impl SessionDriver {
    /// Drive `machine` over `stream` until either side ends the session.
    pub async fn run(
        stream: TcpStream,
        machine: Machine,
        packets: PacketEndpoints,
        mut shutdown: watch::Receiver<bool>,
    ) -> SessionSummary {
        let mtu = machine.mtu();
        let PacketEndpoints { to_helper, mut from_helper } = packets;
        let (rd, wr) = stream.into_split();
        let (out_tx, out_rx) = mpsc::channel::<Frame>(QUEUE_DEPTH);
        let (in_tx, mut in_rx) = mpsc::channel::<Result<Frame, FrameError>>(QUEUE_DEPTH);
        let reader = tokio::spawn(reader_task(rd, mtu, in_tx));
        let mut driver = Self {
            machine,
            out_tx,
            to_helper,
            keepalive: Keepalive::new(),
            phase: PhaseTimer::new(),
            stats: Stats::default(),
            writer: tokio::spawn(writer_task(wr, out_rx)),
        };
        let mut watch_shutdown = true;

        let end = loop {
            let active = driver.stats.reached_active;
            let idle = sleep_until(driver.keepalive.deadline());
            let phase_deadline = driver.phase.deadline(driver.machine.state());
            let phase = sleep_until(phase_deadline.unwrap_or_else(Instant::now));
            let end = tokio::select! {
                biased;
                r = shutdown.changed(), if watch_shutdown => match r {
                    Ok(()) if *shutdown.borrow() => Some(driver.on_shutdown().await),
                    Ok(()) => None,
                    // Sender gone (no Ctrl-C handler): stop polling this branch.
                    Err(_) => { watch_shutdown = false; None }
                },
                pkt = from_helper.recv(), if active => driver.on_helper_packet(pkt).await,
                _ = idle, if active => driver.on_idle().await,
                _ = phase, if phase_deadline.is_some() => Some(driver.on_phase_deadline().await),
                r = in_rx.recv() => driver.on_inbound(r).await,
            };
            if let Some(end) = end {
                break end;
            }
        };
        reader.abort();
        driver.finish(end).await
    }

    pub(super) fn writer_ended(&self) -> bool {
        self.writer.is_finished()
    }

    /// Close the writer (flushing queued frames) and report.
    async fn finish(self, end: SessionEnd) -> SessionSummary {
        let Self { machine, out_tx, writer, stats, .. } = self;
        drop(out_tx);
        match tokio::time::timeout(Duration::from_millis(500), writer).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(e))) => warn!(error = %e, "TCP writer failed"),
            Ok(Err(e)) => warn!(error = %e, "TCP writer task panicked"),
            Err(_) => warn!("TCP writer did not flush within 500ms"),
        }
        info!(end = %end, "session ended");
        SessionSummary {
            end,
            reached_active: stats.reached_active,
            packets_to_phone: stats.to_phone,
            packets_from_phone: stats.from_phone,
            bad_packets: machine.bad_packets,
        }
    }
}
