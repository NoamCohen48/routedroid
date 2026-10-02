//! Recovery after a crash: `cleanup` (systemd `ExecStopPost`, and every
//! `serve` start) replays the journals no live process holds; `check`
//! reports whether any remain.

use std::path::Path;

use anyhow::{Result, ensure};
use tracing::{info, warn};

use crate::env::Env;
use crate::journal::{self, Taken};
use crate::kernel::Kernel;
use crate::session;

/// Undo every orphaned session, then drop sysctl holders that have no
/// journal at all. Each journal succeeds or fails on its own; any failure
/// makes the whole run fail, after everything replayable was replayed.
pub fn cleanup<K: Kernel>(env: &Env<K>) -> Result<()> {
    let mut failed = 0;
    for (path, taken) in journal::take_all(&env.journal_dir)? {
        let path = path.display();
        match taken {
            Ok(Taken::Orphan(journal)) => {
                info!(journal = %path, "replaying");
                if let Err(error) = session::undo_all(env, journal) {
                    warn!(journal = %path, error = %format!("{error:#}"), "not fully undone");
                    failed += 1;
                }
            }
            Ok(Taken::Live) => info!(journal = %path, "held by a live process; leaving it"),
            Ok(Taken::Gone) => {}
            Err(error) => {
                warn!(journal = %path, error = %format!("{error:#}"), "cannot replay");
                failed += 1;
            }
        }
    }
    if let Err(error) = env
        .claims
        .collect_garbage(&env.kernel, || journal::sessions(&env.journal_dir))
    {
        warn!(error = %format!("{error:#}"), "claims not collected");
        failed += 1;
    }
    ensure!(
        failed == 0,
        "cleanup incomplete ({failed} problem(s)); see the log"
    );
    Ok(())
}

/// Fails while any journal is orphaned or unreadable (live ones are fine).
pub fn check(journal_dir: &Path) -> Result<()> {
    let mut problems = Vec::new();
    for (path, taken) in journal::take_all(journal_dir)? {
        match taken {
            Ok(Taken::Orphan(_)) => problems.push(format!("{} (orphaned)", path.display())),
            Err(error) => problems.push(format!("{} ({error:#})", path.display())),
            Ok(Taken::Live | Taken::Gone) => {}
        }
    }
    ensure!(
        problems.is_empty(),
        "unresolved journal(s): {}",
        problems.join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests;
