//! nftables through the `nft` binary: create the session table in one
//! transaction, find a table by name (with its handle and comment) via the
//! JSON listing, and delete by handle, which names exactly one table.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::{command, ruleset};
use crate::kernel::{Firewall, NftTable};

pub const CANDIDATES: &[&str] = &["/usr/sbin/nft", "/usr/bin/nft", "/sbin/nft"];
const TIMEOUT: Duration = Duration::from_secs(10);

pub fn create(nft: &Path, firewall: &Firewall) -> Result<()> {
    command::run(nft, &["-f", "-"], Some(&ruleset::render(firewall)), TIMEOUT).map(drop)
}

pub fn find(nft: &Path, name: &str) -> Result<Option<NftTable>> {
    let json = command::run(nft, &["-j", "list", "tables", "inet"], None, TIMEOUT)?;
    parse_tables(&json, name)
}

/// Every `inet` table, by name.
pub fn all(nft: &Path) -> Result<Vec<(String, NftTable)>> {
    let json = command::run(nft, &["-j", "list", "tables", "inet"], None, TIMEOUT)?;
    Ok(tables(&json)?
        .into_iter()
        .filter(|t| t.family == "inet")
        .map(|t| {
            let table = NftTable {
                handle: t.handle,
                comment: t.comment,
            };
            (t.name, table)
        })
        .collect())
}

pub fn delete(nft: &Path, handle: u64) -> Result<()> {
    command::run(
        nft,
        &["delete", "table", "inet", "handle", &handle.to_string()],
        None,
        TIMEOUT,
    )
    .map(drop)
}

#[derive(Deserialize)]
struct Listing {
    nftables: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    table: Option<Table>,
}

#[derive(Deserialize)]
struct Table {
    family: String,
    name: String,
    handle: u64,
    comment: Option<String>,
}

fn tables(json: &str) -> Result<Vec<Table>> {
    let listing: Listing = serde_json::from_str(json).context("parse `nft -j list tables`")?;
    Ok(listing
        .nftables
        .into_iter()
        .filter_map(|item| item.table)
        .collect())
}

fn parse_tables(json: &str, name: &str) -> Result<Option<NftTable>> {
    let mut found = tables(json)?
        .into_iter()
        .filter(|table| table.family == "inet" && table.name == name);
    let table = found.next();
    if found.next().is_some() {
        bail!("nft lists table inet {name} twice");
    }
    Ok(table.map(|t| NftTable {
        handle: t.handle,
        comment: t.comment,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = r#"{"nftables": [{"metainfo": {"version": "1.1.7", "json_schema_version": 1}},
        {"table": {"family": "inet", "name": "filter", "handle": 1}},
        {"table": {"family": "inet", "name": "routedroid_phone0", "handle": 7, "comment": "routedroid:00000000000000ab"}}]}"#;

    #[test]
    fn finds_by_name_with_handle_and_comment() {
        let table = parse_tables(LISTING, "routedroid_phone0").unwrap().unwrap();
        assert_eq!(
            table,
            NftTable {
                handle: 7,
                comment: Some("routedroid:00000000000000ab".into())
            }
        );
        assert_eq!(
            parse_tables(LISTING, "filter").unwrap().unwrap().comment,
            None
        );
        assert_eq!(parse_tables(LISTING, "routedroid_phone1").unwrap(), None);
    }

    #[test]
    fn lists_every_inet_table() {
        let names: Vec<_> = tables(LISTING)
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, ["filter", "routedroid_phone0"]);
    }

    #[test]
    fn unreadable_listings_are_errors_not_absence() {
        assert!(parse_tables("", "x").is_err());
        assert!(parse_tables(r#"{"tables": []}"#, "x").is_err());
    }
}
