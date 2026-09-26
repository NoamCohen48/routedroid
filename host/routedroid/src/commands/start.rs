//! `routedroid start`: ask the daemon to connect a phone and, unless detached,
//! follow it until it ends. Ctrl-C asks the daemon to stop it and waits for
//! the final `ended` event, so the exit code reflects how the connection ended.

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Result};
use clap::Args;
use routedroid_ipc::{Client, ConnectionState, Event, Kind, Outcome, Request, Response, StartRequest};

use crate::connect::connect;
use crate::output::state_line;

#[derive(Debug, Args)]
pub struct StartArgs {
    /// ADB serial of the phone (see `routedroid devices`).
    #[arg(long, short = 's', env = "ANDROID_SERIAL")]
    pub serial: String,
    /// LAN interface the phone joins (e.g. eno1).
    #[arg(long)]
    pub lan_if: String,
    /// Address the phone gets on that LAN (must be free; automatic DHCP comes in Phase 3).
    #[arg(long)]
    pub phone_ip: Ipv4Addr,
    /// TUN interface name the helper creates; the daemon picks a free phoneN by default.
    #[arg(long)]
    pub tun: Option<String>,
    /// Packet MTU offered in HELLO_ACK (576..=65535); the daemon's default otherwise.
    #[arg(long, value_parser = clap::value_parser!(u32).range(576..=65535))]
    pub mtu: Option<u32>,
    /// DNS server(s) to hand the phone; defaults to none.
    #[arg(long = "dns")]
    pub dns: Vec<Ipv4Addr>,
    /// How long to wait for the app to connect after launch.
    #[arg(long, default_value = "90s", value_parser = parse_seconds)]
    pub connect_timeout: Duration,
    /// Start over a network ADB serial (host:port or mDNS). Unverified in
    /// version 1: the VPN default route may cut ADB itself (decision 0001, gate 5).
    #[arg(long)]
    pub allow_network_adb: bool,
    /// Return as soon as the daemon has accepted the request instead of following it.
    #[arg(long)]
    pub detach: bool,
}

fn parse_seconds(text: &str) -> std::result::Result<Duration, String> {
    text.trim_end_matches('s').parse::<u64>().map(Duration::from_secs).map_err(|error| error.to_string())
}

impl StartArgs {
    fn request(&self) -> StartRequest {
        StartRequest {
            serial: self.serial.clone(),
            lan_if: self.lan_if.clone(),
            phone_ip: self.phone_ip,
            tun: self.tun.clone(),
            mtu: self.mtu,
            dns: self.dns.clone(),
            connect_timeout_secs: Some(self.connect_timeout.as_secs()),
            allow_network_adb: self.allow_network_adb,
        }
    }
}

pub async fn run(client: &mut Client, socket: &Path, args: StartArgs) -> Result<i32> {
    // Subscribe first so no state change between `Started` and our first read is missed.
    client.call_ok(Request::Subscribe).await?;
    match client.call_ok(Request::Start(args.request())).await? {
        Response::Started { serial } => println!("started: {serial}"),
        other => bail!("unexpected answer to start: {other:?}"),
    }
    if args.detach {
        return Ok(0);
    }
    tokio::spawn(stop_on_ctrl_c(socket.to_path_buf(), args.serial.clone()));
    let outcome = follow(client, &args.serial).await?;
    Ok(exit_code(&outcome))
}

/// Prints each state of our connection until it ends; returns how it ended.
async fn follow(client: &mut Client, serial: &str) -> Result<Outcome> {
    loop {
        let Some(event) = client.next_event().await? else { bail!("routedroidd closed the connection") };
        match event {
            Event::Connection { serial: other, .. } if other != serial => {}
            Event::Connection { state: ConnectionState::Ended(outcome), .. } => {
                println!("{}", state_line(&ConnectionState::Ended(outcome.clone())));
                return Ok(outcome);
            }
            Event::Connection { state, .. } => println!("{}", state_line(&state)),
            Event::Shutdown => eprintln!("routedroidd is shutting down"),
            // We may have missed our `ended`; ask instead of waiting forever.
            Event::Lagged { .. } => {
                if let Some(outcome) = ended_meanwhile(client, serial).await? {
                    println!("{}", state_line(&ConnectionState::Ended(outcome.clone())));
                    return Ok(outcome);
                }
            }
            Event::Traffic { .. } | Event::Devices { .. } => {}
        }
    }
}

/// After missed events: `Some(outcome)` if our phone is no longer connected.
async fn ended_meanwhile(client: &mut Client, serial: &str) -> Result<Option<Outcome>> {
    match client.call_ok(Request::Status).await? {
        Response::Status { connections } if connections.iter().any(|c| c.serial == serial) => Ok(None),
        Response::Status { .. } => {
            Ok(Some(Outcome { ok: false, kind: None, message: "the connection ended while events were missed".into() }))
        }
        other => bail!("unexpected answer to status: {other:?}"),
    }
}

/// First Ctrl-C asks the daemon to disconnect the phone (over its own connection,
/// so the event stream is never interrupted); a second one gives up waiting.
async fn stop_on_ctrl_c(socket: PathBuf, serial: String) {
    if tokio::signal::ctrl_c().await.is_err() {
        return;
    }
    eprintln!("stopping (press Ctrl-C again to abandon the session)");
    tokio::spawn(async move {
        if let Ok(stopper) = connect(&socket).await {
            let _ = stopper.call(Request::Stop { serial }).await;
        }
    });
    if tokio::signal::ctrl_c().await.is_ok() {
        std::process::exit(130);
    }
}

fn exit_code(outcome: &Outcome) -> i32 {
    if outcome.ok {
        0
    } else {
        outcome.kind.map(Kind::exit_code).unwrap_or(Kind::Internal.exit_code())
    }
}
