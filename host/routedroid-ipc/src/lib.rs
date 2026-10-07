//! Control API of the routedroid daemon (`routedroidd`), shared with every
//! client so the wire cannot drift.
//!
//! Transport: a Unix `SOCK_STREAM` socket owned by the user (mode 0600),
//! newline-delimited JSON. The client sends [`ClientMessage`] lines; the
//! daemon answers each with a [`ServerMessage::Response`] carrying the same
//! `id`, and, once the connection has sent [`Request::Subscribe`], also
//! pushes [`ServerMessage::Event`] lines at any time. Every shape is pinned
//! by the golden tests in `golden/`.

pub mod client;
mod describe;
mod doctor;
mod event;
mod kind;
mod phone;
mod request;
mod response;
pub mod socket;
mod state;
pub mod wire;

pub use client::{Calls, Client, ConnectError, DaemonError, Events};
pub use describe::bytes;
pub use doctor::{Check, CheckStatus};
pub use event::Event;
pub use kind::Kind;
pub use phone::{Phone, label};
pub use request::{DnsChoice, Request, StartRequest};
pub use response::{
    ConnectionInfo, DeviceInfo, InterfaceInfo, Ipv4Net, Lease, NetworkInfo, Response,
};
pub use state::{ConnectionState, EndReason, Outcome, Traffic};
pub use wire::{ClientMessage, ServerMessage};

/// Bumped on any incompatible change to the messages of this crate.
/// 2: a phone's connectivity is a *device connection* on the wire.
/// 3: tagged envelope, `Outcome` as clean-or-failed, a response per request,
///    optional `phone_ip` (DHCP), DNS choice, traffic bytes and drops,
///    the phone's network, `interfaces` and `doctor`, remembered phones
///    (names, auto-connect) and `start` without a serial or LAN.
pub const API_VERSION: u32 = 3;

#[cfg(test)]
mod golden;
