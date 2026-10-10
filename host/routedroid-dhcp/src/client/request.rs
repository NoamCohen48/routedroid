//! Transactions from a lease-holding state: RENEW (unicast), REBIND
//! (broadcast) and INIT-REBOOT.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use anyhow::Result;
use tracing::{info, warn};

use super::timing::retry_delay;
use super::{Client, Outcome, Want};
use crate::dhcp::{self, MessageType};
use crate::lease::Lease;
use crate::link::Heard;
use crate::packet::BROADCAST_MAC;
use crate::random;

const ACK_OR_NAK: &[MessageType] = &[MessageType::Ack, MessageType::Nak];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tx {
    Renew,
    Rebind,
    InitReboot,
}

impl Client {
    /// One REQUEST and one reply wait of `wait`.
    pub(super) async fn request_once(
        &mut self,
        lease: &Lease,
        tx: Tx,
        wait: Duration,
    ) -> Result<Outcome> {
        let xid = random::xid()?;
        let (id, secs, addr) = (&self.identity, self.secs(), lease.address);
        let xid_shown = format!("{xid:#010x}");
        match tx {
            Tx::Renew => {
                info!(xid = %xid_shown, server = %lease.server_id, server_mac = %lease.server_mac, "RENEWING: unicast REQUEST with ciaddr");
                let msg = dhcp::request_renew(id, xid, secs, addr);
                self.send(&msg, &lease.server_mac()?, (addr, lease.server_id))
                    .await?;
            }
            Tx::Rebind => {
                info!(xid = %xid_shown, "REBINDING: broadcast REQUEST with ciaddr");
                let msg = dhcp::request_renew(id, xid, secs, addr);
                self.send(&msg, &BROADCAST_MAC, (addr, Ipv4Addr::BROADCAST))
                    .await?;
            }
            Tx::InitReboot => {
                info!(xid = %xid_shown, requested = %addr, "INIT-REBOOT: broadcast REQUEST with option 50");
                self.broadcast(&dhcp::request_init_reboot(id, xid, secs, addr))
                    .await?;
            }
        }
        let want = Want {
            xid,
            kinds: ACK_OR_NAK,
            client_id: &self.option61,
        };
        match self
            .link
            .wait(Instant::now() + wait, Some(&want), None)
            .await?
        {
            Some(Heard::Reply(r)) if r.kind == MessageType::Ack => {
                let bound = self.lease_from_ack(&r)?;
                if bound.lease.address != addr {
                    warn!(old = %addr, new = %bound.lease.address, "server changed our address");
                }
                Ok(Outcome::Bound(bound))
            }
            Some(Heard::Reply(r)) => Ok(Outcome::Nak(r.opts.message.unwrap_or_default())),
            Some(Heard::Conflict(_)) | None => Ok(Outcome::Timeout),
        }
    }

    /// `tx` with RFC backoff until a definite outcome or `timeout`.
    async fn request_retried(
        &mut self,
        lease: &Lease,
        tx: Tx,
        timeout: Duration,
    ) -> Result<Outcome> {
        self.link.answer_for(Some(lease.address));
        let deadline = Instant::now() + timeout;
        let mut attempt = 0;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(Outcome::Timeout);
            }
            match self
                .request_once(lease, tx, retry_delay(attempt)?.min(left))
                .await?
            {
                Outcome::Timeout => attempt += 1,
                o => return Ok(o),
            }
        }
    }

    /// Unicast RENEW of a lease this process did not acquire (read from disk).
    pub async fn renew(&mut self, lease: &Lease, timeout: Duration) -> Result<Outcome> {
        self.request_retried(lease, Tx::Renew, timeout).await
    }

    /// INIT-REBOOT: broadcast REQUEST for a remembered address, no server id.
    pub async fn init_reboot(&mut self, lease: &Lease, timeout: Duration) -> Result<Outcome> {
        self.request_retried(lease, Tx::InitReboot, timeout).await
    }
}
