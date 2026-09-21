//! The event log: newest lines at the bottom, errors in red.

use ratatui::layout::Rect;
use ratatui::style::{Color, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::{App, Level};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let visible = area.height.saturating_sub(2) as usize;
    let skipped = app.log.len().saturating_sub(visible);
    let lines: Vec<Line> = app
        .log
        .iter()
        .skip(skipped)
        .map(|entry| match entry.level {
            Level::Info => Line::from(entry.text.as_str()),
            Level::Error => Line::from(entry.text.as_str()).fg(Color::Red),
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).block(Block::bordered().title(" Events ")), area);
}
