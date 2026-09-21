//! Routedroid Phase 0 helper-gate spike (architecture §5.3 / plan §3.1 helper gate).
//! Throwaway quality; proves the journal + systemd cleanup + relay shape.

mod client;
mod journal;
mod ops;
use routedroid_helper_ipc::proto;
use routedroid_helper_ipc::seqpacket;
mod serve;
mod session;
mod tun;

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};

pub const DEFAULT_SOCKET: &str = "/run/routedroid/phase0-helper.sock";
pub const DEFAULT_JOURNAL: &str = "/var/lib/routedroid/phase0-journal";
pub const DEFAULT_CRASH_FILE: &str = "/run/routedroid/crash-at";

#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    #[arg(long, default_value = DEFAULT_JOURNAL, global = true)]
    journal_dir: PathBuf,
    /// Test hook file: if its content equals a stage name, the helper SIGKILLs itself there.
    #[arg(long, default_value = DEFAULT_CRASH_FILE, global = true)]
    crash_file: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Privileged: accept one controller (systemd socket or --socket), run one session, exit.
    Serve {
        #[arg(long)]
        socket: Option<PathBuf>,
        /// Only accept a controller with this uid (socket permissions are the primary gate).
        #[arg(long)]
        allow_uid: Option<u32>,
    },
    /// Privileged (ExecStartPre): exit 1 while any unresolved journal exists.
    Check,
    /// Privileged (ExecStopPost): replay unresolved journals; exit 1 if anything remains.
    Cleanup,
    /// Unprivileged controller stand-in.
    Client {
        #[arg(long, default_value = DEFAULT_SOCKET)]
        socket: PathBuf,
        #[arg(long)]
        lan_if: String,
        #[arg(long)]
        phone_ip: Ipv4Addr,
        #[arg(long, default_value = "phone0")]
        tun: String,
        #[arg(long, default_value_t = 1400)]
        mtu: u32,
        /// Seconds to keep the session up before stopping.
        #[arg(long, default_value_t = 0)]
        hold: u64,
        /// Send N ICMP echo requests through the relay and count kernel replies.
        #[arg(long, default_value_t = 0)]
        bench: u32,
        /// SIGKILL self at: before_start | after_start | during_traffic | before_stop
        #[arg(long)]
        crash_at: Option<String>,
        /// Disconnect without sending Stop (helper must clean up on its own).
        #[arg(long)]
        no_stop: bool,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")))
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let hook = session::CrashHook(cli.crash_file.clone());
    match cli.cmd {
        Cmd::Serve { socket, allow_uid } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(serve::serve(serve::ServeConfig { socket, journal_dir: cli.journal_dir, crash_file: cli.crash_file, allow_uid }))
        }
        Cmd::Check => session::check(&cli.journal_dir),
        Cmd::Cleanup => session::cleanup(&cli.journal_dir, &hook),
        Cmd::Client { socket, lan_if, phone_ip, tun, mtu, hold, bench, crash_at, no_stop } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(client::run(
                &socket,
                client::ClientArgs { lan_if, phone_ip, tun, mtu, hold: Duration::from_secs(hold), bench, crash_at, no_stop },
            ))
        }
    }
}
