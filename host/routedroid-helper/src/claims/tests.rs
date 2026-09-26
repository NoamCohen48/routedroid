use std::collections::BTreeSet;

use routedroid_helper_ipc::IfName;

use super::*;
use crate::kernel::fake::Fake;
use crate::op::Leaf;
use crate::test_util::Scratch;

fn setup() -> (Scratch, Claims, Fake, SysctlKey) {
    let scratch = Scratch::new();
    let claims = Claims::new(scratch.path().join("sysctl"));
    let kernel = Fake::default();
    kernel.lock().add_link("lan0", None);
    (scratch, claims, kernel, SysctlKey::new(IfName::new("lan0").unwrap(), Leaf::ProxyArp))
}

fn value(kernel: &Fake, key: &SysctlKey) -> Option<String> {
    kernel.sysctl_read(key).unwrap()
}

#[test]
fn the_last_holder_restores_the_baseline() {
    let (_scratch, claims, kernel, key) = setup();
    let (a, b) = (SessionId::from_raw(1), SessionId::from_raw(2));
    claims.acquire(&kernel, a, &key).unwrap();
    claims.acquire(&kernel, b, &key).unwrap();
    assert_eq!(value(&kernel, &key).as_deref(), Some(ENABLED));
    assert!(claims.holds(a, &key).unwrap() && claims.holds(b, &key).unwrap());

    claims.release(&kernel, a, &key).unwrap();
    assert_eq!(value(&kernel, &key).as_deref(), Some(ENABLED), "b still needs it");
    assert!(!claims.holds(a, &key).unwrap());

    claims.release(&kernel, b, &key).unwrap();
    assert_eq!(value(&kernel, &key).as_deref(), Some("0"));
    assert!(claims.list().unwrap().is_empty());
    claims.release(&kernel, b, &key).unwrap(); // idempotent
}

#[test]
fn a_value_someone_else_changed_is_left_alone() {
    let (_scratch, claims, kernel, key) = setup();
    let a = SessionId::from_raw(1);
    claims.acquire(&kernel, a, &key).unwrap();
    kernel.sysctl_write(&key, "2").unwrap();
    claims.release(&kernel, a, &key).unwrap();
    assert_eq!(value(&kernel, &key).as_deref(), Some("2"));
    assert!(claims.list().unwrap().is_empty());
}

#[test]
fn a_vanished_interface_releases_quietly() {
    let (_scratch, claims, kernel, key) = setup();
    let a = SessionId::from_raw(1);
    claims.acquire(&kernel, a, &key).unwrap();
    kernel.lock().links.clear();
    claims.release(&kernel, a, &key).unwrap();
    assert!(claims.list().unwrap().is_empty());
}

#[test]
fn a_missing_interface_is_never_claimed() {
    let (_scratch, claims, kernel, _) = setup();
    let key = SysctlKey::new(IfName::new("nosuch0").unwrap(), Leaf::Forwarding);
    assert!(claims.acquire(&kernel, SessionId::from_raw(1), &key).is_err());
    assert!(claims.list().unwrap().is_empty());
}

#[test]
fn a_failed_sysctl_write_still_leaves_the_claim_recorded() {
    let (_scratch, claims, kernel, key) = setup();
    kernel.lock().failing.insert("sysctl_write");
    assert!(claims.acquire(&kernel, SessionId::from_raw(1), &key).is_err());
    assert!(claims.holds(SessionId::from_raw(1), &key).unwrap(), "undo must find it");
}

#[test]
fn garbage_collection_drops_holders_without_a_journal() {
    let (_scratch, claims, kernel, key) = setup();
    let (live, lost) = (SessionId::from_raw(1), SessionId::from_raw(2));
    claims.acquire(&kernel, live, &key).unwrap();
    claims.acquire(&kernel, lost, &key).unwrap();

    claims.collect_garbage(&kernel, || Ok(BTreeSet::from([live]))).unwrap();
    assert!(claims.holds(live, &key).unwrap() && !claims.holds(lost, &key).unwrap());
    assert_eq!(value(&kernel, &key).as_deref(), Some(ENABLED));

    claims.collect_garbage(&kernel, || Ok(BTreeSet::new())).unwrap();
    assert_eq!(value(&kernel, &key).as_deref(), Some("0"));
    assert!(claims.list().unwrap().is_empty());
}

#[test]
fn files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let (_scratch, claims, kernel, key) = setup();
    claims.acquire(&kernel, SessionId::from_raw(1), &key).unwrap();
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&claims.dir), 0o700);
    assert_eq!(mode(&claims.path(&key)), 0o600);
}
