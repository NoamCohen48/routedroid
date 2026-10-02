//! Private on-disk state: directories 0700, files 0600, an exclusive
//! directory lock, and durable atomic replacement.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;

use anyhow::{Context, Result};
use rustix::fs::FlockOperation;

pub fn ensure_dir(dir: &Path) -> Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("create {}", dir.display()))
}

pub fn private_file(options: &mut OpenOptions, path: &Path) -> Result<File> {
    options
        .mode(0o600)
        .open(path)
        .with_context(|| format!("open {}", path.display()))
}

/// Exclusive for as long as the value lives. Blocking: holders keep it briefly.
pub struct DirLock(#[allow(dead_code)] File);

pub fn lock(dir: &Path) -> Result<DirLock> {
    ensure_dir(dir)?;
    let path = dir.join(".lock");
    let file = private_file(
        OpenOptions::new().write(true).create(true).truncate(false),
        &path,
    )?;
    rustix::fs::flock(&file, FlockOperation::LockExclusive)
        .with_context(|| format!("lock {}", path.display()))?;
    Ok(DirLock(file))
}

pub fn fsync_dir(dir: &Path) -> Result<()> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .with_context(|| format!("fsync {}", dir.display()))
}

/// Replace `path` with `bytes` so a crash leaves either the old or the new file.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = private_file(
        OpenOptions::new().write(true).create(true).truncate(true),
        &tmp,
    )?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("rename {}", tmp.display()))?;
    fsync_dir(path.parent().unwrap_or(Path::new(".")))
}
