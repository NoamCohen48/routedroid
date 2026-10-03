//! A host forward chain that drops by default: in nftables every base chain
//! on a hook sees the packet, so it drops phone traffic Routedroid's own
//! chain accepted. Who manages it decides what a person should run.

/// Who manages the chain's table, so the advice uses its own tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFirewall {
    /// ufw over iptables-nft (the table holds `ufw-*` chains).
    Ufw,
    Firewalld,
    /// iptables-nft rules written by hand or another tool.
    Iptables,
    Nftables,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardDrop {
    pub family: String,
    pub table: String,
    pub chain: String,
    pub firewall: HostFirewall,
}

impl ForwardDrop {
    pub fn subject(&self) -> String {
        format!("nft {} {} chain {}", self.family, self.table, self.chain)
    }

    /// What lets the phones' traffic through, in the manager's own terms.
    /// Routedroid's table still limits each phone to its address and LAN.
    pub fn advice(&self) -> String {
        let Self {
            family,
            table,
            chain,
            ..
        } = self;
        match self.firewall {
            HostFirewall::Ufw => {
                "sudo ufw route allow in on phone+ && sudo ufw route allow out on phone+".into()
            }
            HostFirewall::Firewalld => "put the phoneN interfaces in a firewalld zone that \
                                        forwards to and from your LAN's zone"
                .into(),
            HostFirewall::Iptables => format!(
                "sudo iptables -I {chain} -i phone+ -j ACCEPT && \
                 sudo iptables -I {chain} -o phone+ -j ACCEPT"
            ),
            HostFirewall::Nftables => format!(
                "sudo nft insert rule {family} {table} {chain} iifname \"phone*\" accept && \
                 sudo nft insert rule {family} {table} {chain} oifname \"phone*\" accept"
            ),
        }
    }
}
