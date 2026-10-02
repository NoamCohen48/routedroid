use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use routedroid_helper_ipc::{Datagram, IfName, Reply, Request, SeqPacket, MAX_DATAGRAM};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::helper::{self, request};
use crate::fault::{Fault, FaultExt, Kind, Result};
use crate::session::{Inject, PacketEndpoints, QUEUE_DEPTH};

const STOP_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The host side of the data path, held for the session by the privileged
/// helper process: the TUN device, the phone's /32 route, proxy ARP and the
/// session firewall. Created by `start`, torn down by `stop`.
pub struct HostNetwork {
    conn: Arc<SeqPacket>,
    /// Control replies seen by the relay's receive task (it owns the socket
    /// once `relay()` ran, so `stop()` must read the ack from here).
    control_rx: Option<mpsc::Receiver<Reply>>,
    pub tun: IfName,
    pub host_ip: Ipv4Addr,
    pub lan_prefix: u8,
    pub session: String,
}

impl HostNetwork {
    /// Connect, agree on the IPC version and issue `Start`; the helper
    /// picks host address and prefix.
    pub async fn start(
        socket: &Path,
        lan_if: &IfName,
        phone_ip: Ipv4Addr,
        tun: &IfName,
        mtu: u32,
    ) -> Result<Self> {
        let start = Request::Start {
            lan_if: lan_if.clone(),
            phone_ip,
            tun: tun.clone(),
            mtu,
        };
        let conn = helper::connect(socket).await?;
        match request(&conn, &start).await.fault(Kind::Helper)? {
            Reply::Started {
                session,
                tun,
                host_ip,
                lan_prefix,
            } => {
                info!(%session, %tun, %host_ip, lan_prefix, "helper session started");
                Ok(Self {
                    conn: Arc::new(conn),
                    control_rx: None,
                    tun,
                    host_ip,
                    lan_prefix,
                    session,
                })
            }
            Reply::Error { code, message } => Err(Fault::msg(
                Kind::Helper,
                format!("helper refused start: {code:?}: {message}"),
            )),
            other => Err(Fault::msg(
                Kind::Helper,
                format!("unexpected helper reply {other:?}"),
            )),
        }
    }

    /// Hand back the session's packet endpoints: injection straight into
    /// the socket (never waiting; a full helper queue drops), and a receive
    /// task for packets from the TUN and control replies. The task ends when
    /// the helper closes, or once both the session and `stop` are done with it.
    pub fn relay(&mut self) -> PacketEndpoints {
        let (from_tx, from_helper) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
        let (control_tx, control_rx) = mpsc::channel::<Reply>(4);
        self.control_rx = Some(control_rx);
        let conn = self.conn.clone();
        let inject: Inject = Arc::new(move |packet: &[u8]| conn.try_send_packet(packet));
        let conn = self.conn.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; MAX_DATAGRAM];
            while let Ok(Some(datagram)) = conn.recv(&mut buf).await {
                match Datagram::<Reply>::decode(datagram) {
                    // Never wait on the session: a full queue drops, as the helper
                    // does; after the session ended the packet has nowhere to go,
                    // but reading goes on so the Stop ack still gets through.
                    Ok(Datagram::Packet(packet)) => {
                        let _ = from_tx.try_send(packet.to_vec());
                    }
                    Ok(Datagram::Control(reply)) => {
                        let _ = control_tx.try_send(reply);
                    }
                    Err(e) => warn!(error = %e, "undecodable datagram from helper"),
                }
                // Nobody left to deliver to (session over, Stop acked or given
                // up): drop our clone so the helper sees its socket close.
                if from_tx.is_closed() && control_tx.is_closed() {
                    break;
                }
            }
        });
        PacketEndpoints {
            inject,
            from_helper,
        }
    }

    /// Ask the helper to undo everything. Errors are logged, not fatal: the
    /// helper's own `ExecStopPost` cleanup is the backstop.
    pub async fn stop(mut self) {
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
