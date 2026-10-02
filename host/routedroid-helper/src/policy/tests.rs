use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::test_util::Scratch;

const TEXT: &str = r#"
[[interface]]
name = "eno1"
phone_addresses = ["192.168.1.200/29", "192.168.1.250/32"]

[[interface]]
name = "eth0.100"
phone_addresses = []
"#;

fn name(s: &str) -> IfName {
    IfName::new(s).unwrap()
}

#[test]
fn allows_only_listed_interfaces_and_addresses() {
    let policy: Policy = TEXT.parse().unwrap();
    policy
        .check(&name("eno1"), "192.168.1.203".parse().unwrap())
        .unwrap();
    policy
        .check(&name("eno1"), "192.168.1.250".parse().unwrap())
        .unwrap();
    assert!(
        policy
            .check(&name("eno1"), "192.168.1.1".parse().unwrap())
            .is_err()
    );
    assert!(
        policy
            .check(&name("eth0.100"), "192.168.1.203".parse().unwrap())
            .is_err()
    );
    let err = policy
        .check(&name("docker0"), "192.168.1.203".parse().unwrap())
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "docker0 is not an interface the policy allows"
    );
}

#[test]
fn an_empty_policy_allows_nothing() {
    let policy: Policy = "".parse().unwrap();
    assert!(
        policy
            .check(&name("eno1"), "192.168.1.203".parse().unwrap())
            .is_err()
    );
}

#[test]
fn malformed_policies_are_refused() {
    for bad in [
        "[[interface]]\nname = \"eno1\"\n",
        "[[interface]]\nname = \"all\"\nphone_addresses = []\n",
        "[[interface]]\nname = \"eno1\"\nphone_addresses = [\"10.0.0.1/24\"]\n",
        "[[interface]]\nname = \"eno1\"\nphone_addresses = []\nextra = 1\n",
        "allow_everything = true\n",
        "[[interface]]\nname = \"eno1\"\nphone_addresses = []\n[[interface]]\nname = \"eno1\"\nphone_addresses = []\n",
    ] {
        assert!(bad.parse::<Policy>().is_err(), "{bad}");
    }
}

#[test]
fn the_file_must_not_be_writable_by_others() {
    let scratch = Scratch::new();
    let path = scratch.path().join("helper.toml");
    assert!(Policy::load(&path).is_err(), "missing file");
    fs::write(&path, TEXT).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    Policy::load(&path).unwrap();
    for mode in [0o664, 0o646] {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        assert!(Policy::load(&path).is_err(), "{mode:o}");
    }
}
