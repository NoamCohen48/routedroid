//! A phone at its lock screen: the wait for the app says so, and stops
//! saying so once the phone is unlocked; the app then connects as usual.

use routedroid_ipc::{ConnectionState, Screen};

use super::fake_app::FakeApp;
use super::{Lab, request};

const PHONE: &str = "R58FAKE01";

fn waiting(screen: Option<Screen>) -> impl Fn(&ConnectionState) -> bool {
    move |state| *state == ConnectionState::WaitingForApp { screen }
}

#[tokio::test]
async fn the_wait_names_a_locked_phone_until_it_is_unlocked() {
    let mut lab = Lab::new(&[PHONE]).await;
    lab.adb.set_screen(PHONE, Some("locked"));
    lab.connections
        .start(request(PHONE, [10, 0, 0, 7]))
        .await
        .unwrap();
    lab.until(PHONE, waiting(Some(Screen::Locked))).await;
    lab.adb.set_screen(PHONE, Some("off"));
    lab.until(PHONE, waiting(Some(Screen::Off))).await;
    lab.adb.set_screen(PHONE, None);
    lab.until(PHONE, waiting(None)).await;

    let _app = FakeApp::connect(&lab.adb, PHONE).await;
    lab.until(PHONE, |s| matches!(s, ConnectionState::Active))
        .await;
    lab.adb.set_screen(PHONE, Some("locked"));
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let states = lab.connections.states();
    assert_eq!(
        states[PHONE],
        ConnectionState::Active,
        "active: no longer asked"
    );
}
