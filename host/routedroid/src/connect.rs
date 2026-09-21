//! Opening the control socket, with a distinct error when nobody listens.

use std::path::{Path, PathBuf};

use anyhow::Result;
use routedroid_ipc::Client;

/// Exit code when the daemon socket cannot be reached.
pub const EXIT_DAEMON_UNREACHABLE: i32 = 3;

#[derive(Debug)]
pub struct DaemonUnreachable {
    path: PathBuf,
    source: anyhow::Error,
}

impl std::fmt::Display for DaemonUnreachable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "cannot reach routedroidd at {}: {}", self.path.display(), self.source.root_cause())
    }
}

impl std::error::Error for DaemonUnreachable {}

pub async fn connect(path: &Path) -> Result<Client> {
    Client::connect(path).await.map_err(|source| DaemonUnreachable { path: path.to_path_buf(), source }.into())
}
