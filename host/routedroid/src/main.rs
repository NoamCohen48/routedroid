//! `routedroid`: the command line of the routedroid daemon (`routedroidd`).
//!
//! Every subcommand is one short conversation over the daemon's control
//! socket; the daemon owns the sessions, this binary only asks and prints.
//! Exit codes follow `routedroid_ipc::Kind::exit_code`, plus 3 when the
//! daemon cannot be reached at all.

mod cli;
mod commands;
mod connect;
mod output;

use clap::Parser;
use routedroid_ipc::Fault;

use crate::connect::DaemonUnreachable;

fn main() {
    let cli = cli::Cli::parse();
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let code = match runtime.block_on(commands::run(cli)) {
        Ok(code) => code,
        Err(error) => exit_code_for(&error),
    };
    std::process::exit(code);
}

/// Prints the error the way scripts and humans expect and picks its exit code.
fn exit_code_for(error: &anyhow::Error) -> i32 {
    if let Some(unreachable) = error.downcast_ref::<DaemonUnreachable>() {
        eprintln!("error: {unreachable}");
        eprintln!("hint: start the daemon with `systemctl --user start routedroid`");
        return connect::EXIT_DAEMON_UNREACHABLE;
    }
    if let Some(fault) = error.downcast_ref::<Fault>() {
        eprintln!("error: {fault}");
        return fault.kind().exit_code();
    }
    eprintln!("error: {error:#}");
    routedroid_ipc::Kind::Internal.exit_code()
}
