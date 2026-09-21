//! State folding: events land on the right row and the log stays bounded.

use routedroid_ipc::{DeviceInfo, Event, Outcome, SessionInfo, SessionState};

use super::{App, Connection, Level, LOG_CAPACITY};
use crate::messages::{Command, Incoming};

fn device(serial: &str, session: Option<SessionState>) -> DeviceInfo {
    DeviceInfo { serial: serial.into(), state: "device".into(), model: None, unusable_reason: None, session }
}

fn session(serial: &str) -> SessionInfo {
    SessionInfo {
        serial: serial.into(),
        lan_if: "eth0".into(),
        phone_ip: "10.0.0.5".parse().unwrap(),
        tun: "phone0".into(),
        state: SessionState::Active,
        started_at: 0,
        packets_to_phone: 0,
        packets_from_phone: 0,
    }
}

fn app_with_two_devices() -> App {
    let mut app = App::new();
    app.apply(Incoming::Devices(vec![device("one", Some(SessionState::Active)), device("two", None)]));
    app.apply(Incoming::Sessions(vec![session("one")]));
    app
}

#[test]
fn traffic_updates_the_right_session() {
    let mut app = app_with_two_devices();
    let event = Event::Traffic { serial: "one".into(), packets_to_phone: 7, packets_from_phone: 3 };
    assert!(app.apply(Incoming::Event(event)).is_empty());
    assert_eq!(app.sessions["one"].packets_to_phone, 7);
    assert_eq!(app.sessions["one"].packets_from_phone, 3);
    assert!(!app.sessions.contains_key("two"));
}

#[test]
fn ended_session_clears_the_row_and_details() {
    let mut app = app_with_two_devices();
    let outcome = Outcome { ok: false, kind: Some(routedroid_ipc::Kind::Vpn), message: "phone refused".into() };
    app.apply(Incoming::Event(Event::Session { serial: "one".into(), state: SessionState::Ended(outcome) }));
    assert_eq!(app.devices[0].session, None);
    assert!(app.sessions.is_empty());
    let last = app.log.back().unwrap();
    assert_eq!(last.level, Level::Error);
    assert!(last.text.contains("phone refused"));
}

#[test]
fn unknown_session_asks_for_status() {
    let mut app = app_with_two_devices();
    let followups = app.apply(Incoming::Event(Event::Session { serial: "two".into(), state: SessionState::Starting }));
    assert!(matches!(followups.as_slice(), [Command::RefreshStatus]));
    assert_eq!(app.devices[1].session, Some(SessionState::Starting));
}

#[test]
fn known_session_changes_state_in_place() {
    let mut app = app_with_two_devices();
    let followups = app.apply(Incoming::Event(Event::Session { serial: "one".into(), state: SessionState::Stopping }));
    assert!(followups.is_empty());
    assert_eq!(app.sessions["one"].state, SessionState::Stopping);
}

#[test]
fn cursor_stays_in_range_when_devices_vanish() {
    let mut app = app_with_two_devices();
    app.move_cursor(5);
    assert_eq!(app.cursor, 1);
    app.apply(Incoming::Event(Event::Devices { devices: vec![device("one", None)] }));
    assert_eq!(app.cursor, 0);
    app.apply(Incoming::Event(Event::Devices { devices: vec![] }));
    assert_eq!(app.cursor, 0);
    assert!(app.selected_device().is_none());
}

#[test]
fn connection_changes_are_tracked_and_reconnect_refreshes() {
    let mut app = App::new();
    app.apply(Incoming::Disconnected { reason: "closed".into() });
    assert_eq!(app.connection, Connection::Disconnected { reason: "closed".into() });
    let followups = app.apply(Incoming::Connected);
    assert!(matches!(followups.as_slice(), [Command::RefreshDevices, Command::RefreshStatus]));
    assert_eq!(app.connection, Connection::Connected);
}

#[test]
fn failures_are_red_and_the_log_is_bounded() {
    let mut app = App::new();
    for index in 0..(LOG_CAPACITY + 5) {
        app.apply(Incoming::Failed { what: "start".into(), message: format!("boom {index}") });
    }
    assert_eq!(app.log.len(), LOG_CAPACITY);
    assert_eq!(app.log.front().unwrap().text, "start failed: boom 5");
    assert!(app.log.iter().all(|line| line.level == Level::Error));
}
