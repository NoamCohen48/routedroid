//! Turning a filled form into a start request. Only the shape of each field
//! is checked here; the daemon checks the values and owns every default.

use std::net::Ipv4Addr;

use anyhow::{Context, Result, bail};
use routedroid_ipc::{DnsChoice, StartRequest};

use super::StartForm;

fn optional(text: &str) -> Option<&str> {
    Some(text.trim()).filter(|text| !text.is_empty())
}

impl StartForm {
    /// The error names the offending field.
    pub fn to_request(&self) -> Result<StartRequest> {
        let Some(lan_if) = optional(self.lan_if.value()) else {
            bail!("LAN interface is required");
        };
        let phone_ip = optional(self.phone_ip.value())
            .map(|ip| {
                ip.parse::<Ipv4Addr>()
                    .with_context(|| format!("phone IP {ip:?} is not an IPv4 address"))
            })
            .transpose()?;
        let mtu = optional(self.mtu.value())
            .map(|mtu| {
                mtu.parse::<u32>()
                    .with_context(|| format!("MTU {mtu:?} is not a number"))
            })
            .transpose()?;
        let timeout = seconds(self.timeout.value(), "timeout")?;
        let reconnect = seconds(self.reconnect_wait.value(), "reconnect wait")?;
        Ok(StartRequest {
            serial: self.serial.clone(),
            lan_if: lan_if.to_string(),
            phone_ip,
            tun: optional(self.tun.value()).map(str::to_string),
            mtu,
            dns: dns(self.dns.value())?,
            connect_timeout_secs: timeout,
            reconnect_secs: reconnect,
            allow_network_adb: self.allow_network_adb,
        })
    }
}

/// A duration like `90s` or `2m`, in whole seconds (rounded up).
fn seconds(text: &str, what: &str) -> Result<Option<u64>> {
    optional(text)
        .map(|text| {
            let d = humantime::parse_duration(text).with_context(|| format!("{what} {text:?}"))?;
            Ok(d.as_secs() + u64::from(d.subsec_nanos() > 0))
        })
        .transpose()
}

fn dns(text: &str) -> Result<DnsChoice> {
    match text.trim() {
        "" => Ok(DnsChoice::Auto),
        "none" => Ok(DnsChoice::None),
        list => list
            .split([',', ' '])
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                entry
                    .parse()
                    .with_context(|| format!("DNS {entry:?} is not an IPv4 address"))
            })
            .collect::<Result<_>>()
            .map(DnsChoice::Servers),
    }
}
