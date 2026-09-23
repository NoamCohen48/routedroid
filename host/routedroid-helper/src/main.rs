//! Routedroid privileged helper (architecture §5.3): owns the phone's TUN,
//! its /32 route, proxy ARP and the per-connection firewall, journals every
//! mutation before making it, and undoes everything when the controller
//! stops, disconnects or dies. systemd runs `check` before and `cleanup`
//! after every instance.

mod claims;
mod connection;
mod fault;
mod journal;
mod ops;
mod recovery;
mod serve;
mod session;
mod tun;

use routedroid_helper_ipc::proto;
use routedroid_helper_ipc::seqpacket;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::fault::CrashHook;

pub const DEFAULT_JOURNAL: &str = "/var/lib/routedroid/journal";

#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    #[arg(long, default_value = DEFAULT_JOURNAL, global = true)]
    journal_dir: PathBuf,
    /// Where sysctl baselines shared between sessions are reference-counted.
    #[arg(long, default_value = claims::DEFAULT_DIR, global = true)]
    claims_dir: PathBuf,
    /// Test hook file: if its content equals a stage name, the helper SIGKILLs itself there.
    #[cfg(feature = "testing")]
    #[arg(long, default_value = fault::DEFAULT_CRASH_FILE, global = true)]
    crash_file: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Serve controllers (systemd socket or --socket), one session each.
    Serve {
        #[arg(long)]
        socket: Option<PathBuf>,
        /// Exit after the first session ends (systemd `Accept=yes` instances always do).
        #[arg(long)]
        once: bool,
        /// Only accept a controller with this uid (socket permissions are the primary gate).
        #[arg(long)]
        allow_uid: Option<u32>,
    },
    /// ExecStartPre: exit 1 while any unresolved journal exists.
    Check,
    /// ExecStopPost: replay unresolved journals; exit 1 if anything remains.
    Cleanup,
}

impl Cli {
    #[cfg(feature = "testing")]
    fn hook(&self) -> CrashHook {
        CrashHook(self.crash_file.clone())
    }

    #[cfg(not(feature = "testing"))]
    fn hook(&self) -> CrashHook {
        CrashHook
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    claims::init(cli.claims_dir.clone());
    let hook = cli.hook();
    match cli.cmd {
        Cmd::Serve { socket, allow_uid, once } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(serve::serve(serve::ServeConfig {
                socket,
                journal_dir: cli.journal_dir,
                hook,
                allow_uid,
                once,
            }))
        }
        Cmd::Check => recovery::check(&cli.journal_dir),
        Cmd::Cleanup => recovery::cleanup(&cli.journal_dir, &hook),
    }
}
