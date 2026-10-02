//! Rendering shared by the commands: aligned tables, JSON, and the phone's
//! place on the LAN. Connection states describe themselves (`Display` in
//! the ipc crate), so every client words them the same.

use routedroid_ipc::NetworkInfo;

/// Prints rows as left-aligned columns, each as wide as its widest cell.
pub fn print_table(rows: &[Vec<String>]) {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            let cells = rows.iter().filter_map(|row| row.get(column));
            cells.map(|cell| cell.chars().count()).max().unwrap_or(0)
        })
        .collect();
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(column, cell)| format!("{cell:<width$}", width = widths[column]))
            .collect();
        println!("{}", cells.join("  ").trim_end());
    }
}

pub fn header(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

pub fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub fn print_json_line<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}

/// "192.168.1.50 on the LAN (host 192.168.1.10/24), DNS 192.168.1.1, leased from ..."
pub fn network_line(network: &NetworkInfo) -> String {
    let NetworkInfo {
        phone_ip,
        host_ip,
        lan_prefix,
        ..
    } = network;
    let mut line = format!("phone is {phone_ip} on the LAN (host {host_ip}/{lan_prefix})");
    line += &match network.dns.as_slice() {
        [] => ", no DNS".to_string(),
        dns => {
            let dns: Vec<String> = dns.iter().map(ToString::to_string).collect();
            format!(", DNS {}", dns.join(" "))
        }
    };
    if let Some(lease) = &network.lease {
        line += &format!(", leased from {}", lease.server);
    }
    line
}

/// "1.2 MB" for counters a person reads.
pub fn bytes(count: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut value = count as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    match unit {
        0 => format!("{count} B"),
        _ => format!("{value:.1} {}", UNITS[unit]),
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use routedroid_ipc::Lease;

    use super::*;

    #[test]
    fn bytes_read_like_bytes() {
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(1_234_567), "1.2 MB");
    }

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
        assert!(network_line(&network).ends_with("DNS 192.168.1.1, leased from 192.168.1.1"));
    }
}
