//! `routedroid`: the command line of the routedroid daemon (`routedroidd`).
//!
//! Every subcommand is one short conversation over the daemon's control
//! socket; the daemon owns the connections, this binary only asks and prints.
//! Exit codes are in `exit`.

mod cli;
mod commands;
mod exit;
mod output;

use clap::Parser;

fn main() {
    // Rust ignores SIGPIPE, so `routedroid events | head` would panic on its
    // next line; like any Unix tool, end quietly when the reader goes away.
    // SAFETY: nothing else runs yet, and SIG_DFL is a valid disposition.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = cli::Cli::parse();
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let code = match runtime.block_on(commands::run(cli)) {
        Ok(code) => code,
        Err(error) => exit::report(&error),
    };
    std::process::exit(code);
}
