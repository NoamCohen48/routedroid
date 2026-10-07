//! The wire's JSON, pinned: every request, response and event serializes to
//! exactly these lines and parses back to the same value. A client written
//! in another language can be checked against this file.

use std::net::Ipv4Addr;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::*;

#[track_caller]
fn pinned<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: T, json: &str) {
    assert_eq!(serde_json::to_string(&value).unwrap(), json);
    assert_eq!(serde_json::from_str::<T>(json).unwrap(), value);
}

fn ask(id: u64, request: Request) -> ClientMessage {
    ClientMessage { id, request }
}

fn answer(response: Response) -> ServerMessage {
    ServerMessage::Response { id: 7, response }
}

fn push(event: Event) -> ServerMessage {
    ServerMessage::Event { event }
}

const IP: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 50);
const HOST: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 10);

fn network() -> NetworkInfo {
    let lease = Some(Lease {
        server: Ipv4Addr::new(192, 168, 1, 1),
        expires_at: 1_800_000_000,
    });
    NetworkInfo {
        phone_ip: IP,
        host_ip: HOST,
        lan_prefix: 24,
        dns: vec![HOST],
        lease,
    }
}

#[test]
fn requests() {
    pinned(ask(1, Request::Version), r#"{"id":1,"type":"version"}"#);
    pinned(ask(2, Request::Devices), r#"{"id":2,"type":"devices"}"#);
    pinned(
        ask(3, Request::Interfaces),
        r#"{"id":3,"type":"interfaces"}"#,
    );
    pinned(ask(4, Request::Status), r#"{"id":4,"type":"status"}"#);
    pinned(ask(5, Request::Subscribe), r#"{"id":5,"type":"subscribe"}"#);
    pinned(
        ask(
            6,
            Request::Stop {
                serial: "R58M".into(),
            },
        ),
        r#"{"id":6,"type":"stop","serial":"R58M"}"#,
    );
}

#[test]
fn start_requests() {
    let minimal = StartRequest {
        serial: "R58M".into(),
        lan_if: "eno1".into(),
        phone_ip: None,
        tun: None,
        mtu: None,
        dns: DnsChoice::Auto,
        connect_timeout_secs: None,
        reconnect_secs: None,
        allow_network_adb: false,
    };
    pinned(
        ask(1, Request::Start(minimal.clone())),
        r#"{"id":1,"type":"start","serial":"R58M","lan_if":"eno1","dns":"auto","allow_network_adb":false}"#,
    );
    let full = StartRequest {
        phone_ip: Some(IP),
        tun: Some("phone3".into()),
        mtu: Some(1400),
        dns: DnsChoice::Servers(vec![Ipv4Addr::new(9, 9, 9, 9)]),
        connect_timeout_secs: Some(30),
        reconnect_secs: Some(0),
        allow_network_adb: true,
        ..minimal.clone()
    };
    pinned(
        ask(2, Request::Start(full)),
        concat!(
            r#"{"id":2,"type":"start","serial":"R58M","lan_if":"eno1","phone_ip":"192.168.1.50","#,
            r#""tun":"phone3","mtu":1400,"dns":{"servers":["9.9.9.9"]},"connect_timeout_secs":30,"#,
            r#""reconnect_secs":0,"allow_network_adb":true}"#
        ),
    );
    let no_dns = Request::Start(StartRequest {
        dns: DnsChoice::None,
        ..minimal
    });
    assert!(
        serde_json::to_string(&no_dns)
            .unwrap()
            .contains(r#""dns":"none""#)
    );
    let sparse: ClientMessage =
        serde_json::from_str(r#"{"id":3,"type":"start","serial":"s","lan_if":"eno1"}"#).unwrap();
    assert!(matches!(
        sparse.request,
        Request::Start(StartRequest {
            dns: DnsChoice::Auto,
            ..
        })
    ));
}

#[test]
fn simple_responses() {
    pinned(
        answer(Response::Version {
            daemon: "0.1.0".into(),
            api: 3,
        }),
        r#"{"msg":"response","id":7,"type":"version","daemon":"0.1.0","api":3}"#,
    );
    pinned(
        answer(Response::Subscribed),
        r#"{"msg":"response","id":7,"type":"subscribed"}"#,
    );
    pinned(
        answer(Response::Started {
            serial: "R58M".into(),
            tun: "phone0".into(),
        }),
        r#"{"msg":"response","id":7,"type":"started","serial":"R58M","tun":"phone0"}"#,
    );
    pinned(
        answer(Response::Error {
            kind: Kind::Timeout,
            message: "slow".into(),
        }),
        r#"{"msg":"response","id":7,"type":"error","kind":"timeout","message":"slow"}"#,
    );
}

#[test]
fn outcomes() {
    let clean = Outcome::Clean {
        reason: EndReason::PhoneStopped,
    };
    pinned(
        answer(Response::Stopped {
            serial: "R58M".into(),
            outcome: clean,
        }),
        concat!(
            r#"{"msg":"response","id":7,"type":"stopped","serial":"R58M","#,
            r#""outcome":{"result":"clean","reason":"phone_stopped"}}"#
        ),
    );
    pinned(
        ConnectionState::InstallingApp,
        r#"{"state":"installing_app"}"#,
    );
    pinned(
        ConnectionState::Reconnecting { wait_secs: 120 },
        r#"{"state":"reconnecting","wait_secs":120}"#,
    );
    let failed = ConnectionState::Ended {
        outcome: Outcome::failed(Kind::Vpn, "denied"),
    };
    pinned(
        failed,
        r#"{"state":"ended","outcome":{"result":"failed","kind":"vpn","message":"denied"}}"#,
    );
    for (reason, word) in [
        (EndReason::StoppedEarly, "stopped_early"),
        (EndReason::Stopped, "stopped"),
        (EndReason::PhoneStopped, "phone_stopped"),
        (EndReason::PhoneClosed, "phone_closed"),
    ] {
        pinned(reason, &format!("\"{word}\""));
    }
    for kind in Kind::ALL {
        pinned(kind, &format!("\"{}\"", kind.as_str()));
    }
}

#[test]
fn lists() {
    let device = DeviceInfo {
        serial: "R58M".into(),
        state: "device".into(),
        model: Some("SM_J810G".into()),
        unusable_reason: None,
        connection: Some(ConnectionState::WaitingForApp),
    };
    pinned(
        answer(Response::Devices {
            devices: vec![device],
        }),
        concat!(
            r#"{"msg":"response","id":7,"type":"devices","devices":[{"serial":"R58M","#,
            r#""state":"device","model":"SM_J810G","unusable_reason":null,"#,
            r#""connection":{"state":"waiting_for_app"}}]}"#
        ),
    );
    let interface = InterfaceInfo {
        name: "eno1".into(),
        up: true,
        addresses: vec![Ipv4Net {
            address: HOST,
            prefix: 24,
        }],
        default_route: true,
        phone_addresses: vec![Ipv4Net {
            address: Ipv4Addr::new(192, 168, 1, 200),
            prefix: 29,
        }],
        dhcp: true,
        ineligible: None,
    };
    pinned(
        answer(Response::Interfaces {
            interfaces: vec![interface],
        }),
        concat!(
            r#"{"msg":"response","id":7,"type":"interfaces","interfaces":[{"name":"eno1","up":true,"#,
            r#""addresses":[{"address":"192.168.1.10","prefix":24}],"default_route":true,"#,
            r#""phone_addresses":[{"address":"192.168.1.200","prefix":29}],"dhcp":true,"ineligible":null}]}"#
        ),
    );
}

#[test]
fn status() {
    let connection = ConnectionInfo {
        serial: "R58M".into(),
        lan_if: "eno1".into(),
        tun: "phone0".into(),
        mtu: 1400,
        state: ConnectionState::Active,
        started_at: 1_700_000_000,
        network: Some(network()),
        traffic: Traffic {
            packets_to_phone: 1,
            bytes_to_phone: 60,
            ..Traffic::default()
        },
    };
    pinned(
        answer(Response::Status {
            connections: vec![connection],
        }),
        concat!(
            r#"{"msg":"response","id":7,"type":"status","connections":[{"serial":"R58M","#,
            r#""lan_if":"eno1","tun":"phone0","mtu":1400,"state":{"state":"active"},"#,
            r#""started_at":1700000000,"network":{"phone_ip":"192.168.1.50","#,
            r#""host_ip":"192.168.1.10","lan_prefix":24,"dns":["192.168.1.10"],"#,
            r#""lease":{"server":"192.168.1.1","expires_at":1800000000}},"#,
            r#""traffic":{"packets_to_phone":1,"packets_from_phone":0,"bytes_to_phone":60,"#,
            r#""bytes_from_phone":0,"dropped_malformed":0,"dropped_congested":0}}]}"#
        ),
    );
}

#[test]
fn events() {
    let serial = || "R58M".to_string();
    pinned(
        push(Event::Connection {
            serial: serial(),
            state: ConnectionState::Handshaking,
        }),
        r#"{"msg":"event","event":"connection","serial":"R58M","state":{"state":"handshaking"}}"#,
    );
    let static_network = NetworkInfo {
        lease: None,
        ..network()
    };
    pinned(
        push(Event::Network {
            serial: serial(),
            network: static_network,
        }),
        concat!(
            r#"{"msg":"event","event":"network","serial":"R58M","network":{"#,
            r#""phone_ip":"192.168.1.50","host_ip":"192.168.1.10","lan_prefix":24,"#,
            r#""dns":["192.168.1.10"],"lease":null}}"#
        ),
    );
    let traffic = Traffic {
        dropped_congested: 2,
        ..Traffic::default()
    };
    let line = serde_json::to_string(&push(Event::Traffic {
        serial: serial(),
        traffic,
    }))
    .unwrap();
    assert!(line.starts_with(r#"{"msg":"event","event":"traffic","serial":"R58M","traffic":{"#));
    pinned(
        push(Event::Devices { devices: vec![] }),
        r#"{"msg":"event","event":"devices","devices":[]}"#,
    );
    pinned(
        push(Event::Shutdown),
        r#"{"msg":"event","event":"shutdown"}"#,
    );
    pinned(
        push(Event::Lagged { missed: 3 }),
        r#"{"msg":"event","event":"lagged","missed":3}"#,
    );
}

#[test]
fn a_line_without_its_tag_is_refused() {
    assert!(serde_json::from_str::<ServerMessage>(r#"{"id":1,"type":"subscribed"}"#).is_err());
    assert!(serde_json::from_str::<ServerMessage>(r#"{"event":"shutdown"}"#).is_err());
}

#[test]
fn a_start_with_an_unknown_field_is_refused() {
    let line = r#"{"id":1,"type":"start","serial":"s","lan_if":"eno1","phone":"1.2.3.4"}"#;
    assert!(serde_json::from_str::<ClientMessage>(line).is_err());
}

#[test]
fn states_read_the_same_everywhere() {
    let ended = ConnectionState::Ended {
        outcome: Outcome::failed(Kind::Vpn, "denied"),
    };
    assert_eq!(ended.to_string(), "ended: failed (vpn): denied");
    let clean = ConnectionState::Ended {
        outcome: Outcome::Clean {
            reason: EndReason::Stopped,
        },
    };
    assert_eq!(clean.to_string(), "ended: stopped");
    assert_eq!(
        ConnectionState::WaitingForApp.to_string(),
        "waiting for app"
    );
    assert!(format!("{:#}", ConnectionState::Handshaking).contains("VPN permission"));
    assert_eq!(ConnectionState::InstallingApp.to_string(), "installing app");
    let away = ConnectionState::Reconnecting { wait_secs: 120 };
    assert_eq!(away.to_string(), "reconnecting");
    assert!(format!("{away:#}").contains("held for up to 120 s"));
}
