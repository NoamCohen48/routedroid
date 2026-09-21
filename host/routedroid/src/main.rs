//! `routedroid` host controller (architecture §5, implementation plan §4).
//!
//! Unprivileged. Owns ADB, the loopback listener, the session state machine
//! and the conversation with the privileged helper; never touches the
//! network itself.

mod adb;
mod cli;
mod commands;
mod fault;
mod helper;
mod listener;
mod ports;
mod logging;
mod session;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    logging::init(&cli.log);
    let code = match cli::run(cli) {
        Ok(()) => 0,
        Err(fault) => {
            tracing::error!(kind = fault.kind().as_str(), "{fault}");
            fault.kind().exit_code()
        }
    };
    std::process::exit(code);
}
