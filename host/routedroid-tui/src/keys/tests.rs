//! Key bindings drive modes and emit the right commands.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use routedroid_ipc::{ConnectionState, DeviceInfo, InterfaceInfo};

use super::{handle, paste};
use crate::app::{App, Level, Mode};
use crate::form::Field;
use crate::messages::{Command, Incoming};

fn press(app: &mut App, code: KeyCode) -> Vec<Command> {
    handle(app, KeyEvent::from(code))
}

fn type_text(app: &mut App, text: &str) {
    text.chars().for_each(|character| {
        press(app, KeyCode::Char(character));
    });
}

fn app_with(connection: Option<ConnectionState>) -> App {
    let mut app = App::new();
    let device = DeviceInfo {
        serial: "abc".into(),
        name: None,
        auto: false,
        state: "device".into(),
        model: None,
        unusable_reason: None,
        connection,
    };
    app.apply(Incoming::Devices(vec![device]));
    app
}

fn form_value(app: &App) -> crate::form::StartForm {
    match &app.mode {
        Mode::StartForm(form) => (**form).clone(),
        other => panic!("not in the form: {other:?}"),
    }
}

#[test]
fn q_quits_and_r_refreshes() {
    let mut app = app_with(None);
    assert_eq!(press(&mut app, KeyCode::Char('r')).len(), 3);
    press(&mut app, KeyCode::Char('q'));
    assert!(app.quit);
}

#[test]
fn start_form_submits_a_start_request() {
    let mut app = app_with(None);
    assert!(matches!(
        press(&mut app, KeyCode::Char('s')).as_slice(),
        [Command::RefreshInterfaces]
    ));
    type_text(&mut app, "eth0");
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "10.0.5");
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Left);
    type_text(&mut app, ".0");
    let commands = press(&mut app, KeyCode::Enter);
    match commands.as_slice() {
        [Command::Start { request, remember }] => {
            assert_eq!(
                (request.serial.as_deref(), request.lan_if.as_deref()),
                (Some("abc"), Some("eth0"))
            );
            assert_eq!(request.phone_ip.unwrap().to_string(), "10.0.0.5");
            assert_eq!(*remember, None, "nothing asked to remember it");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn the_interface_is_picked_with_the_arrows() {
    let mut app = app_with(None);
    let interface = |name: &str| InterfaceInfo {
        name: name.into(),
        up: true,
        addresses: vec![],
        default_route: false,
        phone_addresses: vec![],
        dhcp: false,
        ineligible: None,
    };
    app.apply(Incoming::Interfaces(vec![
        interface("eno1"),
        interface("wlan0"),
    ]));
    press(&mut app, KeyCode::Char('s'));
    assert_eq!(form_value(&app).lan_if.value(), "eno1");
    press(&mut app, KeyCode::Right);
    type_text(&mut app, "x");
    assert_eq!(
        form_value(&app).lan_if.value(),
        "wlan0",
        "a picked name is not typed into"
    );
}

#[test]
fn paste_and_toggle_fill_the_form() {
    let mut app = app_with(None);
    press(&mut app, KeyCode::Char('s'));
    paste(&mut app, "eno1\n");
    assert_eq!(form_value(&app).lan_if.value(), "eno1 ");
    press(&mut app, KeyCode::BackTab);
    press(&mut app, KeyCode::Char(' '));
    assert!(form_value(&app).allow_network_adb);
    handle(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    );
}

#[test]
fn invalid_form_stays_open_and_logs_red() {
    let mut app = app_with(None);
    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "nope");
    assert!(press(&mut app, KeyCode::Enter).is_empty());
    assert!(matches!(app.mode, Mode::StartForm(_)));
    assert_eq!(app.log.last().unwrap().level, Level::Error);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn stop_needs_a_connection_and_a_confirmation() {
    let mut app = app_with(None);
    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.log.last().unwrap().level, Level::Error);

    let mut app = app_with(Some(ConnectionState::Active));
    press(&mut app, KeyCode::Char('x'));
    assert_eq!(
        app.mode,
        Mode::ConfirmStop {
            serial: "abc".into()
        }
    );
    assert!(press(&mut app, KeyCode::Char('n')).is_empty());
    assert_eq!(app.mode, Mode::Normal);
    press(&mut app, KeyCode::Char('x'));
    let commands = press(&mut app, KeyCode::Char('y'));
    assert!(matches!(commands.as_slice(), [Command::Stop { serial }] if serial == "abc"));
}

#[test]
fn page_keys_scroll_the_log() {
    let mut app = app_with(None);
    (0..30).for_each(|n| app.info(format!("line {n}")));
    press(&mut app, KeyCode::PageUp);
    assert_eq!(app.log.scroll(), 10);
    press(&mut app, KeyCode::End);
    assert_eq!(app.log.scroll(), 0);
}

#[test]
fn a_name_in_the_form_remembers_the_phone() {
    let mut app = app_with(None);
    press(&mut app, KeyCode::Char('s'));
    while !matches!(&app.mode, Mode::StartForm(form) if form.focused == Field::Name) {
        press(&mut app, KeyCode::Tab);
    }
    type_text(&mut app, "pixel");
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Char(' '));
    match press(&mut app, KeyCode::Enter).as_slice() {
        [
            Command::Start {
                remember: Some(phone),
                ..
            },
        ] => {
            assert_eq!(
                (phone.serial.as_str(), phone.name.as_deref()),
                ("abc", Some("pixel"))
            );
            assert!(phone.auto, "the box was ticked");
        }
        other => panic!("unexpected {other:?}"),
    }
}
