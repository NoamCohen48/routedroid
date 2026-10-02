//! Applying and undoing one [`Op`]. Undo is idempotent and exact: it acts
//! only on an object that carries this session's tag (or, for a sysctl,
//! this session's claim), so it can run any number of times, in-process or
//! from recovery, without touching anyone else's state.

use anyhow::{Context, Result};
use routedroid_dhcp::Held;
use routedroid_helper_ipc::IfName;
use tracing::{info, warn};

use crate::env::Env;
use crate::kernel::{HostRoute, Kernel, Link};
use crate::op::{Op, SysctlKey, nft_table_name};
use crate::plan::Plan;
use crate::session_id::SessionId;

pub fn apply<K: Kernel>(
    env: &Env<K>,
    plan: &Plan,
    op: &Op,
    tun: &mut Option<K::Tun>,
) -> Result<()> {
    let session = plan.session();
    match op {
        Op::Lease { .. } => Ok(()),
        Op::Tun { name } => {
            *tun = Some(
                env.kernel
                    .create_tun(name, &session.tag(), plan.request().mtu)?,
            );
            Ok(())
        }
        Op::NftTable { .. } => env.kernel.create_firewall(&plan.firewall()),
        Op::Sysctl { ifname, leaf } => {
            env.claims
                .acquire(&env.kernel, session, &SysctlKey::new(ifname.clone(), *leaf))
        }
        Op::Route { dst, tun, src } => {
            let link = owned_link(&env.kernel, tun, session)?
                .with_context(|| format!("{tun} is not ours"))?;
            env.kernel.add_route(&HostRoute {
                dst: *dst,
                oif: link.index,
                src: *src,
            })
        }
    }
}

pub fn undo<K: Kernel>(env: &Env<K>, session: SessionId, op: &Op) -> Result<()> {
    let kernel = &env.kernel;
    match op {
        Op::Lease {
            lan_if,
            client_id,
            address,
            server_id,
            server_mac,
        } => kernel.release_lease(&Held {
            iface: lan_if.to_string(),
            client_id: client_id.clone(),
            address: *address,
            server_id: *server_id,
            server_mac: server_mac.clone(),
        }),
        Op::Tun { name } => match owned_link(kernel, name, session)? {
            Some(link) => {
                warn!(tun = %name, "TUN outlived its owner; deleting");
                kernel.delete_link(link.index)
            }
            None => Ok(()),
        },
        Op::NftTable { tun } => {
            let name = nft_table_name(tun);
            match kernel.nft_table(&name)? {
                Some(table) if table.comment == Some(session.tag()) => {
                    kernel.delete_nft_table(table.handle)
                }
                Some(_) => {
                    info!(table = %name, "table belongs to another owner; leaving it");
                    Ok(())
                }
                None => Ok(()),
            }
        }
        Op::Sysctl { ifname, leaf } => {
            env.claims
                .release(kernel, session, &SysctlKey::new(ifname.clone(), *leaf))
        }
        Op::Route { dst, tun, src } => {
            // The route lives and dies with our TUN: no tagged TUN, no route.
            let Some(link) = owned_link(kernel, tun, session)? else {
                return Ok(());
            };
            let route = HostRoute {
                dst: *dst,
                oif: link.index,
                src: *src,
            };
            if kernel.routes()?.iter().any(|r| route.matches(r)) {
                kernel.delete_route(&route)
            } else {
                Ok(())
            }
        }
    }
}

/// `name`, if it exists and carries `session`'s tag.
fn owned_link(kernel: &impl Kernel, name: &IfName, session: SessionId) -> Result<Option<Link>> {
    Ok(kernel
        .link(name)?
        .filter(|link| link.alias == Some(session.tag())))
}
