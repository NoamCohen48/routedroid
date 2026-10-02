//! The driver once Active: each packet direction runs on its own, and
//! either side of the data path ending ends the session.

use std::net::Ipv4Addr;
use std::sync::Arc;

use routedroid_proto::auth;
use routedroid_proto::messages::{Auth, VpnReady};

use super::*;

const CLIENT_NONCE: [u8; 32] = [0xaa; 32];

/// A UDP datagram from the phone's address with an empty payload.
fn packet() -> Vec<u8> {
    let mut p = vec![
        0x45, 0, 0, 28, 0, 0, 0, 0, 64, 17, 0, 0, 10, 0, 0, 2, 10, 0, 0, 1,
    ];
    p.extend([0, 53, 0, 53, 0, 8, 0, 0]);
    p
}

async fn send(peer: &mut TcpStream, frame: Frame) {
    peer.write_all(&frame.encode()).await.unwrap();
}

async fn expect(peer: &mut TcpStream, want: MessageType) {
    assert_eq!(
        frame::read_frame(peer, 1400).await.unwrap().message_type,
        want
    );
}

/// Play the app's side of the handshake up to Configuring.
async fn authenticate(peer: &mut TcpStream) {
    let hello = Hello {
        protocol: 1,
        session: "s1".into(),
        device_port: 9000,
        client_nonce: CLIENT_NONCE,
        app: None,
    };
    send(peer, Frame::json(MessageType::Hello, &hello)).await;
    expect(peer, MessageType::HelloAck).await;
    let transcript = auth::transcript("s1", 9000, &CLIENT_NONCE, &[0xbb; 32]);
    let proof = auth::proof(&Secret::new([7; 32]), auth::Role::Android, &transcript);
    send(
        peer,
        Frame::json(
            MessageType::Auth,
            &Auth {
                android_proof: proof,
            },
        ),
    )
    .await;
    expect(peer, MessageType::ConfigureVpn).await;
}

/// Play the app's side of the handshake up to Active.
async fn activate(peer: &mut TcpStream) {
    authenticate(peer).await;
    let ready = VpnReady {
        addresses: vec![Prefix::new(Ipv4Addr::new(10, 0, 0, 2), 32)],
        mtu: 1400,
    };
    send(peer, Frame::json(MessageType::VpnReady, &ready)).await;
}

#[tokio::test(start_paused = true)]
async fn unanswered_consent_is_a_consent_timeout() {
    let (mut peer, handle, _stop) = super::start().await;
    authenticate(&mut peer).await;
    tokio::time::sleep(super::CONSENT_DEADLINE + Duration::from_secs(1)).await;
    let e = super::expect_error(&mut peer).await;
    assert_eq!(e.code, "consent_timeout");
    assert!(matches!(handle.await.unwrap(), SessionEnd::Refused(_)));
}

#[tokio::test(start_paused = true)]
async fn stop_is_read_while_the_downlink_is_saturated() {
    let (from_tx, from_helper) = mpsc::channel(4);
    let inject: Inject = Arc::new(|_: &[u8]| Ok(true));
    let (mut peer, handle, _stop) = start_with(PacketEndpoints {
        inject,
        from_helper,
    })
    .await;
    activate(&mut peer).await;
    // The helper floods; the peer never reads, so the TCP writer and the
    // downlink stall. Control from the peer must still get through.
    tokio::spawn(async move { while from_tx.send(packet()).await.is_ok() {} });
    tokio::time::sleep(Duration::from_secs(1)).await;
    send(&mut peer, Frame::empty(MessageType::Stop)).await;
    assert_eq!(handle.await.unwrap(), SessionEnd::PeerStop);
}

#[tokio::test(start_paused = true)]
async fn a_gone_helper_ends_the_session() {
    let (from_tx, from_helper) = mpsc::channel::<Vec<u8>>(4);
    let inject: Inject = Arc::new(|_: &[u8]| Err(std::io::ErrorKind::BrokenPipe.into()));
    let (mut peer, handle, _stop) = start_with(PacketEndpoints {
        inject,
        from_helper,
    })
    .await;
    activate(&mut peer).await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    send(&mut peer, Frame::ip_packet(packet())).await;
    assert_eq!(handle.await.unwrap(), SessionEnd::HelperClosed);
    drop(from_tx);

    let (from_tx, from_helper) = mpsc::channel(4);
    let inject: Inject = Arc::new(|_: &[u8]| Ok(true));
    let (mut peer, handle, _stop) = start_with(PacketEndpoints {
        inject,
        from_helper,
    })
    .await;
    activate(&mut peer).await;
    drop(from_tx);
    assert_eq!(handle.await.unwrap(), SessionEnd::HelperClosed);
}

#[tokio::test(start_paused = true)]
async fn packets_alone_keep_the_session_alive() {
    let (mut peer, handle, stop) = start().await;
    activate(&mut peer).await;
    for _ in 0..12 {
        tokio::time::sleep(Duration::from_secs(5)).await;
        send(&mut peer, Frame::ip_packet(packet())).await;
    }
    assert!(
        !handle.is_finished(),
        "a minute of packets without PONG is still life"
    );
    stop.send(true).unwrap();
    assert_eq!(handle.await.unwrap(), SessionEnd::LocalStop);
}
