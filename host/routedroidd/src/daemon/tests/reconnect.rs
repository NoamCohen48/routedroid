//! A phone that goes away mid-connection: unplugged, it keeps its host side
//! for `reconnect_wait` and resumes on the same helper session when it is
//! back; its app closing the connection, or an unplug with no wait, still
//! ends it at once.

use routedroid_helper_ipc::Request;
use routedroid_ipc::{ConnectionState, EndReason, Kind, Outcome, StartRequest};
use routedroid_proto::frame::{Frame, MessageType};

use super::fake_app::FakeApp;
use super::{Lab, request};

const PHONE: &str = "R58FAKE01";

fn waiting(secs: u64) -> StartRequest {
    StartRequest {
        reconnect_secs: Some(secs),
        ..request(PHONE, [10, 0, 0, 7])
    }
}

fn is_active(state: &ConnectionState) -> bool {
    *state == ConnectionState::Active
}

fn is_away(state: &ConnectionState) -> bool {
    matches!(state, ConnectionState::Reconnecting { .. })
}

fn starts(lab: &Lab) -> usize {
    let seen = lab.helper.seen.lock().unwrap();
    seen.iter()
        .filter(|r| matches!(r, Request::Start { .. }))
        .count()
}

/// Started and active, with the app's end of it.
async fn active(lab: &mut Lab, request: StartRequest) -> FakeApp {
    lab.connections.start(request).await.expect("start");
    let app = FakeApp::connect(&lab.adb, PHONE).await;
    lab.until(PHONE, is_active).await;
    app
}

#[tokio::test]
async fn an_unplugged_phone_resumes_on_the_same_host_side() {
    let mut lab = Lab::new(&[PHONE]).await;
    let app = active(&mut lab, waiting(30)).await;
    lab.adb.unplug(PHONE);
    drop(app);
    let away = lab.until(PHONE, is_away).await;
    assert!(
        matches!(away, ConnectionState::Reconnecting { wait_secs } if wait_secs <= 30 && wait_secs > 0)
    );
    assert_eq!(lab.helper.stops(), 0, "the host side is held");

    lab.adb.plug(PHONE);
    let mut app = FakeApp::connect(&lab.adb, PHONE).await;
    lab.until(PHONE, is_active).await;
    assert_eq!(starts(&lab), 1, "no second helper session");
    let info = lab.connections.info().remove(0);
    assert_eq!(info.tun, "phone0");
    // Packets flow on the new session.
    let mut packet = vec![0u8; 40];
    (packet[0], packet[3]) = (0x45, 40);
    app.send(Frame::ip_packet(packet.clone())).await;
    let echoed = app.next().await;
    assert_eq!(
        (echoed.message_type, echoed.body),
        (MessageType::IpPacket, packet)
    );

    let outcome = lab.connections.stop(PHONE).await.unwrap();
    let stopped = Outcome::Clean {
        reason: EndReason::Stopped,
    };
    assert_eq!(outcome, stopped);
    assert_eq!(lab.helper.stops(), 1);
}

#[tokio::test]
async fn a_phone_that_is_not_back_in_time_ends_the_connection() {
    let mut lab = Lab::new(&[PHONE]).await;
    let app = active(&mut lab, waiting(1)).await;
    lab.adb.unplug(PHONE);
    drop(app);
    lab.until(PHONE, is_away).await;
    let Outcome::Failed { kind, message } = lab.ended(PHONE).await else {
        panic!("a phone that never came back is a failure");
    };
    assert_eq!(kind, Kind::Adb);
    assert!(message.contains("not back within 1 s"), "{message}");
    assert_eq!(lab.helper.stops(), 1);
}

#[tokio::test]
async fn stop_while_away_ends_it_cleanly() {
    let mut lab = Lab::new(&[PHONE]).await;
    let app = active(&mut lab, waiting(30)).await;
    lab.adb.unplug(PHONE);
    drop(app);
    lab.until(PHONE, is_away).await;
    let outcome = lab.connections.stop(PHONE).await.unwrap();
    assert_eq!(
        outcome,
        Outcome::Clean {
            reason: EndReason::Stopped
        }
    );
    assert_eq!(lab.helper.stops(), 1);
}

#[tokio::test]
async fn the_app_closing_with_the_phone_attached_ends_it() {
    let mut lab = Lab::new(&[PHONE]).await;
    let app = active(&mut lab, waiting(30)).await;
    drop(app);
    let outcome = lab.ended(PHONE).await;
    assert_eq!(
        outcome,
        Outcome::Clean {
            reason: EndReason::PhoneClosed
        }
    );
}

#[tokio::test]
async fn with_no_wait_an_unplug_ends_it_at_once() {
    let mut lab = Lab::new(&[PHONE]).await;
    let app = active(&mut lab, waiting(0)).await;
    lab.adb.unplug(PHONE);
    drop(app);
    let outcome = lab.ended(PHONE).await;
    assert_eq!(
        outcome,
        Outcome::Clean {
            reason: EndReason::PhoneClosed
        }
    );
}
