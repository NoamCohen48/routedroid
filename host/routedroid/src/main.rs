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
    let cli = cli::Cli::parse();
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let code = match runtime.block_on(commands::run(cli)) {
        Ok(code) => code,
        Err(error) => exit::report(&error),
    };
    std::process::exit(code);
}
