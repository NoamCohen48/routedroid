//! Routedroid's DHCP alias client: an ADDITIONAL lease on a physical
//! interface for a phone identity (option 61), over a raw `AF_PACKET`
//! socket, never assigned to the interface and never touching the PC's own
//! lease (architecture §6). The helper leases phone addresses through it;
//! the `routedroid-dhcp` binary drives it by hand for lab work.
//!
//! - [`Client`]: one identity on one interface; acquire, maintain, release.
//! - [`Link`]: the frame I/O under it, and ARP: probe, announce, conflicts.
//! - [`Lease`]: what a server granted, durable on disk; [`Schedule`]: when
//!   to act on it, on the monotonic clock.
//! - [`ClientId`]: the only identities sent, `routedroid:<device>:<mac>`.

mod client;
pub mod dhcp;
mod identity;
mod lease;
mod link;
pub mod packet;
mod random;
mod sock;

pub use client::{Acquire, Bound, Client, Event, Lost, Outcome, mask_prefix, release_now};
pub use identity::{BadClientId, ClientId};
pub use lease::{Held, Lease, Schedule, unix_now};
pub use link::{ArpMode, Link, PROBE, Probe};
pub use sock::Iface;

#[cfg(test)]
mod golden;
