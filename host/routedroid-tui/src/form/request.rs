//! Turning a filled form into a start request. Only the shape of each field
//! is checked here; the daemon checks the values and owns every default.

use std::net::Ipv4Addr;

use anyhow::{bail, Context, Result};
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
        let timeout = optional(self.timeout.value())
            .map(|text| {
                humantime::parse_duration(text).with_context(|| format!("timeout {text:?}"))
            })
            .transpose()?;
        Ok(StartRequest {
            serial: self.serial.clone(),
            lan_if: lan_if.to_string(),
            phone_ip,
            tun: optional(self.tun.value()).map(str::to_string),
            mtu,
            dns: dns(self.dns.value())?,
            connect_timeout_secs: timeout.map(|d| d.as_secs_f64().ceil() as u64),
            allow_network_adb: self.allow_network_adb,
        })
    }
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
