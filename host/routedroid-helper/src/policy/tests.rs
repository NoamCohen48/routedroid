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
dhcp = true

[[interface]]
name = "wlan0"
phone_addresses = ["10.1.0.0/24"]
dhcp = true
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
fn leases_are_opt_in_and_bounded_by_the_blocks() {
    let policy: Policy = TEXT.parse().unwrap();
    let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
    assert!(!policy.dhcp("eno1") && policy.dhcp("eth0.100") && policy.dhcp("wlan0"));
    assert_eq!(
        policy.check_dhcp(&name("eno1")).unwrap_err().to_string(),
        "the policy does not allow DHCP on eno1"
    );
    assert!(
        policy
            .check_leased(&name("eno1"), ip("192.168.1.203"))
            .is_err()
    );
    policy
        .check_leased(&name("eth0.100"), ip("172.16.9.9"))
        .unwrap();
    policy
        .check_leased(&name("wlan0"), ip("10.1.0.77"))
        .unwrap();
    assert!(
        policy
            .check_leased(&name("wlan0"), ip("10.2.0.77"))
            .is_err()
    );
    assert!(policy.check_dhcp(&name("docker0")).is_err());
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
        "[[interface]]\nname = \"eno1\"\nphone_addresses = []\ndhcp = false\n",
        "[[interface]]\nname = \"eno1\"\ndhcp = \"yes\"\n",
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
