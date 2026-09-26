//! One applied session as an RAII guard. Every mutation goes through the
//! write-ahead journal and passes the crash hook; teardown has exactly one
//! path, [`undo_all`], which `stop`, a failed `start`, `Drop` and recovery
//! all use. The journal is resolved only once every step is undone.

use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{error, info, warn};

use crate::env::Env;
use crate::journal::{Journal, Phase, Step};
use crate::kernel::Kernel;
use crate::op::Op;
use crate::plan::Plan;

mod step;

pub struct Session<K: Kernel> {
    env: Arc<Env<K>>,
    plan: Plan,
    /// `None` once torn down.
    journal: Option<Journal>,
    tun: Option<K::Tun>,
}

impl<K: Kernel> Session<K> {
    /// Apply the plan. On failure everything applied so far is undone before
    /// returning; if that fails too, the journal is left for `cleanup`.
    pub fn start(env: Arc<Env<K>>, plan: Plan) -> Result<Self> {
        let journal = Journal::create(&env.journal_dir, plan.session(), plan.reservation())?;
        let mut session = Self { env, plan, journal: Some(journal), tun: None };
        for op in session.plan.ops() {
            if let Err(error) = session.apply(op) {
                return Err(match session.teardown() {
                    Ok(()) => error,
                    Err(undo) => error.context(format!("rollback failed too: {undo:#}")),
                });
            }
        }
        Ok(session)
    }

    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    pub fn tun(&self) -> Option<&K::Tun> {
        self.tun.as_ref()
    }

    pub fn stop(mut self) -> Result<()> {
        self.teardown()
    }

    fn apply(&mut self, op: Op) -> Result<()> {
        let hook = &self.env.hook;
        let label = op.label();
        let journal = self.journal.as_mut().context("session already torn down")?;
        let seq = journal.intend(op.clone())?;
        hook.at(&format!("pending:{label}"));
        step::apply(&self.env, &self.plan, &op, &mut self.tun).with_context(|| format!("apply {label}"))?;
        hook.at(&format!("applied:{label}"));
        journal.advance(seq, Phase::Done)?;
        hook.at(&format!("done:{label}"));
        info!(op = %label, "applied");
        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        // Closing the TUN first takes its routes with it.
        drop(self.tun.take());
        match self.journal.take() {
            Some(journal) => undo_all(&self.env, journal),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
impl<K: Kernel> Session<K> {
    /// What SIGKILL does: the TUN fd and the journal lock go, nothing is undone.
    pub fn crash(mut self) {
        drop(self.tun.take());
        drop(self.journal.take());
    }
}

impl<K: Kernel> Drop for Session<K> {
    fn drop(&mut self) {
        if self.journal.is_some() {
            warn!(session = %self.plan.session(), "session dropped without stop; undoing");
            if let Err(e) = self.teardown() {
                error!(error = %format!("{e:#}"), "undo failed; journal left for cleanup");
            }
        }
    }
}

/// Undo every outstanding step, newest first, and resolve the journal.
/// Stops at the first failure: undoing an older step (the firewall) while a
/// newer one (the route) is still in place would break deny-first. The
/// journal then stays for the next `cleanup`.
pub fn undo_all<K: Kernel>(env: &Env<K>, mut journal: Journal) -> Result<()> {
    for (seq, step) in journal.outstanding() {
        undo_step(env, &mut journal, seq, &step)
            .with_context(|| format!("{} kept for cleanup", journal.path().display()))?;
    }
    let session = journal.session();
    journal.resolve()?;
    info!(%session, "session undone; journal resolved");
    Ok(())
}

fn undo_step<K: Kernel>(env: &Env<K>, journal: &mut Journal, seq: u32, step: &Step) -> Result<()> {
    let label = step.op.label();
    if step.phase != Phase::UndoPending {
        journal.advance(seq, Phase::UndoPending)?;
    }
    env.hook.at(&format!("undo_pending:{label}"));
    step::undo(env, journal.session(), &step.op).with_context(|| format!("undo {label}"))?;
    env.hook.at(&format!("undo_applied:{label}"));
    journal.advance(seq, Phase::Undone)?;
    env.hook.at(&format!("undone:{label}"));
    info!(op = %label, "undone");
    Ok(())
}

#[cfg(test)]
mod tests;
