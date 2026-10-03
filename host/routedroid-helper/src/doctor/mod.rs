//! `doctor`: what Routedroid left behind, and what on the host gets in its
//! way. `inspect` changes nothing (beyond trimming a journal's torn final
//! line, as any read for recovery does); `repair` replays orphaned journals
//! as `cleanup` does, removes leftovers no journal accounts for, then looks
//! again. Only objects proven Routedroid's are ever touched.

use anyhow::Result;
use routedroid_helper_ipc::Finding;
use tracing::{info, warn};

use crate::env::Env;
use crate::journal::{self, Taken};
use crate::kernel::Kernel;
use crate::recovery;

mod leftover;

/// `routedroid-helper doctor`: the findings on stdout, and the changes
/// made with `--repair`.
pub fn run<K: Kernel>(env: &Env<K>, fix: bool) -> Result<()> {
    let findings = if fix {
        let (done, remaining) = repair(env)?;
        done.iter().for_each(|change| println!("done: {change}"));
        remaining
    } else {
        inspect(env)?
    };
    for finding in &findings {
        let warning = if finding.warning { "warning: " } else { "" };
        println!("{warning}{}: {}", finding.subject, finding.problem);
        let verb = if fix { "failed" } else { "would" };
        finding
            .repair
            .iter()
            .for_each(|change| println!("  {verb}: {change}"));
    }
    // Warnings are the host's own setup: said, but nothing Routedroid can repair.
    let problems = findings.iter().filter(|f| !f.warning).count();
    anyhow::ensure!(problems == 0, "{problems} problem(s)");
    println!("nothing to repair");
    Ok(())
}

pub fn inspect<K: Kernel>(env: &Env<K>) -> Result<Vec<Finding>> {
    let mut findings = journals(env)?;
    findings.extend(leftover::find(env)?.iter().map(leftover::Leftover::finding));
    findings.extend(env.kernel.forward_drops()?.into_iter().map(|chain| {
        Finding {
            subject: format!("nft {chain}"),
            problem: "drops forwarded traffic by default, so it may drop the phones' too; \
                          let iifname/oifname \"phone*\" through it (Routedroid's own table \
                          already limits what they carry)"
                .into(),
            warning: true,
            repair: vec![],
        }
    }));
    Ok(findings)
}

/// The changes made, and what is still wrong afterwards.
pub fn repair<K: Kernel>(env: &Env<K>) -> Result<(Vec<String>, Vec<Finding>)> {
    let before = inspect(env)?;
    if let Err(e) = recovery::cleanup(env) {
        warn!(error = %format!("{e:#}"), "cleanup incomplete");
    }
    for leftover in leftover::find(env)? {
        match leftover.remove(&env.kernel) {
            Ok(()) => info!(?leftover, "removed"),
            Err(e) => warn!(?leftover, error = %format!("{e:#}"), "not removed"),
        }
    }
    // Collects the claims of sessions whose leftovers just went.
    if let Err(e) = recovery::cleanup(env) {
        warn!(error = %format!("{e:#}"), "cleanup incomplete");
    }
    let after = inspect(env)?;
    let done = before
        .iter()
        .filter(|f| !after.contains(f))
        .flat_map(|f| f.repair.clone())
        .collect();
    Ok((done, after))
}

/// Orphaned journals with the undo each still needs, and unreadable ones.
fn journals<K: Kernel>(env: &Env<K>) -> Result<Vec<Finding>> {
    let mut out = Vec::new();
    for (path, taken) in journal::peek_all(&env.journal_dir)? {
        let path = path.display();
        match taken {
            Ok(Taken::Orphan(journal)) => out.push(Finding {
                subject: format!("session {}", journal.session()),
                problem: format!("orphaned journal {path}"),
                warning: false,
                repair: journal
                    .outstanding()
                    .iter()
                    .map(|(_, step)| format!("undo {}", step.op.label()))
                    .collect(),
            }),
            Ok(Taken::Live | Taken::Gone) => {}
            Err(e) => out.push(Finding {
                subject: format!("journal {path}"),
                problem: format!("unreadable: {e:#}; inspect and remove it by hand"),
                warning: false,
                repair: vec![],
            }),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
