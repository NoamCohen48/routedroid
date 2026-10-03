//! The helper's half of `doctor`: what Routedroid left behind and what on
//! the host gets in its way, and with `repair` the changes it made.

use std::path::Path;
use std::time::Duration;

use routedroid_helper_ipc::{Finding, Reply, Request};

use super::helper::{self, request, unexpected};
use crate::fault::{Fault, FaultExt, Kind, Result};

/// Replaying journals waits for nothing remote, but RELEASEs and `nft` take a moment.
const DOCTOR_TIMEOUT: Duration = Duration::from_secs(60);

/// The changes made (none without `repair`), and what is (still) wrong.
pub async fn doctor(socket: &Path, repair: bool) -> Result<(Vec<String>, Vec<Finding>)> {
    let ask = async {
        let conn = helper::connect(socket).await?;
        let what = if repair {
            Request::Repair
        } else {
            Request::Inspect
        };
        match request(&conn, &what).await.fault(Kind::Helper)? {
            Reply::Health { findings } => Ok((Vec::new(), findings)),
            Reply::Repaired { done, remaining } => Ok((done, remaining)),
            Reply::Error { code, message } => Err(Fault::msg(
                Kind::Helper,
                format!("helper could not inspect the host: {code:?}: {message}"),
            )),
            other => Err(unexpected(&other)),
        }
    };
    tokio::time::timeout(DOCTOR_TIMEOUT, ask)
        .await
        .map_err(|_| {
            Fault::msg(
                Kind::Helper,
                format!("helper did not answer within {DOCTOR_TIMEOUT:?}"),
            )
        })?
}
