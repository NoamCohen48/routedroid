//! Socket activation (the `sd_listen_fds` convention): systemd passes one
//! socket on fd 3 and describes it in `LISTEN_PID`/`LISTEN_FDS`.

use std::io;
use std::os::fd::{FromRawFd, OwnedFd};

use socket2::Socket;

use super::{Listener, SeqPacket};

const LISTEN_FDS_START: i32 = 3;
const LISTEN_VARS: [&str; 3] = ["LISTEN_PID", "LISTEN_FDS", "LISTEN_FDNAMES"];

/// The socket systemd handed over, not yet registered with tokio.
pub struct Activation {
    socket: Socket,
}

/// What the socket turned out to be.
pub enum Activated {
    /// `Accept=no`: the listening socket; we accept ourselves.
    Listener(Listener),
    /// `Accept=yes`: one already-accepted connection; one process per controller.
    Connection(SeqPacket),
}

impl Activation {
    /// Take the socket systemd passed, if any. Marks it close-on-exec and
    /// removes the `LISTEN_*` variables, so child processes neither inherit
    /// the controller's connection nor believe they were activated. Call
    /// it before any other thread exists: it changes the environment.
    pub fn take() -> io::Result<Option<Self>> {
        let Some(pid) = std::env::var_os("LISTEN_PID") else { return Ok(None) };
        let pid: u32 = pid.to_str().and_then(|pid| pid.parse().ok()).ok_or_else(|| invalid("LISTEN_PID"))?;
        if pid != std::process::id() {
            return Ok(None);
        }
        let fds = std::env::var("LISTEN_FDS").map_err(|_| invalid("LISTEN_FDS"))?;
        if fds != "1" {
            return Err(invalid(&format!("LISTEN_FDS={fds} (exactly one socket expected)")));
        }
        for var in LISTEN_VARS {
            std::env::remove_var(var);
        }
        // SAFETY: fd 3 was handed to this process by systemd (LISTEN_PID is
        // ours) and nothing else in the process has wrapped it.
        let socket = Socket::from(unsafe { OwnedFd::from_raw_fd(LISTEN_FDS_START) });
        socket.set_cloexec(true)?;
        Ok(Some(Self { socket }))
    }

    /// Register with tokio; needs a runtime.
    pub fn register(self) -> io::Result<Activated> {
        if rustix::net::sockopt::socket_acceptconn(&self.socket)? {
            Ok(Activated::Listener(Listener::from_socket(self.socket)?))
        } else {
            Ok(Activated::Connection(SeqPacket::from_socket(self.socket)?))
        }
    }
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, format!("bad socket activation: {what}"))
}
