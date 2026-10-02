//! The LAN's default gateway, read from the kernel's routing table (no
//! privilege needed): the phone's DNS when nothing better is known.

use std::net::Ipv4Addr;

use routedroid_helper_ipc::IfName;

/// The gateway of the main table's default route through `lan_if`, if any.
pub fn default_gateway(lan_if: &IfName) -> Option<Ipv4Addr> {
    let table = std::fs::read_to_string("/proc/net/route").ok()?;
    parse(&table, lan_if.as_str())
}

/// `/proc/net/route`: a header, then `Iface Destination Gateway Flags ...`
/// with addresses as host-order hex of the network-order bytes.
fn parse(table: &str, lan_if: &str) -> Option<Ipv4Addr> {
    const RTF_GATEWAY: u32 = 0x2;
    table.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [iface, destination, gateway, flags, ..] = fields[..] else {
            return None;
        };
        let flags = u32::from_str_radix(flags, 16).ok()?;
        if iface != lan_if || destination != "00000000" || flags & RTF_GATEWAY == 0 {
            return None;
        }
        let gateway = u32::from_str_radix(gateway, 16).ok()?;
        Some(Ipv4Addr::from(gateway.to_le_bytes()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0
eno1\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
eno1\t00000000\t FE01A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0
";

    #[test]
    fn finds_the_default_route_of_that_interface() {
        assert_eq!(parse(TABLE, "eno1"), Some(Ipv4Addr::new(192, 168, 1, 254)));
        assert_eq!(parse(TABLE, "wlan0"), Some(Ipv4Addr::new(192, 168, 1, 1)));
        assert_eq!(parse(TABLE, "eth9"), None);
    }

    #[test]
    fn a_connected_route_is_not_a_gateway() {
        let only_connected = TABLE.lines().take(3).collect::<Vec<_>>().join("\n");
        assert_eq!(parse(&only_connected, "eno1"), None);
    }
}
