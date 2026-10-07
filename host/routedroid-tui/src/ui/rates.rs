//! The connection pane's two throughput lines: the last minute as a bar
//! graph (newest on the right, each line scaled to its own peak), then the
//! current rate.

use ratatui::style::{Color, Stylize};
use ratatui::text::{Line, Span};
use routedroid_ipc::bytes;

use super::connection::{LABEL_WIDTH, pair};
use crate::app::Rates;

const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Room kept for the rate: "999.9 MB/s" and the space before it, and one to spare.
const RATE_WIDTH: usize = 12;

/// "receives ▂▃▅▇ 120.0 kB/s" and "sends ▁▁▂ 8.0 kB/s", fitted to `width`.
pub fn lines(rates: Option<&Rates>, width: u16) -> Vec<Line<'static>> {
    let Some(rates) = rates.filter(|rates| rates.now().is_some()) else {
        return vec![pair("receives", "(measuring)".into(), None)];
    };
    let room = usize::from(width).saturating_sub(LABEL_WIDTH + RATE_WIDTH);
    vec![
        line("receives", rates.to_phone.iter().copied(), room),
        line("sends", rates.from_phone.iter().copied(), room),
    ]
}

fn line(label: &str, samples: impl DoubleEndedIterator<Item = u64>, room: usize) -> Line<'static> {
    let mut recent: Vec<u64> = samples.rev().take(room).collect();
    recent.reverse();
    let rate = format!(" {}/s", bytes(recent.last().copied().unwrap_or(0)));
    let mut line = pair(label, String::new(), None);
    line.spans.pop();
    line.spans
        .push(Span::from(graph(&recent, room)).fg(Color::Cyan));
    line.spans.push(Span::from(rate));
    line
}

/// `samples` as bars, right-aligned in `room` cells; zero is a blank.
pub fn graph(samples: &[u64], room: usize) -> String {
    let peak = samples.iter().copied().max().unwrap_or(0).max(1);
    let bars = samples.iter().map(|&rate| match rate {
        0 => ' ',
        // 1..=8 eighths of the peak, so any traffic shows.
        _ => {
            BARS[usize::try_from(rate.saturating_mul(8).div_ceil(peak))
                .unwrap_or(8)
                .clamp(1, 8)
                - 1]
        }
    });
    let pad = room.saturating_sub(samples.len());
    std::iter::repeat_n(' ', pad).chain(bars).collect()
}

#[cfg(test)]
mod tests;
