//! One line: connection state on the left, key hints for the current mode on the right.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Stylize};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, DaemonLink, Mode};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let [daemon_area, hints_area] =
        Layout::horizontal([Constraint::Min(20), Constraint::Percentage(70)]).areas(area);
    let daemon = match &app.daemon {
        DaemonLink::Connected => Line::from(" connected ").fg(Color::Green),
        DaemonLink::Disconnected { reason } => {
            Line::from(format!(" DISCONNECTED ({reason}); retrying ")).fg(Color::Red)
        }
    };
    let hints = match app.mode {
        Mode::Normal => "↑/↓ j/k select   s connect   x disconnect   r refresh   q quit",
        Mode::StartForm(_) => "Tab next field   Enter submit   Esc cancel",
        Mode::ConfirmStop { .. } => "y confirm stop   any other key cancels",
    };
    frame.render_widget(Paragraph::new(daemon), daemon_area);
    frame.render_widget(
        Paragraph::new(Line::from(hints).dim()).right_aligned(),
        hints_area,
    );
}
