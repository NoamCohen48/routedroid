//! The device table: one row per attached device, cursor on the selected one.
//! A remembered name comes before the serial, and long ones are cut with an
//! ellipsis; a phone's last end stays on its row.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Text;
use ratatui::widgets::{Block, Row, Table, TableState};

use super::text::{cells, fit};
use crate::app::App;
use crate::describe;

const SERIAL: usize = 24;
const MODEL: usize = 16;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(["Phone", "Model", "ADB", "Usable", "Connection"])
        .style(Style::new().add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = if app.devices.is_empty() {
        vec![Row::new(["(no devices attached)", "", "", "", ""]).dim()]
    } else {
        app.devices.iter().map(|device| row(app, device)).collect()
    };
    let widths = [
        Constraint::Max(cells(SERIAL)),
        Constraint::Max(cells(MODEL)),
        Constraint::Length(12),
        Constraint::Fill(1),
        Constraint::Fill(2),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::bordered().title(" Devices "))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
    let selected = (!app.devices.is_empty()).then_some(app.cursor);
    frame.render_stateful_widget(
        table,
        area,
        &mut TableState::default().with_selected(selected),
    );
}

fn row<'a>(app: &App, device: &'a routedroid_ipc::DeviceInfo) -> Row<'a> {
    let usable_color = if device.unusable_reason.is_none() {
        Color::Green
    } else {
        Color::Yellow
    };
    let connection = match (&device.connection, app.last_end.get(&device.serial)) {
        (Some(state), _) => Text::from(state.to_string()),
        (None, Some(end)) if end.failed => Text::from(end.text.clone()).fg(Color::Red),
        (None, Some(end)) => Text::from(end.text.clone()).dim(),
        (None, None) => Text::default(),
    };
    Row::new(vec![
        fit(
            &routedroid_ipc::label(&device.serial, device.name.as_deref()),
            SERIAL,
        )
        .into(),
        fit(device.model.as_deref().unwrap_or_default(), MODEL).into(),
        device.state.clone().into(),
        Text::from(describe::usable(device)).fg(usable_color),
        connection,
    ])
}
