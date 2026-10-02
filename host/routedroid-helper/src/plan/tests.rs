use super::*;
use crate::kernel::{Address, Route};

const POLICY: &str = r#"
[[interface]]
name = "lan0"
phone_addresses = ["10.0.0.0/24"]
"#;

fn ip(s: &str) -> Ipv4Addr {
    s.parse().unwrap()
}

fn request(phone_ip: &str) -> Request {
    Request {
        lan_if: IfName::new("lan0").unwrap(),
        phone_ip: ip(phone_ip),
        tun: IfName::new("phone0").unwrap(),
        mtu: 1400,
    }
}

/// lan0 (#2) is 10.0.0.2/24 with a gateway at .1 and a neighbour at .9;
/// the host also owns 10.0.0.3 as a secondary and 192.168.9.1 elsewhere.
fn facts() -> Facts {
    let address = |index, addr: &str, prefix| Address {
        index,
        addr: ip(addr),
        prefix,
    };
    let route = |dst: &str, prefix, gateway: Option<&str>| Route {
        dst: ip(dst),
        prefix,
        gateway: gateway.map(ip),
        oif: Some(2),
        protocol: 4,
    };
    Facts {
        lan: Some(2),
        tun_exists: false,
        addresses: vec![
            address(3, "192.168.9.1", 24),
            address(2, "10.0.0.2", 24),
            address(2, "10.0.0.3", 24),
        ],
        routes: vec![
            route("0.0.0.0", 0, Some("10.0.0.1")),
            route("10.0.0.0", 24, None),
            route("10.0.0.77", 32, None),
        ],
        neighbours: vec![ip("10.0.0.9")],
    }
}

fn build(phone_ip: &str, facts: &Facts) -> Result<Plan> {
    Plan::build(
        SessionId::from_raw(7),
        request(phone_ip),
        &POLICY.parse().unwrap(),
        facts,
    )
}

fn refusal(phone_ip: &str, facts: &Facts) -> String {
    build(phone_ip, facts).unwrap_err().to_string()
}

#[test]
fn a_free_address_in_policy_is_planned() {
    let plan = build("10.0.0.5", &facts()).unwrap();
    assert_eq!((plan.host_ip(), plan.lan_prefix()), (ip("10.0.0.2"), 24));
    assert_eq!(
        plan.reservation(),
        Reservation {
            tun: IfName::new("phone0").unwrap(),
            phone_ip: ip("10.0.0.5")
        }
    );
    let labels: Vec<_> = plan.ops().iter().map(Op::label).collect();
    assert_eq!(
        labels,
        [
            "tun:phone0",
            "nft:inet:routedroid_phone0",
            "sysctl:net.ipv4.conf.phone0.forwarding",
            "sysctl:net.ipv4.conf.lan0.forwarding",
            "sysctl:net.ipv4.conf.lan0.proxy_arp",
            "route:10.0.0.5/32@phone0",
        ]
    );
    assert_eq!(plan.firewall().tag, "routedroid:0000000000000007");
}

#[test]
fn addresses_already_in_use_are_refused() {
    let facts = facts();
    assert_eq!(refusal("10.0.0.1", &facts), "10.0.0.1 is a gateway");
    assert_eq!(
        refusal("10.0.0.2", &facts),
        "10.0.0.2 is one of this host's addresses"
    );
    assert_eq!(
        refusal("10.0.0.3", &facts),
        "10.0.0.3 is one of this host's addresses"
    );
    assert_eq!(refusal("10.0.0.9", &facts), "10.0.0.9 is in use on lan0");
    assert_eq!(
        refusal("10.0.0.77", &facts),
        "10.0.0.77 already has a host route"
    );
}

#[test]
fn network_and_broadcast_addresses_are_refused() {
    assert_eq!(
        refusal("10.0.0.0", &facts()),
        "10.0.0.0 is the network or broadcast address of 10.0.0.2/24"
    );
    assert_eq!(
        refusal("10.0.0.255", &facts()),
        "10.0.0.255 is the network or broadcast address of 10.0.0.2/24"
    );
}

#[test]
fn only_the_operators_interfaces_and_addresses() {
    let mut facts = facts();
    let outside = Plan::build(
        SessionId::from_raw(7),
        request("192.168.9.5"),
        &POLICY.parse().unwrap(),
        &facts,
    );
    assert_eq!(
        outside.unwrap_err().to_string(),
        "192.168.9.5 is not a phone address the policy allows on lan0"
    );
    let mut other = request("10.0.0.5");
    other.lan_if = IfName::new("wg0").unwrap();
    let refused = Plan::build(
        SessionId::from_raw(7),
        other,
        &POLICY.parse().unwrap(),
        &facts,
    )
    .unwrap_err();
    assert_eq!(
        refused.to_string(),
        "wg0 is not an interface the policy allows"
    );
    facts.lan = None;
    assert_eq!(refusal("10.0.0.5", &facts), "lan0 does not exist");
}

#[test]
fn the_phone_must_be_on_the_lan_subnet() {
    let wide: Policy = "[[interface]]\nname = \"lan0\"\nphone_addresses = [\"0.0.0.0/0\"]\n"
        .parse()
        .unwrap();
    let refused = Plan::build(
        SessionId::from_raw(7),
        request("172.16.0.5"),
        &wide,
        &facts(),
    )
    .unwrap_err();
    assert_eq!(
        refused.to_string(),
        "172.16.0.5 is not inside any IPv4 subnet of lan0"
    );
}

#[test]
fn tun_name_and_mtu_are_checked() {
    let mut facts = facts();
    let policy: Policy = POLICY.parse().unwrap();
    let mut r = request("10.0.0.5");
    r.tun = IfName::new("tun0").unwrap();
    assert!(Plan::build(SessionId::from_raw(7), r, &policy, &facts).is_err());
    let mut r = request("10.0.0.5");
    r.mtu = 100;
    assert!(Plan::build(SessionId::from_raw(7), r, &policy, &facts).is_err());
    facts.tun_exists = true;
    assert_eq!(refusal("10.0.0.5", &facts), "phone0 already exists");
}

#[test]
fn point_to_point_subnets_have_no_broadcast() {
    let mut facts = facts();
    facts.addresses = vec![Address {
        index: 2,
        addr: ip("10.0.0.2"),
        prefix: 31,
    }];
    facts.routes.clear();
    build("10.0.0.3", &facts).unwrap();
}
