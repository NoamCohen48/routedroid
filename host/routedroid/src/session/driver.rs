//! Async driver: one accepted TCP stream in, the helper's packet channel on
//! the other side, keepalive (§5.1), and an orderly close.

use std::time::Duration;

use routedroid_proto::frame::{self, Frame, FrameError, MessageType};
use routedroid_proto::messages::{ErrorBody, ErrorCode};
use routedroid_proto::state::State;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use super::{Close, Machine, Outbound, SessionEnd};

/// Bounded queue depth for every packet/frame channel.
pub const QUEUE_DEPTH: usize = 256;
/// §5.1: PING after this much silence, dead after `KEEPALIVE_DEAD`.
pub const KEEPALIVE_IDLE: Duration = Duration::from_secs(10);
pub const KEEPALIVE_DEAD: Duration = Duration::from_secs(30);

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

async fn writer_task<W: AsyncWrite + Unpin>(mut wr: W, mut rx: mpsc::Receiver<Frame>) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(frame::HEADER_LEN + 65_536);
    while let Some(f) = rx.recv().await {
        buf.clear();
        f.encode_into(&mut buf);
        wr.write_all(&buf).await?;
    }
    wr.shutdown().await.ok();
    Ok(())
}

pub async fn run_session(
    stream: TcpStream,
    mut machine: Machine,
    packets: PacketEndpoints,
    mut shutdown: watch::Receiver<bool>,
) -> SessionSummary {
    let mtu = machine.mtu();
    let PacketEndpoints { to_helper, mut from_helper } = packets;
    let (mut rd, wr) = stream.into_split();
    let (out_tx, out_rx) = mpsc::channel::<Frame>(QUEUE_DEPTH);
    let writer = tokio::spawn(writer_task(wr, out_rx));

    let mut reached_active = false;
    let mut to_phone = 0u64;
    let mut from_phone = 0u64;
    let mut last_rx = tokio::time::Instant::now();
    let mut pinged = false;

    let end = loop {
        let idle = tokio::time::sleep_until(last_rx + if pinged { KEEPALIVE_DEAD } else { KEEPALIVE_IDLE });
        let frame = tokio::select! {
            biased;
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    let _ = out_tx.send(Frame::empty(MessageType::Stop)).await;
                    break SessionEnd::LocalStop;
                }
                continue;
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
            r = frame::read_frame(&mut rd, mtu) => match r {
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
                    Close::PeerError(e) => SessionEnd::PeerError(e),
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
    match tokio::time::timeout(Duration::from_millis(500), writer).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => warn!(error = %e, "TCP writer failed"),
        Ok(Err(e)) => warn!(error = %e, "TCP writer task panicked"),
        Err(_) => warn!("TCP writer did not flush within 500ms"),
    }
    info!(end = %end, "session ended");
    SessionSummary { end, reached_active, packets_to_phone: to_phone, packets_from_phone: from_phone, bad_packets: machine.bad_packets }
}
