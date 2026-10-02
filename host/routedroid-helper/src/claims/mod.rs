//! Reference-counted sysctl ownership shared by every helper instance. A LAN
//! interface's `proxy_arp`/`forwarding` may serve several phones at once:
//! the first session records the baseline and sets the value, later ones
//! only join, and the last one out restores the baseline, but only if the
//! kernel still shows the value Routedroid wrote.
//!
//! One JSON file per key, every update under the directory lock, each write
//! atomic. The claim is written before the sysctl, so a crash in between
//! leaves a record, never an unrecorded change.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::kernel::Kernel;
use crate::op::SysctlKey;
use crate::session_id::SessionId;
use crate::storage;

/// The only value Routedroid ever writes to a claimed key.
pub const ENABLED: &str = "1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    key: SysctlKey,
    /// The value before Routedroid touched the key.
    baseline: String,
    holders: BTreeSet<SessionId>,
}

pub struct Claims {
    dir: PathBuf,
}

impl Claims {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Make `key` read [`ENABLED`] on behalf of `session`.
    pub fn acquire(&self, kernel: &impl Kernel, session: SessionId, key: &SysctlKey) -> Result<()> {
        let _lock = storage::lock(&self.dir)?;
        let path = self.path(key);
        let mut claim = match read(&path)? {
            Some(claim) => claim,
            None => {
                let baseline = kernel
                    .sysctl_read(key)?
                    .with_context(|| format!("{} does not exist", key.ifname))?;
                Claim {
                    key: key.clone(),
                    baseline,
                    holders: BTreeSet::new(),
                }
            }
        };
        claim.holders.insert(session);
        write(&path, &claim)?;
        if kernel.sysctl_read(key)?.as_deref() != Some(ENABLED) {
            kernel.sysctl_write(key, ENABLED)?;
        }
        Ok(())
    }

    /// Drop `session`'s hold; the last holder restores the baseline.
    pub fn release(&self, kernel: &impl Kernel, session: SessionId, key: &SysctlKey) -> Result<()> {
        let _lock = storage::lock(&self.dir)?;
        let path = self.path(key);
        let Some(mut claim) = read(&path)? else {
            return Ok(());
        };
        claim.holders.remove(&session);
        settle(kernel, &path, &claim)
    }

    #[cfg(test)]
    pub fn holds(&self, session: SessionId, key: &SysctlKey) -> Result<bool> {
        Ok(read(&self.path(key))?.is_some_and(|claim| claim.holders.contains(&session)))
    }

    /// Drop holders with no journal left (a record lost to a bug or by hand),
    /// so no key stays claimed forever. `sessions` must be read from the
    /// journal directory after this call started; see `recovery`.
    pub fn collect_garbage(
        &self,
        kernel: &impl Kernel,
        sessions: impl Fn() -> Result<BTreeSet<SessionId>>,
    ) -> Result<()> {
        let _lock = storage::lock(&self.dir)?;
        let live = sessions()?;
        let mut failed = 0;
        for path in self.list()? {
            let result = read(&path).and_then(|claim| {
                let Some(mut claim) = claim else { return Ok(()) };
                let before = claim.holders.len();
                claim.holders.retain(|holder| live.contains(holder));
                if claim.holders.len() == before {
                    return Ok(());
                }
                info!(key = %claim.key, dropped = before - claim.holders.len(), "dropping holders without a journal");
                settle(kernel, &path, &claim)
            });
            if let Err(error) = result {
                warn!(path = %path.display(), error = %format!("{error:#}"), "claim not collected");
                failed += 1;
            }
        }
        anyhow::ensure!(failed == 0, "{failed} claim(s) could not be collected");
        Ok(())
    }

    fn path(&self, key: &SysctlKey) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    fn list(&self) -> Result<Vec<PathBuf>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("read {}", self.dir.display())),
        };
        let mut out = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                out.push(path);
            }
        }
        Ok(out)
    }
}

/// Persist a claim that still has holders, or restore and remove one that has none.
fn settle(kernel: &impl Kernel, path: &Path, claim: &Claim) -> Result<()> {
    if !claim.holders.is_empty() {
        return write(path, claim);
    }
    match kernel.sysctl_read(&claim.key)? {
        Some(current) if current == ENABLED => kernel.sysctl_write(&claim.key, &claim.baseline)?,
        // The interface is gone, or someone else changed the value: not ours to touch.
        Some(_) | None => {}
    }
    fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
    storage::fsync_dir(path.parent().unwrap_or(Path::new(".")))
}

fn read(path: &Path) -> Result<Option<Claim>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(
            serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?,
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

fn write(path: &Path, claim: &Claim) -> Result<()> {
    storage::write_atomically(path, &serde_json::to_vec(claim)?)
}

#[cfg(test)]
mod tests;
