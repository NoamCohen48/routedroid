//! `doctor`: what stands between this host and a working phone, each with
//! the changes `doctor --repair` would make for it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// What was checked: `adb`, `helper`, `policy`, or the object a finding
    /// is about (`session 92c534fc6ddea902`, `link phone0`, ...).
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
    /// The changes a repair would make; empty when it takes a person.
    pub repair: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    /// May get in the way.
    Warn,
    /// Stops phones from connecting, or is state left behind.
    Fail,
}

impl Check {
    pub fn new(name: &str, status: CheckStatus, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status,
            detail: detail.into(),
            repair: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests;
