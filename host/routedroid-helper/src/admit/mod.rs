//! Who may control the helper. The socket is open to every local user, so
//! this is the gate: each controller is checked before its first request
//! is read, and a refused one is told why.
//!
//! The group is looked up in the database as it is now, not in the
//! caller's credentials, which are fixed at login: a user `setup` has just
//! added to the group may connect without logging out.

use std::time::Duration;

use anyhow::{Result, bail};
use routedroid_helper_ipc::{ErrorCode, Reply, SeqPacket};
use tokio::task::spawn_blocking;
use tokio::time::timeout;
use tracing::{info, warn};

mod members;

#[cfg(test)]
mod tests;

/// A slow NSS source (LDAP, say) refuses rather than hangs the helper.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Default, Clone)]
pub struct Gate {
    /// Only this uid.
    pub uid: Option<u32>,
    /// Only root and this group's members.
    pub group: Option<String>,
}

pub async fn admit(gate: &Gate, conn: SeqPacket) -> Result<SeqPacket> {
    let uid = conn.peer_uid()?;
    if let Err(why) = check(gate, uid).await {
        warn!(uid, "rejecting controller: {why}");
        let refusal = Reply::Error {
            code: ErrorCode::Refused,
            message: why.clone(),
        };
        let _ = conn.send_control(&refusal).await;
        bail!("controller uid {uid} refused: {why}");
    }
    info!(uid, "controller connected");
    Ok(conn)
}

/// `Err` says why `uid` may not control the helper.
async fn check(gate: &Gate, uid: u32) -> Result<(), String> {
    if let Some(want) = gate.uid
        && uid != want
    {
        return Err(format!("uid {uid} is not allowed"));
    }
    let Some(group) = gate.group.clone() else {
        return Ok(());
    };
    if uid == 0 {
        return Ok(());
    }
    let name = group.clone();
    let lookup = spawn_blocking(move || members::is_member(uid, &name));
    match timeout(LOOKUP_TIMEOUT, lookup).await {
        Ok(Ok(Ok(true))) => Ok(()),
        Ok(Ok(Ok(false))) => Err(format!(
            "uid {uid} is not in group {group}; `sudo routedroid setup` adds it"
        )),
        Ok(Ok(Err(error))) => Err(format!("cannot look up uid {uid}'s groups: {error}")),
        Ok(Err(error)) => Err(format!("the group lookup failed: {error}")),
        Err(_) => Err(format!(
            "looking up uid {uid}'s groups took over {LOOKUP_TIMEOUT:?}"
        )),
    }
}
