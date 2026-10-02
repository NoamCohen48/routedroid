//! A stand-in privileged helper on a real SOCK_SEQPACKET socket: it answers
//! the handshake, grants or refuses `Start`, echoes every packet back (as if
//! the LAN answered), and acknowledges `Stop`. Without a requested address
//! it "leases" `LEASED_IP`; `kick` then renews once and ends the session.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use routedroid_helper_ipc::{
    Datagram, ErrorCode, Lease, Listener, MAX_DATAGRAM, Reply, Request, SeqPacket, VERSION,
};
use tokio::sync::Notify;

pub const HOST_IP: [u8; 4] = [10, 0, 0, 1];
pub const LEASED_IP: [u8; 4] = [10, 0, 0, 50];
pub const LEASE_DNS: [u8; 4] = [10, 0, 0, 53];
pub const ENDED: &str = "02:00:00:00:00:99 also uses 10.0.0.50";

pub fn lease(expires_at: u64) -> Lease {
    Lease {
        server: [10, 0, 0, 254].into(),
        router: Some([10, 0, 0, 254].into()),
        dns: vec![LEASE_DNS.into()],
        expires_at,
    }
}

#[derive(Clone)]
pub struct FakeHelper {
    pub socket: PathBuf,
    /// Every control request, in arrival order.
    pub seen: Arc<Mutex<Vec<Request>>>,
    pub refuse: Arc<AtomicBool>,
    /// Renew the started session's lease (to expire at 2000), then end it.
    pub kick: Arc<Notify>,
}

impl FakeHelper {
    pub fn spawn(dir: &Path) -> Self {
        let socket = dir.join("helper.sock");
        let listener = Listener::bind(&socket).unwrap();
        let helper = Self {
            socket,
            seen: Arc::default(),
            refuse: Arc::default(),
            kick: Arc::default(),
        };
        let serving = helper.clone();
        tokio::spawn(async move {
            while let Ok(conn) = listener.accept().await {
                tokio::spawn(serving.clone().serve(conn));
            }
        });
        helper
    }

    pub fn stops(&self) -> usize {
        let seen = self.seen.lock().unwrap();
        seen.iter().filter(|r| **r == Request::Stop).count()
    }

    async fn serve(self, conn: SeqPacket) {
        let mut buf = vec![0u8; MAX_DATAGRAM];
        let mut started = false;
        loop {
            let datagram = tokio::select! {
                received = conn.recv(&mut buf) => match received {
                    Ok(Some(datagram)) => datagram,
                    _ => return,
                },
                () = self.kick.notified(), if started => {
                    let _ = conn.send_control(&Reply::Lease { lease: lease(2000) }).await;
                    let ended = Reply::Error { code: ErrorCode::SessionEnded, message: ENDED.into() };
                    let _ = conn.send_control(&ended).await;
                    return;
                }
            };
            let request = match Datagram::<Request>::decode(datagram) {
                Ok(Datagram::Packet(packet)) => {
                    let _ = conn.send_packet(packet).await;
                    continue;
                }
                Ok(Datagram::Control(request)) => request,
                Err(_) => return,
            };
            self.seen.lock().unwrap().push(request.clone());
            let (reply, last) = match request {
                Request::Hello { .. } => (Reply::Hello { version: VERSION }, false),
                Request::Start { .. } if self.refuse.load(Ordering::SeqCst) => (
                    Reply::Error {
                        code: ErrorCode::Refused,
                        message: "lan0 is not an interface the policy allows".into(),
                    },
                    true,
                ),
                Request::Start { tun, phone_ip, .. } => {
                    started = true;
                    let started = Reply::Started {
                        session: "fake".into(),
                        tun,
                        phone_ip: phone_ip.unwrap_or(LEASED_IP.into()),
                        host_ip: HOST_IP.into(),
                        lan_prefix: 24,
                        lease: phone_ip.is_none().then(|| lease(1000)),
                    };
                    (started, false)
                }
                Request::Stop => (Reply::Stopped, true),
                Request::Ping => (Reply::Pong, false),
                Request::Interfaces => (Reply::Interfaces { interfaces: vec![] }, false),
            };
            let _ = conn.send_control(&reply).await;
            if last {
                return;
            }
        }
    }
}
