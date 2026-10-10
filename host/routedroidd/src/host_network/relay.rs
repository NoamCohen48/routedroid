//! The packet path to the helper: injection straight into the socket, and
//! one receive task, for the helper connection's whole life, that sorts what
//! comes back into packets for the current protocol session, replies to our
//! requests, and what the helper says unasked. A connection outlives its
//! sessions (a phone that reconnects gets a new one), so the task does too.

use std::sync::{Arc, Mutex, MutexGuard};

use routedroid_helper_ipc::{Datagram, ErrorCode, Lease, MAX_DATAGRAM, Reply};
use tokio::sync::mpsc;
use tracing::warn;

use super::client::HostNetwork;
use crate::session::{Inject, PacketEndpoints, QUEUE_DEPTH};

/// What the helper says during a session without being asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperEvent {
    Renewed(Lease),
    /// The helper ended the session (it lost the phone's address, its TUN
    /// failed) and has undone it; the packet channel closes next.
    Ended(String),
}

/// Where packets from the TUN go: the current session's downlink, if any.
/// Once the helper closed, no session gets any: their channels close at once.
#[derive(Default)]
pub struct Downlink {
    to_session: Option<mpsc::Sender<Vec<u8>>>,
    closed: bool,
}

type Slot = Arc<Mutex<Downlink>>;

impl HostNetwork {
    /// Start the receive task; returns what the helper says unasked. Once
    /// per helper connection. The task ends when the helper closes, or once
    /// `stop` and the last session are both done with it.
    pub fn listen(&mut self) -> mpsc::Receiver<HelperEvent> {
        assert!(self.control_rx.is_none(), "listen() runs once");
        let (control_tx, control_rx) = mpsc::channel::<Reply>(4);
        let (events_tx, events_rx) = mpsc::channel::<HelperEvent>(4);
        self.control_rx = Some(control_rx);
        let conn = self.conn.clone();
        let slot = self.downlink.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; MAX_DATAGRAM];
            while let Ok(Some(datagram)) = conn.recv(&mut buf).await {
                match Datagram::<Reply>::decode(datagram) {
                    // Never wait on the session: a full queue drops, as the helper
                    // does; with no session the packet has nowhere to go, but
                    // reading goes on so the Stop ack still gets through.
                    Ok(Datagram::Packet(packet)) => {
                        if let Some(tx) = &lock(&slot).to_session {
                            let _ = tx.try_send(packet.to_vec());
                        }
                    }
                    Ok(Datagram::Control(Reply::Lease { lease })) => {
                        let _ = events_tx.try_send(HelperEvent::Renewed(lease));
                    }
                    Ok(Datagram::Control(Reply::Error {
                        code: ErrorCode::SessionEnded,
                        message,
                    })) => {
                        let _ = events_tx.try_send(HelperEvent::Ended(message));
                    }
                    Ok(Datagram::Control(reply)) => {
                        let _ = control_tx.try_send(reply);
                    }
                    Err(e) => warn!(error = %e, "undecodable datagram from helper"),
                }
                // Nobody left to deliver to (Stop acked or given up, no live
                // session): drop our clone so the helper sees its socket close.
                let session_gone = lock(&slot)
                    .to_session
                    .as_ref()
                    .is_none_or(|tx| tx.is_closed());
                if control_tx.is_closed() && session_gone {
                    break;
                }
            }
            // The session's downlink ends with the helper connection.
            let mut downlink = lock(&slot);
            downlink.to_session = None;
            downlink.closed = true;
        });
        events_rx
    }

    /// A protocol session's packet endpoints: injection straight into the
    /// socket (never waiting; a full helper queue drops), and the packets
    /// from the TUN, from now on. Replaces the previous session's.
    pub fn packets(&mut self) -> PacketEndpoints {
        let (to_session, from_helper) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
        let mut downlink = lock(&self.downlink);
        if !downlink.closed {
            downlink.to_session = Some(to_session);
        }
        drop(downlink);
        let conn = self.conn.clone();
        let inject: Inject = Arc::new(move |packet: &[u8]| conn.try_send_packet(packet));
        PacketEndpoints {
            inject,
            from_helper,
        }
    }
}

/// Every use is a read or a few assignments; a panic elsewhere leaves it whole.
fn lock(slot: &Slot) -> MutexGuard<'_, Downlink> {
    slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
