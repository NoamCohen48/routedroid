use anyhow::anyhow;
use routedroid_helper_ipc::Net;

use super::*;
use crate::kernel::Route;
use crate::kernel::fake::Fake;

const POLICY: &str = r#"
[[interface]]
name = "lan0"
phone_addresses = ["10.0.0.200/29"]

[[interface]]
name = "br0"
dhcp = true
"#;

/// lan0 (eligible, default route), wlan0 (no carrier), br0 with port eth1,
/// lo, a phone TUN, and eth2 without an address.
fn kernel() -> Fake {
    let kernel = Fake::default();
    let mut s = kernel.lock();
    let lan = s.add_link("lan0", None);
    let wlan = s.add_link("wlan0", None);
    let bridge = s.add_link("br0", None);
    s.add_link("eth1", None);
    let lo = s.add_link("lo", None);
    s.add_link("phone0", Some("tag"));
    s.add_link("eth2", None);
    s.links.get_mut("wlan0").unwrap().carrier = false;
    s.links.get_mut("eth1").unwrap().master = Some(bridge);
    s.links.get_mut("lo").unwrap().kind = LinkKind::Loopback;
    s.links.get_mut("phone0").unwrap().kind = LinkKind::TunTap;
    for (index, addr) in [(lan, "10.0.0.2"), (wlan, "10.1.0.2"), (bridge, "10.2.0.2")] {
        s.addresses.push(Address {
            index,
            addr: addr.parse().unwrap(),
            prefix: 24,
        });
    }
    s.addresses.push(Address {
        index: lo,
        addr: "127.0.0.1".parse().unwrap(),
        prefix: 8,
    });
    s.routes.push(Route {
        dst: "0.0.0.0".parse().unwrap(),
        prefix: 0,
        gateway: Some("10.0.0.1".parse().unwrap()),
        oif: Some(lan),
        protocol: 4,
    });
    drop(s);
    kernel
}

fn verdicts(policy: &Result<Policy>) -> Vec<(String, Option<String>)> {
    survey(&kernel(), policy)
        .unwrap()
        .into_iter()
        .map(|i| (i.name, i.ineligible))
        .collect()
}

#[test]
fn each_link_gets_a_verdict() {
    let reason = |s: &str| Some(s.to_string());
    assert_eq!(
        verdicts(&Ok(POLICY.parse().unwrap())),
        vec![
            ("lan0".into(), None),
            ("wlan0".into(), reason("no carrier (cable or Wi-Fi down)")),
            ("br0".into(), None),
            ("eth1".into(), reason("a port of br0; use that instead")),
            ("lo".into(), reason("loopback")),
            ("phone0".into(), reason("a phone's TUN")),
            ("eth2".into(), reason("no IPv4 address")),
        ]
    );
}

#[test]
fn the_eligible_link_carries_its_details() {
    let all = survey(&kernel(), &Ok(POLICY.parse().unwrap())).unwrap();
    let lan = &all[0];
    assert!(lan.up && lan.default_route);
    let net = |address: &str, prefix| Net {
        address: address.parse().unwrap(),
        prefix,
    };
    assert_eq!(lan.addresses, vec![net("10.0.0.2", 24)]);
    assert_eq!(lan.phone_addresses, vec![net("10.0.0.200", 29)]);
    assert!(!lan.dhcp);
    let br0 = &all[2];
    assert!(br0.dhcp && br0.phone_addresses.is_empty() && br0.ineligible.is_none());
    assert!(!all[1].up && !all[1].default_route);
}

#[test]
fn an_unreadable_policy_is_the_reason_not_a_failure() {
    let lan = verdicts(&Err(anyhow!("policy /x: not found"))).remove(0);
    assert_eq!(
        lan.1.as_deref(),
        Some("the helper policy cannot be read: policy /x: not found")
    );
}
