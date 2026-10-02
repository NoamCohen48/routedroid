//! Framing: one JSON object per line. A server line says on `"msg"` which
//! of the two it is, so a client never has to guess from the fields.

use serde::{Deserialize, Serialize};

use crate::{Event, Request, Response};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientMessage {
    pub id: u64,
    #[serde(flatten)]
    pub request: Request,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "msg", rename_all = "snake_case")]
pub enum ServerMessage {
    /// The answer to the request with the same `id`. A line the daemon
    /// could not read is answered with id 0 when it carried none.
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
