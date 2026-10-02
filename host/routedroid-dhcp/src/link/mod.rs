//! One interface's frame I/O: the packet socket, the receive loop every
//! wait goes through, and ARP (the responder, RFC 5227 probes,
//! announcements and conflict detection). DHCP rides on top, in `client`.

use std::net::Ipv4Addr;
use std::time::Instant;

use anyhow::Result;
use tracing::{debug, info};

use crate::client::{Reply, Want, validate};
use crate::packet::{self, Mac};
use crate::sock::{Iface, PACKET_OUTGOING, PacketSocket, Received, RecvMeta};

mod arp;

pub use arp::{PROBE, Probe};

/// Who answers ARP for a leased address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpMode {
    /// This client, with the interface MAC: nothing else on the host will
    /// (a standalone lease, as the probe CLI holds).
    Respond,
    /// The host kernel, through proxy ARP for the phone's route.
    Kernel,
}

/// Something a wait was waiting for.
#[derive(Debug)]
pub enum Heard {
    Reply(Box<Reply>),
    /// Another station uses the watched address; its MAC.
    Conflict(Mac),
}

/// An address to watch for conflicts while waiting.
#[derive(Debug, Clone, Copy)]
pub struct Watch {
    pub addr: Ipv4Addr,
    /// RFC 5227 §2.1.1: while probing, another host probing for the same
    /// address counts as a conflict too.
    pub probing: bool,
}

pub struct Link {
    sock: PacketSocket,
    mode: ArpMode,
    /// The address `Respond` mode answers for, once bound.
    answering: Option<Ipv4Addr>,
    vlan_seen: bool,
    vlan_logged: bool,
    buf: Vec<u8>,
}

impl Link {
    pub fn open(iface: &str, mode: ArpMode) -> Result<Self> {
        let sock = PacketSocket::open(iface)?;
        info!(iface, ifindex = sock.iface.index, mac = %packet::fmt_mac(&sock.iface.mac), "AF_PACKET socket bound");
        Ok(Self {
            sock,
            mode,
            answering: None,
            vlan_seen: false,
            vlan_logged: false,
            buf: vec![0; 65536],
        })
    }

    pub fn iface(&self) -> &Iface {
        &self.sock.iface
    }

    pub fn vlan_seen(&self) -> bool {
        self.vlan_seen
    }

    pub async fn send(&self, frame: &[u8]) -> Result<()> {
        self.sock.send(frame).await
    }

    /// Receive until `until`: answer ARP, watch for conflicts, and return
    /// the first DHCP reply `want` accepts. `None` at the deadline.
    pub async fn wait(
        &mut self,
        until: Instant,
        want: Option<&Want<'_>>,
        watch: Option<Watch>,
    ) -> Result<Option<Heard>> {
        loop {
            let received = tokio::select! {
                r = self.sock.recv(&mut self.buf) => r?,
                () = tokio::time::sleep_until(until.into()) => return Ok(None),
            };
            let (n, meta) = match received {
                Received::Frame(n, meta) if meta.pkttype != PACKET_OUTGOING => (n, meta),
                Received::Frame(..) => continue,
                Received::Truncated(len) => {
                    debug!(len, "oversized frame dropped");
                    continue;
                }
            };
            self.note_meta(&meta);
            let frame = &self.buf[..n];
            if let Some(a) = packet::parse_arp(frame) {
                if let Some(mac) = self.on_arp(&a, watch).await? {
                    return Ok(Some(Heard::Conflict(mac)));
                }
            } else if let Some(want) = want
                && let Some(reply) = validate(frame, &meta, want, &self.sock.iface.mac)
            {
                return Ok(Some(Heard::Reply(Box::new(reply))));
            }
        }
    }

    fn note_meta(&mut self, meta: &RecvMeta) {
        if self.vlan_logged && (meta.vlan.is_none() || self.vlan_seen) {
            return;
        }
        self.vlan_logged = true;
        match meta.vlan {
            Some((tci, tpid)) => {
                self.vlan_seen = true;
                let tpid = tpid.map_or_else(|| "n/a".into(), |t| format!("{t:#06x}"));
                info!(
                    vid = tci & 0x0fff,
                    pcp = tci >> 13,
                    tpid,
                    "PACKET_AUXDATA: VLAN tag present on received frames"
                );
            }
            None => info!(
                aux = meta.aux_present,
                "PACKET_AUXDATA: no VLAN tag on received frames"
            ),
        }
    }
}

#[cfg(test)]
mod tests;
