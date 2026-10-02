//! Screen layout: devices on top, the selected connection in the middle, the
//! event log below, a status/key-hint bar at the bottom, popups over it all.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};

use crate::app::{App, Mode};

mod connection;
mod devices;
mod log;
mod popups;
mod status_bar;
mod text;

/// Past this many rows the device table scrolls instead of growing.
const MAX_DEVICE_ROWS: u16 = 8;

pub fn draw(frame: &mut Frame, app: &App) {
    let device_rows = text::cells(app.devices.len().max(1)).min(MAX_DEVICE_ROWS);
    let [devices_area, connection_area, log_area, status_area] = Layout::vertical([
        Constraint::Length(device_rows + 3),
        Constraint::Length(connection::HEIGHT),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    devices::draw(frame, app, devices_area);
    connection::draw(frame, app, connection_area);
    log::draw(frame, app, log_area);
    status_bar::draw(frame, app, status_area);

    match &app.mode {
        Mode::Normal => {}
        Mode::StartForm(form) => popups::start_form(frame, form),
        Mode::ConfirmStop { serial } => popups::confirm_stop(frame, serial),
    }
}
