//! Framing: one JSON object per line, correlated by `id`.

use serde::{Deserialize, Serialize};

use crate::api::{Event, Request, Response};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientMessage {
    pub id: u64,
    #[serde(flatten)]
    pub request: Request,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerMessage {
    Response {
        id: u64,
        #[serde(flatten)]
        response: Response,
    },
    Event {
        #[serde(flatten)]
        event: Event,
    },
}

/// Largest accepted line; the daemon closes a connection that exceeds it.
pub const MAX_LINE: usize = 64 * 1024;
