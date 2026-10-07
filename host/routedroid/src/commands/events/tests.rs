use routedroid_ipc::{ConnectionState, DeviceInfo, EndReason, NetworkInfo, Outcome, Traffic};

use super::*;

fn phone(serial: &str, state: &str) -> DeviceInfo {
    DeviceInfo {
        serial: serial.into(),
        state: state.into(),
        model: None,
        unusable_reason: None,
        connection: None,
    }
}

#[test]
fn connections_read_as_sentences() {
    let ended = Event::Connection {
        serial: "R58".into(),
        state: ConnectionState::Ended {
            outcome: Outcome::Clean {
                reason: EndReason::Stopped,
            },
        },
    };
    assert_eq!(line(&ended).unwrap(), "R58: ended: stopped");
    let network = NetworkInfo {
        phone_ip: [10, 0, 0, 7].into(),
        host_ip: [10, 0, 0, 2].into(),
        lan_prefix: 24,
        dns: vec![],
        lease: None,
    };
    let placed = Event::Network {
        serial: "R58".into(),
        network,
    };
    assert_eq!(
        line(&placed).unwrap(),
        "R58: phone is 10.0.0.7 on the LAN (host 10.0.0.2/24), no DNS"
    );
}

#[test]
fn phones_are_listed_with_any_state_but_ready() {
    let devices = Event::Devices {
        devices: vec![phone("R58", "device"), phone("ZX1", "unauthorized")],
    };
    assert_eq!(
        line(&devices).unwrap(),
        "phones attached: R58, ZX1 (unauthorized)"
    );
    let none = Event::Devices { devices: vec![] };
    assert_eq!(line(&none).unwrap(), "phones attached: none");
}

#[test]
fn traffic_is_left_to_status() {
    let traffic = Event::Traffic {
        serial: "R58".into(),
        traffic: Traffic::default(),
    };
    assert_eq!(line(&traffic), None);
}
