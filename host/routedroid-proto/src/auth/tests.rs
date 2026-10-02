use super::*;
use crate::fixtures::{unhex, AUTH};

#[test]
fn fixture_vectors() {
    let f: serde_json::Value = serde_json::from_str(AUTH).unwrap();
    assert_eq!(f["domain"].as_str().unwrap().as_bytes(), DOMAIN);
    let vectors = f["vectors"].as_array().unwrap();
    assert!(vectors.len() >= 3);
    for v in vectors {
        let name = v["name"].as_str().unwrap();
        let secret = Secret::from_hex(v["secret_hex"].as_str().unwrap()).unwrap();
        let cn = nonce_from_hex(v["client_nonce_hex"].as_str().unwrap()).unwrap();
        let hn = nonce_from_hex(v["host_nonce_hex"].as_str().unwrap()).unwrap();
        let session = v["session"].as_str().unwrap();
        assert!(
            crate::messages::valid_session(session),
            "{name}: session the wire could carry"
        );
        let t = transcript(session, v["device_port"].as_u64().unwrap() as u16, &cn, &hn);
        assert_eq!(
            t,
            unhex(v["transcript_hex"].as_str().unwrap()),
            "{name}: transcript"
        );
        let hp = proof_from_hex(v["host_proof_hex"].as_str().unwrap()).unwrap();
        let ap = proof_from_hex(v["android_proof_hex"].as_str().unwrap()).unwrap();
        assert_eq!(proof(&secret, Role::Host, &t), hp, "{name}: host proof");
        assert_eq!(
            proof(&secret, Role::Android, &t),
            ap,
            "{name}: android proof"
        );
        assert!(verify(&secret, Role::Host, &t, &hp));
        assert!(verify(&secret, Role::Android, &t, &ap));
        // Reflection: a host proof is never a valid android proof.
        assert!(
            !verify(&secret, Role::Android, &t, &hp),
            "{name}: reflection"
        );
        assert!(!verify(&secret, Role::Host, &t, &ap), "{name}: reflection");
    }
}

#[test]
fn proofs_bind_every_transcript_field() {
    let secret = Secret::new([7; 32]);
    let base = transcript("s1", 9000, &[1; 32], &[2; 32]);
    let p = proof(&secret, Role::Host, &base);
    for other in [
        transcript("s2", 9000, &[1; 32], &[2; 32]),
        transcript("s1", 9001, &[1; 32], &[2; 32]),
        transcript("s1", 9000, &[3; 32], &[2; 32]),
        transcript("s1", 9000, &[1; 32], &[3; 32]),
    ] {
        assert!(!verify(&secret, Role::Host, &other, &p));
    }
    assert!(!verify(&Secret::new([8; 32]), Role::Host, &base, &p));
}

#[test]
fn secret_is_redacted_and_hex_is_strict() {
    assert_eq!(format!("{:?}", Secret::new([0; 32])), "Secret(<redacted>)");
    assert!(Secret::from_hex(&"00".repeat(31)).is_none());
    assert!(Secret::from_hex(&"zz".repeat(32)).is_none());
    assert!(Secret::from_hex(&format!("{}\n", "ab".repeat(32))).is_some());
}
