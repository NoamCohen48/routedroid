use super::*;
use crate::kernel::{ROUTE_PROTOCOL, Route, Rule};

const LAN: u32 = 2;

fn ip(s: &str) -> Ipv4Addr {
    s.parse().unwrap()
}

fn host() -> Address {
    Address {
        index: LAN,
        addr: ip("10.0.0.2"),
        prefix: 24,
    }
}

fn default_via(gateway: &str, oif: u32) -> Route {
    Route {
        table: MAIN_TABLE,
        dst: Ipv4Addr::UNSPECIFIED,
        prefix: 0,
        gateway: Some(ip(gateway)),
        oif: Some(oif),
        protocol: 4,
    }
}

fn plan_for(router: Option<&str>, facts: &Facts) -> Result<Egress> {
    plan(ip("10.0.0.5"), router.map(ip), LAN, &host(), facts)
}

#[test]
fn the_table_is_the_phones_address_and_holds_its_subnet() {
    let egress = plan_for(None, &Facts::default()).unwrap();
    assert_eq!(egress.table, u32::from(ip("10.0.0.5")));
    assert_eq!((egress.lan_net, egress.prefix), (ip("10.0.0.0"), 24));
    assert_eq!(egress.gateway, None, "no gateway known: the LAN only");
}

#[test]
fn the_leases_router_wins_over_the_lans_default_route() {
    let facts = Facts {
        routes: vec![default_via("10.0.0.1", LAN)],
        ..Facts::default()
    };
    let gateway = |router| plan_for(router, &facts).unwrap().gateway;
    assert_eq!(gateway(Some("10.0.0.254")), Some(ip("10.0.0.254")));
    assert_eq!(gateway(None), Some(ip("10.0.0.1")));
    // A router off the LAN, or the phone itself, is no gateway.
    assert_eq!(gateway(Some("192.168.1.1")), Some(ip("10.0.0.1")));
    assert_eq!(gateway(Some("10.0.0.5")), Some(ip("10.0.0.1")));
}

#[test]
fn another_interfaces_default_route_is_never_the_phones() {
    // The host's default leaves through wlan0 (#3); the phone gets the LAN only.
    let facts = Facts {
        routes: vec![default_via("192.168.9.1", 3), default_via("10.0.0.1", 9)],
        ..Facts::default()
    };
    assert_eq!(plan_for(None, &facts).unwrap().gateway, None);
}

#[test]
fn a_table_or_rule_already_in_use_is_refused() {
    let table = u32::from(ip("10.0.0.5"));
    let taken = Facts {
        routes: vec![Route {
            table,
            ..default_via("10.0.0.1", LAN)
        }],
        ..Facts::default()
    };
    let refusal = plan_for(None, &taken).unwrap_err().to_string();
    assert!(refusal.contains("table 167772165"), "{refusal}");
    let rule = |table, src| Rule {
        priority: 100,
        table,
        src,
        protocol: 0,
    };
    let by_table = Facts {
        rules: vec![rule(table, None)],
        ..Facts::default()
    };
    assert!(plan_for(None, &by_table).is_err());
    let by_source = Facts {
        rules: vec![rule(77, Some((ip("10.0.0.5"), 32)))],
        ..Facts::default()
    };
    let refusal = plan_for(None, &by_source).unwrap_err().to_string();
    assert!(refusal.contains("from 10.0.0.5"), "{refusal}");
    // Routedroid's own are a session's: the journal decides about those.
    let ours = Facts {
        rules: vec![Rule {
            protocol: ROUTE_PROTOCOL,
            ..rule(table, Some((ip("10.0.0.5"), 32)))
        }],
        ..Facts::default()
    };
    assert!(plan_for(None, &ours).is_ok());
}
