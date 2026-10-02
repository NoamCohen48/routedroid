use std::net::Ipv4Addr;

use super::maintain::Lost;
use super::timing::{classful_mask, mask_prefix, retry_delay};

#[test]
fn mask_prefix_contiguous_only() {
    assert_eq!(mask_prefix(Ipv4Addr::new(255, 255, 255, 0)), Some(24));
    assert_eq!(mask_prefix(Ipv4Addr::new(255, 255, 255, 255)), Some(32));
    assert_eq!(mask_prefix(Ipv4Addr::new(0, 0, 0, 0)), Some(0));
    assert_eq!(mask_prefix(Ipv4Addr::new(255, 0, 255, 0)), None);
    assert_eq!(mask_prefix(Ipv4Addr::new(0, 0, 0, 255)), None);
}

#[test]
fn classful_defaults() {
    let mask = |a, b, c, d| classful_mask(Ipv4Addr::new(a, b, c, d));
    assert_eq!(mask(10, 1, 1, 1), Ipv4Addr::new(255, 0, 0, 0));
    assert_eq!(mask(172, 16, 0, 1), Ipv4Addr::new(255, 255, 0, 0));
    assert_eq!(mask(192, 168, 1, 1), Ipv4Addr::new(255, 255, 255, 0));
}

#[test]
fn retry_delay_doubles_to_64s_with_bounded_jitter() {
    for (attempt, base) in [
        (0, 4.0),
        (1, 8.0),
        (2, 16.0),
        (3, 32.0),
        (4, 64.0),
        (9, 64.0),
    ] {
        let d = retry_delay(attempt).unwrap().as_secs_f64();
        assert!((base..base + 1.0).contains(&d), "attempt {attempt}: {d}");
    }
}

#[test]
fn losses_say_why() {
    assert_eq!(
        Lost::Nak(String::new()).to_string(),
        "the server refused to renew it (NAK)"
    );
    assert_eq!(
        Lost::Nak("gone".into()).to_string(),
        "the server refused to renew it (NAK: gone)"
    );
    assert_eq!(
        Lost::Moved(Ipv4Addr::new(10, 0, 0, 9)).to_string(),
        "the server moved it to 10.0.0.9"
    );
    assert_eq!(
        Lost::Conflict([2, 0, 0, 0, 0, 1]).to_string(),
        "02:00:00:00:00:01 also uses the address"
    );
}
