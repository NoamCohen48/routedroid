//! The device table: one row per attached device, cursor on the selected one.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::widgets::{Block, Row, Table, TableState};
use ratatui::Frame;

use crate::app::App;
use crate::describe;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let header =
        Row::new(["Serial", "Model", "ADB", "Usable", "Session"]).style(Style::new().add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = if app.devices.is_empty() {
        vec![Row::new(["(no devices attached)", "", "", "", ""]).dim()]
    } else {
        app.devices
            .iter()
            .map(|device| {
                let session = device.session.as_ref().map(describe::session_state).unwrap_or_default();
                let usable = describe::usable(device);
                let usable_color = if device.unusable_reason.is_none() { Color::Green } else { Color::Yellow };
                Row::new(vec![
                    device.serial.clone().into(),
                    device.model.clone().unwrap_or_default().into(),
                    device.state.clone().into(),
                    ratatui::text::Text::from(usable).fg(usable_color),
                    session.into(),
                ])
            })
            .collect()
    };
    let widths = [
        Constraint::Length(20),
        Constraint::Length(18),
        Constraint::Length(12),
        Constraint::Min(16),
        Constraint::Min(16),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::bordered().title(" Devices "))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
    let mut state = TableState::default().with_selected(if app.devices.is_empty() { None } else { Some(app.cursor) });
    frame.render_stateful_widget(table, area, &mut state);
}
