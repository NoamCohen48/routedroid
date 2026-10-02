use super::*;
use crate::Net;

#[test]
fn wire_shape_is_stable() {
    let start = Request::Start {
        lan_if: IfName::new("eno1").unwrap(),
        phone_ip: Ipv4Addr::new(10, 0, 0, 5),
        tun: IfName::new("phone0").unwrap(),
        mtu: 1400,
    };
    assert_eq!(
        serde_json::to_string(&start).unwrap(),
        r#"{"type":"start","lan_if":"eno1","phone_ip":"10.0.0.5","tun":"phone0","mtu":1400}"#
    );
    assert_eq!(
        serde_json::to_string(&Request::Hello { version: 1 }).unwrap(),
        r#"{"type":"hello","version":1}"#
    );
    let error = Reply::Error {
        code: ErrorCode::StartFailed,
        message: "m".into(),
    };
    assert_eq!(
        serde_json::to_string(&error).unwrap(),
        r#"{"type":"error","code":"start_failed","message":"m"}"#
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
            ineligible: Some("not in the policy".into()),
        }],
    };
    assert_eq!(
        serde_json::to_string(&interfaces).unwrap(),
        concat!(
            r#"{"type":"interfaces","interfaces":[{"name":"eno1","up":true,"#,
            r#""addresses":[{"address":"10.0.0.2","prefix":24}],"default_route":true,"#,
            r#""phone_addresses":[],"ineligible":"not in the policy"}]}"#
        )
    );
}

#[test]
fn unknown_fields_and_bad_names_are_refused() {
    // A typo'd field is a precise error, not a silently ignored one.
    let typo = r#"{"type":"start","lan_if":"eno1","phoneip":"10.0.0.5","tun":"phone0","mtu":1400}"#;
    let error = serde_json::from_str::<Request>(typo)
        .unwrap_err()
        .to_string();
    assert!(error.contains("phoneip"), "{error}");
    let bad_tun =
        r#"{"type":"start","lan_if":"eno1","phone_ip":"10.0.0.5","tun":"../x","mtu":1400}"#;
    assert!(serde_json::from_str::<Request>(bad_tun).is_err());
}
