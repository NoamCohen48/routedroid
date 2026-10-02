//! One line: daemon link on the left, key hints for the current mode on the
//! right, dropped when the terminal is too narrow for both.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Stylize};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, DaemonLink, Mode};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let daemon = match &app.daemon {
        DaemonLink::Connecting => Line::from(" connecting ").fg(Color::Yellow),
        DaemonLink::Connected => Line::from(" connected ").fg(Color::Green),
        DaemonLink::Disconnected { reason } => {
            Line::from(format!(" DISCONNECTED ({reason}); retrying ")).fg(Color::Red)
        }
    };
    let hints = match app.mode {
        Mode::Normal => "↑/↓ select  s connect  x disconnect  r refresh  PgUp/PgDn log  q quit",
        Mode::StartForm(_) => "Tab next  ←/→ pick  Space toggle  Enter start  Esc keep & close",
        Mode::ConfirmStop { .. } => "y confirm  any other key cancels",
    };
    let needed = daemon.width() + hints.chars().count() + 1;
    if (area.width as usize) < needed {
        frame.render_widget(Paragraph::new(daemon), area);
        return;
    }
    let [daemon_area, hints_area] = Layout::horizontal([
        Constraint::Length(daemon.width() as u16),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(daemon), daemon_area);
    frame.render_widget(
        Paragraph::new(Line::from(hints).dim()).right_aligned(),
        hints_area,
    );
}
