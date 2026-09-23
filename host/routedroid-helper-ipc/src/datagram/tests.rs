use super::*;
use crate::{Reply, Request};

#[test]
fn control_round_trips() {
    let bytes = encode_control(&Request::Ping);
    assert_eq!(bytes[0], KIND_CONTROL);
    assert_eq!(&bytes[1..], br#"{"type":"ping"}"#);
    assert_eq!(Datagram::<Request>::decode(&bytes).unwrap(), Datagram::Control(Request::Ping));
}

#[test]
fn packets_are_passed_through_untouched() {
    let bytes = [KIND_PACKET, 0x45, 0, 0, 20];
    assert_eq!(Datagram::<Reply>::decode(&bytes).unwrap(), Datagram::Packet(&bytes[1..]));
}

#[test]
fn malformed_datagrams_are_errors() {
    assert!(matches!(Datagram::<Request>::decode(&[]), Err(DecodeError::Empty)));
    assert!(matches!(Datagram::<Request>::decode(&[0x7f]), Err(DecodeError::UnknownKind(0x7f))));
    let bad = [&[KIND_CONTROL][..], br#"{"type":"nope"}"#].concat();
    assert!(matches!(Datagram::<Request>::decode(&bad), Err(DecodeError::Control(_))));
}
