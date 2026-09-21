//! Host-side session: a pure state machine (`machine.rs`) fed validated
//! frames, and an async driver (`driver.rs`) that connects it to the phone's
//! TCP stream and the helper's packet channel.

mod close;
mod config;
mod driver;
#[cfg(test)]
mod driver_tests;
mod handlers;
mod machine;
mod progress;
mod tasks;
#[cfg(test)]
mod tests;
mod timers;

pub use close::{Close, SessionEnd};
pub use config::SessionConfig;
pub use driver::{PacketEndpoints, SessionDriver};
pub use machine::{Machine, Outbound};
pub use progress::{Counters, Progress};
pub use tasks::QUEUE_DEPTH;
