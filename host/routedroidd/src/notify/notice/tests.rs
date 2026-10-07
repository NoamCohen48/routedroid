use routedroid_ipc::{ConnectionState, EndReason, Event, Kind, NetworkInfo, Outcome, Screen};

use super::Notices;

const PHONE: &str = "R58M";

fn state(state: ConnectionState) -> Event {
    Event::Connection {
        serial: PHONE.into(),
        state,
    }
}

fn said(notices: &mut Notices, event: Event) -> Option<String> {
    let notice = notices.on(&event, |serial| format!("pixel ({serial})"))?;
    Some(format!("{} | {}", notice.summary, notice.body))
}

#[test]
fn a_connection_is_told_at_the_moments_that_matter() {
    let mut notices = Notices::default();
    assert_eq!(said(&mut notices, state(ConnectionState::Starting)), None);
    let network = NetworkInfo {
        phone_ip: [192, 168, 1, 201].into(),
        host_ip: [192, 168, 1, 10].into(),
        lan_prefix: 24,
        dns: vec![],
        lease: None,
    };
    let placed = Event::Network {
        serial: PHONE.into(),
        network,
    };
    assert_eq!(said(&mut notices, placed), None);
    let locked = |screen| state(ConnectionState::WaitingForApp { screen });
    assert_eq!(
        said(&mut notices, locked(Some(Screen::Locked))).as_deref(),
        Some("Unlock pixel (R58M) | It is locked: unlock it to connect.")
    );
    assert_eq!(said(&mut notices, locked(None)), None);
    let consent = ConnectionState::Handshaking {
        screen: Some(Screen::Locked),
    };
    assert_eq!(said(&mut notices, state(consent)), None, "told once");
    assert_eq!(
        said(&mut notices, state(ConnectionState::Active)).as_deref(),
        Some("pixel (R58M) is on the LAN | Its address is 192.168.1.201.")
    );
    let away = state(ConnectionState::Reconnecting { wait_secs: 120 });
    assert_eq!(
        said(&mut notices, away).as_deref(),
        Some("pixel (R58M) went away | Its address is held for up to 2 min while it comes back.")
    );
    assert!(
        said(&mut notices, state(ConnectionState::Active))
            .unwrap()
            .starts_with("pixel (R58M) is back on the LAN")
    );
}

#[test]
fn an_end_is_told_unless_the_user_asked_for_it() {
    let mut notices = Notices::default();
    let ended = |outcome| state(ConnectionState::Ended { outcome });
    let stopped = Outcome::Clean {
        reason: EndReason::Stopped,
    };
    assert_eq!(said(&mut notices, ended(stopped)), None);
    let on_phone = Outcome::Clean {
        reason: EndReason::PhoneStopped,
    };
    assert_eq!(
        said(&mut notices, ended(on_phone)).as_deref(),
        Some("pixel (R58M) is off the LAN | Stopped on the phone.")
    );
    let failed = Outcome::Failed {
        kind: Kind::Adb,
        message: "the phone was not back within 2 min".into(),
    };
    let notice = notices.on(&ended(failed), str::to_string).unwrap();
    assert!(notice.urgent);
    assert_eq!(notice.body, "the phone was not back within 2 min");
}

#[test]
fn waits_read_as_minutes() {
    assert_eq!(super::duration(45), "45 s");
    assert_eq!(super::duration(600), "10 min");
    assert_eq!(super::duration(90), "1 min 30 s");
}
