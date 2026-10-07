use std::net::Ipv4Addr;

use routedroid_ipc::{ConnectionState, EndReason, Kind, Outcome};
use routedroid_proto::frame::{Frame, MessageType};

use super::fake_app::FakeApp;
use super::{Lab, request};

const PHONE: &str = "R58FAKE01";
const OTHER: &str = "R58FAKE02";

/// A minimal IPv4 header; the daemon checks only its shape.
fn packet(len: u16) -> Vec<u8> {
    let mut packet = vec![0u8; usize::from(len)];
    packet[0] = 0x45;
    packet[2..4].copy_from_slice(&len.to_be_bytes());
    packet
}

fn is_active(state: &ConnectionState) -> bool {
    *state == ConnectionState::Active
}

#[tokio::test]
async fn a_phone_goes_active_carries_packets_and_stops_cleanly() {
    let mut lab = Lab::new(&[PHONE]).await;
    let tun = lab
        .connections
        .start(request(PHONE, [10, 0, 0, 7]))
        .await
        .unwrap();
    assert_eq!(tun.tun.as_str(), "phone0");
    let mut app = FakeApp::connect(&lab.adb, PHONE).await;
    assert_eq!(
        app.configure.addresses[0].address,
        Ipv4Addr::new(10, 0, 0, 7)
    );
    lab.until(PHONE, is_active).await;

    // The helper echoes: what the phone sends comes back from the "LAN".
    app.send(Frame::ip_packet(packet(60))).await;
    let echoed = app.next().await;
    assert_eq!(
        (echoed.message_type, echoed.body),
        (MessageType::IpPacket, packet(60))
    );
    let info = lab.connections.info().remove(0);
    let network = info.network.expect("placed on the LAN");
    assert_eq!(
        (network.phone_ip, network.host_ip),
        (Ipv4Addr::new(10, 0, 0, 7), Ipv4Addr::new(10, 0, 0, 1))
    );

    let outcome = lab.connections.stop(PHONE).await.unwrap();
    assert_eq!(
        outcome,
        Outcome::Clean {
            reason: EndReason::Stopped
        }
    );
    assert_eq!(lab.helper.stops(), 1, "the helper was told to undo");
    assert!(
        lab.adb.reverse(PHONE).is_empty(),
        "the reverse mapping is gone"
    );
    assert!(lab.connections.info().is_empty());
}

#[tokio::test]
async fn the_phone_stopping_is_a_clean_end() {
    let mut lab = Lab::new(&[PHONE]).await;
    lab.connections
        .start(request(PHONE, [10, 0, 0, 7]))
        .await
        .unwrap();
    let mut app = FakeApp::connect(&lab.adb, PHONE).await;
    lab.until(PHONE, is_active).await;
    app.send(Frame::empty(MessageType::Stop)).await;
    let outcome = lab.ended(PHONE).await;
    assert_eq!(
        outcome,
        Outcome::Clean {
            reason: EndReason::PhoneStopped
        }
    );
    assert_eq!(lab.helper.stops(), 1);
}

#[tokio::test]
async fn starts_are_checked_against_the_live_connections() {
    let mut lab = Lab::new(&[PHONE, OTHER]).await;
    let refusal = |r: crate::fault::Result<_>| r.unwrap_err().to_string();
    let unattached = lab
        .connections
        .start(request("R58GONE", [10, 0, 0, 9]))
        .await;
    assert_eq!(refusal(unattached), "R58GONE is not attached");

    lab.connections
        .start(request(PHONE, [10, 0, 0, 7]))
        .await
        .unwrap();
    let again = lab.connections.start(request(PHONE, [10, 0, 0, 8])).await;
    assert_eq!(refusal(again), "R58FAKE01 is already connected");
    let same_ip = lab.connections.start(request(OTHER, [10, 0, 0, 7])).await;
    assert_eq!(refusal(same_ip), "10.0.0.7 is already used by R58FAKE01");
    let tun = lab
        .connections
        .start(request(OTHER, [10, 0, 0, 8]))
        .await
        .unwrap();
    assert_eq!(tun.tun.as_str(), "phone1", "the next free TUN");

    // Neither app ever connects: a stop now ends before the session began.
    lab.until(PHONE, |s| {
        matches!(s, ConnectionState::WaitingForApp { .. })
    })
    .await;
    let outcome = lab.connections.stop(PHONE).await.unwrap();
    assert_eq!(
        outcome,
        Outcome::Clean {
            reason: EndReason::StoppedEarly
        }
    );
    assert!(lab.connections.stop(OTHER).await.unwrap().is_clean());
    assert!(lab.connections.info().is_empty());
}

#[tokio::test]
async fn a_start_outside_the_policy_is_refused_before_it_begins() {
    let lab = Lab::new(&[PHONE]).await;
    let outside = lab.connections.start(request(PHONE, [10, 9, 9, 9])).await;
    let error = outside.unwrap_err();
    assert_eq!(error.kind(), Kind::Usage);
    assert!(
        error
            .to_string()
            .starts_with("10.9.9.9 is not a phone address"),
        "{error}"
    );
    let mut elsewhere = request(PHONE, [10, 0, 0, 7]);
    elsewhere.lan_if = Some("eth9".into());
    let error = lab.connections.start(elsewhere).await.unwrap_err();
    assert_eq!(error.to_string(), "eth9: no such interface");
    assert!(lab.connections.info().is_empty());
}

#[tokio::test]
async fn a_refusing_helper_fails_the_connection_and_frees_the_serial() {
    let mut lab = Lab::new(&[PHONE]).await;
    lab.helper
        .refuse
        .store(true, std::sync::atomic::Ordering::SeqCst);
    lab.connections
        .start(request(PHONE, [10, 0, 0, 7]))
        .await
        .unwrap();
    match lab.ended(PHONE).await {
        Outcome::Failed { kind, message } => {
            assert_eq!(kind, Kind::Helper);
            assert!(
                message.contains("not an interface the policy allows"),
                "{message}"
            );
        }
        other => panic!("expected a failure, got {other:?}"),
    }
    assert!(
        lab.connections.info().is_empty(),
        "the failed entry left the table"
    );
    assert!(lab.adb.record(PHONE).is_none(), "nothing reached the phone");

    lab.helper
        .refuse
        .store(false, std::sync::atomic::Ordering::SeqCst);
    lab.connections
        .start(request(PHONE, [10, 0, 0, 7]))
        .await
        .unwrap();
    FakeApp::connect(&lab.adb, PHONE).await;
    lab.until(PHONE, is_active).await;
}
