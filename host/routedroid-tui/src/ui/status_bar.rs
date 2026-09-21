//! One line: connection state on the left, key hints for the current mode on the right.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Stylize};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, Connection, Mode};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let [connection_area, hints_area] =
        Layout::horizontal([Constraint::Min(20), Constraint::Percentage(70)]).areas(area);
    let connection = match &app.connection {
        Connection::Connected => Line::from(" connected ").fg(Color::Green),
        Connection::Disconnected { reason } => {
            Line::from(format!(" DISCONNECTED ({reason}); retrying ")).fg(Color::Red)
        }
    };
    let hints = match app.mode {
        Mode::Normal => "↑/↓ j/k select   s start   x stop   r refresh   q quit",
        Mode::StartForm(_) => "Tab next field   Enter submit   Esc cancel",
        Mode::ConfirmStop { .. } => "y confirm stop   any other key cancels",
    };
    frame.render_widget(Paragraph::new(connection), connection_area);
    frame.render_widget(Paragraph::new(Line::from(hints).dim()).right_aligned(), hints_area);
}
