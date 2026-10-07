//! Control socket: bind (owner-only), accept, one task per connection, and
//! an orderly shutdown on SIGINT/SIGTERM.

mod bind;
mod client;
mod view;

use std::path::PathBuf;

use anyhow::Result;
use tokio::net::UnixStream;
use tokio::signal::unix::{SignalKind, signal};
use tracing::{info, warn};

use self::client::{ClientConnection, Handles};
use crate::Args;
use crate::adb::{Adb, DEFAULT_TIMEOUT};
use crate::app::BundledApp;
use crate::daemon::Daemon;

/// The control socket and the daemon behind it: owns both for the process's
/// life and takes both down together.
pub struct Server {
    bound: bind::Bound,
    path: PathBuf,
    daemon: Daemon,
}

impl Server {
    pub async fn bind(args: Args) -> Result<Self> {
        let bound = bind::listen(&args.socket)?;
        let app = BundledApp::embedded();
        let daemon = Daemon::start(
            Adb::new(&args.adb, DEFAULT_TIMEOUT),
            args.helper_socket.clone(),
            app,
        )
        .await;
        let app = app.map_or_else(|| "none".to_string(), |app| app.version());
        info!(socket = %args.socket.display(), %app, "routedroidd ready");
        Ok(Self {
            bound,
            path: args.socket,
            daemon,
        })
    }

    /// Accept until a signal, then stop every device connection.
    pub async fn run(self) -> Result<()> {
        // Polling adb and ticking traffic counters belong to the components
        // that own that state; nothing has to be started here.
        let mut sigterm = signal(SignalKind::terminate())?;
        loop {
            tokio::select! {
                accepted = self.bound.listener.accept() => match accepted {
                    Ok((stream, _)) => self.accept(stream),
                    Err(error) => warn!(%error, "accept failed"),
                },
                _ = tokio::signal::ctrl_c() => break,
                _ = sigterm.recv() => break,
            }
        }
        self.shutdown(&mut sigterm).await;
        Ok(())
    }

    fn accept(&self, stream: UnixStream) {
        // Only the owning user may drive connections: the socket is 0600, and
        // SO_PEERCRED (the uid the kernel attests for the peer, which a
        // client cannot forge) is the second line of defence.
        match stream.peer_cred() {
            Ok(cred) if cred.uid() == bind::current_uid() => {}
            Ok(cred) => {
                warn!(uid = cred.uid(), "rejected connection from another user");
                return;
            }
            Err(error) => {
                warn!(%error, "could not read peer credentials");
                return;
            }
        }
        let handles = Handles {
            devices: self.daemon.devices(),
            connections: self.daemon.connections(),
        };
        let client = ClientConnection::new(handles, self.daemon.events(), stream);
        tokio::spawn(client.run());
    }

    /// Unlink first so new clients get "unreachable", not a silent backlog;
    /// then stop the connections, unless a second signal says to give up.
    async fn shutdown(self, sigterm: &mut tokio::signal::unix::Signal) {
        // Unlink under the lock (still held by `bound`), then stop listening.
        let _ = std::fs::remove_file(&self.path);
        drop(self.bound);
        info!("shutting down: disconnecting devices (signal again to give up waiting)");
        tokio::select! {
            _ = self.daemon.stop_all() => {}
            _ = tokio::signal::ctrl_c() => warn!("second signal: leaving the helper's cleanup to undo them"),
            _ = sigterm.recv() => warn!("second signal: leaving the helper's cleanup to undo them"),
        }
    }
}
