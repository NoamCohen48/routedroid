//! The host's own nftables chains that may undo the session firewall's
//! accept: in nftables every base chain on a hook sees the packet, so a
//! forward chain with policy drop (firewalld, ufw over iptables-nft) drops
//! phone traffic Routedroid's chain accepted. `doctor` reports them.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::command;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Base chains on the forward hook whose policy is drop, other than
/// Routedroid's: each may drop phone traffic our accept already passed.
pub fn forward_drops(nft: &Path) -> Result<Vec<String>> {
    parse(&command::run(
        nft,
        &["-j", "list", "chains"],
        None,
        TIMEOUT,
    )?)
}

fn parse(json: &str) -> Result<Vec<String>> {
    let listing: Chains = serde_json::from_str(json).context("parse `nft -j list chains`")?;
    Ok(listing
        .nftables
        .into_iter()
        .filter_map(|item| item.chain)
        .filter(|c| c.hook.as_deref() == Some("forward") && c.policy.as_deref() == Some("drop"))
        .filter(|c| !c.table.starts_with("routedroid_"))
        .map(|c| format!("{} {} chain {}", c.family, c.table, c.name))
        .collect())
}

#[derive(Deserialize)]
struct Chains {
    nftables: Vec<ChainItem>,
}

#[derive(Deserialize)]
struct ChainItem {
    chain: Option<Chain>,
}

#[derive(Deserialize)]
struct Chain {
    family: String,
    table: String,
    name: String,
    hook: Option<String>,
    policy: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_forward_chains_that_drop_by_default_except_ours() {
        let json = r#"{"nftables": [{"metainfo": {"version": "1.1.7"}},
            {"chain": {"family": "inet", "table": "firewalld", "name": "filter_FORWARD", "handle": 3, "type": "filter", "hook": "forward", "prio": 10, "policy": "drop"}},
            {"chain": {"family": "ip", "table": "filter", "name": "FORWARD", "handle": 1, "type": "filter", "hook": "forward", "prio": 0, "policy": "accept"}},
            {"chain": {"family": "inet", "table": "routedroid_phone0", "name": "forward", "handle": 2, "hook": "forward", "policy": "drop"}},
            {"chain": {"family": "inet", "table": "firewalld", "name": "filter_FWD_public", "handle": 9}}]}"#;
        assert_eq!(
            parse(json).unwrap(),
            ["inet firewalld chain filter_FORWARD"]
        );
        assert!(parse("").is_err());
    }
}
