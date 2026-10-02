//! The replyless messages: RELEASE when done with a lease, DECLINE when
//! its address turns out to be in use.

use anyhow::Result;
use tracing::{info, warn};

use super::{Bound, Client};
use crate::dhcp::{self, CLIENT_PORT, Identity, SERVER_PORT};
use crate::identity::ClientId;
use crate::lease::Lease;
use crate::packet;
use crate::random;
use crate::sock;

impl Client {
    /// RELEASE, unicast to the server; no reply is expected.
    pub async fn release(&mut self, lease: &Lease) -> Result<()> {
        let msg = dhcp::release(
            &self.identity,
            random::xid()?,
            lease.address,
            lease.server_id,
        );
        info!(address = %lease.address, server = %lease.server_id, "RELEASE (unicast)");
        self.send(&msg, &lease.server_mac()?, (lease.address, lease.server_id))
            .await?;
        self.link.answer_for(None);
        Ok(())
    }

    /// DECLINE: the address is in use; the server should not offer it again.
    pub async fn decline(&mut self, bound: &Bound) -> Result<()> {
        let lease = &bound.lease;
        let msg = dhcp::decline(
            &self.identity,
            random::xid()?,
            lease.address,
            lease.server_id,
        );
        warn!(address = %lease.address, server = %lease.server_id, "DECLINE");
        self.link.answer_for(None);
        self.broadcast(&msg).await
    }
}

/// RELEASE `lease` without a runtime or a client: open, send, close. For
/// undoing a lease after a crash, when only its record is left.
pub fn release_now(lease: &Lease) -> Result<()> {
    let client_id = ClientId::parse(&lease.client_id)?;
    let server_mac = lease.server_mac()?;
    let xid = random::xid()?;
    sock::send_once(&lease.iface, |iface| {
        let id = Identity {
            mac: iface.mac,
            client_id,
        };
        let msg = dhcp::release(&id, xid, lease.address, lease.server_id);
        let ips = (lease.address, lease.server_id);
        packet::ipv4_udp_frame(
            &iface.mac,
            &server_mac,
            ips,
            (CLIENT_PORT, SERVER_PORT),
            &msg.encode(),
        )
    })
}
