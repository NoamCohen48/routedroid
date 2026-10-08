//! `routedroid start`: ask the daemon to connect a phone and, unless
//! detached, follow it until it ends. Every default (which phone, which LAN,
//! MTU, timeout, TUN name, DNS) is the daemon's: an option left out is
//! simply not sent.

pub mod first_time;
mod follow;

use std::time::Duration;

use anyhow::Result;
use clap::Args;
use routedroid_ipc::{Client, Request, Response, StartRequest, label};

use super::answer;
use super::options::{Options, whole_secs};
use super::phones::remember;
use crate::output::print_json_line;

#[derive(Debug, Args)]
pub struct StartArgs {
    /// The phone: a serial or remembered name (see `routedroid devices`);
    /// by default the one attached.
    pub phone: Option<String>,
    /// The phone, as a flag (also read from ANDROID_SERIAL).
    #[arg(long, short = 's', env = "ANDROID_SERIAL", hide_env_values = true)]
    pub serial: Option<String>,
    #[command(flatten)]
    pub options: Options,
    /// TUN interface name (phoneN); the daemon picks a free one by default.
    #[arg(long)]
    pub tun: Option<String>,
    /// How long the app has to connect after launch, e.g. `90s` or `2m`.
    #[arg(long, value_parser = humantime::parse_duration)]
    pub connect_timeout: Option<Duration>,
    /// Start over a network ADB serial (host:port or mDNS). Untested: once
    /// the phone's traffic goes through the PC, ADB over the network may cut out.
    #[arg(long)]
    pub allow_network_adb: bool,
    /// Remember this phone and these options, and connect it whenever it is
    /// plugged in (`routedroid forget` undoes it).
    #[arg(long)]
    pub remember: bool,
    /// Call the phone NAME from now on (remembers it, as --remember does).
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
    /// Return as soon as the daemon has accepted the request instead of following it.
    #[arg(long)]
    pub detach: bool,
}

impl StartArgs {
    fn request(&self) -> StartRequest {
        StartRequest {
            serial: self.phone.clone().or_else(|| self.serial.clone()),
            lan_if: self.options.lan_if.clone(),
            phone_ip: self.options.phone_ip,
            tun: self.tun.clone(),
            mtu: self.options.mtu,
            dns: self.options.dns(),
            connect_timeout_secs: self.connect_timeout.map(whole_secs),
            reconnect_secs: self.options.reconnect_secs(),
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
    let (serial, name, lan_if, phone_ip, tun) = answer!(&response,
        Response::Started { serial, name, lan_if, phone_ip, tun } =>
            (serial.clone(), name.clone(), lan_if.clone(), *phone_ip, tun.clone()));
    if json {
        print_json_line(&response)?;
    } else {
        let address = match phone_ip {
            Some(ip) => format!("as {ip}"),
            None => "leasing an address by DHCP".into(),
        };
        let phone = label(&serial, name.as_deref());
        println!("started {phone} on {lan_if}, {address} (TUN {tun})");
    }
    if args.remember || args.name.is_some() {
        let phone = remember(
            &client,
            &serial,
            args.name.clone(),
            Some(true),
            &args.options,
        )
        .await?;
        if !json {
            println!(
                "remembered {}: it connects whenever it is plugged in",
                phone.label()
            );
        }
    }
    if args.detach {
        return Ok(0);
    }
    follow::Follow::new(client, serial, json)
        .run(interrupts)
        .await
}

#[cfg(test)]
mod tests;
