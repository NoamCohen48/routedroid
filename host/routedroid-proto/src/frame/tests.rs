use super::*;
use crate::fixtures::{unhex, FRAMES};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    version: u8,
    mtu: u32,
    control_limit: u32,
    valid: Vec<Valid>,
    invalid: Vec<Invalid>,
}
#[derive(Deserialize)]
struct Valid {
    name: String,
    #[serde(rename = "type")]
    type_: u8,
    body_hex: String,
    wire_hex: String,
}
#[derive(Deserialize)]
struct Invalid {
    name: String,
    wire_hex: String,
    error: String,
}

fn fixture() -> Fixture {
    serde_json::from_str(FRAMES).unwrap()
}

#[test]
fn constants_match_fixture() {
    let f = fixture();
    assert_eq!(f.version, PROTOCOL_VERSION);
    assert_eq!(f.mtu, DEFAULT_MTU);
    assert_eq!(f.control_limit, MAX_CONTROL_BODY);
}

#[test]
fn valid_fixtures_encode_and_decode_exactly() {
    let f = fixture();
    assert!(f.valid.len() >= 16);
    for v in f.valid {
        let t = MessageType::from_u8(v.type_).unwrap_or_else(|| panic!("{}: type", v.name));
        let body = unhex(&v.body_hex);
        let wire = unhex(&v.wire_hex);
        assert_eq!(Frame::new(t, body.clone()).encode(), wire, "{}: encode", v.name);
        let (d, used) = decode(&wire, f.mtu).unwrap_or_else(|e| panic!("{}: {e}", v.name));
        assert_eq!(used, wire.len(), "{}: consumed", v.name);
        assert_eq!(d.message_type, t, "{}: type", v.name);
        assert_eq!(d.body, body, "{}: body", v.name);
    }
}

#[test]
fn invalid_fixtures_are_rejected_with_the_named_code() {
    let f = fixture();
    assert!(f.invalid.len() >= 20);
    for i in f.invalid {
        let wire = unhex(&i.wire_hex);
        let err = decode(&wire, f.mtu).expect_err(&i.name);
        assert_eq!(err.fixture_code(), i.error, "{}: {err}", i.name);
    }
}

#[test]
fn type_names_round_trip() {
    for t in MessageType::ALL {
        assert_eq!(MessageType::from_name(t.name()), Some(t));
        assert_eq!(MessageType::from_u8(t as u8), Some(t));
    }
    assert_eq!(MessageType::from_name("NOPE"), None);
}

#[test]
fn mtu_is_capped_at_absolute_ipv4_limit() {
    let h = RawHeader { body_length: 65_535, version: 1, message_type: 0x10, flags: 0 };
    assert_eq!(validate_header(&h, u32::MAX), Ok(MessageType::IpPacket));
    let h = RawHeader { body_length: 65_536, ..h };
    assert_eq!(
        validate_header(&h, u32::MAX),
        Err(FrameError::PacketBodyOutOfRange { body_length: 65_536, mtu: 65_535 })
    );
}

#[tokio::test]
async fn async_reader_never_allocates_for_hostile_length() {
    let f = fixture();
    let hostile = f.invalid.iter().find(|i| i.name == "control_hostile_length").unwrap();
    let mut cursor = std::io::Cursor::new(unhex(&hostile.wire_hex));
    let err = read_frame(&mut cursor, f.mtu).await.unwrap_err();
    assert!(matches!(err, FrameError::ControlBodyTooLarge { .. }));
    assert_eq!(cursor.position() as usize, HEADER_LEN);
}

#[tokio::test]
async fn async_reader_streams_all_valid_fixtures_then_reports_clean_eof() {
    let f = fixture();
    let mut stream = Vec::new();
    for v in &f.valid {
        stream.extend(unhex(&v.wire_hex));
    }
    let mut cursor = std::io::Cursor::new(stream);
    for v in &f.valid {
        let fr = read_frame(&mut cursor, f.mtu).await.unwrap();
        assert_eq!(fr.body, unhex(&v.body_hex), "{}", v.name);
    }
    assert_eq!(read_frame(&mut cursor, f.mtu).await.unwrap_err(), FrameError::Truncated { clean: true });
    let cut = unhex(&f.valid[0].wire_hex);
    let mut cut = std::io::Cursor::new(cut[..cut.len() - 1].to_vec());
    assert_eq!(read_frame(&mut cut, f.mtu).await.unwrap_err(), FrameError::Truncated { clean: false });
}
