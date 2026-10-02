//! The DHCP side of settling an address: lease one, or give it back.

use std::time::Duration;

use anyhow::anyhow;
use routedroid_dhcp::{Acquire, Bound, Client, Outcome, PROBE};
use routedroid_helper_ipc::{ErrorCode, IfName};
use tracing::{info, warn};

use super::address::Refusal;

/// How long a server has to grant a usable lease, declines included.
const LEASE_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to wait for more OFFERs after the first.
const OFFER_WINDOW: Duration = Duration::from_millis(500);

pub async fn lease(client: &mut Client, lan_if: &IfName) -> Result<Bound, Refusal> {
    let how = Acquire {
        timeout: LEASE_TIMEOUT,
        offer_window: OFFER_WINDOW,
        probe: Some(PROBE),
    };
    let no_lease = |reason| Err(Refusal(ErrorCode::NoLease, reason));
    match client.acquire(&how).await {
        Ok(Outcome::Bound(bound)) => {
            info!(address = %bound.lease.address, server = %bound.lease.server_id, "leased");
            Ok(bound)
        }
        Ok(Outcome::Nak(m)) => no_lease(anyhow!("the DHCP server on {lan_if} refused: {m}")),
        Ok(Outcome::Timeout) => no_lease(anyhow!(
            "no usable DHCP lease on {lan_if} within {}s",
            LEASE_TIMEOUT.as_secs()
        )),
        Err(e) => no_lease(e.context(format!("DHCP on {lan_if}"))),
    }
}

/// RELEASE a lease the session will not use. A duplicate (the session's own
/// undo releases too) is harmless: the server matches on the client-id.
pub async fn give_back(client: &mut Client, bound: Option<&Bound>) {
    if let Some(bound) = bound
        && let Err(e) = client.release(&bound.lease).await
    {
        warn!(error = %format!("{e:#}"), "RELEASE failed; the lease will expire");
    }
}
