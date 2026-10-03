//! What `Inspect` and `Repair` report: something Routedroid left behind, or
//! something on the host that gets in its way.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    /// What it is about, e.g. `session 92c534fc6ddea902` or `link phone0`.
    pub subject: String,
    pub problem: String,
    /// It may get in the way (the host's own firewall), as opposed to
    /// something Routedroid left wrong.
    pub warning: bool,
    /// Every change `Repair` would make for it, in order; empty when it
    /// takes a person (an unreadable journal, the host's own firewall).
    pub repair: Vec<String>,
}
