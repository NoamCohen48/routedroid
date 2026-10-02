//! The fuzz targets' invariants (host/fuzz), run on stable over seeded noise
//! and single-byte mutations of valid inputs: no parser panics, and what
//! decodes re-encodes to the same bytes.

use crate::auth::Secret;
use crate::frame::{self, Frame, MessageType};
use crate::messages::{self, Auth, ConfigureVpn, ErrorBody, Hello, HelloAck, VpnReady};
use crate::{bootstrap, ipv4};

/// xorshift64: reproducible, and no dependency for it.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn byte(&mut self) -> u8 {
        self.next().to_le_bytes()[0]
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).unwrap()
    }

    fn bytes(&mut self, max: usize) -> Vec<u8> {
        let len = self.below(max + 1);
        (0..len).map(|_| self.byte()).collect()
    }

    /// `seed` with one byte changed, sometimes cut short.
    fn mutate(&mut self, seed: &[u8]) -> Vec<u8> {
        let mut out = seed.to_vec();
        let at = self.below(out.len());
        out[at] = self.byte();
        if self.below(4) == 0 {
            out.truncate(self.below(out.len() + 1));
        }
        out
    }
}

fn all_parsers(data: &[u8]) {
    if let Ok((frame, used)) = frame::decode(data, 1500) {
        assert_eq!(frame.encode(), data[..used]);
    }
    let _ = messages::parse::<Hello>(data);
    let _ = messages::parse::<HelloAck>(data);
    let _ = messages::parse::<Auth>(data);
    let _ = messages::parse::<ConfigureVpn>(data);
    let _ = messages::parse::<VpnReady>(data);
    let _ = messages::parse::<ErrorBody>(data);
    let _ = ipv4::check(data);
    if let Ok(record) = bootstrap::decode(data) {
        let again = bootstrap::encode(&record.session, record.device_port, &record.secret)
            .expect("a decoded record re-encodes");
        assert_eq!(again.as_slice(), data);
    }
}

#[test]
fn noise_never_panics_a_parser() {
    let mut noise = Noise(0x5eed_1234_abcd_ef01);
    for _ in 0..20_000 {
        all_parsers(&noise.bytes(160));
    }
}

#[test]
fn mutations_of_valid_inputs_never_panic_a_parser() {
    let record = bootstrap::encode("s1.abc", 23456, &Secret::new([9; 32])).unwrap();
    let hello = br#"{"protocol":1,"session":"s1","device_port":9000,"client_nonce":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
    let configure = br#"{"mtu":1400,"addresses":[{"address":"10.0.0.2","prefix":32}],"routes":[{"address":"0.0.0.0","prefix":0}],"dns":[],"session_name":"x"}"#;
    let seeds: Vec<Vec<u8>> = vec![
        record.to_vec(),
        hello.to_vec(),
        configure.to_vec(),
        Frame::json(
            MessageType::Hello,
            &serde_json::from_slice::<serde_json::Value>(hello).unwrap(),
        )
        .encode(),
        Frame::empty(MessageType::Stop).encode(),
        Frame::ip_packet(vec![
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 10, 0, 0, 2, 10, 0, 0, 1,
        ])
        .encode(),
    ];
    let mut noise = Noise(0xfeed_beef_0bad_cafe);
    for _ in 0..5_000 {
        for seed in &seeds {
            all_parsers(&noise.mutate(seed));
        }
    }
}
