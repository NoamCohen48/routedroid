use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use routedroid_dhcp::{
    Acquire, ArpMode, Bound, Client, Event, Lease, Lost, Outcome, PROBE, Schedule,
};
use routedroid_helper_ipc::DeviceId;
use tokio::signal::unix::{SignalKind, signal};
use tracing::{error, info, warn};

use crate::args::{AcquireArgs, StateArgs};
use crate::{EXIT_NAK, EXIT_TIMEOUT, host};

fn arp_mode(no_arp: bool) -> ArpMode {
    if no_arp {
        ArpMode::Kernel
    } else {
        ArpMode::Respond
    }
}

fn publish(lease: &Lease, state: Option<&Path>) -> Result<()> {
    println!("{}", lease.to_json());
    if let Some(p) = state {
        lease.save(p)?;
        info!(path = %p.display(), "state written");
    }
    Ok(())
}

pub async fn acquire(a: AcquireArgs) -> Result<u8> {
    let device = DeviceId::from_serial(&a.serial);
    let mut client = Client::open(&a.iface, device.bytes(), arp_mode(a.no_arp))?;
    client.exclude(host::addresses()?);
    let how = Acquire {
        timeout: Duration::from_secs(a.timeout),
        offer_window: Duration::from_secs_f64(a.offer_window.max(0.0)),
        probe: (!a.no_probe).then_some(PROBE),
    };
    let mut bound = match client.acquire(&how).await? {
        Outcome::Bound(b) => b,
        Outcome::Nak(m) => {
            error!(message = %m, "NAK");
            return Ok(EXIT_NAK);
        }
        Outcome::Timeout => {
            error!(timeout = a.timeout, "no lease within the deadline");
            return Ok(EXIT_TIMEOUT);
        }
    };
    let l = &bound.lease;
    info!(
        address = %format_args!("{}/{}", l.address, l.prefix), router = ?l.router, dns = ?l.dns,
        server = %l.server_id, server_mac = %l.server_mac, lease_secs = l.lease_secs, t1 = l.t1, t2 = l.t2,
        routes = l.static_routes.len(),
        "BOUND (address NOT configured on the interface)"
    );
    for r in &l.static_routes {
        info!(route = %r, "classless static route (informational only)");
    }
    publish(l, a.state.as_deref())?;
    if let Some(secs) = a.hold
        && let Some(code) = hold(&mut client, &mut bound, &a, Duration::from_secs(secs)).await?
    {
        return Ok(code);
    }
    if a.release_on_exit
        && let Err(e) = client.release(&bound.lease).await
    {
        warn!(error = %e, "RELEASE failed (best effort)");
        return Ok(1);
    }
    Ok(0)
}

/// BOUND for `secs`, or until a signal. `Some(code)` if the lease was lost.
async fn hold(
    client: &mut Client,
    bound: &mut Bound,
    a: &AcquireArgs,
    secs: Duration,
) -> Result<Option<u8>> {
    if let Some(after) = a.renew_after {
        bound.schedule.t1 = Instant::now() + Duration::from_secs(after);
    }
    let mut sigint = signal(SignalKind::interrupt()).context("SIGINT handler")?;
    let mut sigterm = signal(SignalKind::terminate()).context("SIGTERM handler")?;
    let end = tokio::time::sleep(secs);
    tokio::pin!(end);
    loop {
        // `maintain` is cancel-safe: `bound` keeps its state between calls.
        let event = tokio::select! {
            event = client.maintain(bound) => event?,
            () = &mut end => return Ok(None),
            _ = sigint.recv() => return Ok(None),
            _ = sigterm.recv() => return Ok(None),
        };
        match event {
            Event::Renewed => publish(&bound.lease, a.state.as_deref())?,
            Event::Lost(lost) => {
                error!(address = %bound.lease.address, "lease lost: {lost}");
                return Ok(Some(if matches!(lost, Lost::Nak(_)) {
                    EXIT_NAK
                } else {
                    EXIT_TIMEOUT
                }));
            }
        }
    }
}

fn load(a: &StateArgs) -> Result<(Client, Lease)> {
    let lease = Lease::load(&a.state)?;
    if lease.iface != a.iface {
        warn!(state_iface = %lease.iface, iface = %a.iface, "state was acquired on a different interface");
    }
    let client = Client::for_lease(&a.iface, &lease, arp_mode(a.no_arp))?;
    Ok((client, lease))
}

fn outcome_code(o: Outcome, state: &Path, what: &str) -> Result<u8> {
    match o {
        Outcome::Bound(b) => {
            info!(address = %b.lease.address, lease_secs = b.lease.lease_secs, "{what}: ACK");
            publish(&b.lease, Some(state))?;
            Ok(0)
        }
        Outcome::Nak(m) => {
            error!(message = %m, "{what}: NAK");
            Ok(EXIT_NAK)
        }
        Outcome::Timeout => {
            error!("{what}: no reply within the deadline");
            Ok(EXIT_TIMEOUT)
        }
    }
}

pub async fn renew(a: StateArgs) -> Result<u8> {
    let (mut client, lease) = load(&a)?;
    let left = Schedule::restored(&lease)
        .expiry
        .saturating_duration_since(Instant::now());
    info!(expires_in = left.as_secs(), "renewing a saved lease");
    let o = client.renew(&lease, Duration::from_secs(a.timeout)).await?;
    outcome_code(o, &a.state, "RENEW")
}

pub async fn init_reboot(a: StateArgs) -> Result<u8> {
    let (mut client, lease) = load(&a)?;
    let o = client
        .init_reboot(&lease, Duration::from_secs(a.timeout))
        .await?;
    outcome_code(o, &a.state, "INIT-REBOOT")
}

pub async fn release(a: StateArgs) -> Result<u8> {
    let (mut client, lease) = load(&a)?;
    client.release(&lease).await?;
    info!(address = %lease.address, "RELEASE sent; state file left in place");
    Ok(0)
}
