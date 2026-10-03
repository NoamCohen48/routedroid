//! The fake helper's canned answers, which tests compare against.

use routedroid_helper_ipc::{Finding, Interface, Lease};

pub const LEASED_IP: [u8; 4] = [10, 0, 0, 50];
pub const LEASE_DNS: [u8; 4] = [10, 0, 0, 53];
pub const ENDED: &str = "02:00:00:00:00:99 also uses 10.0.0.50";

/// lan0, where phones may lease an address.
pub fn interfaces() -> Vec<Interface> {
    vec![Interface {
        name: "lan0".into(),
        up: true,
        addresses: vec![],
        default_route: true,
        phone_addresses: vec![],
        dhcp: true,
        ineligible: None,
    }]
}

/// What `Inspect` always finds, and `Repair` always removes.
pub fn leftover() -> Finding {
    Finding {
        subject: "nft table inet routedroid_phone9".into(),
        problem: "tagged for session 00000000000000aa, which has no journal".into(),
        warning: false,
        repair: vec!["delete table inet routedroid_phone9".into()],
    }
}

pub fn lease(expires_at: u64) -> Lease {
    Lease {
        server: [10, 0, 0, 254].into(),
        router: Some([10, 0, 0, 254].into()),
        dns: vec![LEASE_DNS.into()],
        expires_at,
    }
}
