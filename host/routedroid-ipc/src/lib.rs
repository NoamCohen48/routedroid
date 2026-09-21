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
pub use client::Client;
pub use fault::{Fault, FaultExt, Kind};

/// Bumped on any incompatible change to the messages in `api`.
pub const API_VERSION: u32 = 1;
