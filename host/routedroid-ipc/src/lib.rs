//! Control API of the routedroid daemon (`routedroidd`), shared with every
//! client so the wire cannot drift.
//!
//! Transport: a Unix `SOCK_STREAM` socket owned by the user (mode 0600),
//! newline-delimited JSON. The client sends [`ClientMessage`] lines; the
//! daemon answers each with a [`ServerMessage::Response`] carrying the same
//! `id`, and, once the connection has sent [`Request::Subscribe`], also
//! pushes [`ServerMessage::Event`] lines at any time.

pub mod api;
pub mod client;
pub mod fault;
pub mod socket;
pub mod wire;

pub use api::*;
pub use client::{Calls, Client, ConnectError, Events};
pub use fault::{Fault, FaultExt, Kind};

/// Bumped on any incompatible change to the messages in `api`.
/// 2: a phone's connectivity is a *device connection* on the wire
/// (`session` -> `connection`); "session" now only means the protocol
/// conversation with the app, which the phone implements.
pub const API_VERSION: u32 = 2;
