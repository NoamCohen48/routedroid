//! Routedroid wire protocol, version 1 (`protocol/version-1.md`).
//!
//! Pure, allocation-conscious building blocks shared by the host CLI and its
//! tests. Nothing here does I/O except the optional tokio frame reader.
//!
//! - [`frame`]: 8-byte header, limits, encode / decode.
//! - [`messages`]: JSON control bodies and their field rules.
//! - [`state`]: the per-state allowlist of received message types.
//! - [`auth`]: transcript, proofs, constant-time verification.
//! - [`bootstrap`]: the 80-byte record streamed to the app's provider.
//! - [`ipv4`]: the four header checks applied before injection.
//!
//! Every module is exercised against `protocol/fixtures/` in its tests.

pub mod auth;
pub mod bootstrap;
pub mod frame;
pub mod ipv4;
pub mod messages;
pub mod state;

/// Frame header `version` and HELLO / HELLO_ACK `protocol`.
pub const PROTOCOL_VERSION: u8 = 1;

#[cfg(test)]
pub(crate) mod fixtures {
    //! The checked-in golden fixtures, embedded at compile time so the tests
    //! do not depend on the working directory.
    pub const FRAMES: &str = include_str!("../../../protocol/fixtures/frames.json");
    pub const AUTH: &str = include_str!("../../../protocol/fixtures/auth.json");
    pub const BOOTSTRAP: &str = include_str!("../../../protocol/fixtures/bootstrap.json");
    pub const STATES: &str = include_str!("../../../protocol/fixtures/states.json");

    pub fn unhex(s: &str) -> Vec<u8> {
        hex::decode(s).expect("fixture hex")
    }
}
