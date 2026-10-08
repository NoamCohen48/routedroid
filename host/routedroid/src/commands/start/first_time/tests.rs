use routedroid_ipc::InterfaceInfo;

use super::{missing, yes};

fn link(name: &str, ineligible: Option<&str>) -> InterfaceInfo {
    InterfaceInfo {
        name: name.into(),
        up: true,
        addresses: vec![],
        default_route: false,
        phone_addresses: vec![],
        dhcp: false,
        ineligible: ineligible.map(Into::into),
    }
}

#[test]
fn what_is_missing_comes_first() {
    let refused = [link("eno1", Some("not in the helper policy"))];
    let allowed = [link("eno1", None)];
    assert_eq!(
        missing(false, None, Some(&allowed)).unwrap(),
        "you are not in group routedroid yet"
    );
    assert_eq!(
        missing(true, Some(false), None).unwrap(),
        "the daemon is not enabled"
    );
    assert_eq!(
        missing(true, None, Some(&refused)).unwrap(),
        "no interface may carry phones yet"
    );
    assert_eq!(missing(true, None, Some(&allowed)), None);
    assert_eq!(
        missing(true, Some(true), None),
        None,
        "enabled but not running: the usual hint, not setup"
    );
    assert_eq!(
        missing(true, None, None),
        None,
        "no survey: not for us to say"
    );
}

#[test]
fn enter_means_yes() {
    assert!(yes("\n") && yes("y\n") && yes(" Yes "));
    assert!(!yes("n\n") && !yes("no") && !yes("later"));
}
