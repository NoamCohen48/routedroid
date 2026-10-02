//! Control messages. The controller opens with `Hello`, then sends one
//! `Start`; the session lives until `Stop` or until either side closes
//! the connection. `Interfaces` may come before `Start`, any number of
//! times: it only reads.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::{IfName, Interface};

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
    Start {
        lan_if: IfName,
        phone_ip: Ipv4Addr,
        tun: IfName,
        mtu: u32,
    },
    /// Undo everything and reply `Stopped`; the connection then ends.
    Stop,
    Ping,
    /// List the host's links and where phones may join; answered with
    /// `Reply::Interfaces`.
    Interfaces,
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
        host_ip: Ipv4Addr,
        lan_prefix: u8,
    },
    Stopped,
    Pong,
    Interfaces {
        interfaces: Vec<Interface>,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
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
    /// Some undo step failed; the journal is left for cleanup.
    StopFailed,
    /// A read-only request (`Interfaces`) could not be answered.
    QueryFailed,
}

#[cfg(test)]
mod tests;
