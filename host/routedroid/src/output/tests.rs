use std::net::Ipv4Addr;

use super::*;

#[test]
fn the_network_line_names_address_dns_and_lease() {
    let mut network = NetworkInfo {
        phone_ip: Ipv4Addr::new(192, 168, 1, 50),
        host_ip: Ipv4Addr::new(192, 168, 1, 10),
        lan_prefix: 24,
        dns: vec![],
        lease: None,
    };
    let line = network_line(&network);
    assert_eq!(
        line,
        "phone is 192.168.1.50 on the LAN (host 192.168.1.10/24), no DNS"
    );
    network.dns = vec![Ipv4Addr::new(192, 168, 1, 1)];
    network.lease = Some(Lease {
        server: Ipv4Addr::new(192, 168, 1, 1),
        expires_at: 0,
    });
    assert!(network_line(&network).ends_with("DNS 192.168.1.1, leased from 192.168.1.1 (expired)"));
}

#[test]
fn lease_time_left_reads_like_a_clock() {
    let lease = |expires_at| Lease {
        server: Ipv4Addr::new(192, 168, 1, 1),
        expires_at,
    };
    assert_eq!(lease_left(&lease(1000), 1000), "expired");
    assert_eq!(lease_left(&lease(1059), 1000), "under a minute left");
    assert_eq!(lease_left(&lease(1000 + 45 * 60), 1000), "45m left");
    assert_eq!(lease_left(&lease(1000 + 65 * 60 + 30), 1000), "1h 05m left");
}
