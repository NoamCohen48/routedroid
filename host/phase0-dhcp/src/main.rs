//! Routedroid Phase 0 §3.3 host probe: obtain an ADDITIONAL DHCP lease on a
//! physical interface for a phone identity (option 61), over a raw
//! `AF_PACKET` socket, without ever assigning the address to the interface
//! and without touching the PC's own lease. Throwaway quality.
//!
//! Exit codes: 0 ok, 1 error, 3 server NAK, 4 timeout / lease expired.

mod client;
mod dhcp;
mod packet;
mod sock;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::{error, info, warn};

use client::{Client, HoldEnd, Lease, Outcome};

const EXIT_NAK: u8 = 3;
const EXIT_TIMEOUT: u8 = 4;

#[derive(Parser, Debug)]
#[command(name = "phase0-dhcp", about = "Routedroid Phase 0 DHCP alias probe (extra lease, never configured locally)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// DISCOVER/REQUEST a lease for --client-id, print it as JSON, optionally hold and release.
    Acquire(AcquireArgs),
    /// One unicast RENEW from a saved state file (ACK -> 0, NAK -> 3, timeout -> 4).
    Renew(StateArgs),
    /// Unicast RELEASE of the lease in a saved state file.
    Release(StateArgs),
    /// Broadcast INIT-REBOOT REQUEST for the saved address (ACK -> 0, NAK -> 3, timeout -> 4).
    InitReboot(StateArgs),
}

#[derive(Parser, Debug, Clone)]
struct AcquireArgs {
    /// Interface to bind the packet socket to (a VLAN netdevice is fine).
    #[arg(long)]
    iface: String,
    /// Option 61 client identifier string (sent as type 0 + bytes).
    #[arg(long)]
    client_id: String,
    /// Overall acquisition deadline in seconds.
    #[arg(long, default_value_t = 30)]
    timeout: u64,
    /// Write the lease record (JSON) here; updated after every ACK.
    #[arg(long)]
    state: Option<PathBuf>,
    /// Send RELEASE when the hold ends or on SIGINT/SIGTERM.
    #[arg(long)]
    release_on_exit: bool,
    /// Stay BOUND for this many seconds after the ACK, renewing at T1.
    #[arg(long)]
    hold: Option<u64>,
    /// While holding, send the first RENEW after this many seconds instead of at T1
    /// (lets a short --hold exercise unicast renewal against a long lease).
    #[arg(long)]
    renew_after: Option<u64>,
    /// Seconds to keep collecting OFFERs after the first one.
    #[arg(long, default_value_t = 2.0)]
    offer_window: f64,
    /// Do not answer ARP for the lease address (unicast replies will then be
    /// unroutable for the server; useful only to demonstrate that).
    #[arg(long)]
    no_arp: bool,
}

#[derive(Parser, Debug, Clone)]
struct StateArgs {
    #[arg(long)]
    iface: String,
    /// Lease record written by `acquire --state`.
    #[arg(long)]
    state: PathBuf,
    /// Deadline in seconds for the reply.
    #[arg(long, default_value_t = 10)]
    timeout: u64,
    #[arg(long)]
    no_arp: bool,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            error!(error = %e, "tokio runtime");
            return ExitCode::FAILURE;
        }
    };
    let result = match cli.cmd {
        Cmd::Acquire(a) => rt.block_on(acquire(a)),
        Cmd::Renew(a) => rt.block_on(renew(a)),
        Cmd::Release(a) => rt.block_on(release(a)),
        Cmd::InitReboot(a) => rt.block_on(init_reboot(a)),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            error!(error = format_args!("{e:#}"), "exit");
            ExitCode::FAILURE
        }
    }
}

fn publish(lease: &Lease, state: Option<&PathBuf>) -> Result<()> {
    println!("{}", lease.to_json());
    if let Some(p) = state {
        lease.save(p)?;
        info!(path = %p.display(), "state written");
    }
    Ok(())
}

async fn acquire(a: AcquireArgs) -> Result<u8> {
    let mut client = Client::new(&a.iface, &a.client_id, !a.no_arp)?;
    let offer_window = Duration::from_secs_f64(a.offer_window.max(0.0));
    let lease = match client.acquire(Duration::from_secs(a.timeout), offer_window).await? {
        Outcome::Bound(l) => l,
        Outcome::Nak(m) => {
            error!(message = %m, "NAK");
            return Ok(EXIT_NAK);
        }
        Outcome::Timeout => {
            error!(timeout = a.timeout, "no lease within the deadline");
            return Ok(EXIT_TIMEOUT);
        }
    };
    info!(
        address = %format_args!("{}/{}", lease.address, lease.prefix),
        router = ?lease.router,
        dns = ?lease.dns,
        server = %lease.server_id,
        server_mac = %lease.server_mac,
        lease_secs = lease.lease_secs,
        t1 = lease.t1,
        t2 = lease.t2,
        routes = lease.static_routes.len(),
        "BOUND (address NOT configured on the interface)"
    );
    for r in &lease.static_routes {
        info!(route = %r, "classless static route (informational only)");
    }
    publish(&lease, a.state.as_ref())?;

    let mut current = lease;
    let mut code = 0;
    if let Some(hold) = a.hold {
        match client.hold(current.clone(), Duration::from_secs(hold), a.renew_after.map(Duration::from_secs)).await? {
            HoldEnd::Done(l) | HoldEnd::Signal(l) => {
                if let Some(p) = &a.state {
                    l.save(p)?;
                }
                current = l;
            }
            HoldEnd::Nak => return Ok(EXIT_NAK),
            HoldEnd::Expired => return Ok(EXIT_TIMEOUT),
        }
    }
    if a.release_on_exit {
        if let Err(e) = client.release(&current).await {
            warn!(error = %e, "RELEASE failed (best effort)");
            code = 1;
        }
    }
    Ok(code)
}

fn load(a: &StateArgs) -> Result<(Client, Lease)> {
    let lease = Lease::load(&a.state)?;
    if lease.iface != a.iface {
        warn!(state_iface = %lease.iface, iface = %a.iface, "state was acquired on a different interface");
    }
    let client = Client::new(&a.iface, &lease.client_id, !a.no_arp).context("open packet socket")?;
    Ok((client, lease))
}

fn outcome_code(o: Outcome, state: &PathBuf, what: &str) -> Result<u8> {
    match o {
        Outcome::Bound(l) => {
            info!(address = %l.address, lease_secs = l.lease_secs, "{what}: ACK");
            publish(&l, Some(state))?;
            Ok(0)
        }
        Outcome::Nak(m) => {
            error!(message = %m, "{what}: NAK");
            Ok(EXIT_NAK)
        }
        Outcome::Timeout => {
            error!("{what}: no reply within the deadline");
            Ok(EXIT_TIMEOUT)
        }
    }
}

async fn renew(a: StateArgs) -> Result<u8> {
    let (mut client, lease) = load(&a)?;
    let o = client.renew(&lease, Duration::from_secs(a.timeout)).await?;
    outcome_code(o, &a.state, "RENEW")
}

async fn init_reboot(a: StateArgs) -> Result<u8> {
    let (mut client, lease) = load(&a)?;
    let o = client.init_reboot(&lease, Duration::from_secs(a.timeout)).await?;
    outcome_code(o, &a.state, "INIT-REBOOT")
}

async fn release(a: StateArgs) -> Result<u8> {
    let (mut client, lease) = load(&a)?;
    client.release(&lease).await?;
    info!(address = %lease.address, "RELEASE sent; state file left in place");
    Ok(0)
}

#[cfg(test)]
mod golden {
    use super::*;

    /// Fixed MAC 02:00:00:00:00:01, XID 0x12345678, secs 3, client-id "test".
    fn discover_frame() -> Vec<u8> {
        let id = dhcp::Identity::new([0x02, 0, 0, 0, 0, 0x01], "test");
        let msg = dhcp::discover(&id, 0x1234_5678, 3);
        packet::ipv4_udp_frame(
            &id.mac,
            &packet::BROADCAST_MAC,
            std::net::Ipv4Addr::UNSPECIFIED,
            std::net::Ipv4Addr::BROADCAST,
            dhcp::CLIENT_PORT,
            dhcp::SERVER_PORT,
            &msg.encode(),
        )
    }

    /// Golden bytes, independently verified with a Python re-implementation
    /// (Ethernet broadcast, IPv4 0.0.0.0 -> 255.255.255.255 id 0 ttl 64
    /// csum 0x79a6, UDP 68 -> 67 len 308 csum 0x437f, BOOTP op 1 htype 1
    /// hlen 6 xid 0x12345678 secs 3 flags 0x8000, chaddr = MAC, options
    /// 53=1, 61=00"test", 55=1,3,6,51,54,58,59,121, 57=1500, END, pad to 300).
    const DISCOVER_GOLDEN: &[&str] = &[
        "ffffffffffff02000000000108004500014800000000401179a600000000ffff",
        "ffff004400430134437f01010600123456780003800000000000000000000000",
        "0000000000000200000000010000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "00000000000000000000000000000000000000000000638253633501013d0500",
        "74657374370801030633363a3b79390205dcff00000000000000000000000000",
        "00000000000000000000000000000000000000000000",
    ];

    #[test]
    fn discover_frame_golden_bytes() {
        let expected: Vec<u8> = {
            let hex: String = DISCOVER_GOLDEN.concat();
            (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
        };
        let got = discover_frame();
        assert_eq!(got.len(), 342);
        if got != expected {
            let first = got.iter().zip(&expected).position(|(a, b)| a != b);
            panic!("DISCOVER frame differs from golden at byte {first:?}");
        }
        // And it must round-trip through our own strict parsers.
        let udp = packet::parse_udp(&got, true).unwrap();
        let msg = dhcp::Message::parse(udp.payload).unwrap();
        assert_eq!(msg.xid, 0x1234_5678);
        assert_eq!(msg.message_type(), Some(dhcp::MessageType::Discover));
    }
}
