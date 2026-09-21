//! Host-side session: a pure state machine (`machine.rs`) fed validated
//! frames, and an async driver (`driver.rs`) that connects it to the phone's
//! TCP stream and the helper's packet channel.

mod close;
mod config;
mod driver;
#[cfg(test)]
mod driver_tests;
mod machine;
mod tasks;
#[cfg(test)]
mod tests;

pub use close::{Close, SessionEnd};
pub use config::SessionConfig;
pub use driver::{run_session, PacketEndpoints};
pub use machine::{Machine, Outbound};
pub use tasks::QUEUE_DEPTH;
