//! Routedroid privileged helper (architecture §5.3): owns the phone's TUN,
//! its /32 route, proxy ARP and the per-connection firewall, within the
//! bounds of the operator's policy file. Every mutation is journaled before
//! it is made and undone when the controller stops, disconnects or dies.
//! `cleanup` runs after every instance (systemd `ExecStopPost`).

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use routedroid_helper_ipc::Activation;

use crate::env::{DEFAULT_STATE_DIR, Env};
use crate::fault::CrashHook;
use crate::kernel::System;

mod claims;
mod connection;
mod env;
mod fault;
mod journal;
mod kernel;
mod op;
mod plan;
mod policy;
mod recovery;
mod serve;
mod session;
mod session_id;
mod storage;
mod survey;
#[cfg(test)]
mod test_util;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    /// Journals (`journal/`) and shared sysctl claims (`sysctl/`).
    #[arg(long, default_value = DEFAULT_STATE_DIR, global = true)]
    state_dir: PathBuf,
    /// Which interfaces and phone addresses the operator allows.
    #[arg(long, default_value = policy::DEFAULT_PATH, global = true)]
    policy: PathBuf,
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
    /// Exit 1 while any orphaned or unreadable journal exists.
    Check,
    /// Undo every orphaned session; exit 1 if anything remains.
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
    let hook = cli.hook();
    let env = || -> Result<Env<System>> {
        Ok(Env::new(
            System::new()?,
            &cli.state_dir,
            cli.policy.clone(),
            hook,
        ))
    };
    match &cli.cmd {
        Cmd::Serve {
            socket,
            once,
            allow_uid,
        } => {
            // SAFETY: the runtime has not started yet, and nothing before
            // it spawns a thread.
            let activation = unsafe { Activation::take() }.context("socket activation")?;
            let options = serve::Options {
                socket: socket.clone(),
                allow_uid: *allow_uid,
                once: *once,
            };
            tokio::runtime::Runtime::new()?.block_on(serve::serve(env()?, options, activation))
        }
        Cmd::Check => recovery::check(&cli.state_dir.join("journal")),
        Cmd::Cleanup => recovery::cleanup(&env()?),
    }
}
