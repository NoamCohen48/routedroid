//! Control socket: bind (owner-only), accept, one task per connection, and
//! an orderly shutdown on SIGINT/SIGTERM.

mod connection;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tokio::net::{UnixListener, UnixStream};
use tracing::{info, warn};

use crate::adb::{Adb, DEFAULT_TIMEOUT};
use crate::daemon::Daemon;
use crate::Args;

pub async fn serve(args: Args) -> Result<()> {
    let listener = bind(&args.socket).await?;
    let daemon = Daemon::new(Adb::new(&args.adb, DEFAULT_TIMEOUT), args.helper_socket.clone());
    info!(socket = %args.socket.display(), "routedroidd ready");
    tokio::spawn(crate::daemon::watch_devices(daemon.clone()));
    tokio::spawn(crate::daemon::traffic_ticker(daemon.clone()));

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            r = listener.accept() => match r {
                Ok((stream, _)) => accept(&daemon, stream),
                Err(e) => warn!(error = %e, "accept failed"),
            },
            _ = tokio::signal::ctrl_c() => break,
            _ = sigterm.recv() => break,
        }
    }
    // Unlink first so new clients get "unreachable", not a silent backlog.
    drop(listener);
    let _ = std::fs::remove_file(&args.socket);
    info!("shutting down: stopping sessions (signal again to give up waiting)");
    tokio::select! {
        _ = daemon.stop_all() => {}
        _ = tokio::signal::ctrl_c() => warn!("second signal: leaving sessions to the helper's cleanup"),
        _ = sigterm.recv() => warn!("second signal: leaving sessions to the helper's cleanup"),
    }
    Ok(())
}

fn accept(daemon: &Arc<Daemon>, stream: UnixStream) {
    // Only the owning user may drive sessions: the socket is 0600, and this
    // is the second line of defence.
    match stream.peer_cred() {
        Ok(cred) if cred.uid() == current_uid() => {}
        Ok(cred) => {
            warn!(uid = cred.uid(), "rejected connection from another user");
            return;
        }
        Err(e) => {
            warn!(error = %e, "could not read peer credentials");
            return;
        }
    }
    tokio::spawn(connection::run(daemon.clone(), stream));
}

fn current_uid() -> u32 {
    std::os::unix::fs::MetadataExt::uid(&std::fs::metadata("/proc/self").expect("/proc/self"))
}

/// Create the socket's directory (0700) if missing — an existing one must be
/// ours and private — refuse if another daemon answers, replace a stale
/// socket file, and bind with mode 0600.
async fn bind(path: &Path) -> Result<UnixListener> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        match std::fs::create_dir(dir) {
            Ok(()) => std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => check_private_dir(dir)?,
            Err(e) => return Err(e).with_context(|| format!("create {}", dir.display())),
        }
    }
    if path.exists() {
        if UnixStream::connect(path).await.is_ok() {
            bail!("another routedroidd is already serving {}", path.display());
        }
        std::fs::remove_file(path).with_context(|| format!("remove stale {}", path.display()))?;
    }
    let listener = UnixListener::bind(path).with_context(|| format!("bind {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// The socket directory must belong to us and be closed to everyone else;
/// a shared directory (say `/tmp`) is refused rather than silently chmod-ed.
fn check_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(dir).with_context(|| format!("stat {}", dir.display()))?;
    if !meta.is_dir() {
        bail!("{} is not a directory", dir.display());
    }
    if meta.uid() != current_uid() {
        bail!("{} is not owned by this user; use --socket with a private directory", dir.display());
    }
    if meta.mode() & 0o077 != 0 {
        bail!(
            "{} is accessible to other users (mode {:o}); use --socket with a private directory",
            dir.display(),
            meta.mode() & 0o777
        );
    }
    Ok(())
}
