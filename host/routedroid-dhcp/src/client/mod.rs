//! The DHCP client proper, for one identity on one interface: INIT →
//! SELECTING → REQUESTING → BOUND, RENEWING/REBINDING, INIT-REBOOT,
//! DECLINE and RELEASE (architecture §6.1).
//!
//! The lease is never configured on the interface. Servers unicast
//! RENEW/REBIND replies to `ciaddr` (RFC 2131 §4.1), so somebody has to
//! answer ARP for it: this client in [`ArpMode::Respond`], the host kernel's
//! proxy ARP in [`ArpMode::Kernel`].
//!
//! Nothing here installs signal handlers or owns the process: every method
//! is a future the caller may drop, and [`Bound`] holds all the state that
//! must survive that.

use std::net::Ipv4Addr;
use std::time::Instant;

use anyhow::Result;
use tracing::info;

use crate::dhcp::{self, CLIENT_PORT, Identity, Message, SERVER_PORT};
use crate::identity::ClientId;
use crate::lease::{Lease, Schedule};
use crate::link::{ArpMode, Link};
use crate::packet::{self, BROADCAST_MAC, Mac, fmt_mac};

mod acquire;
mod maintain;
mod release;
mod request;
mod timing;
mod validate;

pub use acquire::Acquire;
pub use maintain::{Event, Lost};
pub use release::release_now;
pub use timing::mask_prefix;
pub(crate) use validate::{Reply, Want, validate};

/// A lease and when to act on it. Survives a dropped future: a renewal
/// interrupted halfway is simply sent again.
#[derive(Debug, Clone)]
pub struct Bound {
    pub lease: Lease,
    pub schedule: Schedule,
    /// When to retransmit an unanswered RENEW or REBIND.
    retry: Option<Instant>,
}

impl Bound {
    pub fn new(lease: Lease, schedule: Schedule) -> Self {
        Self {
            lease,
            schedule,
            retry: None,
        }
    }
}

/// What a transaction ended with.
#[derive(Debug)]
pub enum Outcome {
    Bound(Bound),
    Nak(String),
    Timeout,
}

pub struct Client {
    link: Link,
    identity: Identity,
    /// `identity`'s option 61 body, which replies must echo if they echo any.
    option61: Vec<u8>,
    /// Addresses a lease may never be: the host's own, other phones'.
    exclude: Vec<Ipv4Addr>,
    started: Instant,
}

impl Client {
    /// The client for `device` (a hash of the phone's serial) on `iface`.
    pub fn open(iface: &str, device: &[u8; 8], arp: ArpMode) -> Result<Self> {
        let link = Link::open(iface, arp)?;
        let client_id = ClientId::new(device, &link.iface().mac);
        Ok(Self::with(link, client_id))
    }

    /// The client that holds `lease`, on `iface`.
    pub fn for_lease(iface: &str, lease: &Lease, arp: ArpMode) -> Result<Self> {
        let client_id = lease.client_id()?;
        Ok(Self::with(Link::open(iface, arp)?, client_id))
    }

    fn with(link: Link, client_id: ClientId) -> Self {
        info!(client_id = %client_id, "DHCP identity");
        Self {
            option61: client_id.option(),
            identity: Identity {
                mac: link.iface().mac,
                client_id,
            },
            link,
            exclude: Vec::new(),
            started: Instant::now(),
        }
    }

    /// Addresses to decline if a server offers them (RFC 2131 §3.1.5: the
    /// server believes them free, but they are in use here).
    pub fn exclude(&mut self, addresses: impl IntoIterator<Item = Ipv4Addr>) {
        self.exclude.extend(addresses);
    }

    pub fn link(&mut self) -> &mut Link {
        &mut self.link
    }

    pub fn client_id(&self) -> &ClientId {
        &self.identity.client_id
    }

    fn secs(&self) -> u16 {
        u16::try_from(self.started.elapsed().as_secs()).unwrap_or(u16::MAX)
    }

    async fn send(&self, msg: &Message, dst_mac: &Mac, ips: (Ipv4Addr, Ipv4Addr)) -> Result<()> {
        let mac = self.identity.mac;
        let frame = packet::ipv4_udp_frame(
            &mac,
            dst_mac,
            ips,
            (CLIENT_PORT, SERVER_PORT),
            &msg.encode(),
        );
        info!(
            kind = %msg.message_type().map(|t| t.to_string()).unwrap_or_default(),
            xid = format_args!("{:#010x}", msg.xid),
            dst_mac = %fmt_mac(dst_mac),
            ip = %format_args!("{} -> {}", ips.0, ips.1),
            ciaddr = %msg.ciaddr,
            secs = msg.secs,
            broadcast_flag = msg.flags & dhcp::FLAG_BROADCAST != 0,
            len = frame.len(),
            "tx"
        );
        self.link.send(&frame).await
    }

    async fn broadcast(&self, msg: &Message) -> Result<()> {
        let ips = (Ipv4Addr::UNSPECIFIED, Ipv4Addr::BROADCAST);
        self.send(msg, &BROADCAST_MAC, ips).await
    }
}

#[cfg(test)]
mod tests;
