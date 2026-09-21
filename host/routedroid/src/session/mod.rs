//! Host-side session: a pure state machine (`machine.rs`) fed validated
//! frames, and an async driver (`driver.rs`) that connects it to the phone's
//! TCP stream and the helper's packet channel.

mod close;
mod config;
mod driver;
mod machine;
#[cfg(test)]
mod tests;

pub use close::{Close, SessionEnd};
pub use config::SessionConfig;
pub use driver::{run_session, PacketEndpoints, QUEUE_DEPTH};
pub use machine::{Machine, Outbound};
