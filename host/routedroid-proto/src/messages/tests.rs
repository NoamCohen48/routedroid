use super::*;
use crate::fixtures::{unhex, BODIES, FRAMES};
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
    assert_eq!(e.known_code(), Some(ErrorCode::ProtocolUnsupported));
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
    let mk = |session: &str, port: u16| Hello {
        protocol: 1,
        session: session.into(),
        device_port: port,
        client_nonce: [0xaa; 32],
        app: None,
    };
    assert!(mk("ok.session_1-", 9000).validate().is_ok());
    assert!(mk("", 9000).validate().is_err());
    assert!(mk(&"s".repeat(41), 9000).validate().is_err());
    assert!(mk("bad space", 9000).validate().is_err());
    assert!(mk("s", 0).validate().is_err());
    let hello = |nonce: &str| format!(r#"{{"protocol":1,"session":"s","device_port":1,"client_nonce":"{nonce}"}}"#);
    assert!(parse::<Hello>(hello(&"a".repeat(64)).as_bytes()).is_ok());
    assert!(parse::<Hello>(hello(&"A".repeat(64)).as_bytes()).is_err(), "uppercase hex");
    assert!(parse::<Hello>(hello(&"a".repeat(63)).as_bytes()).is_err());
    assert!(parse::<Hello>(hello(&"a".repeat(66)).as_bytes()).is_err());

    let (_, b) = body("configure_vpn");
    let mut cfg: ConfigureVpn = parse(&b).unwrap();
    cfg.addresses.push(Prefix::new(Ipv4Addr::new(10, 0, 0, 9), 32));
    assert!(cfg.validate().is_err(), "two addresses");
    cfg.addresses.pop();
    cfg.routes.clear();
    assert!(cfg.validate().is_err(), "no routes");
    cfg.routes.push(Prefix::new(Ipv4Addr::UNSPECIFIED, 33));
    assert!(cfg.validate().is_err(), "prefix 33");
    cfg.routes[0].prefix = 0;
    cfg.mtu = 575;
    assert!(cfg.validate().is_err(), "mtu below 576");
    cfg.mtu = 65_535;
    assert!(cfg.validate().is_ok());

    let ack = HelloAck { protocol: 2, mtu: 1400, host_nonce: [1; 32], host_proof: [2; 32] };
    assert!(ack.validate().is_err(), "wrong protocol in ack");

    let ready = |prefix| VpnReady { addresses: vec![Prefix::new(Ipv4Addr::new(10, 0, 0, 1), prefix)], mtu: 1400 };
    assert!(ready(32).validate().is_ok());
    assert!(ready(33).validate().is_err());
    assert!(VpnReady { addresses: vec![], mtu: 1400 }.validate().is_err());
}

/// The cases where JSON libraries disagree (bodies.json); the app must agree
/// on every one, so both sides test the same bytes.
#[test]
fn body_fixtures() {
    let f: serde_json::Value = serde_json::from_str(BODIES).unwrap();
    let mtu = f["mtu"].as_u64().unwrap() as u32;
    let accept = |kind: &str, b: &[u8]| -> Result<(), BodyError> {
        match kind {
            "configure_vpn" => {
                let c: ConfigureVpn = parse(b)?;
                if c.mtu != mtu {
                    return Err(field("mtu", "must equal the negotiated mtu"));
                }
                Ok(())
            }
            "error" => parse::<ErrorBody>(b).map(drop),
            "hello_ack" => parse::<HelloAck>(b).map(drop),
            "vpn_ready" => {
                let r: VpnReady = parse(b)?;
                if r.mtu != mtu {
                    return Err(field("mtu", "must equal the negotiated mtu"));
                }
                Ok(())
            }
            other => panic!("unknown kind {other}"),
        }
    };
    for v in f["valid"].as_array().unwrap() {
        let name = v["name"].as_str().unwrap();
        let b = unhex(v["body_hex"].as_str().unwrap());
        accept(v["kind"].as_str().unwrap(), &b).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    for v in f["invalid"].as_array().unwrap() {
        let name = v["name"].as_str().unwrap();
        let b = unhex(v["body_hex"].as_str().unwrap());
        assert!(accept(v["kind"].as_str().unwrap(), &b).is_err(), "{name}: accepted");
    }
}

#[test]
fn prefix_canonical_and_unicast() {
    assert!(Prefix::new(Ipv4Addr::UNSPECIFIED, 0).is_canonical());
    assert!(Prefix::new(Ipv4Addr::new(10, 0, 0, 0), 8).is_canonical());
    assert!(!Prefix::new(Ipv4Addr::new(10, 0, 0, 1), 8).is_canonical());
    assert!(Prefix::new(Ipv4Addr::new(10, 0, 0, 1), 32).is_canonical());
    assert!(!Prefix::new(Ipv4Addr::new(10, 0, 0, 1), 33).is_canonical());
    for bad in ["0.0.0.0", "0.1.2.3", "127.0.0.1", "224.0.0.1", "240.0.0.1", "255.255.255.255"] {
        assert!(!vpn::is_unicast_host(bad.parse().unwrap()), "{bad}");
    }
    assert!(vpn::is_unicast_host(Ipv4Addr::new(10, 0, 0, 1)));
}
