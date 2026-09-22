//! `routedroidd`: the long-lived, unprivileged owner of phone sessions.
//!
//! Owns ADB, one loopback listener and protocol session per phone, and the
//! conversation with the privileged helper; never touches the network
//! itself. Clients (CLI, TUI) talk to it over the control socket
//! (`routedroid-ipc`).

mod adb;
mod app_listener;
mod daemon;
mod device;
mod host_network;
mod logging;
mod server;
mod session;

use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "routedroidd",
    version,
    about = "Routedroid daemon: owns phone sessions and serves the control socket"
)]
pub struct Args {
    #[command(flatten)]
    pub log: logging::LogOptions,
    /// Control socket path (clients use the same default).
    #[arg(long, env = routedroid_ipc::socket::SOCKET_ENV, default_value_os_t = routedroid_ipc::socket::default_path())]
    pub socket: PathBuf,
    /// Path to the adb binary.
    #[arg(long, env = "ROUTEDROID_ADB", default_value = "adb")]
    pub adb: String,
    /// Privileged helper's socket.
    #[arg(long, default_value = host_network::DEFAULT_SOCKET)]
    pub helper_socket: PathBuf,
}

fn main() {
    let args = Args::parse();
    logging::init(&args.log);
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let code = match rt.block_on(server::serve(args)) {
        // FIX: this should probably be init server and then call server on it. if you disagree let me know.
        Ok(()) => 0,
        Err(e) => {
            tracing::error!("{e:#}");
            1
        }
    };
    std::process::exit(code);
}
