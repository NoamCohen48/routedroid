//! Transaction ids and retry jitter. A predictable XID helps an attacker
//! inject a spoofed OFFER or ACK, so there is no fallback: no randomness,
//! no transaction.

use anyhow::{Context, Result};

pub fn xid() -> Result<u32> {
    getrandom::u32().context("getrandom")
}

/// Uniform in `[0, 1)`.
pub fn unit() -> Result<f64> {
    Ok(f64::from(xid()? >> 8) / f64::from(1u32 << 24))
}
