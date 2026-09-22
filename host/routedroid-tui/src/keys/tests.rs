//! Key bindings drive modes and emit the right commands.

use crossterm::event::{KeyCode, KeyEvent};
use routedroid_ipc::{ConnectionState, DeviceInfo};

use super::handle;
use crate::app::{App, Level, Mode};
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
    let device =
        DeviceInfo { serial: "abc".into(), state: "device".into(), model: None, unusable_reason: None, connection };
    app.apply(Incoming::Devices(vec![device]));
    app
}

#[test]
fn q_quits_and_r_refreshes() {
    let mut app = app_with(None);
    assert!(matches!(
        press(&mut app, KeyCode::Char('r')).as_slice(),
        [Command::RefreshDevices, Command::RefreshStatus]
    ));
    press(&mut app, KeyCode::Char('q'));
    assert!(app.quit);
}

#[test]
fn start_form_submits_a_start_request() {
    let mut app = app_with(None);
    press(&mut app, KeyCode::Char('s'));
    assert!(matches!(app.mode, Mode::StartForm(_)));
    type_text(&mut app, "eth0");
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "10.0.0.5");
    let commands = press(&mut app, KeyCode::Enter);
    match commands.as_slice() {
        [Command::Start(request)] => {
            assert_eq!(request.serial, "abc");
            assert_eq!(request.lan_if, "eth0");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn invalid_form_stays_open_and_logs_red() {
    let mut app = app_with(None);
    press(&mut app, KeyCode::Char('s'));
    assert!(press(&mut app, KeyCode::Enter).is_empty());
    assert!(matches!(app.mode, Mode::StartForm(_)));
    assert_eq!(app.log.back().unwrap().level, Level::Error);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn stop_needs_a_connection_and_a_confirmation() {
    let mut app = app_with(None);
    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.log.back().unwrap().level, Level::Error);

    let mut app = app_with(Some(ConnectionState::Active));
    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.mode, Mode::ConfirmStop { serial: "abc".into() });
    assert!(press(&mut app, KeyCode::Char('n')).is_empty());
    assert_eq!(app.mode, Mode::Normal);
    press(&mut app, KeyCode::Char('x'));
    let commands = press(&mut app, KeyCode::Char('y'));
    assert!(matches!(commands.as_slice(), [Command::Stop { serial }] if serial == "abc"));
}
