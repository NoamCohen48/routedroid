//! Helper → phone: packets read from the TUN, framed for the writer. Its
//! own task, so a slow TCP writer delays only this direction (the helper
//! drops at its end once its queue to us fills), and nothing reaches the
//! phone before the session is Active (§5 allows no IP_PACKET earlier).

use std::sync::Arc;

use routedroid_proto::frame::Frame;
use tokio::sync::{mpsc, watch};

use super::SessionEnd;
use super::progress::Counters;

/// Runs until the helper's channel closes or the writer is gone.
pub async fn pump(
    mut from_helper: mpsc::Receiver<Vec<u8>>,
    out: mpsc::Sender<Frame>,
    mut active: watch::Receiver<bool>,
    counters: Arc<Counters>,
) -> SessionEnd {
    while !*active.borrow_and_update() {
        tokio::select! {
            changed = active.changed() => if changed.is_err() {
                // The driver is gone and aborts this task.
                std::future::pending::<()>().await;
            },
            // Not deliverable yet; the helper's closing still ends the session.
            packet = from_helper.recv() => if packet.is_none() {
                return SessionEnd::HelperClosed;
            },
        }
    }
    while let Some(packet) = from_helper.recv().await {
        let len = packet.len();
        if out.send(Frame::ip_packet(packet)).await.is_err() {
            return SessionEnd::Transport("TCP writer gone".into());
        }
        Counters::carried(&counters.to_phone, &counters.bytes_to_phone, len);
    }
    SessionEnd::HelperClosed
}
