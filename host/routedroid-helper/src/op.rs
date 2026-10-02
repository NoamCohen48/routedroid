//! The reversible mutations a session makes, as the journal records them.
//! Each record carries everything needed to find and undo its effect
//! without consulting anything else; the owning session's tag (see
//! [`SessionId::tag`](crate::session_id::SessionId::tag)) makes the match exact.

use std::fmt;
use std::net::Ipv4Addr;

use routedroid_helper_ipc::IfName;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    /// The session's TUN, alias-tagged. It dies with the helper's fd, so undo
    /// only matters for a leftover (which a live fd holder would explain).
    Tun { name: IfName },
    /// Table `inet routedroid_<tun>`, comment-tagged: the session's firewall.
    NftTable { tun: IfName },
    /// A shared, reference-counted sysctl claim (see `claims`).
    Sysctl { ifname: IfName, leaf: Leaf },
    /// `dst/32 dev tun src src` with Routedroid's route protocol number.
    Route {
        dst: Ipv4Addr,
        tun: IfName,
        src: Ipv4Addr,
    },
}

impl Op {
    /// The stable name crash stages and logs use, e.g. `route:10.0.0.5/32@phone0`.
    pub fn label(&self) -> String {
        match self {
            Op::Tun { name } => format!("tun:{name}"),
            Op::NftTable { tun } => format!("nft:inet:{}", nft_table_name(tun)),
            Op::Sysctl { ifname, leaf } => {
                format!("sysctl:{}", SysctlKey::new(ifname.clone(), *leaf))
            }
            Op::Route { dst, tun, .. } => format!("route:{dst}/32@{tun}"),
        }
    }
}

/// Per-session table, so sessions on one LAN interface never share rules.
pub fn nft_table_name(tun: &IfName) -> String {
    format!("routedroid_{tun}")
}

/// The only per-interface IPv4 sysctls the helper ever writes. `all` and
/// `default` cannot be named: [`IfName`] refuses them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Leaf {
    Forwarding,
    ProxyArp,
}

impl Leaf {
    pub fn as_str(self) -> &'static str {
        match self {
            Leaf::Forwarding => "forwarding",
            Leaf::ProxyArp => "proxy_arp",
        }
    }
}

/// `net.ipv4.conf.<ifname>.<leaf>`, kept as its two typed halves so nothing
/// ever re-parses the dotted form (a VLAN `eth0.100` contains dots itself).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SysctlKey {
    pub ifname: IfName,
    pub leaf: Leaf,
}

impl SysctlKey {
    pub fn new(ifname: IfName, leaf: Leaf) -> Self {
        Self { ifname, leaf }
    }
}

impl fmt::Display for SysctlKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "net.ipv4.conf.{}.{}", self.ifname, self.leaf.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(s: &str) -> IfName {
        IfName::new(s).unwrap()
    }

    #[test]
    fn labels_name_the_kernel_object() {
        let route = Op::Route {
            dst: "10.0.0.5".parse().unwrap(),
            tun: name("phone0"),
            src: "10.0.0.2".parse().unwrap(),
        };
        assert_eq!(route.label(), "route:10.0.0.5/32@phone0");
        assert_eq!(
            Op::NftTable {
                tun: name("phone0")
            }
            .label(),
            "nft:inet:routedroid_phone0"
        );
        let vlan = Op::Sysctl {
            ifname: name("eth0.100"),
            leaf: Leaf::ProxyArp,
        };
        assert_eq!(vlan.label(), "sysctl:net.ipv4.conf.eth0.100.proxy_arp");
    }

    #[test]
    fn journal_form_is_strict() {
        let op = Op::Sysctl {
            ifname: name("lan0"),
            leaf: Leaf::Forwarding,
        };
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"sysctl","ifname":"lan0","leaf":"forwarding"}"#
        );
        assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), op);
        assert!(
            serde_json::from_str::<Op>(r#"{"kind":"sysctl","ifname":"all","leaf":"forwarding"}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<Op>(r#"{"kind":"sysctl","ifname":"lan0","leaf":"rp_filter"}"#)
                .is_err()
        );
        assert!(serde_json::from_str::<Op>(r#"{"kind":"tun","name":"phone0","extra":1}"#).is_err());
    }
}
