//! INIT → SELECTING → REQUESTING → BOUND, with RFC 2131 backoff, and the
//! checks before a lease is used: not an address the host owns, and nobody
//! answering ARP for it (RFC 5227). A failed check DECLINEs and starts over.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use tracing::{info, warn};

use super::timing::{DECLINE_PAUSE, REQUEST_ATTEMPTS, retry_delay};
use super::{Bound, Client, Outcome, Reply, Want};
use crate::dhcp::{self, MessageType};
use crate::link::{Heard, Probe};
use crate::packet::fmt_mac;
use crate::random;

/// More conflicts than this in one acquisition is a LAN problem, not bad luck.
const MAX_DECLINES: u32 = 3;

#[derive(Debug, Clone, Copy)]
pub struct Acquire {
    pub timeout: Duration,
    /// How long to keep collecting OFFERs after the first.
    pub offer_window: Duration,
    /// ARP-probe the address before taking it; `None` skips the probe.
    pub probe: Option<Probe>,
}

impl Client {
    pub async fn acquire(&mut self, how: &Acquire) -> Result<Outcome> {
        let deadline = Instant::now() + how.timeout;
        let (mut attempt, mut declined) = (0, 0);
        while Instant::now() < deadline {
            let xid = random::xid()?;
            let Some(offer) = self
                .select(xid, attempt, deadline, how.offer_window)
                .await?
            else {
                attempt += 1;
                continue;
            };
            match self.request(xid, &offer, deadline).await? {
                Outcome::Bound(bound) => match self.check(&bound, how.probe).await? {
                    None => {
                        self.link.answer_for(Some(bound.lease.address));
                        return Ok(Outcome::Bound(bound));
                    }
                    Some(why) => {
                        warn!(address = %bound.lease.address, why, "lease unusable");
                        self.decline(&bound).await?;
                        declined += 1;
                        if declined == MAX_DECLINES {
                            bail!("declined {declined} leases in a row; the last: {why}");
                        }
                        self.pause(DECLINE_PAUSE, deadline).await?;
                    }
                },
                Outcome::Nak(m) => {
                    warn!(message = %m, "NAK in REQUESTING; backing off and restarting");
                    attempt += 1;
                    self.pause(retry_delay(attempt)?, deadline).await?;
                }
                Outcome::Timeout => attempt += 1,
            }
        }
        Ok(Outcome::Timeout)
    }

    /// DISCOVER, then the OFFERs within one retransmission interval and
    /// `window` after the first: the first usable one.
    async fn select(
        &mut self,
        xid: u32,
        attempt: u32,
        deadline: Instant,
        window: Duration,
    ) -> Result<Option<Reply>> {
        let wait = retry_delay(attempt)?.min(deadline.saturating_duration_since(Instant::now()));
        info!(attempt, xid = %format!("{xid:#010x}"), wait_secs = wait.as_secs_f64(), "SELECTING: sending DISCOVER");
        self.broadcast(&dhcp::discover(&self.identity, xid, self.secs()))
            .await?;
        let mut offers = Vec::new();
        let mut until = Instant::now() + wait;
        let want = Want {
            xid,
            kinds: &[MessageType::Offer],
            client_id: &self.option61,
        };
        while let Some(Heard::Reply(o)) = self.link.wait(until, Some(&want), None).await? {
            if o.opts.server_id.is_none() {
                warn!(yiaddr = %o.msg.yiaddr, "OFFER without server identifier; ignoring it");
                continue;
            }
            if offers.is_empty() {
                until = (Instant::now() + window).min(deadline);
            }
            info!(n = offers.len() + 1, yiaddr = %o.msg.yiaddr, server = ?o.opts.server_id, "OFFER");
            offers.push(*o);
        }
        if offers.is_empty() {
            warn!(attempt, "no OFFER received");
            return Ok(None);
        }
        // An excluded address would only be declined; prefer any other.
        let pick = offers
            .iter()
            .position(|o| !self.exclude.contains(&o.msg.yiaddr))
            .unwrap_or(0);
        Ok(Some(offers.swap_remove(pick)))
    }

    /// REQUESTING: up to [`REQUEST_ATTEMPTS`] REQUESTs for `offer`.
    async fn request(&mut self, xid: u32, offer: &Reply, deadline: Instant) -> Result<Outcome> {
        let (requested, server) = (
            offer.msg.yiaddr,
            offer.opts.server_id.unwrap_or(offer.src_ip),
        );
        for n in 0..REQUEST_ATTEMPTS {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            info!(req_attempt = n, %requested, %server, "REQUESTING: sending REQUEST");
            let msg = dhcp::request_selecting(&self.identity, xid, self.secs(), requested, server);
            self.broadcast(&msg).await?;
            let want = Want {
                xid,
                kinds: &[MessageType::Ack, MessageType::Nak],
                client_id: &self.option61,
            };
            let until = Instant::now() + retry_delay(n)?.min(left);
            match self.link.wait(until, Some(&want), None).await? {
                Some(Heard::Reply(r)) if r.kind == MessageType::Ack => {
                    return Ok(Outcome::Bound(self.lease_from_ack(&r)?));
                }
                Some(Heard::Reply(r)) => {
                    return Ok(Outcome::Nak(r.opts.message.unwrap_or_default()));
                }
                Some(Heard::Conflict(_)) | None => {}
            }
        }
        warn!("no ACK or NAK after {REQUEST_ATTEMPTS} REQUESTs; restarting from DISCOVER");
        Ok(Outcome::Timeout)
    }

    /// Why the ACKed address must not be used, if it must not.
    async fn check(&mut self, bound: &Bound, probe: Option<Probe>) -> Result<Option<String>> {
        let addr = bound.lease.address;
        if self.exclude.contains(&addr) {
            return Ok(Some(format!("{addr} is in use on this host")));
        }
        let Some(how) = probe else { return Ok(None) };
        let user = self.link.probe(addr, how).await?;
        Ok(user.map(|mac| format!("{} answers ARP for {addr}", fmt_mac(&mac))))
    }

    /// Service ARP, ignore DHCP, for `d` (at most until `deadline`).
    async fn pause(&mut self, d: Duration, deadline: Instant) -> Result<()> {
        self.link
            .wait((Instant::now() + d).min(deadline), None, None)
            .await?;
        Ok(())
    }
}
