//! Session states and the per-state allowlist of *received* message types
//! (§5). The allowlist is data, checked against `states.json`; the session
//! logic that drives transitions lives in the host crate.

use crate::frame::MessageType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    Connected,
    Authenticating,
    Negotiated,
    Configuring,
    Active,
    Closed,
}

impl State {
    pub const ALL: [State; 6] =
        [Self::Connected, Self::Authenticating, Self::Negotiated, Self::Configuring, Self::Active, Self::Closed];

    pub fn name(self) -> &'static str {
        match self {
            Self::Connected => "Connected",
            Self::Authenticating => "Authenticating",
            Self::Negotiated => "Negotiated",
            Self::Configuring => "Configuring",
            Self::Active => "Active",
            Self::Closed => "Closed",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|st| st.name() == s)
    }
}

pub use crate::Role;

/// Message types `role` may receive while in `state`.
pub fn allowed(role: Role, state: State) -> &'static [MessageType] {
    use MessageType as M;
    use State as S;
    match (role, state) {
        (Role::Host, S::Connected) => &[M::Hello, M::Stop],
        (Role::Host, S::Authenticating) => &[M::Auth, M::Stop],
        (Role::Host, S::Negotiated) => &[M::Stop],
        (Role::Host, S::Configuring) => &[M::VpnReady, M::VpnError, M::Stop],
        (Role::Host, S::Active) => &[M::IpPacket, M::Ping, M::Pong, M::Stop, M::VpnError],
        (Role::Android, S::Connected) => &[M::Stop],
        (Role::Android, S::Authenticating) => &[M::HelloAck, M::Error, M::Stop],
        (Role::Android, S::Negotiated) => &[M::ConfigureVpn, M::Error, M::Stop],
        (Role::Android, S::Configuring) => &[M::Error, M::Stop],
        (Role::Android, S::Active) => &[M::IpPacket, M::Ping, M::Pong, M::Stop, M::Error],
        (_, S::Closed) => &[],
    }
}

pub fn is_allowed(role: Role, state: State, t: MessageType) -> bool {
    allowed(role, state).contains(&t)
}

#[cfg(test)]
mod tests;
