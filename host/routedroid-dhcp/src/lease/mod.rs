//! A bound lease: the record (what the server granted, with wall-clock
//! times, as stored and reported) and its schedule (T1, T2 and expiry on
//! the monotonic clock, so a clock step never mis-times a renewal).

use std::io::Write;
use std::net::Ipv4Addr;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::dhcp::StaticRoute;
use crate::identity::ClientId;
use crate::packet::{Mac, parse_mac};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub iface: String,
    pub client_id: String,
    pub address: Ipv4Addr,
    pub prefix: u8,
    pub subnet_mask: Ipv4Addr,
    pub router: Option<Ipv4Addr>,
    pub dns: Vec<Ipv4Addr>,
    pub static_routes: Vec<StaticRoute>,
    pub server_id: Ipv4Addr,
    /// Ethernet source of the ACK: the server, or the relay or next hop it
    /// is reached through. Unicast RENEW and RELEASE go there.
    pub server_mac: String,
    pub lease_secs: u32,
    pub t1: u32,
    pub t2: u32,
    /// Unix seconds when the ACK arrived; for the record only.
    pub acquired_at: u64,
    /// True if any reply frame carried VLAN offload metadata.
    pub vlan_tagged_replies: bool,
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Lease {
    pub fn server_mac(&self) -> Result<Mac> {
        parse_mac(&self.server_mac)
            .ok_or_else(|| anyhow!("lease: bad server_mac {:?}", self.server_mac))
    }

    pub fn client_id(&self) -> Result<ClientId> {
        ClientId::parse(&self.client_id).context("lease")
    }

    pub fn expires_at(&self) -> u64 {
        self.acquired_at.saturating_add(u64::from(self.lease_secs))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a lease serialises")
    }

    pub fn load(path: &Path) -> Result<Self> {
        let s =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&s).with_context(|| format!("parse {}", path.display()))
    }

    /// Durably replace `path`: a uniquely named temp file in the same
    /// directory, fsynced, renamed over, and the directory fsynced.
    pub fn save(&self, path: &Path) -> Result<()> {
        let dir = match path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d,
            _ => Path::new("."),
        };
        let name = path.file_name().context("lease path has no file name")?;
        let tmp = dir.join(format!(
            ".{}.{}.{:08x}",
            name.display(),
            std::process::id(),
            crate::random::xid()?
        ));
        let written = (|| {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            writeln!(f, "{}", serde_json::to_string_pretty(self)?)?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)?;
            std::fs::File::open(dir)?.sync_all()
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written.with_context(|| format!("write {}", path.display()))
    }
}

/// When to renew, rebind and give up, on the monotonic clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub t1: Instant,
    pub t2: Instant,
    pub expiry: Instant,
}

impl Schedule {
    /// From an ACK that arrived at `received`.
    pub fn new(lease: &Lease, received: Instant) -> Self {
        let at = |secs: u32| received + Duration::from_secs(u64::from(secs));
        Self {
            t1: at(lease.t1),
            t2: at(lease.t2),
            expiry: at(lease.lease_secs),
        }
    }

    /// For a lease read back from disk: what is left of each interval by
    /// the wall clock, read once; a deadline already past is now.
    pub fn restored(lease: &Lease) -> Self {
        let (unix, now) = (unix_now(), Instant::now());
        let at = |secs: u32| {
            let deadline = lease.acquired_at.saturating_add(u64::from(secs));
            now + Duration::from_secs(deadline.saturating_sub(unix))
        };
        Self {
            t1: at(lease.t1),
            t2: at(lease.t2),
            expiry: at(lease.lease_secs),
        }
    }
}

#[cfg(test)]
mod tests;
