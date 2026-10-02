//! BOUND: RENEW at T1, REBIND at T2, give up at expiry, all on the
//! monotonic schedule; and meanwhile watch for another station claiming
//! the address (RFC 5227 §2.4), which loses the lease too.

use std::fmt;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use anyhow::Result;
use tracing::{info, warn};

use super::request::Tx;
use super::timing::{RENEW_REPLY_WAIT, RENEW_RETRY_MIN};
use super::{Bound, Client, Outcome};
use crate::link::{Heard, Watch};
use crate::packet::{Mac, fmt_mac};

#[derive(Debug)]
pub enum Event {
    /// An ACK refreshed the lease (same address, new timers).
    Renewed,
    Lost(Lost),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lost {
    Nak(String),
    Expired,
    /// The server renewed us onto another address; `Bound` now holds it.
    Moved(Ipv4Addr),
    /// Another station uses the address (it has been declined).
    Conflict(Mac),
}

impl fmt::Display for Lost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Lost::Nak(m) if m.is_empty() => f.write_str("the server refused to renew it (NAK)"),
            Lost::Nak(m) => write!(f, "the server refused to renew it (NAK: {m})"),
            Lost::Expired => f.write_str("it expired without a renewal"),
            Lost::Moved(a) => write!(f, "the server moved it to {a}"),
            Lost::Conflict(mac) => write!(f, "{} also uses the address", fmt_mac(mac)),
        }
    }
}

impl Client {
    /// Until the lease is renewed or lost. Cancel-safe: everything that
    /// must survive a dropped call lives in `bound`.
    pub async fn maintain(&mut self, bound: &mut Bound) -> Result<Event> {
        let addr = bound.lease.address;
        self.link.answer_for(Some(addr));
        loop {
            let now = Instant::now();
            let s = bound.schedule;
            if now >= s.expiry {
                warn!(address = %addr, "lease expired without a successful renewal");
                return Ok(Event::Lost(Lost::Expired));
            }
            let retry = bound.retry.unwrap_or(now);
            let (due, tx, horizon) = if now >= s.t2 {
                (retry.max(s.t2), Tx::Rebind, s.expiry)
            } else if now >= s.t1 {
                (retry.max(s.t1), Tx::Renew, s.t2)
            } else {
                (s.t1, Tx::Renew, s.t2)
            };
            if due > now {
                info!(in_secs = (due - now).as_secs(), ?tx, "BOUND: next renewal");
                let watch = Some(Watch {
                    addr,
                    probing: false,
                });
                if let Some(Heard::Conflict(mac)) =
                    self.link.wait(due.min(s.expiry), None, watch).await?
                {
                    self.decline(bound).await?;
                    return Ok(Event::Lost(Lost::Conflict(mac)));
                }
                continue;
            }
            let wait = RENEW_REPLY_WAIT.min(
                horizon
                    .saturating_duration_since(now)
                    .max(Duration::from_secs(1)),
            );
            match self.request_once(&bound.lease, tx, wait).await? {
                Outcome::Bound(renewed) => {
                    let moved = renewed.lease.address;
                    *bound = renewed;
                    if moved != addr {
                        return Ok(Event::Lost(Lost::Moved(moved)));
                    }
                    let l = &bound.lease;
                    info!(address = %l.address, lease_secs = l.lease_secs, t1 = l.t1, t2 = l.t2, "renewal ACK; lease refreshed");
                    return Ok(Event::Renewed);
                }
                Outcome::Nak(m) => {
                    warn!(message = %m, "NAK on renewal; lease lost");
                    self.link.answer_for(None);
                    return Ok(Event::Lost(Lost::Nak(m)));
                }
                Outcome::Timeout => {
                    let now = Instant::now();
                    let delay = (horizon.saturating_duration_since(now) / 2).max(RENEW_RETRY_MIN);
                    warn!(
                        retry_in = delay.as_secs(),
                        ?tx,
                        "no reply to renewal; will retry"
                    );
                    bound.retry = Some(now + delay);
                }
            }
        }
    }
}
