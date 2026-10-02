//! A stand-in privileged helper on a real SOCK_SEQPACKET socket: it answers
//! the handshake, grants or refuses `Start`, echoes every packet back (as if
//! the LAN answered), and acknowledges `Stop`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use routedroid_helper_ipc::{
    Datagram, ErrorCode, Listener, MAX_DATAGRAM, Reply, Request, SeqPacket, VERSION,
};

pub const HOST_IP: [u8; 4] = [10, 0, 0, 1];

#[derive(Clone)]
pub struct FakeHelper {
    pub socket: PathBuf,
    /// Every control request, in arrival order.
    pub seen: Arc<Mutex<Vec<Request>>>,
    pub refuse: Arc<AtomicBool>,
}

impl FakeHelper {
    pub fn spawn(dir: &Path) -> Self {
        let socket = dir.join("helper.sock");
        let listener = Listener::bind(&socket).unwrap();
        let helper = Self {
            socket,
            seen: Arc::default(),
            refuse: Arc::default(),
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
        while let Ok(Some(datagram)) = conn.recv(&mut buf).await {
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
                Request::Start { tun, .. } => (
                    Reply::Started {
                        session: "fake".into(),
                        tun,
                        host_ip: HOST_IP.into(),
                        lan_prefix: 24,
                    },
                    false,
                ),
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
