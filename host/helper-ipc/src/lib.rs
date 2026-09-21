//! IPC between the unprivileged controller and the privileged helper
//! (architecture §5.3). Shared by both binaries so the wire cannot drift.

pub mod proto;
pub mod seqpacket;
