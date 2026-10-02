use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "routedroid-dhcp",
    about = "Routedroid DHCP alias client (extra lease, never configured locally)"
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// DISCOVER/REQUEST a lease for --serial, print it as JSON, optionally hold and release.
    Acquire(AcquireArgs),
    /// One unicast RENEW from a saved state file (ACK -> 0, NAK -> 3, timeout -> 4).
    Renew(StateArgs),
    /// Unicast RELEASE of the lease in a saved state file.
    Release(StateArgs),
    /// Broadcast INIT-REBOOT REQUEST for the saved address (ACK -> 0, NAK -> 3, timeout -> 4).
    InitReboot(StateArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct AcquireArgs {
    /// Interface to bind the packet socket to (a VLAN netdevice is fine).
    #[arg(long)]
    pub iface: String,
    /// The phone's ADB serial (or any lab name). Only its hash reaches the
    /// LAN, in option 61 as routedroid:<hash>:<interface MAC>.
    #[arg(long)]
    pub serial: String,
    /// Overall acquisition deadline in seconds.
    #[arg(long, default_value_t = 30)]
    pub timeout: u64,
    /// Write the lease record (JSON) here; updated after every ACK.
    #[arg(long)]
    pub state: Option<PathBuf>,
    /// Send RELEASE when the hold ends or on SIGINT/SIGTERM.
    #[arg(long)]
    pub release_on_exit: bool,
    /// Stay BOUND for this many seconds after the ACK, renewing at T1.
    #[arg(long)]
    pub hold: Option<u64>,
    /// While holding, send the first RENEW after this many seconds instead of at T1
    /// (lets a short --hold exercise unicast renewal against a long lease).
    #[arg(long)]
    pub renew_after: Option<u64>,
    /// Seconds to keep collecting OFFERs after the first one.
    #[arg(long, default_value_t = 2.0)]
    pub offer_window: f64,
    /// Skip the RFC 5227 ARP probe of the offered address.
    #[arg(long)]
    pub no_probe: bool,
    /// Do not answer ARP for the lease address (unicast replies will then be
    /// unroutable for the server; useful only to demonstrate that).
    #[arg(long)]
    pub no_arp: bool,
}

#[derive(Parser, Debug, Clone)]
pub struct StateArgs {
    #[arg(long)]
    pub iface: String,
    /// Lease record written by `acquire --state`.
    #[arg(long)]
    pub state: PathBuf,
    /// Deadline in seconds for the reply.
    #[arg(long, default_value_t = 10)]
    pub timeout: u64,
    #[arg(long)]
    pub no_arp: bool,
}
