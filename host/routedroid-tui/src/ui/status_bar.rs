//! One line: daemon link on the left, key hints for the current mode on the
//! right: all of them when they fit, else the essential ones (an 80-column
//! terminal), else none.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Stylize};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::text::cells;
use crate::app::{App, DaemonLink, Mode};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let daemon = match &app.daemon {
        DaemonLink::Connecting => Line::from(" connecting ").fg(Color::Yellow),
        DaemonLink::Connected => Line::from(" connected ").fg(Color::Green),
        DaemonLink::Disconnected { reason } => {
            Line::from(format!(" DISCONNECTED ({reason}); retrying ")).fg(Color::Red)
        }
    };
    let hints: &[&str] = match app.mode {
        Mode::Normal => &[
            "↑/↓ select  s connect  x disconnect  r refresh  PgUp/PgDn log  q quit",
            "s connect  x disconnect  q quit",
        ],
        Mode::StartForm(_) => &[
            "Tab next  ←/→ pick  Space toggle  Enter start  Esc keep & close",
            "Tab next  Enter start  Esc close",
        ],
        Mode::ConfirmStop { .. } => &["y confirm  any other key cancels"],
    };
    let room = usize::from(area.width).saturating_sub(daemon.width() + 1);
    let Some(hints) = hints.iter().find(|hints| hints.chars().count() <= room) else {
        frame.render_widget(Paragraph::new(daemon), area);
        return;
    };
    let [daemon_area, hints_area] = Layout::horizontal([
        Constraint::Length(cells(daemon.width())),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(daemon), daemon_area);
    frame.render_widget(
        Paragraph::new(Line::from(*hints).dim()).right_aligned(),
        hints_area,
    );
}
