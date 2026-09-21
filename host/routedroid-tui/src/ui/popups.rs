//! Centered popups: the start form and the stop confirmation.

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::form::{Field, StartForm};

pub fn start_form(frame: &mut Frame, form: &StartForm) {
    let area = centered(frame.area(), 60, Field::ALL.len() as u16 * 2 + 3);
    let mut lines = Vec::new();
    for field in Field::ALL {
        let focused = field == form.focused;
        let label = if focused { format!("> {}", field.label()) } else { format!("  {}", field.label()) };
        let label_style = if focused { Style::new().add_modifier(Modifier::BOLD) } else { Style::new() };
        lines.push(Line::from(Span::styled(label, label_style)));
        let value = format!("  {}{}", form.value(field), if focused { "_" } else { "" });
        lines.push(Line::from(value).fg(if focused { Color::Cyan } else { Color::Reset }));
    }
    lines.push(Line::from("Enter to start, Esc to cancel").dim());
    let title = format!(" Start session on {} ", form.serial);
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(Block::bordered().title(title)), area);
}

pub fn confirm_stop(frame: &mut Frame, serial: &str) {
    let area = centered(frame.area(), 50, 3);
    let text = Line::from(format!("Stop the session on {serial}? [y/N]"));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(text).block(Block::bordered().title(" Confirm ")), area);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [horizontal] = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center).areas(area);
    let [rect] = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).areas(horizontal);
    rect
}
