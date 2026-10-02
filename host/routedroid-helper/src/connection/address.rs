//! Settling the phone's address before anything is applied: leased from
//! the LAN's DHCP server, or the requested one. Either way it must pass the
//! plan's checks and an RFC 5227 ARP probe, and the DHCP client that probed
//! it stays to hold it for the session (see `keeper`).
//!
//! The policy is checked before any packet is sent, and a lease that turns
//! out unusable is given back at once.

use std::net::Ipv4Addr;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use routedroid_dhcp::{ArpMode, Bound, Client, PROBE};
use routedroid_helper_ipc::{DeviceId, ErrorCode, IfName};
use tokio::task::spawn_blocking;

use super::lease::{give_back, lease};
use crate::env::Env;
use crate::kernel::System;
use crate::plan::{Facts, Plan, Request, exclusions};
use crate::policy::Policy;
use crate::session_id::SessionId;

/// The controller's `Start`, as received.
#[derive(Debug)]
pub struct Start {
    pub lan_if: IfName,
    pub phone_ip: Option<Ipv4Addr>,
    pub device: DeviceId,
    pub tun: IfName,
    pub mtu: u32,
}

/// Why a `Start` ends before the session: the reply's code, and the reason.
pub struct Refusal(pub ErrorCode, pub anyhow::Error);

fn refused(reason: anyhow::Error) -> Refusal {
    Refusal(ErrorCode::Refused, reason)
}

/// The plan, and the client that holds its address (and lease, if any).
pub struct Settled {
    pub plan: Plan,
    pub client: Client,
    pub bound: Option<Bound>,
}

pub async fn settle(env: &Arc<Env<System>>, start: Start) -> Result<Settled, Refusal> {
    let Start {
        lan_if,
        phone_ip,
        device,
        tun,
        mtu,
    } = start;
    let (policy, facts) = read(env, &lan_if, &tun).await.map_err(refused)?;
    match phone_ip {
        Some(ip) => policy.check(&lan_if, ip),
        None => policy.check_dhcp(&lan_if),
    }
    .map_err(refused)?;
    if facts.lan.is_none() {
        return Err(refused(anyhow!("{lan_if} does not exist")));
    }
    let mut client = Client::open(lan_if.as_str(), device.bytes(), ArpMode::Kernel)
        .map_err(|e| Refusal(ErrorCode::StartFailed, e))?;
    let (bound, facts) = match phone_ip {
        Some(_) => (None, facts),
        None => {
            client.exclude(exclusions(&facts));
            let bound = lease(&mut client, &lan_if).await?;
            // Read again: the kernel may have changed during the lease.
            match read(env, &lan_if, &tun).await {
                Ok((_, facts)) => (Some(bound), facts),
                Err(e) => {
                    give_back(&mut client, Some(&bound)).await;
                    return Err(refused(e));
                }
            }
        }
    };
    let phone_ip = phone_ip.or(bound.as_ref().map(|b| b.lease.address));
    let request = Request {
        lan_if,
        phone_ip: phone_ip.expect("requested or leased"),
        tun,
        mtu,
        lease: bound.as_ref().map(|b| b.lease.held()),
    };
    let plan = SessionId::random().and_then(|id| Plan::build(id, request, &policy, &facts));
    let plan = match plan {
        Ok(plan) => plan,
        Err(e) => {
            give_back(&mut client, bound.as_ref()).await;
            return Err(refused(e));
        }
    };
    if bound.is_none() {
        probe(&mut client, &plan).await?;
    }
    Ok(Settled {
        plan,
        client,
        bound,
    })
}

/// The policy and the kernel's state, both read fresh.
async fn read(env: &Arc<Env<System>>, lan_if: &IfName, tun: &IfName) -> Result<(Policy, Facts)> {
    let (env, lan_if, tun) = (Arc::clone(env), lan_if.clone(), tun.clone());
    spawn_blocking(move || {
        let policy = Policy::load(&env.policy)?;
        Ok((policy, Facts::gather(&env.kernel, &lan_if, &tun)?))
    })
    .await
    .context("reading the policy and the kernel panicked")?
}

/// A requested address must not answer ARP: someone else has it.
async fn probe(client: &mut Client, plan: &Plan) -> Result<(), Refusal> {
    let request = plan.request();
    let ip = request.phone_ip;
    match client.link().probe(ip, PROBE).await {
        Ok(None) => Ok(()),
        Ok(Some(mac)) => Err(refused(anyhow!(
            "{ip} is in use on {}: {} answers ARP for it",
            request.lan_if,
            routedroid_dhcp::packet::fmt_mac(&mac)
        ))),
        Err(e) => Err(Refusal(ErrorCode::StartFailed, e.context("ARP probe"))),
    }
}
