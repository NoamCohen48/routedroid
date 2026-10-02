//! ARP on the link (RFC 5227): answering for a standalone lease, probing
//! an address before it is used, announcing it once it is, and noticing
//! another station that claims it.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use anyhow::Result;
use tracing::{debug, info, warn};

use super::{ArpMode, Heard, Link, Watch};
use crate::packet::{self, ARP_REQUEST, Arp, Mac, fmt_mac};

/// How hard to probe. RFC 5227 asks for 3 probes 1-2 s apart and 2 s of
/// listening (about 7 s); a phone connection cannot wait that long, and on
/// a LAN a live owner answers within milliseconds, so the default is short.
#[derive(Debug, Clone, Copy)]
pub struct Probe {
    pub count: u32,
    pub interval: Duration,
    /// Listening after the last probe.
    pub wait: Duration,
}

pub const PROBE: Probe = Probe {
    count: 3,
    interval: Duration::from_millis(200),
    wait: Duration::from_millis(500),
};

/// RFC 5227 §2.3: two announcements, here a second apart.
const ANNOUNCEMENTS: u32 = 2;
const ANNOUNCE_INTERVAL: Duration = Duration::from_secs(1);

impl Link {
    /// Start (or, with `None`, stop) answering ARP for `ip` in `Respond` mode.
    pub fn answer_for(&mut self, ip: Option<Ipv4Addr>) {
        if self.mode == ArpMode::Respond && ip != self.answering {
            match ip {
                Some(ip) => info!(%ip, "ARP responder: answering requests for the lease address"),
                None => info!("ARP responder: off"),
            }
            self.answering = ip;
        }
    }

    /// Whether anyone else on the link uses `ip`: `Some(their MAC)` if so.
    pub async fn probe(&mut self, ip: Ipv4Addr, how: Probe) -> Result<Option<Mac>> {
        let watch = Some(Watch {
            addr: ip,
            probing: true,
        });
        let frame = packet::probe_frame(&self.iface().mac, ip);
        for n in 0..how.count {
            self.send(&frame).await?;
            let listen = if n + 1 == how.count {
                how.wait
            } else {
                how.interval
            };
            if let Some(Heard::Conflict(mac)) =
                self.wait(Instant::now() + listen, None, watch).await?
            {
                warn!(%ip, by = %fmt_mac(&mac), "ARP probe: address in use");
                return Ok(Some(mac));
            }
        }
        debug!(%ip, "ARP probe: no other user");
        Ok(None)
    }

    /// Announce `ip` at our MAC, refreshing stale caches (a reused lease,
    /// a phone that moved). Watches for a conflict meanwhile.
    pub async fn announce(&mut self, ip: Ipv4Addr) -> Result<Option<Mac>> {
        let watch = Some(Watch {
            addr: ip,
            probing: false,
        });
        let frame = packet::announce_frame(&self.iface().mac, ip);
        for n in 0..ANNOUNCEMENTS {
            self.send(&frame).await?;
            let pause = if n + 1 == ANNOUNCEMENTS {
                Duration::ZERO
            } else {
                ANNOUNCE_INTERVAL
            };
            if let Some(Heard::Conflict(mac)) =
                self.wait(Instant::now() + pause, None, watch).await?
            {
                return Ok(Some(mac));
            }
        }
        info!(%ip, "ARP: announced");
        Ok(None)
    }

    /// Until another station claims `ip`; its MAC then.
    pub async fn watch(&mut self, ip: Ipv4Addr) -> Result<Mac> {
        let watch = Some(Watch {
            addr: ip,
            probing: false,
        });
        loop {
            let far = Instant::now() + Duration::from_secs(3600);
            if let Some(Heard::Conflict(mac)) = self.wait(far, None, watch).await? {
                return Ok(mac);
            }
        }
    }

    /// A conflict's MAC, after answering the frame if it asks for our lease.
    pub(super) async fn on_arp(&self, a: &Arp, watch: Option<Watch>) -> Result<Option<Mac>> {
        let mine = self.iface().mac;
        if a.sha == mine {
            return Ok(None);
        }
        if let Some(w) = watch
            && conflicts(a, w)
        {
            warn!(ip = %w.addr, by = %fmt_mac(&a.sha), "ARP: another station claims the address");
            return Ok(Some(a.sha));
        }
        if let Some(ip) = self.answering
            && a.op == ARP_REQUEST
            && a.tpa == ip
            && !a.spa.is_unspecified()
        {
            debug!(from = %fmt_mac(&a.sha), spa = %a.spa, %ip, "ARP: replying for lease address");
            self.send(&packet::reply_frame(&mine, ip, &a.sha, a.spa))
                .await?;
        }
        Ok(None)
    }
}

/// RFC 5227 §2.1.1 and §2.4: a sender address equal to ours is a conflict;
/// while probing, so is someone else's probe for the same address.
pub fn conflicts(a: &Arp, w: Watch) -> bool {
    a.spa == w.addr
        || (w.probing && a.op == ARP_REQUEST && a.spa.is_unspecified() && a.tpa == w.addr)
}
