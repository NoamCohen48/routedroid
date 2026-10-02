//! Details of the selected device's connection, if it has one.

use ratatui::layout::Rect;
use ratatui::style::{Color, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::describe;

/// Border plus four lines of details.
pub const HEIGHT: u16 = 6;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let title = match app.selected_device() {
        Some(device) => format!(" Connection to {} ", device.serial),
        None => " Connection ".to_string(),
    };
    let lines = match app.selected_connection() {
        Some(connection) => {
            let state_color = if describe::is_failure(&connection.state) {
                Color::Red
            } else {
                Color::Green
            };
            vec![
                pair("phone_ip", connection.phone_ip.to_string(), None),
                pair(
                    "lan_if",
                    format!("{}    tun: {}", connection.lan_if, connection.tun),
                    None,
                ),
                pair(
                    "state",
                    describe::connection_state(&connection.state),
                    Some(state_color),
                ),
                pair(
                    "packets",
                    format!(
                        "to phone {}    from phone {}",
                        connection.packets_to_phone, connection.packets_from_phone
                    ),
                    None,
                ),
            ]
        }
        None => match app
            .selected_device()
            .and_then(|device| device.connection.as_ref())
        {
            Some(state) => {
                vec![
                    pair("state", describe::connection_state(state), None),
                    Line::from("(fetching details)"),
                ]
            }
            None => vec![Line::from("not connected; press s to connect").dim()],
        },
    };
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(title)),
        area,
    );
}

fn pair(label: &'static str, value: String, color: Option<Color>) -> Line<'static> {
    let value_span = match color {
        Some(color) => Span::from(value).fg(color),
        None => Span::from(value),
    };
    Line::from(vec![Span::from(format!("{label:>9}: ")).dim(), value_span])
}
