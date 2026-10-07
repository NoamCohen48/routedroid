//! The app the daemon carries goes on phones that lack it or have an older
//! one, before anything else; a phone with the same or a newer app is left
//! alone. The fake APK is its own versionCode (see fake_adb.rs).

use routedroid_helper_ipc::Request;
use routedroid_ipc::{ConnectionState, Kind, Outcome};

use super::fake_app::FakeApp;
use super::{Lab, WAIT, request};
use crate::app::BundledApp;

const PHONE: &str = "R58FAKE01";
const CARRIED: u64 = 1_002_003;

async fn lab() -> Lab {
    Lab::carrying(&[PHONE], Some(BundledApp::new(b"1002003", CARRIED))).await
}

/// Start, let the app connect, and return every state up to active.
async fn connect(lab: &mut Lab) -> Vec<ConnectionState> {
    lab.connections
        .start(request(PHONE, [10, 0, 0, 7]))
        .await
        .unwrap();
    let _app = FakeApp::connect(&lab.adb, PHONE).await;
    let mut states = Vec::new();
    let collect = async {
        while states.last() != Some(&ConnectionState::Active) {
            if let routedroid_ipc::Event::Connection { state, .. } =
                lab.events.recv().await.unwrap()
            {
                states.push(state);
            }
        }
    };
    tokio::time::timeout(WAIT, collect)
        .await
        .expect("never active");
    lab.connections.stop(PHONE).await.unwrap();
    states
}

#[tokio::test]
async fn a_phone_without_the_app_gets_it_first() {
    let mut lab = lab().await;
    let states = connect(&mut lab).await;
    assert_eq!(lab.adb.app(PHONE), Some(CARRIED));
    assert_eq!(
        states[..3],
        [
            ConnectionState::Starting,
            ConnectionState::InstallingApp,
            ConnectionState::Starting
        ]
    );
}

#[tokio::test]
async fn an_older_app_is_upgraded() {
    let mut lab = lab().await;
    lab.adb.set_app(PHONE, Some(1_002_002));
    let states = connect(&mut lab).await;
    assert_eq!(lab.adb.app(PHONE), Some(CARRIED));
    assert!(states.contains(&ConnectionState::InstallingApp));
}

#[tokio::test]
async fn the_same_or_a_newer_app_is_left_alone() {
    for installed in [CARRIED, 2_000_000] {
        let mut lab = lab().await;
        lab.adb.set_app(PHONE, Some(installed));
        let states = connect(&mut lab).await;
        assert_eq!(lab.adb.app(PHONE), Some(installed));
        assert!(
            !states.contains(&ConnectionState::InstallingApp),
            "{states:?}"
        );
    }
}

#[tokio::test]
async fn a_refused_upgrade_connects_with_the_installed_app() {
    let mut lab = lab().await;
    lab.adb.set_app(PHONE, Some(1_002_002));
    lab.adb.refuse_installs(PHONE);
    let states = connect(&mut lab).await;
    assert_eq!(lab.adb.app(PHONE), Some(1_002_002));
    let at = states
        .iter()
        .position(|s| *s == ConnectionState::InstallingApp);
    assert_eq!(
        states.get(at.unwrap() + 1),
        Some(&ConnectionState::Starting)
    );
}

#[tokio::test]
async fn a_refused_install_ends_the_connection_and_says_why() {
    let mut lab = lab().await;
    lab.adb.refuse_installs(PHONE);
    lab.connections
        .start(request(PHONE, [10, 0, 0, 7]))
        .await
        .unwrap();
    let Outcome::Failed { kind, message } = lab.ended(PHONE).await else {
        panic!("a clean end");
    };
    assert_eq!(kind, Kind::Adb);
    assert!(
        message.contains("could not install the Routedroid app"),
        "{message}"
    );
    assert!(
        message.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE"),
        "{message}"
    );
    let seen = lab.helper.seen.lock().unwrap();
    let started = seen.iter().any(|r| matches!(r, Request::Start { .. }));
    assert!(!started, "nothing was set up on the host");
}
