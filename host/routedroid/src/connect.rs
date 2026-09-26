//! Opening the control socket. Why it failed decides the advice and the
//! exit code: nobody listening is not the same as an outdated daemon.

use std::path::Path;

use anyhow::Result;
use routedroid_ipc::{Client, ConnectError};

/// Exit code when the daemon socket cannot be reached.
pub const EXIT_DAEMON_UNREACHABLE: i32 = 3;
/// Exit code when the daemon speaks another API version.
pub const EXIT_DAEMON_INCOMPATIBLE: i32 = 4;

pub async fn connect(path: &Path) -> Result<Client> {
    Ok(Client::connect(path).await?)
}

/// Prints the error and a hint; `None` if `error` is not a connect error.
pub fn report(error: &anyhow::Error) -> Option<i32> {
    let error = error.downcast_ref::<ConnectError>()?;
    eprintln!("error: {error}");
    Some(match error {
        ConnectError::Unreachable { .. } => {
            eprintln!("hint: start the daemon with `systemctl --user start routedroid`");
            EXIT_DAEMON_UNREACHABLE
        }
        ConnectError::Incompatible { .. } => {
            eprintln!("hint: routedroid and routedroidd are from different releases; restart the daemon after an upgrade (`systemctl --user restart routedroid`)");
            EXIT_DAEMON_INCOMPATIBLE
        }
        ConnectError::Handshake(_) => routedroid_ipc::Kind::Internal.exit_code(),
    })
}
