//! Apply / inspect / undo for each journaled [`Op`]. Every external command
//! is `ip`, `nft` or a `/proc/sys` write with fully typed, validated
//! arguments; nothing here is built from a free-form string.

use std::net::Ipv4Addr;
use std::process::Command;

use anyhow::{bail, Context, Result};
use tracing::{info, warn};

use crate::claims;
use crate::journal::Op;

pub fn valid_ifname(name: &str) -> bool {
    routedroid_helper_ipc::IfName::new(name).is_ok()
}

fn run(bin: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(bin).args(args).output().with_context(|| format!("spawn {bin}"))?;
    if !out.status.success() {
        bail!("{bin} {} failed ({}): {}", args.join(" "), out.status, String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn sysctl_path(key: &str) -> Result<String> {
    // Only keys under net.ipv4.conf.<if>.<leaf> and net.ipv4.ip_forward are allowed.
    let ok = key == "net.ipv4.ip_forward"
        || key
            .strip_prefix("net.ipv4.conf.")
            .and_then(|rest| rest.rsplit_once('.'))
            .map(|(ifname, leaf)| valid_ifname(ifname) && matches!(leaf, "forwarding" | "proxy_arp" | "rp_filter"))
            .unwrap_or(false);
    if !ok {
        bail!("sysctl key {key} not allowed");
    }
    // Interface names may contain '.', which /proc spells as '/'.
    let rest = key.strip_prefix("net.ipv4.").unwrap_or(key);
    let path = if let Some(r) = rest.strip_prefix("conf.") {
        let (ifname, leaf) = r.rsplit_once('.').unwrap();
        format!("/proc/sys/net/ipv4/conf/{}/{}", ifname.replace('.', "/"), leaf)
    } else {
        format!("/proc/sys/net/ipv4/{rest}")
    };
    Ok(path)
}

pub fn sysctl_read(key: &str) -> Result<String> {
    let p = sysctl_path(key)?;
    Ok(std::fs::read_to_string(&p).with_context(|| format!("read {p}"))?.trim().to_string())
}

pub fn sysctl_write(key: &str, value: &str) -> Result<()> {
    if !matches!(value, "0" | "1" | "2") {
        bail!("sysctl value {value:?} not allowed");
    }
    let p = sysctl_path(key)?;
    std::fs::write(&p, value).with_context(|| format!("write {p}"))
}

/// `(host address, prefix length)` of the first IPv4 address on `ifname`.
pub fn primary_ipv4(ifname: &str) -> Result<(Ipv4Addr, u8)> {
    let out = run("ip", &["-4", "-o", "addr", "show", "dev", ifname])?;
    for line in out.lines() {
        let mut it = line.split_whitespace();
        while let Some(tok) = it.next() {
            if tok == "inet" {
                let cidr = it.next().unwrap_or("");
                let (a, p) = cidr.split_once('/').unwrap_or((cidr, "32"));
                return Ok((a.parse().context("parse address")?, p.parse().context("parse prefix")?));
            }
        }
    }
    bail!("{ifname} has no IPv4 address")
}

/// Via iproute2 rather than /sys/class/net: sysfs is not re-mounted per network
/// namespace, so the sysfs view would be wrong inside the userns lab.
pub fn link_exists(name: &str) -> bool {
    valid_ifname(name)
        && Command::new("ip")
            .args(["-o", "link", "show", "dev", name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

/// Another session (ours or not) already routes this host address somewhere.
pub fn host_route_exists(dst: Ipv4Addr) -> bool {
    run("ip", &["-4", "route", "show", &format!("{dst}/32")]).map(|o| !o.trim().is_empty()).unwrap_or(false)
}

pub fn route_exists(dst: Ipv4Addr, dev: &str) -> bool {
    run("ip", &["-4", "route", "show", &format!("{dst}/32"), "dev", dev]).map(|o| !o.trim().is_empty()).unwrap_or(false)
}

/// Per-session table so sessions on the same LAN interface never share rules.
pub fn nft_table_name(tun: &str) -> String {
    format!("routedroid_{tun}")
}

fn valid_nft_table(family: &str, name: &str) -> bool {
    family == "inet" && name.strip_prefix("routedroid_").is_some_and(valid_ifname)
}

pub fn nft_table_exists(family: &str, name: &str) -> bool {
    Command::new("nft").args(["list", "table", family, name]).output().map(|o| o.status.success()).unwrap_or(false)
}

/// Deny-first session table (design §11): only phone<->LAN forwarding and
/// phone<->host traffic for exactly one address; everything else on the TUN drops.
pub fn nft_rules(table: &str, tun: &str, lan_if: &str, phone_ip: Ipv4Addr, host_ip: Ipv4Addr) -> String {
    format!(
        r#"table inet {table} {{
    chain raw_prerouting {{
        type filter hook prerouting priority -300; policy accept;
        iifname "{tun}" ip saddr != {phone_ip} counter drop
    }}
    chain input {{
        type filter hook input priority -10; policy accept;
        iifname "{tun}" ip saddr {phone_ip} ip daddr {host_ip} counter accept
        iifname "{tun}" counter drop
    }}
    chain forward {{
        type filter hook forward priority -10; policy accept;
        iifname "{tun}" oifname "{lan_if}" ip saddr {phone_ip} counter accept
        iifname "{lan_if}" oifname "{tun}" ip daddr {phone_ip} counter accept
        iifname "{tun}" counter drop
        oifname "{tun}" counter drop
    }}
    chain postrouting {{
        type filter hook postrouting priority -10; policy accept;
        oifname "{tun}" ip daddr != {phone_ip} counter drop
    }}
    chain output {{
        type filter hook output priority -10; policy accept;
        oifname "{tun}" ip daddr {phone_ip} counter accept
        oifname "{tun}" meta nfproto ipv4 counter drop
    }}
}}
"#
    )
}

/// Apply one op on behalf of `session`. `nft_ruleset` is needed only for `NftTable`.
pub fn apply(session: &str, op: &Op, nft_ruleset: Option<&str>) -> Result<()> {
    match op {
        Op::Tun { .. } => Ok(()), // created by the caller (owns the fd); journaled for inspection
        // Shared keys (the LAN interface's) are reference-counted across sessions.
        Op::Sysctl { key, new, .. } => claims::acquire(session, key, new).map(|_| ()),
        Op::Route { dst, dev, src } => {
            run("ip", &["-4", "route", "replace", &format!("{dst}/32"), "dev", dev, "src", &src.to_string()])
                .map(|_| ())
        }
        Op::NftTable { family, name } => {
            let rules = nft_ruleset.context("nft ruleset missing")?;
            if !valid_nft_table(family, name) {
                bail!("bad nft table id");
            }
            let mut child = Command::new("nft")
                .args(["-f", "-"])
                .stdin(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .context("spawn nft")?;
            use std::io::Write;
            child.stdin.take().unwrap().write_all(rules.as_bytes())?;
            let out = child.wait_with_output()?;
            if !out.status.success() {
                bail!("nft -f failed: {}", String::from_utf8_lossy(&out.stderr).trim());
            }
            Ok(())
        }
    }
}

/// Undo one op, idempotently: inspect first, act only if the effect is present.
pub fn undo(session: &str, op: &Op) -> Result<()> {
    match op {
        Op::Tun { name } => {
            if link_exists(name) {
                warn!(tun = %name, "TUN still present after owner exit; deleting");
                run("ip", &["link", "del", "dev", name]).map(|_| ())
            } else {
                Ok(())
            }
        }
        // The last holder restores the baseline; a vanished interface needs nothing.
        Op::Sysctl { key, .. } => claims::release(session, key),
        Op::Route { dst, dev, .. } => {
            if route_exists(*dst, dev) {
                run("ip", &["-4", "route", "del", &format!("{dst}/32"), "dev", dev]).map(|_| ())
            } else {
                Ok(())
            }
        }
        Op::NftTable { family, name } => {
            if nft_table_exists(family, name) {
                run("nft", &["delete", "table", family, name]).map(|_| ())
            } else {
                Ok(())
            }
        }
    }
}

/// Whether the op's effect is currently present (in the kernel, or for a
/// sysctl: whether this session still holds its claim on the key).
pub fn present(session: &str, op: &Op) -> Result<bool> {
    Ok(match op {
        Op::Tun { name } => link_exists(name),
        Op::Sysctl { key, .. } => claims::holds(session, key),
        Op::Route { dst, dev, .. } => route_exists(*dst, dev),
        Op::NftTable { family, name } => nft_table_exists(family, name),
    })
}

pub fn log_undo(op: &Op, r: &Result<()>) {
    match r {
        Ok(()) => info!(op = %op.label(), "undone"),
        Err(e) => warn!(op = %op.label(), error = %e, "undo failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sysctl_keys_are_whitelisted() {
        assert_eq!(sysctl_path("net.ipv4.ip_forward").unwrap(), "/proc/sys/net/ipv4/ip_forward");
        assert_eq!(sysctl_path("net.ipv4.conf.eno1.proxy_arp").unwrap(), "/proc/sys/net/ipv4/conf/eno1/proxy_arp");
        assert_eq!(
            sysctl_path("net.ipv4.conf.eth0.100.forwarding").unwrap(),
            "/proc/sys/net/ipv4/conf/eth0/100/forwarding"
        );
        assert!(sysctl_path("net.ipv4.conf.all.forwarding").is_err());
        assert!(sysctl_path("net.ipv4.conf.eno1.accept_redirects").is_err());
        assert!(sysctl_path("kernel.core_pattern").is_err());
        assert!(sysctl_path("net.ipv4.conf.../forwarding").is_err());
        assert!(sysctl_write("net.ipv4.ip_forward", "7").is_err());
    }

    #[test]
    fn nft_tables_are_ours_and_per_session() {
        assert!(valid_nft_table("inet", "routedroid_phone0"));
        assert!(!valid_nft_table("ip", "routedroid_phone0"));
        assert!(!valid_nft_table("inet", "filter"));
        assert!(!valid_nft_table("inet", "routedroid_"));
    }

    #[test]
    fn nft_rules_mention_only_the_session() {
        let table = nft_table_name("phone0");
        let r = nft_rules(&table, "phone0", "eno1", "10.0.0.5".parse().unwrap(), "10.0.0.2".parse().unwrap());
        assert!(r.contains("table inet routedroid_phone0"));
        assert_eq!(r.matches("10.0.0.5").count(), 6);
        assert!(r.contains("iifname \"phone0\" ip saddr != 10.0.0.5 counter drop"));
    }
}
