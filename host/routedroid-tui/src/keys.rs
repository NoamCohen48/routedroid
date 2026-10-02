//! Keyboard handling: turns a key press into state changes and daemon commands.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{App, Mode};
use crate::form::StartForm;
use crate::messages::Command;

#[cfg(test)]
mod tests;

pub fn handle(app: &mut App, key: KeyEvent) -> Vec<Command> {
    if key.kind == KeyEventKind::Release {
        return vec![];
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.quit = true;
        return vec![];
    }
    match app.mode.clone() {
        Mode::Normal => handle_normal(app, key.code),
        Mode::ConfirmStop { serial } => handle_confirm_stop(app, key.code, serial),
        Mode::StartForm(form) => handle_form(app, key, form),
    }
}

fn handle_normal(app: &mut App, code: KeyCode) -> Vec<Command> {
    match code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Up | KeyCode::Char('k') => app.move_cursor(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_cursor(1),
        KeyCode::Char('r') => return vec![Command::RefreshDevices, Command::RefreshStatus],
        KeyCode::Char('s') => match app.selected_device() {
            Some(device) if device.connection.is_some() => {
                app.error(format!("{}: already connected", device.serial))
            }
            Some(device) => app.mode = Mode::StartForm(StartForm::new(device.serial.clone())),
            None => app.error("no device selected"),
        },
        KeyCode::Char('x') => match app.selected_device() {
            Some(device) if device.connection.is_some() => {
                app.mode = Mode::ConfirmStop {
                    serial: device.serial.clone(),
                }
            }
            Some(device) => app.error(format!("{}: not connected", device.serial)),
            None => app.error("no device selected"),
        },
        _ => {}
    }
    vec![]
}

fn handle_confirm_stop(app: &mut App, code: KeyCode, serial: String) -> Vec<Command> {
    app.mode = Mode::Normal;
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.info(format!("{serial}: stopping"));
            vec![Command::Stop { serial }]
        }
        _ => {
            app.info(format!("{serial}: stop cancelled"));
            vec![]
        }
    }
}

fn handle_form(app: &mut App, key: KeyEvent, mut form: StartForm) -> Vec<Command> {
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            return vec![];
        }
        KeyCode::Enter => {
            return match form.to_request() {
                Ok(request) => {
                    app.mode = Mode::Normal;
                    app.info(format!(
                        "{}: starting on {} for {}",
                        request.serial, request.lan_if, request.phone_ip
                    ));
                    vec![Command::Start(request)]
                }
                Err(error) => {
                    app.error(error.to_string());
                    vec![]
                }
            };
        }
        KeyCode::Tab | KeyCode::Down => form.focus_next(),
        KeyCode::BackTab | KeyCode::Up => form.focus_previous(),
        KeyCode::Backspace => form.backspace(),
        KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            form.insert(character)
        }
        _ => {}
    }
    app.mode = Mode::StartForm(form);
    vec![]
}
