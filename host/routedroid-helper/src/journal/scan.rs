//! Whole-directory views for recovery and claims garbage collection.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::info;

use super::{dir, Journal, Taken};
use crate::session_id::SessionId;
use crate::storage;

/// Try to take every journal in `dir`, each with its own outcome, so one
/// unreadable file never hides the others. Runs under the directory lock:
/// two scans never race for the same file, and a temporary left by a
/// creator that died before its rename is removed (it recorded nothing).
pub fn take_all(dir: &Path) -> Result<Vec<(PathBuf, Result<Taken>)>> {
    let _lock = storage::lock(dir)?;
    for tmp in dir::list(dir, dir::TMP_SUFFIX)? {
        info!(path = %tmp.display(), "removing a journal that was never created");
        fs::remove_file(&tmp).with_context(|| format!("remove {}", tmp.display()))?;
    }
    Ok(dir::list(dir, dir::SUFFIX)?.into_iter().map(|path| (path.clone(), Journal::take(&path))).collect())
}

/// Every session that still has a journal, live or orphaned.
pub fn sessions(dir: &Path) -> Result<BTreeSet<SessionId>> {
    let mut out = BTreeSet::new();
    for path in dir::list(dir, dir::SUFFIX)? {
        let stem = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".journal"));
        let session = stem.unwrap_or_default().parse().with_context(|| format!("name of {}", path.display()))?;
        out.insert(session);
    }
    Ok(out)
}
