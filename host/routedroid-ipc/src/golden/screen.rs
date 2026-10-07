use super::*;

#[test]
fn a_locked_phone_is_named_in_the_state() {
    let serial = "R58M".to_string();
    let state = |screen| ConnectionState::WaitingForApp { screen };
    pinned(
        push(Event::Connection {
            serial: serial.clone(),
            state: state(Some(Screen::Locked)),
        }),
        r#"{"msg":"event","event":"connection","serial":"R58M","state":{"state":"waiting_for_app","screen":"locked"}}"#,
    );
    pinned(
        push(Event::Connection {
            serial,
            state: ConnectionState::Handshaking { screen: None },
        }),
        r#"{"msg":"event","event":"connection","serial":"R58M","state":{"state":"handshaking"}}"#,
    );
    assert_eq!(
        state(Some(Screen::Off)).to_string(),
        "waiting for app (screen off)"
    );
    assert!(format!("{:#}", state(Some(Screen::Locked))).contains("unlock it to continue"));
}
