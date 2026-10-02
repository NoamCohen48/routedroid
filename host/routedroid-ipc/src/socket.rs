//! Where the control socket lives.

use std::path::PathBuf;

pub const SOCKET_ENV: &str = "ROUTEDROID_SOCKET";

/// `$ROUTEDROID_SOCKET`, else `$XDG_RUNTIME_DIR/routedroid/control.sock`,
/// else `/run/user/<uid>/routedroid/control.sock`.
pub fn default_path() -> PathBuf {
    if let Ok(p) = std::env::var(SOCKET_ENV) {
        return PathBuf::from(p);
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(format!("/run/user/{}", rustix::process::getuid().as_raw())));
    runtime.join("routedroid").join("control.sock")
}
