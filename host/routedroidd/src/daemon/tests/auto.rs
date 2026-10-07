//! Remembered phones: connected as they are plugged in, by name, with what
//! was remembered for them.

use routedroid_ipc::{ConnectionState, Phone, StartRequest};

use super::fake_app::FakeApp;
use super::{Lab, request};
use crate::daemon::auto;
use crate::daemon::background::Background;

const PHONE: &str = "R58FAKE01";

fn is_active(state: &ConnectionState) -> bool {
    matches!(state, ConnectionState::Active)
}

fn remembered(auto: bool) -> Phone {
    Phone {
        serial: PHONE.into(),
        name: Some("pixel".into()),
        auto,
        phone_ip: Some([10, 0, 0, 9].into()),
        ..Phone::default()
    }
}

async fn gone(lab: &Lab) {
    let mut changes = lab.devices.changes();
    while changes
        .borrow_and_update()
        .iter()
        .any(|d| d.serial == PHONE)
    {
        changes.changed().await.unwrap();
    }
}

#[tokio::test]
async fn a_remembered_phone_connects_when_plugged_in_and_again_after_a_stop() {
    let mut lab = Lab::new(&[PHONE]).await;
    lab.connections.phones().remember(remembered(true)).unwrap();
    lab.adb.unplug(PHONE);
    gone(&lab).await;
    let _auto = Background::spawn(auto::run(lab.connections.clone(), lab.devices.changes()));

    lab.adb.plug(PHONE);
    let app = FakeApp::connect(&lab.adb, PHONE).await;
    lab.until(PHONE, is_active).await;
    let info = lab.connections.info();
    assert_eq!(info[0].name.as_deref(), Some("pixel"));
    assert_eq!(info[0].lan_if, "lan0", "the one LAN the policy allows");
    drop(app);

    assert!(
        lab.connections.stop("pixel").await.unwrap().is_clean(),
        "stopped by name"
    );
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    assert!(
        lab.connections.info().is_empty(),
        "a stop is not undone while plugged in"
    );

    lab.adb.unplug(PHONE);
    gone(&lab).await;
    lab.adb.plug(PHONE);
    FakeApp::connect(&lab.adb, PHONE).await;
    lab.until(PHONE, is_active).await;
}

#[tokio::test]
async fn attached_when_the_daemon_starts_counts_as_plugged_in() {
    let mut lab = Lab::new(&[PHONE]).await;
    lab.connections.phones().remember(remembered(true)).unwrap();
    let _auto = Background::spawn(auto::run(lab.connections.clone(), lab.devices.changes()));
    FakeApp::connect(&lab.adb, PHONE).await;
    lab.until(PHONE, is_active).await;
}

#[tokio::test]
async fn without_auto_a_remembered_phone_waits_to_be_asked() {
    let mut lab = Lab::new(&[PHONE]).await;
    lab.connections
        .phones()
        .remember(remembered(false))
        .unwrap();
    let _auto = Background::spawn(auto::run(lab.connections.clone(), lab.devices.changes()));
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert!(lab.connections.info().is_empty());

    let by_name = StartRequest {
        serial: Some("pixel".into()),
        lan_if: None,
        phone_ip: None,
        ..request(PHONE, [10, 0, 0, 7])
    };
    let accepted = lab.connections.start(by_name).await.unwrap();
    assert_eq!(accepted.serial, PHONE);
    assert_eq!(
        accepted.phone_ip,
        Some([10, 0, 0, 9].into()),
        "as remembered"
    );
    FakeApp::connect(&lab.adb, PHONE).await;
    lab.until(PHONE, is_active).await;
}
