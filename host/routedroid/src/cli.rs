//! Command-line surface. Each subcommand lives in `commands/`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::commands::start::StartArgs;

const EXIT_CODES: &str = "\
Exit codes:
  0   success (for `start`: the connection ended cleanly)
  2   usage error
  3   daemon unreachable; start it with `systemctl --user start routedroid`
  4   daemon speaks another API version; restart it after an upgrade
  10  adb          11  transport rule    12  protocol
  13  auth         14  vpn               15  helper
  70  internal";

#[derive(Debug, Parser)]
#[command(
    name = "routedroid",
    version,
    about = "Make an Android phone a reachable host on your LAN over ADB (client of routedroidd)",
    after_help = EXIT_CODES
)]
pub struct Cli {
    /// Control socket of routedroidd.
    #[arg(long, global = true, env = routedroid_ipc::socket::SOCKET_ENV, default_value_os_t = routedroid_ipc::socket::default_path())]
    pub socket: PathBuf,
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
    /// Connect one phone with a statically chosen address; stays attached until Ctrl-C unless --detach.
    Start(StartArgs),
    /// Disconnect one phone.
    Stop {
        /// ADB serial of the phone.
        #[arg(long, short = 's', env = "ANDROID_SERIAL")]
        serial: String,
    },
    /// Show live device connections.
    Status,
    /// Print every daemon event as one JSON line, forever.
    Events,
    /// Print the CLI and daemon versions.
    Version,
}
