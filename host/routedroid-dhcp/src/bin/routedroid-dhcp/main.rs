//! Lab CLI for the DHCP alias client: acquire, renew, INIT-REBOOT and
//! release extra leases by hand, printing each lease as JSON on stdout.
//! The identity is derived from a phone serial exactly as routedroidd
//! derives it, so a lab lease is the lease that phone would get.
//!
//! Exit codes: 0 ok, 1 error, 3 server NAK, 4 timeout / lease expired.

use std::process::ExitCode;

use clap::Parser;
use tracing::error;

mod args;
mod commands;
mod host;

use args::{Cli, Cmd};

const EXIT_NAK: u8 = 3;
const EXIT_TIMEOUT: u8 = 4;

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
    let result = rt.block_on(async {
        match cli.cmd {
            Cmd::Acquire(a) => commands::acquire(a).await,
            Cmd::Renew(a) => commands::renew(a).await,
            Cmd::Release(a) => commands::release(a).await,
            Cmd::InitReboot(a) => commands::init_reboot(a).await,
        }
    });
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            error!(error = format_args!("{e:#}"), "exit");
            ExitCode::FAILURE
        }
    }
}
