//! Keys inside the start form: line editing in text fields, ←/→ to pick the
//! interface, Space to toggle, Tab to move, Enter to start, Esc to leave
//! (the draft is kept either way).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Mode};
use crate::form::{Field, StartForm};
use crate::messages::Command;

pub fn handle(app: &mut App, key: KeyEvent, mut form: Box<StartForm>) -> Vec<Command> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => {
            app.close_form(form);
            return vec![];
        }
        KeyCode::Enter => return submit(app, form),
        KeyCode::Tab | KeyCode::Down => form.focus_next(),
        KeyCode::BackTab | KeyCode::Up => form.focus_previous(),
        KeyCode::Left if form.picking() => form.pick(-1),
        KeyCode::Right if form.picking() => form.pick(1),
        KeyCode::Char(' ') if form.focused == Field::NetworkAdb => {
            form.allow_network_adb = !form.allow_network_adb
        }
        code => {
            if let Some(input) = form.input(form.focused) {
                match code {
                    KeyCode::Char('u') if control => input.clear(),
                    KeyCode::Char('a') if control => input.home(),
                    KeyCode::Char('e') if control => input.end(),
                    KeyCode::Char(c) if !control => input.insert(c.encode_utf8(&mut [0; 4])),
                    KeyCode::Backspace => input.backspace(),
                    KeyCode::Delete => input.delete(),
                    KeyCode::Left => input.left(),
                    KeyCode::Right => input.right(),
                    KeyCode::Home => input.home(),
                    KeyCode::End => input.end(),
                    _ => {}
                }
            }
        }
    }
    app.mode = Mode::StartForm(form);
    vec![]
}

/// Bracketed paste lands in the focused text field, on one line.
pub fn paste(app: &mut App, text: &str) {
    if let Mode::StartForm(form) = &mut app.mode {
        if let Some(input) = form.input(form.focused) {
            input.insert(&text.replace(['\n', '\r'], " "));
        }
    }
}

fn submit(app: &mut App, form: Box<StartForm>) -> Vec<Command> {
    match form.to_request() {
        Ok(request) => {
            let address = match request.phone_ip {
                Some(ip) => ip.to_string(),
                None => "a DHCP address".into(),
            };
            app.info(format!(
                "{}: starting on {} with {address}",
                request.serial, request.lan_if
            ));
            app.close_form(form);
            vec![Command::Start(request)]
        }
        Err(error) => {
            app.error(error.to_string());
            app.mode = Mode::StartForm(form);
            vec![]
        }
    }
}
