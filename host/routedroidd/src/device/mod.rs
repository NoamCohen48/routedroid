//! The phone side of a session: everything the host does *to the device*
//! through adb — reserving the reverse port, delivering the secret, launching
//! the app — and undoing it. `adb::Adb` is the executable; this is the
//! policy, including which transports version 1 accepts.

mod ports;
mod session;
#[cfg(test)]
mod tests;
mod transport;

pub use ports::{DevicePorts, ReservedPort};
pub use session::DeviceSession;
pub use transport::Transport;
