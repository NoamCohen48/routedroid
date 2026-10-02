//! Command-line surface. Each subcommand lives in `commands/`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::commands::start::StartArgs;

#[derive(Debug, Parser)]
#[command(
    name = "routedroid",
    version,
    about = "Make an Android phone a reachable host on your LAN over ADB (client of routedroidd)",
    after_help = crate::exit::help()
)]
pub struct Cli {
    /// Control socket of routedroidd.
    #[arg(long, global = true, env = routedroid_ipc::socket::SOCKET_ENV, default_value_os_t = routedroid_ipc::socket::default_path())]
    pub socket: PathBuf,
    /// Print JSON instead of text: one document per answer, one line per event.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List attached devices and whether Routedroid can use them.
    Devices,
    /// List the host's network interfaces and whether a phone may join through each.
    Interfaces,
    /// Connect one phone; stays attached until it ends or Ctrl-C, unless --detach.
    Start(StartArgs),
    /// Disconnect one phone and wait until it is gone.
    Stop {
        /// ADB serial of the phone.
        #[arg(long, short = 's', env = "ANDROID_SERIAL")]
        serial: String,
    },
    /// Show live device connections.
    Status,
    /// Print every daemon event, forever (JSON lines, with or without --json).
    Events,
    /// Print the CLI and daemon versions.
    Version,
}
