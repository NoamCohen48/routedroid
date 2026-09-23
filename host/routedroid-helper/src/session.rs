//! One helper session: the ordered list of journaled mutations, applied on
//! `Start` and undone on `Stop`/disconnect/cleanup. Every step goes through
//! the write-ahead journal and passes a crash hook the kill tests use.

use std::net::Ipv4Addr;
use std::path::Path;

use anyhow::{bail, Context, Result};
use tracing::{info, warn};

use crate::fault::CrashHook;
use crate::journal::{Journal, Op};
use crate::ops;
use crate::proto::valid_ifname;
use crate::tun::Tun;

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
        if ops::host_route_exists(phone_ip) {
            bail!("{phone_ip} is already served by another session");
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
    /// Sysctl `prev` values are filled in at apply time (the TUN's do not exist yet);
    /// they are informational — restoring goes through the shared claims.
    pub fn ops(&self) -> Vec<Op> {
        let sys = |key: String| Op::Sysctl { key, prev: String::new(), new: "1".into() };
        vec![
            Op::Tun { name: self.tun.clone() },
            Op::NftTable { family: "inet".into(), name: ops::nft_table_name(&self.tun) },
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
    let table = ops::nft_table_name(&plan.tun);
    let rules = ops::nft_rules(&table, &plan.tun, &plan.lan_if, plan.phone_ip, plan.host_ip);

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
            _ => ops::apply(&plan.session, &op, Some(&rules)),
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

pub(crate) fn undo_one(journal: &mut Journal, seq: u32, op: &Op, hook: &CrashHook) -> Result<()> {
    let label = op.label();
    journal.undo_pending(seq, op)?;
    hook.at(&format!("undo_pending:{label}"));
    let r = ops::undo(journal.session(), op);
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
