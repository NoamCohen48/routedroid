//! Write-ahead journal for every kernel mutation (architecture §5.3): intent
//! is fsynced before the mutation and completion after it, the same again
//! for each undo; `cleanup` replays whatever a dead session left behind.
//!
//! One file per session, `<dir>/<session>.journal`, mode 0600. The owning
//! process holds an exclusive `flock` on it for the session's whole life;
//! the lock follows the inode, so the file is created as a locked temporary
//! and renamed into place (never visible unlocked). A resolved journal is
//! unlinked, which also releases its reservation.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use rustix::fs::{FlockOperation, RenameFlags, CWD};

use crate::op::Op;
use crate::storage;
use crate::session_id::SessionId;

mod dir;
mod record;
mod scan;

pub use record::{Header, Phase, Reservation, Step, VERSION};
pub use scan::{sessions, take_all};

use record::{Record, Steps};

pub struct Journal {
    path: PathBuf,
    file: File,
    header: Header,
    steps: Steps,
    /// A write failed: the file may end in a partial line, so nothing more
    /// may be appended. What reached the disk is for recovery to read.
    poisoned: bool,
}

/// What `take` found at a journal path.
pub enum Taken {
    /// Another process holds it: a live session, or another cleanup.
    Live,
    /// Resolved (unlinked) between listing and locking.
    Gone,
    Orphan(Journal),
}

impl Journal {
    /// Reserve and create; refused if any journal, live or orphaned, holds
    /// the same TUN name or phone address, or cannot be read to tell.
    pub fn create(dir: &Path, session: SessionId, reservation: Reservation) -> Result<Self> {
        let _lock = storage::lock(dir)?;
        for path in dir::list(dir, dir::SUFFIX)? {
            let other = dir::read_header(&path).context("cannot rule out a conflicting reservation")?;
            if other.reservation.conflicts(&reservation) {
                bail!("{} or {} is held by session {}", reservation.tun, reservation.phone_ip, other.session);
            }
        }
        let header = Header { version: VERSION, session, reservation };
        let path = dir.join(format!("{session}.{}", dir::SUFFIX));
        let tmp = dir.join(format!("{session}.{}", dir::TMP_SUFFIX));
        let mut file = storage::private_file(OpenOptions::new().append(true).create_new(true), &tmp)?;
        rustix::fs::flock(&file, FlockOperation::NonBlockingLockExclusive).context("lock new journal")?;
        let written = write_line(&mut file, &header)
            .and_then(|()| Ok(rustix::fs::renameat_with(CWD, &tmp, CWD, &path, RenameFlags::NOREPLACE)?))
            .and_then(|()| storage::fsync_dir(dir));
        if let Err(e) = written {
            let _ = fs::remove_file(&tmp);
            return Err(e).with_context(|| format!("create {}", path.display()));
        }
        Ok(Self { path, file, header, steps: Steps::default(), poisoned: false })
    }

    /// Lock an existing journal for recovery, then (and only then) read it.
    pub fn take(path: &Path) -> Result<Taken> {
        let mut file = match OpenOptions::new().read(true).append(true).open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Taken::Gone),
            Err(e) => return Err(e).with_context(|| format!("open {}", path.display())),
        };
        match rustix::fs::flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {}
            Err(rustix::io::Errno::WOULDBLOCK) => return Ok(Taken::Live),
            Err(e) => return Err(e).with_context(|| format!("lock {}", path.display())),
        }
        if rustix::fs::fstat(&file)?.st_nlink == 0 {
            return Ok(Taken::Gone);
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).with_context(|| format!("read {}", path.display()))?;
        let parsed = record::parse(&bytes).with_context(|| format!("parse {}", path.display()))?;
        let stem = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".journal"));
        ensure!(stem == Some(&parsed.header.session.to_string()), "{} names another session", path.display());
        if parsed.complete < bytes.len() {
            tracing::warn!(journal = %path.display(), "dropping a torn final line");
            file.set_len(parsed.complete as u64)?;
            file.sync_all()?;
        }
        let (header, steps) = (parsed.header, parsed.steps);
        Ok(Taken::Orphan(Self { path: path.to_owned(), file, header, steps, poisoned: false }))
    }

    pub fn session(&self) -> SessionId {
        self.header.session
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Record the intent to apply `op`; returns its step number.
    pub fn intend(&mut self, op: Op) -> Result<u32> {
        let seq = self.steps.next_seq();
        self.append(Record { seq, phase: Phase::Pending, op: Some(op) })?;
        Ok(seq)
    }

    pub fn advance(&mut self, seq: u32, phase: Phase) -> Result<()> {
        self.append(Record { seq, phase, op: None })
    }

    pub fn outstanding(&self) -> Vec<(u32, Step)> {
        self.steps.outstanding()
    }

    /// Delete the journal; only legal once every step is undone.
    pub fn resolve(self) -> Result<()> {
        ensure!(self.steps.outstanding().is_empty(), "{} still has steps to undo", self.path.display());
        fs::remove_file(&self.path).with_context(|| format!("remove {}", self.path.display()))?;
        storage::fsync_dir(self.path.parent().unwrap_or(Path::new(".")))
    }

    fn append(&mut self, record: Record) -> Result<()> {
        ensure!(!self.poisoned, "{} failed an earlier write", self.path.display());
        self.steps.check(&record)?;
        if let Err(e) = write_line(&mut self.file, &record) {
            self.poisoned = true;
            return Err(e).with_context(|| format!("append to {}", self.path.display()));
        }
        self.steps.apply(record)
    }
}

fn write_line(file: &mut File, value: &impl serde::Serialize) -> Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    file.write_all(&line)?;
    file.sync_data()?;
    Ok(())
}

#[cfg(test)]
mod tests;
