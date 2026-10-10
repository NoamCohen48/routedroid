//! For the session's life the address stays the phone's: announced once it
//! is in use, a lease renewed on schedule (each renewal reported), and
//! another station claiming it ends the session (RFC 5227 §2.4, no defence:
//! the phone is the newcomer, so it gives way).

use std::net::Ipv4Addr;

use routedroid_dhcp::packet::fmt_mac;
use routedroid_dhcp::{Bound, Client, Event, Lost};
use routedroid_helper_ipc::Lease;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Debug)]
pub enum Kept {
    Renewed(Lease),
    /// Why the address is no longer the phone's.
    Lost(String),
}

/// What the controller is told about `lease`.
pub fn report(lease: &routedroid_dhcp::Lease) -> Lease {
    Lease {
        server: lease.server_id,
        router: lease.router,
        dns: lease.dns.clone(),
        expires_at: lease.expires_at(),
    }
}

/// Hold `ip` (with its lease, if `bound`) until lost; the last event is
/// always `Lost`. Aborting the task stops holding.
pub fn spawn(
    mut client: Client,
    mut bound: Option<Bound>,
    ip: Ipv4Addr,
) -> (JoinHandle<()>, mpsc::Receiver<Kept>) {
    let (tx, rx) = mpsc::channel(4);
    let task = tokio::spawn(async move {
        let why = keep(&mut client, bound.as_mut(), ip, &tx).await;
        let _ = tx.send(Kept::Lost(why)).await;
    });
    (task, rx)
}

async fn keep(
    client: &mut Client,
    bound: Option<&mut Bound>,
    ip: Ipv4Addr,
    tx: &mpsc::Sender<Kept>,
) -> String {
    match client.link().announce(ip).await {
        Ok(None) => {}
        Ok(Some(mac)) => return format!("{} also uses {ip}", fmt_mac(&mac)),
        Err(e) => return format!("announcing {ip}: {e:#}"),
    }
    let Some(bound) = bound else {
        return match client.link().watch(ip).await {
            Ok(mac) => format!("{} also uses {ip}", fmt_mac(&mac)),
            Err(e) => format!("watching {ip}: {e:#}"),
        };
    };
    loop {
        match client.maintain(bound).await {
            Ok(Event::Renewed) => {
                if tx.send(Kept::Renewed(report(&bound.lease))).await.is_err() {
                    return "the session is over".into();
                }
            }
            Ok(Event::Lost(lost @ Lost::Moved(_))) => {
                // The new address was never the phone's; give it straight back.
                let _ = client.release(&bound.lease).await;
                return format!("lost the lease: {lost}");
            }
            // Declined already; worded as for a requested address.
            Ok(Event::Lost(Lost::Conflict(mac))) => {
                return format!("{} also uses {ip}", fmt_mac(&mac));
            }
            Ok(Event::Lost(lost)) => return format!("lost the lease: {lost}"),
            Err(e) => return format!("DHCP: {e:#}"),
        }
    }
}
