//! Host-side session: a pure state machine (`machine.rs`) fed validated
//! frames; an async driver (`driver.rs`) for control, timers and shutdown;
//! and one pump per packet direction (`uplink.rs`, `downlink.rs`) between
//! the phone's TCP stream and the helper.

mod close;
mod config;
mod downlink;
mod driver;
#[cfg(test)]
mod driver_tests;
mod handlers;
mod machine;
mod progress;
#[cfg(test)]
mod tests;
mod timers;
mod uplink;
mod writer;

pub use close::{Close, SessionEnd};
pub use config::SessionConfig;
pub use driver::{PacketEndpoints, SessionDriver};
pub use machine::{Machine, Outbound};
pub use progress::{Counters, Progress};
pub use uplink::Inject;
pub use writer::QUEUE_DEPTH;
