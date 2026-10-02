//! Rendering shared by the commands: aligned tables, JSON, and the phone's
//! place on the LAN. Connection states describe themselves (`Display` in
//! the ipc crate), so every client words them the same.

use std::time::{SystemTime, UNIX_EPOCH};

use routedroid_ipc::{Lease, NetworkInfo};

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
        line += &format!(
            ", leased from {} ({})",
            lease.server,
            lease_left(lease, now())
        );
    }
    line
}

/// "1h 05m left" until the lease runs out unless renewed (renewals are announced).
pub fn lease_left(lease: &Lease, now: u64) -> String {
    let minutes = lease.expires_at.saturating_sub(now) / 60;
    match minutes {
        _ if lease.expires_at <= now => "expired".into(),
        0 => "under a minute left".into(),
        1..60 => format!("{minutes}m left"),
        _ => format!("{}h {:02}m left", minutes / 60, minutes % 60),
    }
}

/// Unix seconds.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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
mod tests;
