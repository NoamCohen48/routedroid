//! `routedroid start`: ask the daemon to connect a phone and, unless
//! detached, follow it until it ends. Every default (MTU, timeout, TUN name,
//! DNS) is the daemon's: an option left out is simply not sent.

mod follow;

use std::net::Ipv4Addr;
use std::time::Duration;

use anyhow::Result;
use clap::Args;
use routedroid_ipc::{Client, DnsChoice, Request, Response, StartRequest};

use super::answer;
use crate::output::print_json_line;

#[derive(Debug, Args)]
pub struct StartArgs {
    /// ADB serial of the phone (see `routedroid devices`).
    #[arg(long, short = 's', env = "ANDROID_SERIAL")]
    pub serial: String,
    /// LAN interface the phone joins (see `routedroid interfaces`).
    #[arg(long)]
    pub lan_if: String,
    /// Address the phone gets on that LAN; leased by DHCP when left out.
    #[arg(long)]
    pub phone_ip: Option<Ipv4Addr>,
    /// TUN interface name (phoneN); the daemon picks a free one by default.
    #[arg(long)]
    pub tun: Option<String>,
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
    /// How long the app has to connect after launch, e.g. `90s` or `2m`.
    #[arg(long, value_parser = humantime::parse_duration)]
    pub connect_timeout: Option<Duration>,
    /// Start over a network ADB serial (host:port or mDNS). Unverified in
    /// version 1: the VPN default route may cut ADB itself (decision 0001, gate 5).
    #[arg(long)]
    pub allow_network_adb: bool,
    /// Return as soon as the daemon has accepted the request instead of following it.
    #[arg(long)]
    pub detach: bool,
}

impl StartArgs {
    fn request(&self) -> StartRequest {
        let dns = match (self.no_dns, self.dns.as_slice()) {
            (true, _) => DnsChoice::None,
            (false, []) => DnsChoice::Auto,
            (false, servers) => DnsChoice::Servers(servers.to_vec()),
        };
        StartRequest {
            serial: self.serial.clone(),
            lan_if: self.lan_if.clone(),
            phone_ip: self.phone_ip,
            tun: self.tun.clone(),
            mtu: self.mtu,
            dns,
            // Rounded up: a sub-second timeout still means "a moment", not "none".
            connect_timeout_secs: self
                .connect_timeout
                .map(|d| d.as_secs() + u64::from(d.subsec_nanos() > 0)),
            allow_network_adb: self.allow_network_adb,
        }
    }
}

pub async fn run(client: Client, args: StartArgs, json: bool) -> Result<i32> {
    // Ctrl-C is ours from before the request: one that lands while the
    // daemon is still answering must stop the connection, not orphan it.
    let interrupts = follow::interrupts();
    // Subscribe first so no state change between `started` and our first read is missed.
    client.call_ok(Request::Subscribe).await?;
    let response = client.call_ok(Request::Start(args.request())).await?;
    let tun = answer!(&response, Response::Started { tun, .. } => tun.clone());
    if json {
        print_json_line(&response)?;
    } else {
        let address = match args.phone_ip {
            Some(ip) => format!("as {ip}"),
            None => "leasing an address by DHCP".into(),
        };
        println!(
            "started {} on {}, {address} (TUN {tun})",
            args.serial, args.lan_if
        );
    }
    if args.detach {
        return Ok(0);
    }
    follow::Follow::new(client, args.serial, json)
        .run(interrupts)
        .await
}

#[cfg(test)]
mod tests;
