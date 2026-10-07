//! Throughput over the last minute, per connection: bytes a second to and
//! from the phone, worked out from the counters `traffic` events carry
//! (about one a second). Only what the screen draws; nothing is kept once a
//! connection ends.

use std::collections::VecDeque;
use std::time::Instant;

use routedroid_ipc::Traffic;

/// About a minute of samples at one a second.
pub const SAMPLES: usize = 60;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rates {
    /// Bytes a second, oldest first.
    pub to_phone: VecDeque<u64>,
    pub from_phone: VecDeque<u64>,
    last: Option<(Instant, u64, u64)>,
}

impl Rates {
    /// Fold in the counters as read at `at`. The first reading only sets
    /// the baseline; counters that went backwards (a new connection) start
    /// the graph over.
    pub fn record(&mut self, at: Instant, traffic: &Traffic) {
        let (to, from) = (traffic.bytes_to_phone, traffic.bytes_from_phone);
        if let Some((then, to_before, from_before)) = self.last {
            if to < to_before || from < from_before {
                *self = Self::default();
            } else {
                let secs = at.saturating_duration_since(then).as_secs_f64();
                if secs > 0.0 {
                    push(&mut self.to_phone, per_second(to - to_before, secs));
                    push(&mut self.from_phone, per_second(from - from_before, secs));
                }
            }
        }
        self.last = Some((at, to, from));
    }

    /// The latest bytes a second each way; `None` before two readings.
    pub fn now(&self) -> Option<(u64, u64)> {
        Some((*self.to_phone.back()?, *self.from_phone.back()?))
    }
}

fn push(samples: &mut VecDeque<u64>, rate: u64) {
    if samples.len() == SAMPLES {
        samples.pop_front();
    }
    samples.push_back(rate);
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn per_second(bytes: u64, secs: f64) -> u64 {
    // Non-negative and far below u64::MAX: a byte count over a second or so.
    (bytes as f64 / secs).round() as u64
}

#[cfg(test)]
mod tests;
