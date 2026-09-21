//! Command-line surface. Each subcommand lives in `commands/`.

use clap::{Parser, Subcommand};

use crate::fault::Result;
use crate::logging::LogOptions;

#[derive(Debug, Parser)]
#[command(name = "routedroid", version, about = "Make an Android phone a reachable host on your LAN over ADB")]
pub struct Cli {
    #[command(flatten)]
    pub log: LogOptions,
    /// Path to the adb binary.
    #[arg(long, global = true, env = "ROUTEDROID_ADB", default_value = "adb")]
    pub adb: String,
    /// Print machine-readable JSON instead of text where a command supports it.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List attached devices and whether Routedroid can use them.
    Devices,
    /// Connect one phone with a statically chosen address and run until Ctrl-C.
    Start(crate::commands::start::StartArgs),
}

pub fn run(cli: Cli) -> Result<()> {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async move {
        match cli.command {
            Command::Devices => crate::commands::devices::run(&cli.adb, cli.json).await,
            Command::Start(args) => crate::commands::start::run(&cli.adb, args).await,
        }
    })
}
