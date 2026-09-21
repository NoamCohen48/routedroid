//! Async driver: one accepted TCP stream in, the helper's packet channel on
//! the other side, keepalive (§5.1), and an orderly close.

use std::time::Duration;

use routedroid_proto::frame::{Frame, FrameError, MessageType};
use routedroid_proto::messages::{ErrorBody, ErrorCode};
use routedroid_proto::state::State;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use super::tasks::{phase_deadline, reader_task, writer_task, KEEPALIVE_DEAD, KEEPALIVE_IDLE, QUEUE_DEPTH};
use super::{Close, Machine, Outbound, SessionEnd};

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

pub async fn run_session(
    stream: TcpStream,
    mut machine: Machine,
    packets: PacketEndpoints,
    mut shutdown: watch::Receiver<bool>,
) -> SessionSummary {
    let mtu = machine.mtu();
    let PacketEndpoints { to_helper, mut from_helper } = packets;
    let (rd, wr) = stream.into_split();
    let (out_tx, out_rx) = mpsc::channel::<Frame>(QUEUE_DEPTH);
    let writer = tokio::spawn(writer_task(wr, out_rx));
    let (in_tx, mut in_rx) = mpsc::channel::<Result<Frame, FrameError>>(QUEUE_DEPTH);
    let reader = tokio::spawn(reader_task(rd, mtu, in_tx));

    let mut reached_active = false;
    let mut to_phone = 0u64;
    let mut from_phone = 0u64;
    let mut last_rx = tokio::time::Instant::now();
    let mut pinged = false;
    let mut phase_started = tokio::time::Instant::now();
    let mut watch_shutdown = true;

    let end = loop {
        let idle = tokio::time::sleep_until(last_rx + if pinged { KEEPALIVE_DEAD } else { KEEPALIVE_IDLE });
        let phase = phase_deadline(machine.state());
        let phase_timer = tokio::time::sleep_until(phase_started + phase.unwrap_or_default());
        let frame = tokio::select! {
            biased;
            r = shutdown.changed(), if watch_shutdown => {
                match r {
                    Ok(()) if *shutdown.borrow() => {
                        let _ = out_tx.send(Frame::empty(MessageType::Stop)).await;
                        break SessionEnd::LocalStop;
                    }
                    Ok(()) => continue,
                    // Sender gone (no Ctrl-C handler): stop polling this branch.
                    Err(_) => { watch_shutdown = false; continue; }
                }
            }
            pkt = from_helper.recv(), if reached_active => match pkt {
                Some(pkt) => {
                    to_phone += 1;
                    if out_tx.send(Frame::ip_packet(pkt)).await.is_err() { break SessionEnd::Transport("writer gone".into()); }
                    continue;
                }
                None => break SessionEnd::HelperClosed,
            },
            _ = idle, if reached_active => {
                if pinged { break SessionEnd::KeepaliveTimeout; }
                pinged = true;
                let _ = out_tx.send(Frame::empty(MessageType::Ping)).await;
                continue;
            }
            _ = phase_timer, if phase.is_some() => {
                let body = ErrorBody::new(ErrorCode::ProtocolError, format!("no progress from {} within {:?}", machine.state().name(), phase.unwrap_or_default()));
                let _ = out_tx.send(Frame::json(MessageType::Error, &body)).await;
                break SessionEnd::Refused(body);
            }
            r = in_rx.recv() => match r.unwrap_or(Err(FrameError::Io("reader task ended".into()))) {
                Ok(f) => f,
                Err(FrameError::Truncated { clean: true }) => break SessionEnd::PeerClosed,
                Err(FrameError::Io(e)) => break SessionEnd::Transport(e),
                Err(e @ FrameError::Truncated { clean: false }) => break SessionEnd::Transport(e.to_string()),
                Err(e) => {
                    let body = ErrorBody::new(ErrorCode::ProtocolError, e.to_string());
                    let _ = out_tx.send(Frame::json(MessageType::Error, &body)).await;
                    break SessionEnd::Refused(body);
                }
            },
        };
        last_rx = tokio::time::Instant::now();
        pinged = false;
        let state_before = machine.state();
        if frame.message_type != MessageType::IpPacket {
            debug!(%frame.message_type, state = machine.state().name(), "control frame");
        }
        match machine.handle(frame) {
            Ok(outbound) => {
                for o in outbound {
                    match o {
                        Outbound::ToPeer(f) => {
                            if out_tx.send(f).await.is_err() {
                                break;
                            }
                        }
                        Outbound::ToHelper(pkt) => {
                            from_phone += 1;
                            // Bounded: suspends the TCP reader when the helper lags.
                            if to_helper.send(pkt).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                if machine.state() != state_before {
                    phase_started = tokio::time::Instant::now();
                }
                if machine.state() == State::Active && !reached_active {
                    reached_active = true;
                    info!("session Active");
                }
                if writer.is_finished() {
                    break SessionEnd::Transport("writer ended".into());
                }
            }
            Err(close) => {
                break match close {
                    Close::PeerStop => SessionEnd::PeerStop,
                    Close::VpnError(e) => SessionEnd::VpnError(e),
                    Close::Refuse(e) => {
                        let _ = out_tx.send(Frame::json(MessageType::Error, &e)).await;
                        SessionEnd::Refused(e)
                    }
                };
            }
        }
    };

    drop(out_tx);
    reader.abort();
    match tokio::time::timeout(Duration::from_millis(500), writer).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => warn!(error = %e, "TCP writer failed"),
        Ok(Err(e)) => warn!(error = %e, "TCP writer task panicked"),
        Err(_) => warn!("TCP writer did not flush within 500ms"),
    }
    info!(end = %end, "session ended");
    SessionSummary { end, reached_active, packets_to_phone: to_phone, packets_from_phone: from_phone, bad_packets: machine.bad_packets }
}
