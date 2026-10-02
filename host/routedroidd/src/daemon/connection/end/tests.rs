use routedroid_proto::messages::ErrorBody;

use super::*;

fn error(code: ErrorCode) -> ErrorBody {
    ErrorBody::new(code, "why")
}

fn kind(end: SessionEnd, reached_active: bool) -> Kind {
    reason(end, reached_active).unwrap_err().kind()
}

#[test]
fn stops_are_clean_in_any_phase() {
    assert_eq!(
        reason(SessionEnd::LocalStop, false).unwrap(),
        EndReason::StoppedEarly
    );
    assert_eq!(
        reason(SessionEnd::LocalStop, true).unwrap(),
        EndReason::Stopped
    );
    assert_eq!(
        reason(SessionEnd::PeerStop, true).unwrap(),
        EndReason::PhoneStopped
    );
    assert_eq!(
        reason(SessionEnd::PeerClosed, true).unwrap(),
        EndReason::PhoneClosed
    );
}

#[test]
fn the_phone_leaving_before_active_is_a_failure() {
    assert_eq!(kind(SessionEnd::PeerStop, false), Kind::Vpn);
    assert_eq!(kind(SessionEnd::PeerClosed, false), Kind::Vpn);
}

#[test]
fn failures_have_the_kind_that_explains_them() {
    assert_eq!(
        kind(SessionEnd::Refused(error(ErrorCode::AuthFailed)), false),
        Kind::Auth
    );
    assert_eq!(
        kind(SessionEnd::Refused(error(ErrorCode::ProtocolError)), false),
        Kind::Protocol
    );
    assert_eq!(
        kind(SessionEnd::VpnError(error(ErrorCode::Internal)), true),
        Kind::Vpn
    );
    assert_eq!(kind(SessionEnd::KeepaliveTimeout, true), Kind::Timeout);
    assert_eq!(kind(SessionEnd::Transport("reset".into()), true), Kind::Adb);
    assert_eq!(kind(SessionEnd::HelperClosed, true), Kind::Helper);
}

#[test]
fn consent_timeout_says_what_to_do() {
    let fault = reason(
        SessionEnd::VpnError(error(ErrorCode::ConsentTimeout)),
        false,
    )
    .unwrap_err();
    assert_eq!(fault.kind(), Kind::Vpn);
    assert!(fault.to_string().contains("VPN permission"));
}

#[test]
fn peer_text_is_escaped() {
    let body = ErrorBody::new(ErrorCode::Internal, "bad\u{1b}[2J");
    let fault = reason(SessionEnd::VpnError(body), true).unwrap_err();
    assert!(!fault.to_string().contains('\u{1b}'), "{fault}");
}
