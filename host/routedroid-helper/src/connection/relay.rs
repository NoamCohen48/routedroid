//! The packet path of an active session: TUN ↔ controller, until `Stop`,
//! disconnect, shutdown or an I/O failure. It never returns early with
//! `?`: every exit is an [`Ended`], so the caller always runs the undo.
//!
//! The helper is the privilege boundary, so it checks every packet itself
//! instead of relying on the firewall alone: a well-formed IPv4 header
//! (`routedroid_proto::ipv4::check`), within the MTU, and only the phone's
//! own address (source towards the TUN, destination towards the phone).
//!
//! Towards the controller, a full queue drops the packet (counted as
//! `congested`), as a full NIC queue would: waiting would stop the loop
//! from reading the controller, and a controller that writes before it
//! reads would deadlock both directions.

use std::net::Ipv4Addr;

use routedroid_helper_ipc::{Datagram, ErrorCode, Reply, Request, SeqPacket, MAX_DATAGRAM};
use routedroid_proto::ipv4;
use tokio::sync::watch;
use tracing::{debug, info};

use crate::kernel::AsyncTun;

#[derive(Debug, PartialEq, Eq)]
pub enum Ended {
    Stop,
    Disconnected,
    Shutdown,
    Failed(String),
}

pub struct Relay<'a> {
    pub conn: &'a SeqPacket,
    pub tun: &'a AsyncTun,
    pub phone_ip: Ipv4Addr,
    pub mtu: usize,
}

#[derive(Default)]
struct Counters {
    to_phone: u64,
    to_lan: u64,
    dropped: u64,
    congested: u64,
}

impl Relay<'_> {
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) -> Ended {
        let mut from_tun = vec![0u8; self.mtu + 1];
        let mut from_conn = vec![0u8; MAX_DATAGRAM];
        let mut counters = Counters::default();
        let ended = loop {
            let step = tokio::select! {
                () = crate::serve::stopped(&mut shutdown) => Some(Ended::Shutdown),
                read = self.tun.read(&mut from_tun) => self.to_phone(read, &from_tun, &mut counters),
                recv = self.conn.recv(&mut from_conn) => self.to_lan(recv, &mut counters).await,
            };
            if let Some(ended) = step {
                break ended;
            }
        };
        let Counters {
            to_phone,
            to_lan,
            dropped,
            congested,
        } = counters;
        info!(to_phone, to_lan, dropped, congested, ?ended, "relay ended");
        ended
    }

    fn to_phone(
        &self,
        read: std::io::Result<usize>,
        buf: &[u8],
        counters: &mut Counters,
    ) -> Option<Ended> {
        let packet = match read {
            Ok(0) => return Some(Ended::Failed("TUN closed".into())),
            Ok(n) => &buf[..n],
            Err(e) => return Some(Ended::Failed(format!("TUN read: {e}"))),
        };
        if !self.acceptable(packet, 16) {
            counters.dropped += 1;
            return None;
        }
        match self.conn.try_send_packet(packet) {
            Ok(true) => counters.to_phone += 1,
            Ok(false) => counters.congested += 1,
            Err(e) => return Some(Ended::Failed(format!("send to controller: {e}"))),
        }
        None
    }

    async fn to_lan(
        &self,
        recv: std::io::Result<Option<&[u8]>>,
        counters: &mut Counters,
    ) -> Option<Ended> {
        let datagram = match recv {
            Ok(Some(datagram)) => datagram,
            Ok(None) => return Some(Ended::Disconnected),
            Err(e) => return Some(Ended::Failed(format!("receive from controller: {e}"))),
        };
        let reply = match Datagram::<Request>::decode(datagram) {
            Ok(Datagram::Packet(packet)) if self.acceptable(packet, 12) => {
                if let Err(e) = self.tun.write(packet).await {
                    return Some(Ended::Failed(format!("TUN write: {e}")));
                }
                counters.to_lan += 1;
                return None;
            }
            Ok(Datagram::Packet(_)) => {
                counters.dropped += 1;
                return None;
            }
            Ok(Datagram::Control(Request::Stop)) => return Some(Ended::Stop),
            Ok(Datagram::Control(Request::Ping)) => Reply::Pong,
            Ok(Datagram::Control(_)) => super::error(ErrorCode::OutOfState, "a session is active"),
            Err(e) => super::error(ErrorCode::BadRequest, e.to_string()),
        };
        match self.conn.send_control(&reply).await {
            Ok(()) => None,
            Err(e) => Some(Ended::Failed(format!("send to controller: {e}"))),
        }
    }

    /// Well-formed, within the MTU, and the address at `offset` (12 for the
    /// source, 16 for the destination) is the phone's.
    fn acceptable(&self, packet: &[u8], offset: usize) -> bool {
        if packet.len() > self.mtu {
            debug!(len = packet.len(), "dropping an oversized packet");
            return false;
        }
        if let Err(e) = ipv4::check(packet) {
            debug!(error = %e, "dropping a malformed packet");
            return false;
        }
        packet[offset..offset + 4] == self.phone_ip.octets()
    }
}
