//! Control socket: bind (owner-only), accept, one task per connection, and
//! an orderly shutdown on SIGINT/SIGTERM.

mod bind;
mod connection;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{signal, SignalKind};
use tracing::{info, warn};

use self::connection::Connection;
use crate::adb::{Adb, DEFAULT_TIMEOUT};
use crate::daemon::Daemon;
use crate::Args;

/// The control socket and the daemon behind it: owns both for the process's
/// life and takes both down together.
pub struct Server {
    listener: UnixListener,
    path: PathBuf,
    daemon: Arc<Daemon>,
}

pub async fn serve(args: Args) -> Result<()> {
    Server::bind(args).await?.run().await
}

impl Server {
    pub async fn bind(args: Args) -> Result<Self> {
        let listener = bind::listen(&args.socket).await?;
        let daemon = Daemon::new(Adb::new(&args.adb, DEFAULT_TIMEOUT), args.helper_socket.clone());
        info!(socket = %args.socket.display(), "routedroidd ready");
        Ok(Self { listener, path: args.socket, daemon })
    }

    /// Accept until a signal, then stop every session before returning.
    pub async fn run(self) -> Result<()> {
        // Each background task owns a handle on the daemon; `Arc::clone` is a
        // refcount bump, not a copy of the daemon.
        tokio::spawn(self.daemon.clone().watch_devices());
        tokio::spawn(self.daemon.clone().watch_traffic());

        let mut sigterm = signal(SignalKind::terminate())?;
        loop {
            tokio::select! {
                accepted = self.listener.accept() => match accepted {
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
        // Only the owning user may drive sessions: the socket is 0600, and
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
        tokio::spawn(Connection::new(self.daemon.clone(), stream).run());
    }

    /// Unlink first so new clients get "unreachable", not a silent backlog;
    /// then stop the sessions, unless a second signal says to give up.
    async fn shutdown(self, sigterm: &mut tokio::signal::unix::Signal) {
        drop(self.listener);
        let _ = std::fs::remove_file(&self.path);
        info!("shutting down: stopping sessions (signal again to give up waiting)");
        tokio::select! {
            _ = self.daemon.stop_all() => {}
            _ = tokio::signal::ctrl_c() => warn!("second signal: leaving sessions to the helper's cleanup"),
            _ = sigterm.recv() => warn!("second signal: leaving sessions to the helper's cleanup"),
        }
    }
}
