//! The session firewall as one `nft -f` transaction (design §11). `create
//! table` fails if the table exists, so a leftover is never merged into;
//! the comment carries the owner tag. Deny-first: only phone<->LAN
//! forwarding and phone<->host traffic for exactly one address pass. DHCP
//! is the helper's business, never the phone's: a renewal ACK unicast to the
//! phone's address must not be forwarded to it, and the phone may neither
//! serve nor ask for leases.

use crate::kernel::Firewall;
use crate::op::nft_table_name;

pub fn render(fw: &Firewall) -> String {
    let Firewall {
        tun,
        lan_if,
        phone_ip,
        host_ip,
        tag,
    } = fw;
    let table = nft_table_name(tun);
    format!(
        r#"create table inet {table} {{ comment "{tag}"; }}
table inet {table} {{
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
        iifname "{lan_if}" oifname "{tun}" udp sport 67 udp dport 68 counter drop
        iifname "{tun}" udp sport 67 counter drop
        iifname "{tun}" udp dport 67 counter drop
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

#[cfg(test)]
mod tests {
    use routedroid_helper_ipc::IfName;

    use super::*;

    #[test]
    fn creates_a_tagged_table_that_mentions_only_the_session() {
        let fw = Firewall {
            tun: IfName::new("phone0").unwrap(),
            lan_if: IfName::new("eno1").unwrap(),
            phone_ip: "10.0.0.5".parse().unwrap(),
            host_ip: "10.0.0.2".parse().unwrap(),
            tag: "routedroid:00000000000000ab".into(),
        };
        let rules = render(&fw);
        assert!(rules.starts_with(
            "create table inet routedroid_phone0 { comment \"routedroid:00000000000000ab\"; }\n"
        ));
        assert_eq!(rules.matches("10.0.0.5").count(), 6);
        assert!(rules.contains("iifname \"phone0\" ip saddr != 10.0.0.5 counter drop"));
        let dhcp = rules
            .find("udp sport 67 udp dport 68 counter drop")
            .unwrap();
        let forward = rules
            .find("oifname \"phone0\" ip daddr 10.0.0.5 counter accept")
            .unwrap();
        assert!(
            dhcp < forward,
            "DHCP is dropped before forwarding is accepted"
        );
    }
}
