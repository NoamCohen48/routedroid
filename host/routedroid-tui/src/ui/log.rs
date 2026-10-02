//! The event log: newest lines at the bottom, errors in red, long lines
//! wrapped; PageUp/PageDown scroll back and forth.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::app::{App, Level};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let visible = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2).max(1) as usize;
    // Newest first until the pane is full, counting wrapped rows.
    let mut rows = 0;
    let mut lines: Vec<Line> = Vec::new();
    for entry in app.log.in_view().rev() {
        let text = format!("{} {}", entry.time, entry.text);
        rows += text.chars().count().div_ceil(inner_width).max(1);
        if rows > visible && !lines.is_empty() {
            break;
        }
        let text = Span::from(entry.text.as_str());
        let text = if entry.level == Level::Error {
            text.fg(Color::Red)
        } else {
            text
        };
        lines.push(Line::from(vec![
            Span::from(format!("{} ", entry.time)).dim(),
            text,
        ]));
    }
    lines.reverse();
    let title = match app.log.scroll() {
        0 => " Events ".to_string(),
        back => format!(" Events ({back} newer below; End follows) "),
    };
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph.block(Block::bordered().title(title)), area);
}
