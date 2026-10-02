use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use routedroid_helper_ipc::{DeviceId, IfName, Lease, Reply, Request, SeqPacket};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::helper::{self, request};
use super::relay::HelperEvent;
use crate::fault::{Fault, FaultExt, Kind, Result};

const STOP_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The host side of the data path, held for the session by the privileged
/// helper process: the TUN device, the phone's /32 route, proxy ARP, the
/// session firewall and, without a requested address, the phone's DHCP
/// lease. Created by `start`, torn down by `stop`.
pub struct HostNetwork {
    pub(super) conn: Arc<SeqPacket>,
    /// Control replies seen by the relay's receive task (it owns the socket
    /// once `relay()` ran, so `stop()` must read the ack from here).
    pub(super) control_rx: Option<mpsc::Receiver<Reply>>,
    /// Renewals and the helper's own end of the session, once `relay()` ran.
    pub(super) events_rx: Option<mpsc::Receiver<HelperEvent>>,
    /// The helper ended the session itself: there is nothing left to stop.
    ended: bool,
    pub tun: IfName,
    /// The requested address, or the leased one.
    pub phone_ip: Ipv4Addr,
    pub host_ip: Ipv4Addr,
    pub lan_prefix: u8,
    pub lease: Option<Lease>,
    pub session: String,
}

impl HostNetwork {
    /// Connect, agree on the IPC version and issue `Start`; the helper
    /// picks host address and prefix, and leases `phone_ip` if it is `None`.
    pub async fn start(
        socket: &Path,
        lan_if: &IfName,
        phone_ip: Option<Ipv4Addr>,
        device: DeviceId,
        tun: &IfName,
        mtu: u32,
    ) -> Result<Self> {
        let start = Request::Start {
            lan_if: lan_if.clone(),
            phone_ip,
            device,
            tun: tun.clone(),
            mtu,
        };
        let conn = helper::connect(socket).await?;
        match request(&conn, &start).await.fault(Kind::Helper)? {
            Reply::Started {
                session,
                tun,
                phone_ip,
                host_ip,
                lan_prefix,
                lease,
            } => {
                info!(%session, %tun, %phone_ip, %host_ip, lan_prefix, leased = lease.is_some(), "helper session started");
                Ok(Self {
                    conn: Arc::new(conn),
                    control_rx: None,
                    events_rx: None,
                    ended: false,
                    tun,
                    phone_ip,
                    host_ip,
                    lan_prefix,
                    lease,
                    session,
                })
            }
            Reply::Error { code, message } => Err(Fault::msg(
                Kind::Helper,
                format!("helper refused start: {code:?}: {message}"),
            )),
            other => Err(helper::unexpected(&other)),
        }
    }

    /// What the helper said since `relay()` ran: renewals, and why it ended
    /// the session if it did. `None` before `relay()` or once taken.
    pub fn take_events(&mut self) -> Option<mpsc::Receiver<HelperEvent>> {
        self.events_rx.take()
    }

    /// The helper said it ended the session (and undid it), so `stop` has
    /// nothing to ask for.
    pub fn ended_by_helper(&mut self) {
        self.ended = true;
    }

    /// Ask the helper to undo everything. Errors are logged, not fatal: the
    /// helper's own `ExecStopPost` cleanup is the backstop.
    pub async fn stop(mut self) {
        if self.ended {
            return;
        }
        let reply = async {
            match self.control_rx.take() {
                Some(mut rx) => {
                    self.conn
                        .send_control(&Request::Stop)
                        .await
                        .context("send Stop")?;
                    rx.recv().await.context("helper closed the connection")
                }
                None => request(&self.conn, &Request::Stop).await,
            }
        };
        match tokio::time::timeout(STOP_ACK_TIMEOUT, reply).await {
            Ok(Ok(Reply::Stopped)) => info!("helper session stopped"),
            Ok(Ok(other)) => warn!(?other, "unexpected reply to Stop"),
            Ok(Err(e)) => warn!(error = %format!("{e:#}"), "helper did not acknowledge Stop"),
            Err(_) => warn!("helper did not acknowledge Stop within {STOP_ACK_TIMEOUT:?}"),
        }
    }
}
