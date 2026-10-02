use super::*;
use crate::Net;

fn device() -> DeviceId {
    "caf60be925035877".parse().unwrap()
}

#[test]
fn wire_shape_is_stable() {
    let start = Request::Start {
        lan_if: IfName::new("eno1").unwrap(),
        phone_ip: Some(Ipv4Addr::new(10, 0, 0, 5)),
        device: device(),
        tun: IfName::new("phone0").unwrap(),
        mtu: 1400,
    };
    assert_eq!(
        serde_json::to_string(&start).unwrap(),
        concat!(
            r#"{"type":"start","lan_if":"eno1","phone_ip":"10.0.0.5","#,
            r#""device":"caf60be925035877","tun":"phone0","mtu":1400}"#
        )
    );
    assert_eq!(
        serde_json::to_string(&Request::Hello { version: 1 }).unwrap(),
        r#"{"type":"hello","version":1}"#
    );
    let error = Reply::Error {
        code: ErrorCode::NoLease,
        message: "m".into(),
    };
    assert_eq!(
        serde_json::to_string(&error).unwrap(),
        r#"{"type":"error","code":"no_lease","message":"m"}"#
    );
    let interfaces = Reply::Interfaces {
        interfaces: vec![Interface {
            name: "eno1".into(),
            up: true,
            addresses: vec![Net {
                address: Ipv4Addr::new(10, 0, 0, 2),
                prefix: 24,
            }],
            default_route: true,
            phone_addresses: vec![],
            dhcp: false,
            ineligible: Some("not in the policy".into()),
        }],
    };
    assert_eq!(
        serde_json::to_string(&interfaces).unwrap(),
        concat!(
            r#"{"type":"interfaces","interfaces":[{"name":"eno1","up":true,"#,
            r#""addresses":[{"address":"10.0.0.2","prefix":24}],"default_route":true,"#,
            r#""phone_addresses":[],"dhcp":false,"ineligible":"not in the policy"}]}"#
        )
    );
}

#[test]
fn a_leased_start_and_its_replies() {
    let start = r#"{"type":"start","lan_if":"eno1","phone_ip":null,"device":"caf60be925035877","tun":"phone0","mtu":1400}"#;
    let Request::Start { phone_ip, .. } = serde_json::from_str(start).unwrap() else {
        panic!("not a start")
    };
    assert_eq!(phone_ip, None);
    let lease = Lease {
        server: Ipv4Addr::new(10, 0, 0, 1),
        router: Some(Ipv4Addr::new(10, 0, 0, 1)),
        dns: vec![Ipv4Addr::new(10, 0, 0, 53)],
        expires_at: 1_800_000_000,
    };
    let started = Reply::Started {
        session: "s".into(),
        tun: IfName::new("phone0").unwrap(),
        phone_ip: Ipv4Addr::new(10, 0, 0, 144),
        host_ip: Ipv4Addr::new(10, 0, 0, 2),
        lan_prefix: 24,
        lease: Some(lease.clone()),
    };
    let json = serde_json::to_string(&started).unwrap();
    assert_eq!(serde_json::from_str::<Reply>(&json).unwrap(), started);
    assert_eq!(
        serde_json::to_string(&Reply::Lease { lease }).unwrap(),
        concat!(
            r#"{"type":"lease","lease":{"server":"10.0.0.1","router":"10.0.0.1","#,
            r#""dns":["10.0.0.53"],"expires_at":1800000000}}"#
        )
    );
}

#[test]
fn unknown_fields_and_bad_names_are_refused() {
    // A typo'd field is a precise error, not a silently ignored one.
    let typo = r#"{"type":"start","lan_if":"eno1","phoneip":"10.0.0.5","device":"caf60be925035877","tun":"phone0","mtu":1400}"#;
    let error = serde_json::from_str::<Request>(typo)
        .unwrap_err()
        .to_string();
    assert!(error.contains("phoneip"), "{error}");
    let bad_tun = r#"{"type":"start","lan_if":"eno1","phone_ip":"10.0.0.5","device":"caf60be925035877","tun":"../x","mtu":1400}"#;
    assert!(serde_json::from_str::<Request>(bad_tun).is_err());
    let bad_device = r#"{"type":"start","lan_if":"eno1","phone_ip":null,"device":"emulator-5554","tun":"phone0","mtu":1400}"#;
    assert!(serde_json::from_str::<Request>(bad_device).is_err());
}
