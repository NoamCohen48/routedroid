//! Details of the selected device's connection, or how its last one ended.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use routedroid_ipc::{ConnectionInfo, bytes};

use crate::app::App;
use crate::describe;

/// Border plus five lines of details, and one for a long state to wrap into.
pub const HEIGHT: u16 = 8;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let device = app.selected_device();
    let title = match device {
        Some(device) => format!(" Connection to {} ", device.serial),
        None => " Connection ".to_string(),
    };
    let lines = match (app.selected_connection(), device) {
        (Some(connection), _) => details(connection),
        (None, Some(device)) => match (&device.connection, app.last_end.get(&device.serial)) {
            (Some(state), _) => vec![
                pair("state", format!("{state:#}"), None),
                "(fetching details)".into(),
            ],
            (None, Some(end)) => {
                let color = if end.failed { Color::Red } else { Color::Reset };
                vec![
                    pair(&format!("at {}", end.time), end.text.clone(), Some(color)),
                    Line::from("press s to connect again").dim(),
                ]
            }
            (None, None) => vec![Line::from("not connected; press s to connect").dim()],
        },
        (None, None) => vec![],
    };
    let block = Block::bordered().title(title);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

fn details(connection: &ConnectionInfo) -> Vec<Line<'static>> {
    let state_color = if describe::is_failure(&connection.state) {
        Color::Red
    } else {
        Color::Green
    };
    let traffic = &connection.traffic;
    let network = match &connection.network {
        Some(network) => {
            let dns: Vec<String> = network.dns.iter().map(ToString::to_string).collect();
            let dns = if dns.is_empty() {
                "none".into()
            } else {
                dns.join(" ")
            };
            let lease = if network.lease.is_some() {
                " (DHCP)"
            } else {
                ""
            };
            format!(
                "{}{lease}   host {}/{}   DNS {dns}",
                network.phone_ip, network.host_ip, network.lan_prefix
            )
        }
        None => "(not on the LAN yet)".into(),
    };
    vec![
        pair("phone", network, None),
        pair(
            "link",
            format!(
                "{} via {}   MTU {}",
                connection.lan_if, connection.tun, connection.mtu
            ),
            None,
        ),
        pair(
            "state",
            format!("{:#}", connection.state),
            Some(state_color),
        ),
        pair(
            "traffic",
            format!(
                "to phone {} pkts / {}   from phone {} pkts / {}",
                traffic.packets_to_phone,
                bytes(traffic.bytes_to_phone),
                traffic.packets_from_phone,
                bytes(traffic.bytes_from_phone)
            ),
            None,
        ),
        pair(
            "dropped",
            format!(
                "{} malformed   {} congested",
                traffic.dropped_malformed, traffic.dropped_congested
            ),
            None,
        ),
    ]
}

fn pair(label: &str, value: String, color: Option<Color>) -> Line<'static> {
    let value = match color {
        Some(color) => Span::from(value).fg(color),
        None => Span::from(value),
    };
    Line::from(vec![Span::from(format!("{label:>9}: ")).dim(), value])
}
