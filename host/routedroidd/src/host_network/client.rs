use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use routedroid_helper_ipc::{DeviceId, ErrorCode, IfName, Lease, Reply, Request, SeqPacket};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::helper::{self, request};
use super::relay::Downlink;
use crate::fault::{Fault, FaultExt, Kind, Result};

const STOP_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The host side of the data path, held for the session by the privileged
/// helper process: the TUN device, the phone's /32 route, proxy ARP, the
/// session firewall and, without a requested address, the phone's DHCP
/// lease. Created by `start`, torn down by `stop`.
pub struct HostNetwork {
    pub(super) conn: Arc<SeqPacket>,
    /// Control replies seen by the receive task (it owns the socket once
    /// `listen()` ran, so `stop()` must read the ack from here).
    pub(super) control_rx: Option<mpsc::Receiver<Reply>>,
    /// Where packets from the TUN go: the current protocol session's
    /// downlink, or nowhere between sessions.
    pub(super) downlink: Arc<Mutex<Downlink>>,
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
                    downlink: Arc::default(),
                    ended: false,
                    tun,
                    phone_ip,
                    host_ip,
                    lan_prefix,
                    lease,
                    session,
                })
            }
            Reply::Error {
                code: ErrorCode::Refused,
                message,
            } => Err(Fault::msg(
                Kind::Helper,
                format!("the helper refused: {message}"),
            )),
            Reply::Error { message, .. } => Err(Fault::msg(
                Kind::Helper,
                format!("the helper could not start the session: {message}"),
            )),
            other => Err(helper::unexpected(&other)),
        }
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
