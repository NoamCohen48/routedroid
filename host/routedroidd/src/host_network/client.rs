use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context};
use routedroid_helper_ipc::{Datagram, IfName, Reply, Request, SeqPacket, MAX_DATAGRAM, VERSION};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::session::{PacketEndpoints, QUEUE_DEPTH};
use routedroid_ipc::fault::{Fault, FaultExt, Kind, Result};

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

/// Send `request` and wait for its reply, skipping packets.
async fn request(conn: &SeqPacket, request: &Request) -> anyhow::Result<Reply> {
    conn.send_control(request).await.context("send to helper")?;
    let mut buf = vec![0u8; MAX_DATAGRAM];
    loop {
        let Some(datagram) = conn.recv(&mut buf).await.context("recv from helper")? else {
            bail!("helper closed the connection");
        };
        if let Datagram::Control(reply) = Datagram::<Reply>::decode(datagram).context("decode helper reply")? {
            return Ok(reply);
        }
    }
}

fn if_name(name: &str) -> Result<IfName> {
    IfName::new(name).map_err(|e| Fault::msg(Kind::Usage, e.to_string()))
}

impl HostNetwork {
    /// Connect, agree on the IPC version and issue `Start`; the helper
    /// picks host address and prefix.
    pub async fn start(socket: &Path, lan_if: &str, phone_ip: Ipv4Addr, tun: &str, mtu: u32) -> Result<Self> {
        let start = Request::Start { lan_if: if_name(lan_if)?, phone_ip, tun: if_name(tun)?, mtu };
        let conn = SeqPacket::connect(socket)
            .with_context(|| format!("connect to helper socket {}", socket.display()))
            .fault(Kind::Helper)?;
        match request(&conn, &Request::Hello { version: VERSION }).await.fault(Kind::Helper)? {
            Reply::Hello { .. } => {}
            Reply::Error { code, message } => {
                return Err(Fault::msg(Kind::Helper, format!("helper refused the handshake: {code:?}: {message}")))
            }
            other => return Err(Fault::msg(Kind::Helper, format!("unexpected helper reply {other:?}"))),
        }
        match request(&conn, &start).await.fault(Kind::Helper)? {
            Reply::Started { session, tun, host_ip, lan_prefix } => {
                info!(%session, %tun, %host_ip, lan_prefix, "helper session started");
                Ok(Self { conn: Arc::new(conn), control_rx: None, tun, host_ip, lan_prefix, session })
            }
            Reply::Error { code, message } => {
                Err(Fault::msg(Kind::Helper, format!("helper refused start: {code:?}: {message}")))
            }
            other => Err(Fault::msg(Kind::Helper, format!("unexpected helper reply {other:?}"))),
        }
    }

    /// Spawn the two relay tasks and hand back the session's packet endpoints.
    /// The relay ends when the helper closes or when `to_helper` is dropped.
    pub fn relay(&mut self) -> PacketEndpoints {
        let (to_helper, mut inject_rx) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
        let (from_tx, from_helper) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
        let (control_tx, control_rx) = mpsc::channel::<Reply>(4);
        self.control_rx = Some(control_rx);
        let conn = self.conn.clone();
        tokio::spawn(async move {
            while let Some(packet) = inject_rx.recv().await {
                if let Err(e) = conn.send_packet(&packet).await {
                    warn!(error = %e, "send to helper failed");
                    break;
                }
            }
        });
        let conn = self.conn.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; MAX_DATAGRAM];
            while let Ok(Some(datagram)) = conn.recv(&mut buf).await {
                match Datagram::<Reply>::decode(datagram) {
                    // After the session ended the receiver is gone; drop late packets
                    // but keep reading so the Stop ack still gets through.
                    Ok(Datagram::Packet(packet)) => {
                        let _ = from_tx.send(packet.to_vec()).await;
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
        PacketEndpoints { to_helper, from_helper }
    }

    /// Ask the helper to undo everything. Errors are logged, not fatal: the
    /// helper's own `ExecStopPost` cleanup is the backstop.
    pub async fn stop(mut self) {
        let reply = async {
            match self.control_rx.take() {
                Some(mut rx) => {
                    self.conn.send_control(&Request::Stop).await.context("send Stop")?;
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
