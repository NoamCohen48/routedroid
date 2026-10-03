//! `doctor` against the fake phone and helper.

use routedroid_ipc::{Check, CheckStatus};

use super::Lab;
use super::fake_replies::leftover;
use crate::daemon::doctor;

fn find<'a>(checks: &'a [Check], name: &str) -> &'a Check {
    checks.iter().find(|c| c.name == name).unwrap()
}

#[tokio::test]
async fn looks_at_adb_the_helper_and_its_findings_and_only_repairs_when_asked() {
    let lab = Lab::new(&["R58FAKE01"]).await;
    let socket = lab.helper.socket.clone();
    let (checks, done) = doctor::run(&lab.devices, &socket, false).await;
    assert!(done.is_empty());
    assert_eq!(find(&checks, "adb").status, CheckStatus::Ok);
    assert_eq!(find(&checks, "helper").status, CheckStatus::Ok);
    let policy = find(&checks, "policy");
    assert_eq!(policy.detail, "phones may join through lan0 (DHCP)");
    let table = find(&checks, &leftover().subject);
    assert_eq!(
        (table.status, &table.repair),
        (CheckStatus::Fail, &leftover().repair)
    );

    let (checks, done) = doctor::run(&lab.devices, &socket, true).await;
    assert_eq!(done, leftover().repair);
    assert_eq!(find(&checks, "leftovers").status, CheckStatus::Ok);
}

#[tokio::test]
async fn an_absent_helper_is_a_failure_with_a_hint() {
    let lab = Lab::new(&[]).await;
    let (checks, _) = doctor::run(
        &lab.devices,
        std::path::Path::new("/nonexistent.sock"),
        false,
    )
    .await;
    assert_eq!(
        find(&checks, "adb").status,
        CheckStatus::Warn,
        "no phone attached"
    );
    let helper = find(&checks, "helper");
    assert_eq!(helper.status, CheckStatus::Fail);
    assert!(
        helper.detail.contains("routedroid-helper.socket enabled"),
        "{}",
        helper.detail
    );
    assert_eq!(checks.len(), 2, "nothing more to ask");
}
