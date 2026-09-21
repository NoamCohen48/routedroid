use super::*;
use crate::fixtures::{unhex, BOOTSTRAP};

#[test]
fn fixture_vectors_encode_and_decode() {
    let f: serde_json::Value = serde_json::from_str(BOOTSTRAP).unwrap();
    assert_eq!(f["magic"].as_str().unwrap().as_bytes(), MAGIC);
    assert_eq!(f["length"].as_u64().unwrap() as usize, RECORD_LEN);
    assert_eq!(f["provider_uri"].as_str().unwrap(), PROVIDER_URI);
    for v in f["vectors"].as_array().unwrap() {
        let name = v["name"].as_str().unwrap();
        let session = v["session"].as_str().unwrap();
        let secret = Secret::from_hex(v["secret_hex"].as_str().unwrap()).unwrap();
        let want = unhex(v["record_hex"].as_str().unwrap());
        assert_eq!(encode(session, &secret).unwrap().as_slice(), want.as_slice(), "{name}: encode");
        let (s, k) = decode(&want).unwrap();
        assert_eq!(s, session, "{name}");
        assert_eq!(k, secret, "{name}");
    }
    for v in f["invalid"].as_array().unwrap() {
        let name = v["name"].as_str().unwrap();
        assert!(decode(&unhex(v["record_hex"].as_str().unwrap())).is_err(), "{name}");
    }
}

#[test]
fn invalid_session_is_refused_on_encode() {
    let secret = Secret::new([1; 32]);
    assert!(encode("", &secret).is_none());
    assert!(encode(&"s".repeat(41), &secret).is_none());
    assert!(encode("has space", &secret).is_none());
    assert!(encode(&"s".repeat(40), &secret).is_some());
}
