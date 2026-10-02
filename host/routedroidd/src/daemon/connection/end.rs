//! How a protocol session's end reads to the operator: a clean reason, or a
//! failure of the kind that explains it.

use routedroid_ipc::EndReason;
use routedroid_proto::messages::ErrorCode;

use crate::fault::{Fault, Kind, Result};
use crate::session::SessionEnd;

pub fn reason(end: SessionEnd, reached_active: bool) -> Result<EndReason> {
    let fail = |kind, message: String| Err(Fault::msg(kind, message));
    match end {
        // A stop we asked for is a success whatever phase it interrupted.
        SessionEnd::LocalStop if reached_active => Ok(EndReason::Stopped),
        SessionEnd::LocalStop => Ok(EndReason::StoppedEarly),
        SessionEnd::PeerStop if reached_active => Ok(EndReason::PhoneStopped),
        SessionEnd::PeerClosed if reached_active => Ok(EndReason::PhoneClosed),
        SessionEnd::PeerStop | SessionEnd::PeerClosed => fail(
            Kind::Vpn,
            "the app gave up before the VPN was up (was the VPN dialog declined?)".into(),
        ),
        SessionEnd::VpnError(e) | SessionEnd::Refused(e)
            if e.known_code() == Some(ErrorCode::ConsentTimeout) =>
        {
            fail(
                Kind::Vpn,
                "VPN permission was not granted on the phone within 2 minutes".into(),
            )
        }
        // Peer-supplied text: `{:?}` escapes control characters before it reaches a terminal.
        SessionEnd::VpnError(e) => fail(Kind::Vpn, format!("{}: {:?}", e.code, e.message)),
        SessionEnd::Refused(e) if e.known_code() == Some(ErrorCode::AuthFailed) => {
            fail(Kind::Auth, e.message)
        }
        SessionEnd::Refused(e) => fail(Kind::Protocol, format!("{}: {:?}", e.code, e.message)),
        SessionEnd::KeepaliveTimeout => fail(
            Kind::Timeout,
            "the phone stopped answering (no frame for 30 s)".into(),
        ),
        SessionEnd::Transport(e) => fail(Kind::Adb, format!("the adb connection broke: {e}")),
        SessionEnd::HelperClosed => {
            fail(Kind::Helper, "the helper closed the packet channel".into())
        }
    }
}

#[cfg(test)]
mod tests;
