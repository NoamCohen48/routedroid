//! Driver tests over a real loopback TCP pair with paused time.

use std::time::Duration;

use routedroid_proto::auth::Secret;
use routedroid_proto::frame::{self, Frame, MessageType};
use routedroid_proto::messages::{ErrorBody, Hello, Prefix};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

use super::timers::{CONSENT_DEADLINE, HANDSHAKE_DEADLINE};
use super::*;

fn cfg() -> SessionConfig {
    SessionConfig {
        mtu: 1400,
        addresses: vec![Prefix { address: "10.0.0.2".into(), prefix: 32 }],
        routes: vec![Prefix { address: "0.0.0.0".into(), prefix: 0 }],
        dns: vec![],
        session_name: "test".into(),
        expected_session: "s1".into(),
        expected_device_port: 9000,
        secret: Secret::new([7; 32]),
    }
}

/// Spawns the driver on one end of a loopback pair; returns the peer socket
/// and the join handle. Time is paused, so deadlines elapse instantly when idle.
async fn start() -> (TcpStream, tokio::task::JoinHandle<SessionEnd>, watch::Sender<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let peer = TcpStream::connect(addr).await.unwrap();
    let (stream, _) = listener.accept().await.unwrap();
    let (to_helper, _keep) = mpsc::channel(4);
    let (_from_tx, from_helper) = mpsc::channel(4);
    let (stop_tx, stop_rx) = watch::channel(false);
    let handle = tokio::spawn(async move {
        let packets = PacketEndpoints { to_helper, from_helper };
        SessionDriver::run(stream, Machine::new(cfg(), [0xbb; 32]), packets, stop_rx, Progress::detached()).await.end
    });
    (peer, handle, stop_tx)
}

async fn expect_error(peer: &mut TcpStream) -> ErrorBody {
    let f = frame::read_frame(peer, 1400).await.expect("ERROR frame");
    assert_eq!(f.message_type, MessageType::Error);
    routedroid_proto::messages::parse(&f.body).unwrap()
}

#[tokio::test(start_paused = true)]
async fn silent_peer_is_refused_after_handshake_deadline() {
    let (mut peer, handle, _stop) = start().await;
    tokio::time::sleep(HANDSHAKE_DEADLINE + Duration::from_secs(1)).await;
    let e = expect_error(&mut peer).await;
    assert_eq!(e.code, "protocol_error");
    assert!(matches!(handle.await.unwrap(), SessionEnd::Refused(_)));
}

#[tokio::test(start_paused = true)]
async fn consent_deadline_applies_after_hello_reset_the_clock() {
    let (mut peer, handle, _stop) = start().await;
    tokio::time::sleep(HANDSHAKE_DEADLINE - Duration::from_secs(1)).await;
    let hello =
        Hello { protocol: 1, session: "s1".into(), device_port: 9000, client_nonce: "aa".repeat(32), app: None };
    peer.write_all(&Frame::json(MessageType::Hello, &hello).encode()).await.unwrap();
    let ack = frame::read_frame(&mut peer, 1400).await.unwrap();
    assert_eq!(ack.message_type, MessageType::HelloAck);
    // The HELLO restarted the phase clock: another near-deadline wait is still fine.
    tokio::time::sleep(HANDSHAKE_DEADLINE - Duration::from_secs(1)).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let e = expect_error(&mut peer).await;
    assert_eq!(e.code, "protocol_error");
    assert!(e.message.contains("Authenticating"));
    assert!(matches!(handle.await.unwrap(), SessionEnd::Refused(_)));
    assert!(CONSENT_DEADLINE > HANDSHAKE_DEADLINE);
}

#[tokio::test(start_paused = true)]
async fn local_stop_sends_stop_frame() {
    let (mut peer, handle, stop) = start().await;
    stop.send(true).unwrap();
    let f = frame::read_frame(&mut peer, 1400).await.unwrap();
    assert_eq!(f.message_type, MessageType::Stop);
    assert!(matches!(handle.await.unwrap(), SessionEnd::LocalStop));
}
