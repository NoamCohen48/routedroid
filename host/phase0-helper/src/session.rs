//! One helper session: the ordered list of journaled mutations, applied on
//! `Start` and undone on `Stop`/disconnect/cleanup. Every step goes through
//! the write-ahead journal and passes a crash hook the kill tests use.

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use tracing::{info, warn};

use crate::journal::{self, Journal, Op, Phase};
use crate::ops;
use crate::proto::valid_ifname;
use crate::tun::Tun;

pub const NFT_TABLE: &str = "routedroid_p0";

/// Test hook: if the crash file's content equals `stage`, SIGKILL ourselves.
/// Root-owned file; absent in normal operation.
pub struct CrashHook(pub PathBuf);

impl CrashHook {
    pub fn at(&self, stage: &str) {
        if let Ok(s) = std::fs::read_to_string(&self.0) {
            if s.trim() == stage {
                warn!(stage, "crash hook: SIGKILL self");
                // SAFETY: plain signal to our own pid.
                unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
            }
        }
    }
}

pub struct Plan {
    pub session: String,
    pub tun: String,
    pub lan_if: String,
    pub phone_ip: Ipv4Addr,
    pub host_ip: Ipv4Addr,
    pub lan_prefix: u8,
    pub mtu: u32,
}

impl Plan {
    /// Validate the request and derive everything else from the kernel.
    pub fn build(session: &str, lan_if: &str, phone_ip: Ipv4Addr, tun: &str, mtu: u32) -> Result<Self> {
        if !valid_ifname(lan_if) || !valid_ifname(tun) || lan_if == tun {
            bail!("bad interface name");
        }
        if !tun.starts_with("phone") {
            bail!("tun name must start with 'phone'");
        }
        if !(576..=9000).contains(&mtu) {
            bail!("mtu out of range");
        }
        if !ops::link_exists(lan_if) {
            bail!("{lan_if} does not exist");
        }
        if ops::link_exists(tun) {
            bail!("{tun} already exists");
        }
        let (host_ip, lan_prefix) = ops::primary_ipv4(lan_if)?;
        let mask = if lan_prefix == 0 { 0 } else { u32::MAX << (32 - lan_prefix) };
        if u32::from(phone_ip) & mask != u32::from(host_ip) & mask {
            bail!("{phone_ip} is not inside {host_ip}/{lan_prefix}");
        }
        if phone_ip == host_ip || phone_ip.is_broadcast() || phone_ip.is_unspecified() || phone_ip.is_multicast() {
            bail!("{phone_ip} is not a usable phone address");
        }
        Ok(Self { session: session.into(), tun: tun.into(), lan_if: lan_if.into(), phone_ip, host_ip, lan_prefix, mtu })
    }

    /// Mutations in application order. Deny-first: nft before anything that could forward.
    /// Sysctl `prev` values are filled in at apply time (the TUN's do not exist yet).
    pub fn ops(&self) -> Vec<Op> {
        let sys = |key: String| Op::Sysctl { key, prev: String::new(), new: "1".into() };
        vec![
            Op::Tun { name: self.tun.clone() },
            Op::NftTable { family: "inet".into(), name: NFT_TABLE.into() },
            sys(format!("net.ipv4.conf.{}.forwarding", self.tun)),
            sys(format!("net.ipv4.conf.{}.forwarding", self.lan_if)),
            sys(format!("net.ipv4.conf.{}.proxy_arp", self.lan_if)),
            Op::Route { dst: self.phone_ip, dev: self.tun.clone(), src: self.host_ip },
        ]
    }
}

pub struct Active {
    pub tun: Tun,
    journal: Journal,
    applied: Vec<(u32, Op)>,
}

/// Apply the plan. On any failure, undo what was applied (journaled) and return the error.
pub fn start(plan: &Plan, journal_dir: &Path, hook: &CrashHook) -> Result<Active> {
    let mut journal = Journal::create(journal_dir, &plan.session)?;
    let mut applied: Vec<(u32, Op)> = Vec::new();
    let mut tun: Option<Tun> = None;
    let rules = ops::nft_rules(NFT_TABLE, &plan.tun, &plan.lan_if, plan.phone_ip, plan.host_ip);

    for mut op in plan.ops() {
        if let Op::Sysctl { key, prev, .. } = &mut op {
            // Read the value to restore *before* recording intent, so the journal is self-contained.
            *prev = match ops::sysctl_read(key) {
                Ok(v) => v,
                Err(e) => {
                    rollback(&mut journal, &mut applied, hook);
                    drop(tun);
                    let _ = journal.resolve();
                    return Err(e).with_context(|| format!("read {key}"));
                }
            };
        }
        let label = op.label();
        let seq = journal.pending(&op)?;
        hook.at(&format!("pending:{label}"));
        let r = match &op {
            Op::Tun { name } => Tun::create(name).and_then(|t| {
                crate::tun::link_up(name, plan.mtu)?;
                tun = Some(t);
                Ok(())
            }),
            _ => ops::apply(&op, Some(&rules)),
        };
        if let Err(e) = r {
            warn!(op = %label, error = %e, "apply failed; rolling back");
            // The intent is journaled as pending: undo it like a crash would.
            let first = undo_one(&mut journal, seq, &op, hook).is_ok();
            let rest = rollback(&mut journal, &mut applied, hook);
            drop(tun);
            if first && rest {
                let _ = journal.resolve();
            }
            return Err(e).with_context(|| format!("apply {label}"));
        }
        hook.at(&format!("applied:{label}"));
        journal.done(seq, &op)?;
        hook.at(&format!("done:{label}"));
        info!(op = %label, "applied");
        applied.push((seq, op));
    }
    Ok(Active { tun: tun.context("plan has no tun op")?, journal, applied })
}

fn undo_one(journal: &mut Journal, seq: u32, op: &Op, hook: &CrashHook) -> Result<()> {
    let label = op.label();
    journal.undo_pending(seq, op)?;
    hook.at(&format!("undo_pending:{label}"));
    let r = ops::undo(op);
    ops::log_undo(op, &r);
    r?;
    hook.at(&format!("undo_applied:{label}"));
    journal.undone(seq, op)?;
    hook.at(&format!("undone:{label}"));
    Ok(())
}

fn rollback(journal: &mut Journal, applied: &mut Vec<(u32, Op)>, hook: &CrashHook) -> bool {
    let mut ok = true;
    while let Some((seq, op)) = applied.pop() {
        if undo_one(journal, seq, &op, hook).is_err() {
            ok = false;
        }
    }
    ok
}

impl Active {
    /// Undo everything in reverse order; resolve the journal only if all undos succeeded.
    pub fn stop(mut self, hook: &CrashHook) -> Result<()> {
        // Close the TUN first so its routes/sysctls vanish with it, then undo the rest.
        drop(self.tun);
        let ok = rollback(&mut self.journal, &mut self.applied, hook);
        if ok {
            self.journal.resolve()?;
            info!("session undone and journal resolved");
            Ok(())
        } else {
            bail!("some undo steps failed; journal left for cleanup")
        }
    }
}

/// `cleanup`: replay every unresolved journal. Returns Err if anything is still present.
pub fn cleanup(journal_dir: &Path, hook: &CrashHook) -> Result<()> {
    let files = journal::open_sessions(journal_dir)?;
    if files.is_empty() {
        info!("cleanup: nothing to do");
        return Ok(());
    }
    let mut all_ok = true;
    for path in files {
        let (mut journal, lines) = Journal::open(&path)?;
        let todo = journal::unresolved(&lines);
        info!(journal = %path.display(), entries = todo.len(), "cleanup: replaying");
        let mut ok = true;
        for (seq, phase, op) in todo {
            let label = op.label();
            match phase {
                Phase::Pending => {
                    // Intent only: act if the effect is there, otherwise just close the entry.
                    match ops::present(&op) {
                        Ok(true) => {
                            info!(op = %label, "pending entry is present; undoing");
                            if undo_one(&mut journal, seq, &op, hook).is_err() { ok = false; }
                        }
                        Ok(false) => { journal.undone(seq, &op)?; info!(op = %label, "pending entry absent; closed"); }
                        Err(e) => { warn!(op = %label, error = %e, "cannot inspect"); ok = false; }
                    }
                }
                Phase::Done | Phase::UndoPending => {
                    if undo_one(&mut journal, seq, &op, hook).is_err() { ok = false; }
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
    if all_ok { Ok(()) } else { bail!("cleanup incomplete") }
}

/// `check` (ExecStartPre): refuse to start while any unresolved journal exists.
pub fn check(journal_dir: &Path) -> Result<()> {
    let files = journal::open_sessions(journal_dir)?;
    if files.is_empty() {
        Ok(())
    } else {
        bail!("unresolved journal(s): {}", files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "))
    }
}
