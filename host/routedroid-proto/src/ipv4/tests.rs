use super::*;
use crate::fixtures::{FRAMES, unhex};

fn packets() -> Vec<(String, Vec<u8>)> {
    let f: serde_json::Value = serde_json::from_str(FRAMES).unwrap();
    f["valid"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["type"] == 0x10)
        .map(|v| {
            (
                v["name"].as_str().unwrap().to_string(),
                unhex(v["body_hex"].as_str().unwrap()),
            )
        })
        .collect()
}

#[test]
fn fixture_packets_pass() {
    let p = packets();
    assert!(p.len() >= 3);
    for (name, body) in p {
        check(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn each_rule_is_enforced() {
    let (_, good) = packets().remove(0);
    assert_eq!(check(&good[..19]), Err(PacketError::TooShort));
    let mut v6 = good.clone();
    v6[0] = 0x65;
    assert_eq!(check(&v6), Err(PacketError::Version(6)));
    let mut ihl = good.clone();
    ihl[0] = 0x44;
    assert_eq!(check(&ihl), Err(PacketError::Ihl(4)));
    let mut longer = good.clone();
    longer.push(0);
    assert!(matches!(
        check(&longer),
        Err(PacketError::TotalLength { .. })
    ));
    // IHL 15 (60-byte header) but total length 28.
    let mut overrun = good.clone();
    overrun[0] = 0x4f;
    assert_eq!(
        check(&overrun),
        Err(PacketError::HeaderOverrun {
            total: 28,
            header: 60
        })
    );
}
