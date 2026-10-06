use super::*;

fn request() -> StartRequest {
    StartRequest {
        serial: "R58M".into(),
        lan_if: "eno1".into(),
        phone_ip: Some(Ipv4Addr::new(192, 168, 1, 50)),
        tun: None,
        mtu: None,
        dns: DnsChoice::Auto,
        connect_timeout_secs: None,
        reconnect_secs: None,
        allow_network_adb: false,
    }
}

#[track_caller]
fn refused(request: StartRequest, needle: &str) -> Kind {
    let fault = StartSpec::parse(request).unwrap_err();
    assert!(
        fault.to_string().contains(needle),
        "{fault} lacks {needle:?}"
    );
    fault.kind()
}

#[test]
fn defaults_are_the_daemons() {
    let spec = StartSpec::parse(request()).unwrap();
    assert_eq!(spec.mtu, DEFAULT_MTU);
    assert_eq!(spec.connect_timeout, DEFAULT_CONNECT_TIMEOUT);
    assert_eq!(spec.reconnect_wait, DEFAULT_RECONNECT_WAIT);
    assert_eq!(spec.tun, None);
    assert_eq!(spec.dns, DnsChoice::Auto);
}

#[test]
fn bad_names_are_usage_errors() {
    let long = StartRequest {
        lan_if: "a-name-longer-than-15".into(),
        ..request()
    };
    assert_eq!(refused(long, "lan_if"), Kind::Usage);
    refused(
        StartRequest {
            lan_if: "phone0".into(),
            ..request()
        },
        "not a LAN interface",
    );
    refused(
        StartRequest {
            tun: Some("eth9".into()),
            ..request()
        },
        "must start with",
    );
    refused(
        StartRequest {
            tun: Some("phone/0".into()),
            ..request()
        },
        "tun",
    );
}

#[test]
fn addresses_must_be_unicast_hosts() {
    for bad in ["0.0.0.0", "127.0.0.1", "224.0.0.1", "255.255.255.255"] {
        let request = StartRequest {
            phone_ip: Some(bad.parse().unwrap()),
            ..request()
        };
        refused(request, "unicast");
    }
    let dns = DnsChoice::Servers(vec!["224.0.0.251".parse().unwrap()]);
    refused(StartRequest { dns, ..request() }, "DNS server");
    refused(
        StartRequest {
            dns: DnsChoice::Servers(vec![]),
            ..request()
        },
        "empty",
    );
}

#[test]
fn mtu_and_timeout_are_bounded() {
    refused(
        StartRequest {
            mtu: Some(70_000),
            ..request()
        },
        "mtu",
    );
    refused(
        StartRequest {
            mtu: Some(575),
            ..request()
        },
        "mtu",
    );
    refused(
        StartRequest {
            connect_timeout_secs: Some(0),
            ..request()
        },
        "timeout",
    );
    let long = StartRequest {
        connect_timeout_secs: Some(u64::MAX),
        ..request()
    };
    assert_eq!(StartSpec::parse(long).unwrap().connect_timeout, MAX_WAIT);
    let wait = |secs| {
        let request = StartRequest {
            reconnect_secs: Some(secs),
            ..request()
        };
        StartSpec::parse(request).unwrap().reconnect_wait
    };
    // Zero is allowed for this one: it means "do not wait".
    assert_eq!(wait(0), Duration::ZERO);
    assert_eq!(wait(u64::MAX), MAX_WAIT);
}

#[test]
fn transport_rule_applies_before_anything_else() {
    let network = StartRequest {
        serial: "10.0.0.2:5555".into(),
        ..request()
    };
    assert_eq!(refused(network, "network ADB"), Kind::Transport);
}
