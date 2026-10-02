//! The session's packet path to the helper: injection straight into the
//! socket, and one receive task that sorts what comes back into packets,
//! replies to our requests, and what the helper says unasked.

use std::sync::Arc;

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

impl HostNetwork {
    /// Hand back the session's packet endpoints: injection straight into
    /// the socket (never waiting; a full helper queue drops), and a receive
    /// task for packets from the TUN and control replies. The task ends when
    /// the helper closes, or once both the session and `stop` are done with it.
    pub fn relay(&mut self) -> PacketEndpoints {
        let (from_tx, from_helper) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
        let (control_tx, control_rx) = mpsc::channel::<Reply>(4);
        let (events_tx, events_rx) = mpsc::channel::<HelperEvent>(4);
        self.control_rx = Some(control_rx);
        self.events_rx = Some(events_rx);
        let conn = self.conn.clone();
        let inject: Inject = Arc::new(move |packet: &[u8]| conn.try_send_packet(packet));
        let conn = self.conn.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; MAX_DATAGRAM];
            while let Ok(Some(datagram)) = conn.recv(&mut buf).await {
                match Datagram::<Reply>::decode(datagram) {
                    // Never wait on the session: a full queue drops, as the helper
                    // does; after the session ended the packet has nowhere to go,
                    // but reading goes on so the Stop ack still gets through.
                    Ok(Datagram::Packet(packet)) => {
                        let _ = from_tx.try_send(packet.to_vec());
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
                // Nobody left to deliver to (session over, Stop acked or given
                // up): drop our clone so the helper sees its socket close.
                if from_tx.is_closed() && control_tx.is_closed() {
                    break;
                }
            }
        });
        PacketEndpoints {
            inject,
            from_helper,
        }
    }
}
