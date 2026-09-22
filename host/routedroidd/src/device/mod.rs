//! The phone side of a connection: everything the host does *to the device*
//! through adb — reserving the reverse port, delivering the secret, launching
//! the app — and undoing it. `adb::Adb` is the executable; this is the
//! policy, including which transports version 1 accepts.

mod bridge;
mod ports;
#[cfg(test)]
mod tests;
mod transport;

pub use bridge::AdbBridge;
pub use ports::{DevicePorts, ReservedPort};
pub use transport::Transport;
