//! Stand-in controller for the helper's test rigs (`integration-tests/helper`):
//! starts one session, optionally benchmarks the relay, can SIGKILL itself at
//! chosen moments, and stops cleanly otherwise. Built only with `--features
//! testing`; routedroidd is the real controller.

mod bench;
mod icmp;
mod link;
mod run;

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use routedroid_helper_ipc::{IfName, DEFAULT_SOCKET};

#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    #[arg(long, default_value = DEFAULT_SOCKET)]
    socket: PathBuf,
    #[arg(long)]
    lan_if: IfName,
    #[arg(long)]
    phone_ip: Ipv4Addr,
    #[arg(long, default_value = "phone0")]
    tun: IfName,
    #[arg(long, default_value_t = 1400)]
    mtu: u32,
    /// Seconds to keep the session up before stopping.
    #[arg(long, default_value_t = 0)]
    hold: u64,
    /// Send N ICMP echo requests through the relay and count kernel replies.
    #[arg(long, default_value_t = 0)]
    bench: u32,
    /// Address the bench echoes target (default: the host's LAN address).
    #[arg(long)]
    bench_target: Option<Ipv4Addr>,
    /// SIGKILL self at: before_start | after_start | during_traffic | before_stop
    #[arg(long)]
    crash_at: Option<String>,
    /// Disconnect without sending Stop (helper must clean up on its own).
    #[arg(long)]
    no_stop: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let args = run::ClientArgs {
        lan_if: cli.lan_if,
        phone_ip: cli.phone_ip,
        tun: cli.tun,
        mtu: cli.mtu,
        hold: Duration::from_secs(cli.hold),
        bench: cli.bench,
        bench_target: cli.bench_target,
        crash_at: cli.crash_at,
        no_stop: cli.no_stop,
    };
    tokio::runtime::Runtime::new()?.block_on(run::run(&cli.socket, args))
}
