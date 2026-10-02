//! Binding the control socket: a private directory, one daemon per socket,
//! mode 0600 from the start. Free functions; they own no daemon state.

use std::fs::{DirBuilder, File, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

use anyhow::{Context, Result, bail};
use rustix::fs::{FlockOperation, Mode, flock};
use tokio::net::UnixListener;

pub fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// The bound socket and the lock that makes it ours; the socket may only be
/// unlinked while the lock is held.
pub struct Bound {
    pub listener: UnixListener,
    _lock: File,
}

/// Create the socket's directory (0700) if missing (an existing one must be
/// ours and private), take `<socket>.lock` so no second daemon can race us,
/// replace whatever socket file a dead daemon left, and bind with mode 0600.
pub fn listen(path: &Path) -> Result<Bound> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        match DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => check_private_dir(dir)?,
            Err(e) => return Err(e).with_context(|| format!("create {}", dir.display())),
        }
    }
    let lock_path = path.with_extension("lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(&lock_path)
        .with_context(|| format!("open {}", lock_path.display()))?;
    if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
        bail!("another routedroidd is already serving {}", path.display());
    }
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("remove stale {}", path.display())),
    }
    // The umask covers the window between bind() creating the file and any
    // chmod: the socket is never reachable by anyone else, not even briefly.
    let old = rustix::process::umask(Mode::from_raw_mode(0o177));
    let listener = UnixListener::bind(path).with_context(|| format!("bind {}", path.display()));
    rustix::process::umask(old);
    Ok(Bound {
        listener: listener?,
        _lock: lock,
    })
}

/// The socket directory must belong to us and be closed to everyone else;
/// a shared directory (say `/tmp`) is refused rather than silently chmod-ed.
fn check_private_dir(dir: &Path) -> Result<()> {
    let meta = std::fs::metadata(dir).with_context(|| format!("stat {}", dir.display()))?;
    if !meta.is_dir() {
        bail!("{} is not a directory", dir.display());
    }
    if meta.uid() != current_uid() {
        bail!(
            "{} is not owned by this user; use --socket with a private directory",
            dir.display()
        );
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
