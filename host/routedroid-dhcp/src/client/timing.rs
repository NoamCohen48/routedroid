//! Retransmission timing (RFC 2131 §4.1) and mask arithmetic.

use std::net::Ipv4Addr;
use std::time::Duration;

use anyhow::Result;

const RETRY_BASE: Duration = Duration::from_secs(4);
const RETRY_MAX: Duration = Duration::from_secs(64);
/// REQUEST retransmissions before falling back to DISCOVER.
pub const REQUEST_ATTEMPTS: u32 = 4;
/// Retry floor while RENEWING/REBINDING (the RFC says 60 s; a phone
/// session would rather notice a dead server sooner).
pub const RENEW_RETRY_MIN: Duration = Duration::from_secs(10);
/// How long one RENEW/REBIND/INIT-REBOOT transmission waits for its reply.
pub const RENEW_REPLY_WAIT: Duration = Duration::from_secs(5);
/// RFC 2131 §3.1.5: after a DECLINE, wait at least 10 s before DISCOVER.
pub const DECLINE_PAUSE: Duration = Duration::from_secs(10);

/// 4, 8, 16, 32, 64 s, plus up to 1 s of jitter. RFC 2131 says ±1 s; never
/// shortening keeps a server's 3 s ping check (dnsmasq's) inside one attempt.
pub fn retry_delay(attempt: u32) -> Result<Duration> {
    let base = RETRY_BASE
        .saturating_mul(1u32 << attempt.min(4))
        .min(RETRY_MAX);
    Ok(base + Duration::from_secs_f64(crate::random::unit()?))
}

pub fn classful_mask(a: Ipv4Addr) -> Ipv4Addr {
    match a.octets()[0] {
        0..128 => Ipv4Addr::new(255, 0, 0, 0),
        128..192 => Ipv4Addr::new(255, 255, 0, 0),
        _ => Ipv4Addr::new(255, 255, 255, 0),
    }
}

/// Prefix length of a contiguous mask, `None` if it has holes.
pub fn mask_prefix(mask: Ipv4Addr) -> Option<u8> {
    let m = u32::from(mask);
    let ones = m.count_ones();
    let contiguous = m == u32::MAX.checked_shl(32 - ones).unwrap_or(0);
    contiguous.then(|| u8::try_from(ones).expect("at most 32"))
}
