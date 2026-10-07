use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use routedroid_ipc::{ConnectionInfo, ConnectionState, DeviceInfo, Traffic};

use super::*;
use crate::app::App;
use crate::messages::Incoming;

fn text(line: &Line) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// A minute of readings: `to` and `from` bytes a second, then a burst.
fn busy() -> Rates {
    let start = Instant::now();
    let mut rates = Rates::default();
    let (mut to, mut from) = (0, 0);
    for second in 0..=70 {
        let burst = if second == 70 { 100_000 } else { 0 };
        to += 120_000 + burst;
        from += 8_000;
        let traffic = Traffic {
            bytes_to_phone: to,
            bytes_from_phone: from,
            ..Traffic::default()
        };
        rates.record(start + Duration::from_secs(second), &traffic);
    }
    rates
}

#[test]
fn bars_scale_to_the_peak_and_sit_on_the_right() {
    assert_eq!(graph(&[0, 1, 4, 8], 6), "   ▁▄█");
    assert_eq!(graph(&[3, 3], 2), "██", "a steady rate fills");
    assert_eq!(graph(&[0, 0], 3), "   ", "silence is blank");
    assert_eq!(graph(&[1, 1_000_000], 2), "▁█", "any traffic shows");
}

#[test]
fn the_lines_fit_and_end_with_the_current_rate() {
    let lines = lines(Some(&busy()), 78);
    assert_eq!(lines.len(), 2);
    let receives = text(&lines[0]);
    assert!(
        receives.trim_start().starts_with("receives: "),
        "{receives}"
    );
    assert!(receives.ends_with(" 220.0 kB/s"), "{receives}");
    assert!(
        text(&lines[1]).ends_with(" 8.0 kB/s"),
        "{}",
        text(&lines[1])
    );
    for line in &lines {
        assert!(text(line).chars().count() <= 78, "{}", text(line));
    }
}

#[test]
fn before_two_readings_it_is_measuring() {
    let lines = lines(None, 78);
    assert_eq!(lines.len(), 1);
    assert!(text(&lines[0]).contains("(measuring)"));
}

#[test]
fn an_active_connection_at_80x24_shows_its_graph_and_keeps_its_hints() {
    let mut app = App::new();
    app.apply(Incoming::Connected);
    let device = DeviceInfo {
        serial: "85e49002".into(),
        state: "device".into(),
        model: Some("SM_J810G".into()),
        unusable_reason: None,
        connection: Some(ConnectionState::Active),
    };
    app.apply(Incoming::Devices(vec![device]));
    app.apply(Incoming::Connections(vec![ConnectionInfo {
        serial: "85e49002".into(),
        lan_if: "lan0".into(),
        tun: "phone0".into(),
        mtu: 1400,
        state: ConnectionState::Active,
        started_at: 0,
        network: None,
        traffic: Traffic::default(),
    }]));
    app.rates.insert("85e49002".into(), busy());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| crate::ui::draw(frame, &app)).unwrap();
    let screen: Vec<String> = terminal
        .backend()
        .buffer()
        .content()
        .chunks(80)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect();
    let screen = screen.join("\n");
    for want in [
        "state: active",
        "receives: ",
        "220.0 kB/s",
        "8.0 kB/s",
        "s connect  x disconnect  q quit",
    ] {
        assert!(screen.contains(want), "{want:?} missing:\n{screen}");
    }
    assert!(screen.contains('█'), "{screen}");
}
