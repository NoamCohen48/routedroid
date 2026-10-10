//! The options a connection is made with, shared by `start` and `remember`.
//! Left out, an option is not sent: the daemon fills it in from what is
//! remembered for the phone, else its own default.

use std::net::Ipv4Addr;
use std::time::Duration;

use clap::Args;
use routedroid_ipc::{DnsChoice, Phone};

#[derive(Debug, Args)]
pub struct Options {
    /// LAN interface the phone joins (see `routedroid interfaces`); by
    /// default the remembered one, else the one the policy allows.
    #[arg(long)]
    pub lan_if: Option<String>,
    /// Address the phone gets on that LAN; leased by DHCP when left out.
    #[arg(long)]
    pub phone_ip: Option<Ipv4Addr>,
    /// Packet MTU offered to the phone.
    #[arg(long)]
    pub mtu: Option<u32>,
    /// DNS server for the phone (repeatable); by default the lease's servers,
    /// else the LAN's gateway.
    #[arg(long = "dns", conflicts_with = "no_dns")]
    pub dns: Vec<Ipv4Addr>,
    /// Give the phone no DNS server at all.
    #[arg(long)]
    pub no_dns: bool,
    /// How long an unplugged phone keeps its address and connection while
    /// it comes back, e.g. `5m`; `0` ends the connection at once.
    #[arg(long, value_parser = humantime::parse_duration)]
    pub reconnect_wait: Option<Duration>,
}

impl Options {
    pub fn dns(&self) -> Option<DnsChoice> {
        match (self.no_dns, self.dns.as_slice()) {
            (true, _) => Some(DnsChoice::None),
            (false, []) => None,
            (false, servers) => Some(DnsChoice::Servers(servers.to_vec())),
        }
    }

    pub fn reconnect_secs(&self) -> Option<u64> {
        self.reconnect_wait.map(whole_secs)
    }

    /// `phone` with every option given here in place of its own.
    pub fn onto(&self, mut phone: Phone) -> Phone {
        phone.lan_if = self.lan_if.clone().or(phone.lan_if);
        phone.phone_ip = self.phone_ip.or(phone.phone_ip);
        phone.mtu = self.mtu.or(phone.mtu);
        phone.dns = self.dns().or(phone.dns);
        phone.reconnect_secs = self.reconnect_secs().or(phone.reconnect_secs);
        phone
    }
}

/// Rounded up: a sub-second wait still means "a moment", not "none".
pub fn whole_secs(wait: Duration) -> u64 {
    wait.as_secs() + u64::from(wait.subsec_nanos() > 0)
}
