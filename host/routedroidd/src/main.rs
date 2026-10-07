//! `routedroidd`: the long-lived, unprivileged owner of device connections.
//!
//! Owns ADB, one loopback listener and protocol session per phone, and the
//! conversation with the privileged helper; never touches the network
//! itself. Clients (CLI, TUI) talk to it over the control socket
//! (`routedroid-ipc`).

mod adb;
mod app;
mod app_listener;
mod daemon;
mod device;
mod fault;
mod host_network;
mod logging;
mod server;
mod session;

use std::path::PathBuf;

use clap::Parser;

use crate::server::Server;

#[derive(Debug, Parser)]
#[command(
    name = "routedroidd",
    version,
    about = "Routedroid daemon: owns device connections and serves the control socket"
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
    /// Where remembered phones are kept (`routedroid phones`).
    #[arg(long, env = "ROUTEDROID_PHONES", default_value_os_t = daemon::default_phones_path())]
    pub phones: PathBuf,
    /// Privileged helper's socket.
    #[arg(long, default_value = host_network::DEFAULT_SOCKET)]
    pub helper_socket: PathBuf,
    /// Print the man page (for packaging).
    #[arg(long, hide = true)]
    pub manpage: bool,
}

fn main() {
    let args = Args::parse();
    if args.manpage {
        use clap::CommandFactory;
        let page = clap_mangen::Man::new(Args::command()).render(&mut std::io::stdout());
        std::process::exit(i32::from(page.is_err()));
    }
    if let Err(e) = logging::init(&args.log) {
        eprintln!("routedroidd: {e}");
        std::process::exit(2);
    }
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    // Bind first: a socket we cannot take (another daemon on it, a shared
    // directory) must fail before anything else starts.
    let code = match rt.block_on(async { Server::bind(args).await?.run().await }) {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!("{e:#}");
            1
        }
    };
    std::process::exit(code);
}
