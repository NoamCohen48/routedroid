use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context};
use routedroid_helper_ipc::proto::{Reply, Request, KIND_CONTROL, KIND_PACKET, MAX_DATAGRAM, RECV_BUF};
use routedroid_helper_ipc::seqpacket::SeqPacket;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::session::{PacketEndpoints, QUEUE_DEPTH};
use routedroid_ipc::fault::{Fault, FaultExt, Kind, Result};

pub const DEFAULT_SOCKET: &str = "/run/routedroid/phase0-helper.sock";
const STOP_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The host side of the data path, held for the session by the privileged
/// helper process: the TUN device, the LAN alias for the phone and proxy
/// ARP. Created by `start`, torn down by `stop`.
pub struct HostNetwork {
    conn: Arc<SeqPacket>,
    /// Control replies seen by the relay's receive task (it owns the socket
    /// once `relay()` ran, so `stop()` must read the ack from here).
    control_rx: Option<mpsc::Receiver<Vec<u8>>>,
    pub tun: String,
    pub host_ip: Ipv4Addr,
    pub lan_prefix: u8,
    pub session: String,
}

async fn send_control(conn: &SeqPacket, req: &Request) -> anyhow::Result<()> {
    let mut datagram = vec![KIND_CONTROL];
    datagram.extend_from_slice(&serde_json::to_vec(req)?);
    conn.send(&datagram).await.context("send to helper")
}

async fn recv_reply(conn: &SeqPacket, buf: &mut [u8]) -> anyhow::Result<Reply> {
    loop {
        let n = conn.recv(buf).await.context("recv from helper")?;
        if n == 0 {
            bail!("helper closed the connection");
        }
        if buf[0] == KIND_CONTROL {
            return serde_json::from_slice(&buf[1..n]).context("parse helper reply");
        }
    }
}

impl HostNetwork {
    /// Connect and issue `Start`; the helper picks host address and prefix.
    pub async fn start(socket: &Path, lan_if: &str, phone_ip: Ipv4Addr, tun: &str, mtu: u32) -> Result<Self> {
        let conn = SeqPacket::connect(socket)
            .await
            .with_context(|| format!("connect to helper socket {}", socket.display()))
            .fault(Kind::Helper)?;
        let req = Request::Start { lan_if: lan_if.to_string(), phone_ip, tun: tun.to_string(), mtu };
        send_control(&conn, &req).await.fault(Kind::Helper)?;
        let mut buf = vec![0u8; RECV_BUF];
        match recv_reply(&conn, &mut buf).await.fault(Kind::Helper)? {
            Reply::Started { session, tun, host_ip, lan_prefix } => {
                info!(%session, %tun, %host_ip, lan_prefix, "helper session started");
                Ok(Self { conn: Arc::new(conn), control_rx: None, tun, host_ip, lan_prefix, session })
            }
            Reply::Error { code, message } => {
                Err(Fault::msg(Kind::Helper, format!("helper refused start: {code}: {message}")))
            }
            other => Err(Fault::msg(Kind::Helper, format!("unexpected helper reply {other:?}"))),
        }
    }

    /// Spawn the two relay tasks and hand back the session's packet endpoints.
    /// The relay ends when the helper closes or when `to_helper` is dropped.
    pub fn relay(&mut self) -> PacketEndpoints {
        let (to_helper, mut inject_rx) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
        let (from_tx, from_helper) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
        let (control_tx, control_rx) = mpsc::channel::<Vec<u8>>(4);
        self.control_rx = Some(control_rx);
        let conn = self.conn.clone();
        tokio::spawn(async move {
            let mut out = vec![0u8; MAX_DATAGRAM];
            while let Some(pkt) = inject_rx.recv().await {
                out[0] = KIND_PACKET;
                out[1..=pkt.len()].copy_from_slice(&pkt);
                if let Err(e) = conn.send(&out[..=pkt.len()]).await {
                    warn!(error = %e, "send to helper failed");
                    break;
                }
            }
        });
        let conn = self.conn.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; RECV_BUF];
            loop {
                let n = match conn.recv(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                match buf[0] {
                    // After the session ended the receiver is gone; drop late packets
                    // but keep reading so the Stop ack still gets through.
                    KIND_PACKET => {
                        let _ = from_tx.send(buf[1..n].to_vec()).await;
                    }
                    KIND_CONTROL => {
                        let _ = control_tx.try_send(buf[1..n].to_vec());
                    }
                    _ => {}
                }
            }
        });
        PacketEndpoints { to_helper, from_helper }
    }

    /// Ask the helper to undo everything. Errors are logged, not fatal: the
    /// helper's own `ExecStopPost` cleanup is the backstop.
    pub async fn stop(mut self) {
        if let Err(e) = send_control(&self.conn, &Request::Stop).await {
            warn!(error = %e, "could not send Stop to helper");
            return;
        }
        let reply = async {
            match self.control_rx.take() {
                Some(mut rx) => match rx.recv().await {
                    Some(b) => serde_json::from_slice::<Reply>(&b).context("parse helper reply"),
                    None => Err(anyhow::anyhow!("helper closed the connection")),
                },
                None => recv_reply(&self.conn, &mut vec![0u8; RECV_BUF]).await,
            }
        };
        let reply = match tokio::time::timeout(STOP_ACK_TIMEOUT, reply).await {
            Ok(r) => r,
            Err(_) => Err(anyhow::anyhow!("no reply within {STOP_ACK_TIMEOUT:?}")),
        };
        match reply {
            Ok(Reply::Stopped) => info!("helper session stopped"),
            Ok(other) => warn!(?other, "unexpected reply to Stop"),
            Err(e) => warn!(error = %e, "helper did not acknowledge Stop"),
        }
    }
}
