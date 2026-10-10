//! The host's own nftables chains that may undo the session firewall's
//! accept: in nftables every base chain on a hook sees the packet, so a
//! forward chain with policy drop (firewalld, ufw over iptables-nft) drops
//! phone traffic Routedroid's chain accepted. `doctor` reports them, unless
//! their table already accepts `phone*` both ways (`ufw route allow in on
//! phone+`, or the equivalent rules by hand).

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use super::command;
use crate::kernel::{ForwardDrop, HostFirewall};

const TIMEOUT: Duration = Duration::from_secs(10);

pub fn forward_drops(nft: &Path) -> Result<Vec<ForwardDrop>> {
    parse(&command::run(
        nft,
        &["-j", "list", "ruleset"],
        None,
        TIMEOUT,
    )?)
}

fn parse(json: &str) -> Result<Vec<ForwardDrop>> {
    let listing: Listing = serde_json::from_str(json).context("parse `nft -j list ruleset`")?;
    let mut chains = Vec::new();
    // (family, table) of every chain name, and of each accept of phone* by direction.
    let (mut names, mut passes) = (BTreeSet::new(), BTreeSet::new());
    for item in listing.nftables {
        if let Some(chain) = item.chain {
            names.insert((
                chain.family.clone(),
                chain.table.clone(),
                chain.name.clone(),
            ));
            chains.push(chain);
        } else if let Some(rule) = item.rule
            && let Some(key) = rule.accepts_phones()
        {
            passes.insert((rule.family.clone(), rule.table.clone(), key.to_string()));
        }
    }
    let passed = |c: &Chain| {
        ["iifname", "oifname"]
            .iter()
            .all(|key| passes.contains(&(c.family.clone(), c.table.clone(), key.to_string())))
    };
    let ufw = |c: &Chain| {
        names
            .iter()
            .any(|(f, t, n)| f == &c.family && t == &c.table && n.starts_with("ufw"))
    };
    Ok(chains
        .iter()
        // Phones are IPv4-only: an ip6 table never sees their traffic.
        .filter(|c| c.family == "ip" || c.family == "inet")
        .filter(|c| c.hook.as_deref() == Some("forward") && c.policy.as_deref() == Some("drop"))
        .filter(|c| !c.table.starts_with("routedroid_") && !passed(c))
        .map(|c| ForwardDrop {
            family: c.family.clone(),
            table: c.table.clone(),
            chain: c.name.clone(),
            firewall: match () {
                _ if c.table == "firewalld" => HostFirewall::Firewalld,
                _ if ufw(c) => HostFirewall::Ufw,
                _ if c.family == "ip" && c.table == "filter" => HostFirewall::Iptables,
                _ => HostFirewall::Nftables,
            },
        })
        .collect())
}

#[derive(Deserialize)]
struct Listing {
    nftables: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    chain: Option<Chain>,
    rule: Option<NftRule>,
}

#[derive(Deserialize)]
struct Chain {
    family: String,
    table: String,
    name: String,
    hook: Option<String>,
    policy: Option<String>,
}

#[derive(Deserialize)]
struct NftRule {
    family: String,
    table: String,
    expr: Vec<Value>,
}

impl NftRule {
    /// "iifname"/"oifname" if this rule accepts every packet whose interface
    /// matches `phone*` (as iptables' `phone+` is listed), and nothing else.
    fn accepts_phones(&self) -> Option<&str> {
        let accepts = self.expr.iter().any(|e| e.get("accept").is_some());
        let matches: Vec<&Value> = self.expr.iter().filter_map(|e| e.get("match")).collect();
        let [only] = matches.as_slice() else {
            return None;
        };
        let key = only["left"]["meta"]["key"].as_str()?;
        let phones = only["op"] == "==" && only["right"] == "phone*";
        (accepts && phones && (key == "iifname" || key == "oifname")).then_some(key)
    }
}

#[cfg(test)]
mod tests;
