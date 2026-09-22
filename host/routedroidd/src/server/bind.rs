//! Binding the control socket: a private directory, no other daemon on it,
//! mode 0600. Free functions — they own no state and touch no daemon.

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use anyhow::{bail, Context, Result};
use tokio::net::{UnixListener, UnixStream};

pub fn current_uid() -> u32 {
    std::fs::metadata("/proc/self").expect("/proc/self").uid()
}

/// Create the socket's directory (0700) if missing — an existing one must be
/// ours and private — refuse if another daemon answers, replace a stale
/// socket file, and bind with mode 0600.
pub async fn listen(path: &Path) -> Result<UnixListener> {
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
