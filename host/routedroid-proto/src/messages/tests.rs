use super::*;
use crate::fixtures::{unhex, FRAMES};
use crate::frame::MessageType;

fn body(name: &str) -> (MessageType, Vec<u8>) {
    let f: serde_json::Value = serde_json::from_str(FRAMES).unwrap();
    let v = f["valid"].as_array().unwrap().iter().find(|v| v["name"] == name).unwrap();
    (MessageType::from_u8(v["type"].as_u64().unwrap() as u8).unwrap(), unhex(v["body_hex"].as_str().unwrap()))
}

/// Every JSON fixture parses, validates, and re-serializes to the exact
/// fixture bytes (field order and no extra whitespace).
#[test]
fn fixtures_round_trip_byte_exact() {
    fn rt<T: Body + std::fmt::Debug>(name: &str) {
        let (_, b) = body(name);
        let v: T = parse(&b).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(serde_json::to_vec(&v).unwrap(), b, "{name}");
    }
    rt::<Hello>("hello");
    rt::<Hello>("hello_minimal");
    rt::<HelloAck>("hello_ack");
    rt::<Auth>("auth");
    rt::<ConfigureVpn>("configure_vpn");
    rt::<VpnReady>("vpn_ready");
    rt::<ErrorBody>("vpn_error");
    rt::<ErrorBody>("error_auth_failed");
    rt::<ErrorBody>("error_protocol_unsupported");
}

#[test]
fn error_codes_are_known() {
    let (_, b) = body("error_protocol_unsupported");
    let e: ErrorBody = parse(&b).unwrap();
    assert_eq!(e.code(), Some(ErrorCode::ProtocolUnsupported));
    assert_eq!(e.supported, Some(vec![1]));
    for c in ErrorCode::ALL {
        assert_eq!(ErrorCode::parse(c.as_str()), Some(c));
    }
}

#[test]
fn unknown_fields_are_ignored_and_missing_ones_rejected() {
    let ok: Hello = parse(
        br#"{"protocol":1,"session":"s","device_port":1,"client_nonce":"00000000000000000000000000000000000000000000000000000000000000ff","future":true}"#,
    )
    .unwrap();
    assert_eq!(ok.session, "s");
    assert!(matches!(parse::<Hello>(br#"{"protocol":1,"session":"s"}"#), Err(BodyError::Json(_))));
    assert!(matches!(parse::<Hello>(b"not json"), Err(BodyError::Json(_))));
}

#[test]
fn field_rules() {
    let nonce = "a".repeat(64);
    let mk = |session: &str, port: u16, nonce: &str| Hello {
        protocol: 1,
        session: session.into(),
        device_port: port,
        client_nonce: nonce.into(),
        app: None,
    };
    assert!(mk("ok.session_1-", 9000, &nonce).validate().is_ok());
    assert!(mk("", 9000, &nonce).validate().is_err());
    assert!(mk(&"s".repeat(41), 9000, &nonce).validate().is_err());
    assert!(mk("bad space", 9000, &nonce).validate().is_err());
    assert!(mk("s", 0, &nonce).validate().is_err());
    assert!(mk("s", 1, &"A".repeat(64)).validate().is_err(), "uppercase hex");
    assert!(mk("s", 1, &"a".repeat(63)).validate().is_err());

    let (_, b) = body("configure_vpn");
    let mut cfg: ConfigureVpn = parse(&b).unwrap();
    cfg.addresses.push(Prefix::new(Ipv4Addr::new(10, 0, 0, 9), 32));
    assert!(cfg.validate().is_err(), "two addresses");
    cfg.addresses.pop();
    cfg.routes.clear();
    assert!(cfg.validate().is_err(), "no routes");
    cfg.routes.push(Prefix { address: "0.0.0.0".into(), prefix: 33 });
    assert!(cfg.validate().is_err(), "prefix 33");
    cfg.routes[0].prefix = 0;
    cfg.mtu = 575;
    assert!(cfg.validate().is_err(), "mtu below 576");
    cfg.mtu = 65_535;
    assert!(cfg.validate().is_ok());

    let ack = HelloAck { protocol: 2, mtu: 1400, host_nonce: nonce.clone(), host_proof: nonce.clone() };
    assert!(ack.validate().is_err(), "wrong protocol in ack");

    let ready = VpnReady { addresses: vec!["10.0.0.1/33".into()], mtu: 1400 };
    assert!(ready.validate().is_err());
    let ready = VpnReady { addresses: vec!["10.0.0.1".into()], mtu: 1400 };
    assert!(ready.validate().is_err());
}
