//! Keyboard handling: turns a key press into state changes and daemon commands.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{App, Mode};
use crate::messages::Command;

mod form;

pub use form::paste;

#[cfg(test)]
mod tests;

/// How far PageUp/PageDown move the log.
const LOG_PAGE: usize = 10;

pub fn handle(app: &mut App, key: KeyEvent) -> Vec<Command> {
    if key.kind == KeyEventKind::Release {
        return vec![];
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.quit = true;
        return vec![];
    }
    match std::mem::replace(&mut app.mode, Mode::Normal) {
        Mode::Normal => handle_normal(app, key.code),
        Mode::ConfirmStop { serial } => handle_confirm_stop(app, key.code, serial),
        Mode::StartForm(start) => form::handle(app, key, start),
    }
}

fn handle_normal(app: &mut App, code: KeyCode) -> Vec<Command> {
    match code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Up | KeyCode::Char('k') => app.move_cursor(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_cursor(1),
        KeyCode::PageUp => app.log.scroll_back(LOG_PAGE),
        KeyCode::PageDown => app.log.scroll_forward(LOG_PAGE),
        KeyCode::End => app.log.scroll_forward(usize::MAX),
        KeyCode::Char('r') => {
            return vec![
                Command::RefreshDevices,
                Command::RefreshStatus,
                Command::RefreshInterfaces,
            ];
        }
        KeyCode::Char('s') => match app.selected_device() {
            Some(device) if device.connection.is_some() => {
                app.error(format!("{}: already connected", device.serial));
            }
            Some(device) => return app.open_form(device.serial.clone()),
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
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.info(format!("{serial}: disconnecting"));
            vec![Command::Stop { serial }]
        }
        _ => {
            app.info(format!("{serial}: disconnect cancelled"));
            vec![]
        }
    }
}
