//! Write-ahead journal for every external mutation (architecture §5.3):
//! persist + fsync *intent* before the mutation, apply, persist + fsync
//! *done*; the same again for each undo. `cleanup` replays whatever is left.
//!
//! One file per session: `<dir>/<session>.journal`, one JSON entry per line.
//! A session is resolved when its last line is `{"resolved":true}`; resolved
//! files are removed. Anything else on disk blocks a new start (`check`).

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// One reversible mutation. Each variant records what is needed to inspect
/// the kernel and to undo it without consulting anything but this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Non-persistent TUN; owned by the helper's fd, so it vanishes with the
    /// process. Recorded so cleanup can verify (and delete a leftover).
    Tun { name: String },
    Sysctl { key: String, prev: String, new: String },
    Route { dst: Ipv4Addr, dev: String, src: Ipv4Addr },
    NftTable { family: String, name: String },
}

impl Op {
    pub fn label(&self) -> String {
        match self {
            Op::Tun { name } => format!("tun:{name}"),
            Op::Sysctl { key, .. } => format!("sysctl:{key}"),
            Op::Route { dst, dev, .. } => format!("route:{dst}/32@{dev}"),
            Op::NftTable { family, name } => format!("nft:{family}:{name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Intent recorded; the mutation may or may not have happened.
    Pending,
    /// Mutation confirmed applied.
    Done,
    /// Undo intent recorded; the undo may or may not have happened.
    UndoPending,
    /// Undo confirmed.
    Undone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Line {
    Entry { seq: u32, phase: Phase, #[serde(flatten)] op: Op },
    Resolved { resolved: bool },
}

pub struct Journal {
    path: PathBuf,
    file: File,
    next_seq: u32,
}

impl Journal {
    pub fn path_for(dir: &Path, session: &str) -> PathBuf {
        dir.join(format!("{session}.journal"))
    }

    /// Create a fresh journal; fails if one for this session exists.
    pub fn create(dir: &Path, session: &str) -> Result<Self> {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        let path = Self::path_for(dir, session);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| format!("create journal {}", path.display()))?;
        fsync_dir(dir)?;
        Ok(Self { path, file, next_seq: 1 })
    }

    /// Open an existing journal for appending undo/resolution records.
    pub fn open(path: &Path) -> Result<(Self, Vec<Line>)> {
        let lines = read_lines(path)?;
        let next_seq = lines
            .iter()
            .filter_map(|l| if let Line::Entry { seq, .. } = l { Some(*seq) } else { None })
            .max()
            .unwrap_or(0)
            + 1;
        let file = OpenOptions::new().append(true).open(path).with_context(|| format!("open {}", path.display()))?;
        Ok((Self { path: path.to_path_buf(), file, next_seq }, lines))
    }

    /// Append one line and fsync before returning.
    fn append(&mut self, line: &Line) -> Result<()> {
        let mut s = serde_json::to_string(line)?;
        s.push('\n');
        self.file.write_all(s.as_bytes()).context("journal write")?;
        self.file.sync_all().context("journal fsync")?;
        Ok(())
    }

    /// Record intent; returns the seq to pass to `done`.
    pub fn pending(&mut self, op: &Op) -> Result<u32> {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.append(&Line::Entry { seq, phase: Phase::Pending, op: op.clone() })?;
        Ok(seq)
    }

    pub fn done(&mut self, seq: u32, op: &Op) -> Result<()> {
        self.append(&Line::Entry { seq, phase: Phase::Done, op: op.clone() })
    }

    pub fn undo_pending(&mut self, seq: u32, op: &Op) -> Result<()> {
        self.append(&Line::Entry { seq, phase: Phase::UndoPending, op: op.clone() })
    }

    pub fn undone(&mut self, seq: u32, op: &Op) -> Result<()> {
        self.append(&Line::Entry { seq, phase: Phase::Undone, op: op.clone() })
    }

    /// Mark resolved and remove the file (a resolved journal is never needed again).
    pub fn resolve(mut self) -> Result<()> {
        self.append(&Line::Resolved { resolved: true })?;
        let dir = self.path.parent().map(Path::to_path_buf).unwrap_or_default();
        fs::remove_file(&self.path).with_context(|| format!("remove {}", self.path.display()))?;
        fsync_dir(&dir)?;
        Ok(())
    }
}

fn fsync_dir(dir: &Path) -> Result<()> {
    File::open(dir).and_then(|d| d.sync_all()).with_context(|| format!("fsync {}", dir.display()))
}

pub fn read_lines(path: &Path) -> Result<Vec<Line>> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut out = Vec::new();
    for (n, line) in BufReader::new(f).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue; // torn final write after a crash mid-line
        }
        match serde_json::from_str::<Line>(&line) {
            Ok(l) => out.push(l),
            Err(e) => bail!("{}:{}: unparseable journal line ({e}); refusing to guess", path.display(), n + 1),
        }
    }
    Ok(out)
}

/// The entries that still need attention, newest first (undo in reverse
/// order of application). An op whose last phase is `Undone` needs nothing.
pub fn unresolved(lines: &[Line]) -> Vec<(u32, Phase, Op)> {
    let mut latest: Vec<(u32, Phase, Op)> = Vec::new();
    for l in lines {
        if let Line::Entry { seq, phase, op } = l {
            if let Some(slot) = latest.iter_mut().find(|(s, _, _)| s == seq) {
                slot.1 = phase.clone();
            } else {
                latest.push((*seq, phase.clone(), op.clone()));
            }
        }
    }
    latest.retain(|(_, phase, _)| *phase != Phase::Undone);
    latest.sort_by(|a, b| b.0.cmp(&a.0));
    latest
}

pub fn is_resolved(lines: &[Line]) -> bool {
    matches!(lines.last(), Some(Line::Resolved { resolved: true }))
}

/// All journal files in `dir` that are not resolved.
pub fn open_sessions(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e).with_context(|| format!("read {}", dir.display())),
    };
    for ent in rd {
        let p = ent?.path();
        if p.extension().and_then(|e| e.to_str()) != Some("journal") {
            continue;
        }
        if !is_resolved(&read_lines(&p)?) {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(n: u8) -> Op {
        Op::Sysctl { key: format!("k{n}"), prev: "0".into(), new: "1".into() }
    }

    #[test]
    fn append_and_replay() {
        let dir = std::env::temp_dir().join(format!("rd-journal-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut j = Journal::create(&dir, "s1").unwrap();
        assert!(Journal::create(&dir, "s1").is_err(), "duplicate session must be refused");
        let a = j.pending(&op(1)).unwrap();
        j.done(a, &op(1)).unwrap();
        let b = j.pending(&op(2)).unwrap();
        j.done(b, &op(2)).unwrap();
        let c = j.pending(&op(3)).unwrap(); // crashed after intent
        j.undo_pending(a, &op(1)).unwrap(); // (out of order on purpose)
        j.undone(a, &op(1)).unwrap();

        let path = Journal::path_for(&dir, "s1");
        let lines = read_lines(&path).unwrap();
        assert!(!is_resolved(&lines));
        let u = unresolved(&lines);
        assert_eq!(u.iter().map(|(s, p, _)| (*s, p.clone())).collect::<Vec<_>>(), vec![(c, Phase::Pending), (b, Phase::Done)]);
        assert_eq!(open_sessions(&dir).unwrap(), vec![path.clone()]);

        // Torn last line is tolerated; garbage is not.
        fs::write(&path, fs::read(&path).unwrap().into_iter().chain(b"{\"seq\":9,\"pha".iter().copied()).collect::<Vec<u8>>()).unwrap();
        assert!(read_lines(&path).is_err());
        let content = fs::read_to_string(&path).unwrap();
        fs::write(&path, content.rsplit_once('\n').map(|(a, _)| format!("{a}\n")).unwrap()).unwrap();
        assert_eq!(read_lines(&path).unwrap().len(), lines.len());

        let (j2, _) = Journal::open(&path).unwrap();
        j2.resolve().unwrap();
        assert!(!path.exists());
        assert!(open_sessions(&dir).unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
