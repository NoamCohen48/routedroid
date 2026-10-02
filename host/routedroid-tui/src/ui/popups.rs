//! Centered popups: the start form and the stop confirmation. The form puts
//! the terminal's own cursor in the focused field.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::text::cells;
use crate::form::{Field, StartForm};

const WIDTH: u16 = 64;

pub fn start_form(frame: &mut Frame, form: &StartForm) {
    let area = centered(frame.area(), WIDTH, cells(Field::ALL.len()) * 2 + 3);
    let mut lines = Vec::new();
    let mut cursor = None;
    for field in Field::ALL {
        let focused = field == form.focused;
        let marker = if focused { "> " } else { "  " };
        let style = if focused {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{}", field.label()),
            style,
        )));
        let mut value = format!("  {}", form.shown(field));
        if field == Field::LanIf && !form.choices.is_empty() {
            value = format!(
                "  ‹ {} ›  ({} allowed)",
                form.shown(field),
                form.choices.len()
            );
        }
        let color = if focused { Color::Cyan } else { Color::Reset };
        let mut line = Line::from(Span::from(value).fg(color));
        if let Some(hint) = (field == Field::PhoneIp)
            .then(|| form.phone_hint())
            .flatten()
        {
            line.push_span(Span::from(format!("  ({hint})")).dim());
        }
        lines.push(line);
        if focused {
            let row = area.y + cells(lines.len());
            if let Some(input) = form.editable(field) {
                cursor = Some(Position::new(area.x + 3 + cells(input.cursor()), row));
            }
        }
    }
    lines.push(Line::from("Enter to start, Esc to close (the draft is kept)").dim());
    let title = format!(" Connect {} ", form.serial);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(title)),
        area,
    );
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
}

pub fn confirm_stop(frame: &mut Frame, serial: &str) {
    let area = centered(frame.area(), 50, 3);
    let text = Line::from(format!("Disconnect {serial}? [y/N]"));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text).block(Block::bordered().title(" Confirm ")),
        area,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [horizontal] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(area);
    let [rect] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(horizontal);
    rect
}
