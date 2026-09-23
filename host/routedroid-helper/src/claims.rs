//! Reference-counted sysctl ownership shared by every helper instance.
//! A LAN interface's `proxy_arp`/`forwarding` may serve several phones at
//! once; the first session records the baseline and sets the value, later
//! ones only add themselves, and the last one out restores the baseline —
//! and only if the value is still the one Routedroid wrote.
//!
//! One JSON file per key under the claims directory, all updates under an
//! exclusive `flock` on `<dir>/lock`, written atomically (tmp + rename).

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::ops::{sysctl_read, sysctl_write};

pub const DEFAULT_DIR: &str = "/var/lib/routedroid/sysctl";
static DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn init(dir: PathBuf) {
    let _ = DIR.set(dir);
}

fn dir() -> PathBuf {
    DIR.get().cloned().unwrap_or_else(|| PathBuf::from(DEFAULT_DIR))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// The value before Routedroid touched the key.
    pub baseline: String,
    /// The value Routedroid wrote.
    pub value: String,
    /// Sessions that currently need it.
    pub holders: Vec<String>,
}

fn claim_path(key: &str) -> Result<PathBuf> {
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        bail!("sysctl key {key:?} cannot name a claim file");
    }
    Ok(dir().join(format!("{key}.json")))
}

/// Run `body` with the claims directory locked against other instances.
fn locked<T>(body: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
    let dir = dir();
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let lock = OpenOptions::new().write(true).create(true).truncate(false).open(dir.join("lock"))?;
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::LockExclusive).context("lock claims")?;
    body(&dir)
}

fn read_claim(path: &Path) -> Result<Option<Claim>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

fn write_claim(path: &Path, claim: &Claim) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = File::create(&tmp)?;
    serde_json::to_writer(&mut file, claim)?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}

/// Make `key` equal `value` on behalf of `session`. Returns the baseline.
pub fn acquire(session: &str, key: &str, value: &str) -> Result<String> {
    let path = claim_path(key)?;
    locked(|_| {
        let mut claim = match read_claim(&path)? {
            Some(claim) if claim.value == value => claim,
            Some(claim) => bail!("{key} is claimed with value {} but {value} was asked", claim.value),
            None => Claim { baseline: sysctl_read(key)?, value: value.to_string(), holders: vec![] },
        };
        if !claim.holders.iter().any(|holder| holder == session) {
            claim.holders.push(session.to_string());
        }
        write_claim(&path, &claim)?;
        if sysctl_read(key)? != value {
            sysctl_write(key, value)?;
        }
        Ok(claim.baseline)
    })
}

/// Drop `session`'s claim; the last holder restores the baseline (if the
/// kernel still shows the value Routedroid wrote). Absent claim: nothing.
pub fn release(session: &str, key: &str) -> Result<()> {
    let path = claim_path(key)?;
    locked(|_| {
        let Some(mut claim) = read_claim(&path)? else { return Ok(()) };
        claim.holders.retain(|holder| holder != session);
        if !claim.holders.is_empty() {
            return write_claim(&path, &claim);
        }
        match sysctl_read(key) {
            Ok(current) if current == claim.value => sysctl_write(key, &claim.baseline)?,
            // Someone else changed it, or the interface is gone: not ours to touch.
            Ok(_) | Err(_) => {}
        }
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))
    })
}

/// Whether `session` currently holds a claim on `key`.
pub fn holds(session: &str, key: &str) -> bool {
    claim_path(key)
        .ok()
        .and_then(|path| read_claim(&path).ok().flatten())
        .is_some_and(|claim| claim.holders.iter().any(|holder| holder == session))
}

#[cfg(test)]
mod tests;
