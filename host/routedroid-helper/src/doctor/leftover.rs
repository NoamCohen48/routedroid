//! Objects proven Routedroid's (a `routedroid:<session>` tag, or route
//! protocol 82 in a phone's egress table) that no journal accounts for: a
//! journal lost to a bug or deleted by hand. Objects are listed before the
//! journals are read: a session writes its journal before its first object,
//! so one starting meanwhile is never mistaken for a leftover.

use std::collections::BTreeSet;
use std::net::Ipv4Addr;

use anyhow::Result;
use routedroid_helper_ipc::Finding;

use crate::env::Env;
use crate::journal;
use crate::kernel::{EGRESS_PRIORITY, Egress, Kernel, MAIN_TABLE, ROUTE_PROTOCOL};
use crate::op::SysctlKey;
use crate::session_id::SessionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leftover {
    Link {
        name: String,
        index: u32,
        session: SessionId,
    },
    Table {
        name: String,
        handle: u64,
        session: SessionId,
    },
    /// The rule and table of a phone address no journal reserves.
    Egress { phone_ip: Ipv4Addr },
    /// Sysctl holders without a journal; `cleanup` drops them.
    Claim {
        key: SysctlKey,
        holders: Vec<SessionId>,
    },
}

pub fn find<K: Kernel>(env: &Env<K>) -> Result<Vec<Leftover>> {
    let kernel = &env.kernel;
    let (links, tables) = (kernel.links()?, kernel.nft_tables()?);
    let (rules, routes) = (kernel.rules()?, kernel.routes()?);
    let sessions = journal::sessions(&env.journal_dir)?;
    let reserved: BTreeSet<Ipv4Addr> = journal::reservations(&env.journal_dir)?
        .into_iter()
        .map(|r| r.phone_ip)
        .collect();
    let orphan = |tag: Option<&str>| {
        tag.and_then(|t| t.strip_prefix("routedroid:"))
            .and_then(|id| id.parse::<SessionId>().ok())
            .filter(|id| !sessions.contains(id))
    };
    let mut out = Vec::new();
    for link in links {
        if let Some(session) = orphan(link.alias.as_deref()) {
            out.push(Leftover::Link {
                name: link.name,
                index: link.index,
                session,
            });
        }
    }
    for (name, table) in tables {
        if let Some(session) =
            orphan(table.comment.as_deref()).filter(|_| name.starts_with("routedroid_"))
        {
            out.push(Leftover::Table {
                name,
                handle: table.handle,
                session,
            });
        }
    }
    let ours = rules
        .iter()
        .filter(|r| r.protocol == ROUTE_PROTOCOL && r.priority == EGRESS_PRIORITY)
        .filter_map(|r| r.src.map(|(ip, _)| u32::from(ip)))
        .chain(
            routes
                .iter()
                .filter(|r| r.protocol == ROUTE_PROTOCOL && r.table != MAIN_TABLE)
                .map(|r| r.table),
        );
    let phones: BTreeSet<Ipv4Addr> = ours
        .map(Ipv4Addr::from)
        .filter(|ip| !reserved.contains(ip))
        .collect();
    out.extend(
        phones
            .into_iter()
            .map(|phone_ip| Leftover::Egress { phone_ip }),
    );
    for (key, holders) in env.claims.unheld(&sessions)? {
        out.push(Leftover::Claim { key, holders });
    }
    Ok(out)
}

impl Leftover {
    pub fn finding(&self) -> Finding {
        let (subject, problem, repair) = match self {
            Leftover::Link { name, session, .. } => (
                format!("link {name}"),
                format!("tagged for session {session}, which has no journal"),
                format!("delete link {name}"),
            ),
            Leftover::Table { name, session, .. } => (
                format!("nft table inet {name}"),
                format!("tagged for session {session}, which has no journal"),
                format!("delete table inet {name}"),
            ),
            Leftover::Egress { phone_ip } => (
                format!("egress of {phone_ip}"),
                "its rule or routing table outlived every session".into(),
                format!(
                    "delete rule from {phone_ip} and table {}",
                    u32::from(*phone_ip)
                ),
            ),
            Leftover::Claim { key, holders } => (
                format!("sysctl {key}"),
                format!("held for {} session(s) with no journal", holders.len()),
                format!("drop those holders from {key}, restoring it if none remain"),
            ),
        };
        Finding {
            subject,
            problem,
            warning: false,
            repair: vec![repair],
        }
    }

    /// Claims are left to `cleanup`, which collects them under the lock.
    pub fn remove(&self, kernel: &impl Kernel) -> Result<()> {
        match self {
            Leftover::Link { index, .. } => kernel.delete_link(*index),
            Leftover::Table { handle, .. } => kernel.delete_nft_table(*handle),
            Leftover::Egress { phone_ip } => kernel.delete_egress(&Egress {
                phone_ip: *phone_ip,
                table: u32::from(*phone_ip),
                lan_net: Ipv4Addr::UNSPECIFIED,
                prefix: 0,
                gateway: None,
            }),
            Leftover::Claim { .. } => Ok(()),
        }
    }
}
