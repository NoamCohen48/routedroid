//! Recovery after a crash: `check` (ExecStartPre) and `cleanup`
//! (ExecStopPost) act on journals no live helper holds.

use std::path::Path;

use anyhow::{bail, Result};
use tracing::{info, warn};

use crate::fault::CrashHook;
use crate::journal::{self, Journal, Phase};
use crate::ops;
use crate::session::undo_one;

/// `cleanup`: replay every orphaned journal (a live session's journal is
/// locked by its process and left alone). Returns Err if anything remains.
pub fn cleanup(journal_dir: &Path, hook: &CrashHook) -> Result<()> {
    let files = journal::orphaned_sessions(journal_dir)?;
    if files.is_empty() {
        info!("cleanup: nothing to do");
        return Ok(());
    }
    let mut all_ok = true;
    for path in files {
        // Another instance's cleanup may have taken it meanwhile: then it is theirs.
        let (mut journal, lines) = match Journal::open(&path) {
            Ok(opened) => opened,
            Err(error) => {
                info!(journal = %path.display(), %error, "cleanup: skipping");
                continue;
            }
        };
        let todo = journal::unresolved(&lines);
        info!(journal = %path.display(), entries = todo.len(), "cleanup: replaying");
        let mut ok = true;
        for (seq, phase, op) in todo {
            let label = op.label();
            match phase {
                Phase::Pending => {
                    // Intent only: act if the effect is there, otherwise just close the entry.
                    match ops::present(journal.session(), &op) {
                        Ok(true) => {
                            info!(op = %label, "pending entry is present; undoing");
                            if undo_one(&mut journal, seq, &op, hook).is_err() {
                                ok = false;
                            }
                        }
                        Ok(false) => {
                            journal.undone(seq, &op)?;
                            info!(op = %label, "pending entry absent; closed");
                        }
                        Err(e) => {
                            warn!(op = %label, error = %e, "cannot inspect");
                            ok = false;
                        }
                    }
                }
                Phase::Done | Phase::UndoPending => {
                    if undo_one(&mut journal, seq, &op, hook).is_err() {
                        ok = false;
                    }
                }
                Phase::Undone => {}
            }
        }
        if ok {
            journal.resolve()?;
            info!(journal = %path.display(), "resolved");
        } else {
            all_ok = false;
            warn!(journal = %path.display(), "left unresolved");
        }
    }
    if all_ok {
        Ok(())
    } else {
        bail!("cleanup incomplete")
    }
}

/// `check` (ExecStartPre): refuse to start while an orphaned journal exists
/// (other sessions may be live: their journals are locked and fine).
pub fn check(journal_dir: &Path) -> Result<()> {
    let files = journal::orphaned_sessions(journal_dir)?;
    if files.is_empty() {
        Ok(())
    } else {
        bail!("unresolved journal(s): {}", files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "))
    }
}
