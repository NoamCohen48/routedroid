use std::path::PathBuf;

use super::*;
use crate::{Datagram, Reply, Request, MAX_DATAGRAM};

fn socket_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rd-seqpacket-{}-{name}.sock", std::process::id()))
}

async fn pair(name: &str) -> (SeqPacket, SeqPacket) {
    let path = socket_path(name);
    let listener = Listener::bind(&path).unwrap();
    let client = SeqPacket::connect(&path).unwrap();
    let server = listener.accept().await.unwrap();
    std::fs::remove_file(&path).unwrap();
    (client, server)
}

#[tokio::test]
async fn control_and_packets_keep_their_boundaries() {
    let (client, server) = pair("boundaries").await;
    client.send_control(&Request::Ping).await.unwrap();
    client.send_packet(&[0x45; 40]).await.unwrap();
    let mut buf = vec![0; MAX_DATAGRAM];
    let first = server.recv(&mut buf).await.unwrap().unwrap();
    assert_eq!(
        Datagram::<Request>::decode(first).unwrap(),
        Datagram::Control(Request::Ping)
    );
    let second = server.recv(&mut buf).await.unwrap().unwrap();
    assert_eq!(
        Datagram::<Request>::decode(second).unwrap(),
        Datagram::Packet(&[0x45; 40][..])
    );
    server.send_control(&Reply::Pong).await.unwrap();
    let reply = client.recv(&mut buf).await.unwrap().unwrap();
    assert_eq!(
        Datagram::<Reply>::decode(reply).unwrap(),
        Datagram::Control(Reply::Pong)
    );
}

#[tokio::test]
async fn an_oversized_datagram_is_an_error_not_a_short_read() {
    let (client, server) = pair("truncated").await;
    client.send_packet(&[0; 100]).await.unwrap();
    let mut small = [0; 50];
    let error = server.recv(&mut small).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(client.send_packet(&vec![0; MAX_PACKET + 1]).await.is_err());
}

#[tokio::test]
async fn a_full_peer_refuses_a_packet_instead_of_blocking() {
    let (client, server) = pair("full").await;
    let mut sent = 0;
    while server.try_send_packet(&[0x45; 1400]).unwrap() {
        sent += 1;
        assert!(sent < 100_000, "the peer never filled up");
    }
    let mut buf = vec![0; MAX_DATAGRAM];
    client.recv(&mut buf).await.unwrap().unwrap();
    assert!(
        server.try_send_packet(&[0x45; 1400]).unwrap(),
        "room again after one receive"
    );
}

#[tokio::test]
async fn close_is_reported_as_none() {
    let (client, server) = pair("close").await;
    drop(client);
    assert!(server.recv(&mut [0; 16]).await.unwrap().is_none());
}

#[tokio::test]
async fn peer_credentials_are_ours() {
    let (client, server) = pair("peercred").await;
    assert_eq!(
        server.peer_uid().unwrap(),
        rustix::process::getuid().as_raw()
    );
    drop(client);
}
