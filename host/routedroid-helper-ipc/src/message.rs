//! Control messages. The controller opens with `Hello`, then sends one
//! `Start`; the session lives until `Stop` or until either side closes
//! the connection. `Interfaces` may come before `Start`, any number of
//! times: it only reads. During a leased session the helper sends `Lease`
//! after each renewal, unasked.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::{DeviceId, Finding, IfName, Interface};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Mandatory first message; answered with `Reply::Hello` when the
    /// versions match and with `ErrorCode::VersionMismatch` otherwise.
    Hello {
        version: u32,
    },
    /// Bring up one phone. The helper derives the host address and LAN
    /// prefix from `lan_if` itself; the controller cannot pick them.
    /// Without `phone_ip` the helper leases one from the LAN's DHCP server
    /// under an identity derived from `device` and `lan_if`'s MAC.
    Start {
        lan_if: IfName,
        phone_ip: Option<Ipv4Addr>,
        device: DeviceId,
        tun: IfName,
        mtu: u32,
    },
    /// Undo everything and reply `Stopped`; the connection then ends.
    Stop,
    Ping,
    /// List the host's links and where phones may join; answered with
    /// `Reply::Interfaces`.
    Interfaces,
    /// What Routedroid left behind and what on the host gets in its way,
    /// with the changes `Repair` would make; changes nothing. Answered with
    /// `Reply::Health`.
    Inspect,
    /// Make those changes; answered with `Reply::Repaired`.
    Repair,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    Hello {
        version: u32,
    },
    Started {
        session: String,
        tun: IfName,
        /// The requested address, or the leased one.
        phone_ip: Ipv4Addr,
        host_ip: Ipv4Addr,
        lan_prefix: u8,
        /// `None` for a requested address.
        lease: Option<Lease>,
    },
    /// The lease was renewed; only during a leased session.
    Lease {
        lease: Lease,
    },
    Stopped,
    Pong,
    Interfaces {
        interfaces: Vec<Interface>,
    },
    Health {
        findings: Vec<Finding>,
    },
    Repaired {
        /// The changes made, in order.
        done: Vec<String>,
        /// What is still wrong afterwards.
        remaining: Vec<Finding>,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
}

/// What the LAN's DHCP server granted the phone, beyond its address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lease {
    pub server: Ipv4Addr,
    pub router: Option<Ipv4Addr>,
    pub dns: Vec<Ipv4Addr>,
    /// Unix seconds when it runs out unless renewed.
    pub expires_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Not a control message this version understands.
    BadRequest,
    /// The controller speaks another IPC version.
    VersionMismatch,
    /// A valid message at the wrong time (e.g. a second `Start`).
    OutOfState,
    /// `Start` was refused before touching the kernel: policy or validation.
    Refused,
    /// Applying the session failed; whatever was applied has been undone.
    StartFailed,
    /// No usable lease: no server answered, or every address offered was
    /// in use.
    NoLease,
    /// Some undo step failed; the journal is left for cleanup.
    StopFailed,
    /// The session ended on the helper's side (the phone's address was lost,
    /// the TUN failed) and has been undone. Unsolicited.
    SessionEnded,
    /// `Interfaces`, `Inspect` or `Repair` could not be answered.
    QueryFailed,
}

#[cfg(test)]
mod tests;
