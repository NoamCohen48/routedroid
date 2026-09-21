//! Screen layout: devices on top, the selected session in the middle, the
//! event log below, a status/key-hint bar at the bottom, popups over it all.

use ratatui::layout::{Constraint, Layout};
use ratatui::Frame;

use crate::app::{App, Mode};

mod devices;
mod log;
mod popups;
mod session;
mod status_bar;

pub fn draw(frame: &mut Frame, app: &App) {
    let device_rows = app.devices.len().max(1) as u16;
    let [devices_area, session_area, log_area, status_area] = Layout::vertical([
        Constraint::Length(device_rows + 3),
        Constraint::Length(session::HEIGHT),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    devices::draw(frame, app, devices_area);
    session::draw(frame, app, session_area);
    log::draw(frame, app, log_area);
    status_bar::draw(frame, app, status_area);

    match &app.mode {
        Mode::Normal => {}
        Mode::StartForm(form) => popups::start_form(frame, form),
        Mode::ConfirmStop { serial } => popups::confirm_stop(frame, serial),
    }
}
