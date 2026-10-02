//! State folding: events land on the right row, ends stay visible, the log
//! stays bounded and scrolls.

use routedroid_ipc::{
    ConnectionInfo, ConnectionState, DeviceInfo, EndReason, Event, Kind, Outcome, Traffic,
};

use super::log::LOG_CAPACITY;
use super::{App, DaemonLink, Level, Mode};
use crate::messages::{Command, Incoming};

fn device(serial: &str, connection: Option<ConnectionState>) -> DeviceInfo {
    DeviceInfo {
        serial: serial.into(),
        state: "device".into(),
        model: None,
        unusable_reason: None,
        connection,
    }
}

fn connection(serial: &str) -> ConnectionInfo {
    ConnectionInfo {
        serial: serial.into(),
        lan_if: "eth0".into(),
        tun: "phone0".into(),
        mtu: 1400,
        state: ConnectionState::Active,
        started_at: 0,
        network: None,
        traffic: Traffic::default(),
    }
}

fn app_with_two_devices() -> App {
    let mut app = App::new();
    app.apply(Incoming::Devices(vec![
        device("one", Some(ConnectionState::Active)),
        device("two", None),
    ]));
    app.apply(Incoming::Connections(vec![connection("one")]));
    app
}

fn ended(serial: &str, outcome: Outcome) -> Incoming {
    let state = ConnectionState::Ended { outcome };
    Incoming::Event(Event::Connection {
        serial: serial.into(),
        state,
    })
}

#[test]
fn traffic_updates_the_right_connection() {
    let mut app = app_with_two_devices();
    let traffic = Traffic {
        packets_to_phone: 7,
        bytes_from_phone: 3,
        ..Traffic::default()
    };
    let event = Event::Traffic {
        serial: "one".into(),
        traffic,
    };
    assert!(app.apply(Incoming::Event(event)).is_empty());
    assert_eq!(app.connections["one"].traffic, traffic);
    assert!(!app.connections.contains_key("two"));
}

#[test]
fn a_failed_end_stays_on_the_row_until_the_next_start() {
    let mut app = app_with_two_devices();
    app.apply(ended("one", Outcome::failed(Kind::Vpn, "phone refused")));
    assert_eq!(app.devices[0].connection, None);
    assert!(app.connections.is_empty());
    let end = &app.last_end["one"];
    assert!(end.failed && end.text.contains("phone refused"), "{end:?}");
    assert_eq!(app.log.last().unwrap().level, Level::Error);
    app.apply(Incoming::Started {
        serial: "one".into(),
        tun: "phone0".into(),
    });
    assert!(!app.last_end.contains_key("one"));
}

#[test]
fn a_clean_end_is_kept_but_not_red() {
    let mut app = app_with_two_devices();
    app.apply(ended(
        "one",
        Outcome::Clean {
            reason: EndReason::PhoneStopped,
        },
    ));
    let end = &app.last_end["one"];
    assert!(!end.failed);
    assert_eq!(end.text, "stopped on the phone");
}

#[test]
fn a_refused_start_is_kept_on_its_row() {
    let mut app = app_with_two_devices();
    let message = "phone_ip is required".to_string();
    app.apply(Incoming::Failed {
        what: "start",
        serial: Some("two".into()),
        message,
    });
    assert!(app.last_end["two"].text.contains("phone_ip is required"));
    assert!(
        app.log
            .last()
            .unwrap()
            .text
            .starts_with("two: start failed")
    );
}

#[test]
fn unknown_connection_asks_for_status() {
    let mut app = app_with_two_devices();
    let state = ConnectionState::Starting;
    let followups = app.apply(Incoming::Event(Event::Connection {
        serial: "two".into(),
        state,
    }));
    assert!(matches!(followups.as_slice(), [Command::RefreshStatus]));
    assert_eq!(app.devices[1].connection, Some(ConnectionState::Starting));
}

#[test]
fn cursor_stays_in_range_when_devices_vanish() {
    let mut app = app_with_two_devices();
    app.move_cursor(5);
    assert_eq!(app.cursor, 1);
    app.apply(Incoming::Event(Event::Devices {
        devices: vec![device("one", None)],
    }));
    assert_eq!(app.cursor, 0);
    app.apply(Incoming::Event(Event::Devices { devices: vec![] }));
    assert!(app.selected_device().is_none());
}

#[test]
fn connecting_refreshes_once_from_one_place() {
    let mut app = App::new();
    assert_eq!(app.daemon, DaemonLink::Connecting);
    app.apply(Incoming::Disconnected {
        reason: "closed".into(),
    });
    assert_eq!(
        app.daemon,
        DaemonLink::Disconnected {
            reason: "closed".into()
        }
    );
    let followups = app.apply(Incoming::Connected);
    assert!(matches!(
        followups.as_slice(),
        [
            Command::RefreshDevices,
            Command::RefreshStatus,
            Command::RefreshInterfaces
        ]
    ));
    assert_eq!(app.daemon, DaemonLink::Connected);
}

#[test]
fn a_draft_survives_closing_the_form() {
    let mut app = app_with_two_devices();
    assert!(matches!(
        app.open_form("two".into()).as_slice(),
        [Command::RefreshInterfaces]
    ));
    let Mode::StartForm(mut form) = std::mem::replace(&mut app.mode, Mode::Normal) else {
        panic!("form open");
    };
    form.phone_ip.insert("10.0.0.9");
    app.close_form(form);
    app.open_form("two".into());
    let Mode::StartForm(form) = &app.mode else {
        panic!("form open")
    };
    assert_eq!(form.phone_ip.value(), "10.0.0.9");
}

#[test]
fn the_log_is_bounded_timestamped_and_scrolls() {
    let mut app = App::new();
    for index in 0..(LOG_CAPACITY + 5) {
        let message = format!("boom {index}");
        app.apply(Incoming::Failed {
            what: "status",
            serial: None,
            message,
        });
    }
    assert_eq!(app.log.len(), LOG_CAPACITY);
    assert_eq!(app.log.first().unwrap().text, "status failed: boom 5");
    assert_eq!(app.log.last().unwrap().time.len(), "12:34:56".len());
    app.log.scroll_back(3);
    assert_eq!(
        app.log.in_view().last().unwrap().text,
        format!("status failed: boom {}", LOG_CAPACITY + 1)
    );
    app.info("new");
    assert_eq!(
        app.log.scroll(),
        4,
        "the view holds still while reading back"
    );
    app.log.scroll_forward(usize::MAX);
    assert_eq!(app.log.in_view().last().unwrap().text, "new");
}
