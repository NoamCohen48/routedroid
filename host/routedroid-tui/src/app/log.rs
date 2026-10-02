//! The event log: bounded, timestamped, and scrollable back.

use std::collections::VecDeque;

pub const LOG_CAPACITY: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Local wall-clock time, `HH:MM:SS`.
    pub time: String,
    pub level: Level,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct Log {
    lines: VecDeque<LogLine>,
    /// Lines scrolled back from the newest; 0 follows the tail.
    scroll: usize,
}

pub fn now() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

impl Log {
    pub fn push(&mut self, level: Level, text: String) {
        if self.lines.len() == LOG_CAPACITY {
            self.lines.pop_front();
        }
        self.lines.push_back(LogLine {
            time: now(),
            level,
            text,
        });
        // Reading back: keep the same lines in view as new ones arrive.
        if self.scroll > 0 {
            self.scroll = (self.scroll + 1).min(self.lines.len().saturating_sub(1));
        }
    }

    pub fn scroll_back(&mut self, lines: usize) {
        self.scroll = (self.scroll + lines).min(self.lines.len().saturating_sub(1));
    }

    pub fn scroll_forward(&mut self, lines: usize) {
        self.scroll = self.scroll.saturating_sub(lines);
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// Oldest first, up to the newest one in view.
    pub fn in_view(&self) -> impl DoubleEndedIterator<Item = &LogLine> {
        self.lines.iter().take(self.lines.len() - self.scroll)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    #[cfg(test)]
    pub fn last(&self) -> Option<&LogLine> {
        self.lines.back()
    }

    #[cfg(test)]
    pub fn first(&self) -> Option<&LogLine> {
        self.lines.front()
    }
}
