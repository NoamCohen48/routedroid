//! `doctor`: adb, the helper, its policy, and the helper's findings, each
//! as a check a person can act on.

use std::path::Path;

use routedroid_helper_ipc::VERSION;
use routedroid_ipc::{Check, CheckStatus, InterfaceInfo};

use super::AttachedDevices;
use crate::device::unusable;
use crate::host_network;

mod groups;

/// The checks, and the changes a repair made.
pub async fn run(
    devices: &AttachedDevices,
    socket: &Path,
    repair: bool,
) -> (Vec<Check>, Vec<String>) {
    let mut checks = vec![adb(devices).await];
    if let Err(fault) = host_network::connect(socket).await {
        checks.push(Check::new(
            "helper",
            CheckStatus::Fail,
            // Refused (EACCES) is about groups; anything else is the socket unit.
            match fault.to_string().contains("Permission denied") {
                true => format!("{fault}; {}", groups::hint()),
                false => format!("{fault}; is routedroid-helper.socket enabled (host/install.sh)?"),
            },
        ));
        return (checks, Vec::new());
    }
    let detail = format!("helper IPC {VERSION} at {}", socket.display());
    checks.push(Check::new("helper", CheckStatus::Ok, detail));
    checks.push(match host_network::interfaces(socket).await {
        Ok(interfaces) => policy(&interfaces),
        Err(fault) => Check::new("policy", CheckStatus::Fail, fault.to_string()),
    });
    let done = match host_network::doctor(socket, repair).await {
        Ok((done, findings)) if findings.is_empty() => {
            checks.push(Check::new(
                "leftovers",
                CheckStatus::Ok,
                "nothing left behind",
            ));
            done
        }
        Ok((done, findings)) => {
            checks.extend(findings.into_iter().map(|f| Check {
                name: f.subject,
                status: if f.warning {
                    CheckStatus::Warn
                } else {
                    CheckStatus::Fail
                },
                detail: f.problem,
                repair: f.repair,
            }));
            done
        }
        Err(fault) => {
            checks.push(Check::new(
                "leftovers",
                CheckStatus::Fail,
                fault.to_string(),
            ));
            Vec::new()
        }
    };
    (checks, done)
}

async fn adb(devices: &AttachedDevices) -> Check {
    let attached = match devices.refresh().await {
        Ok(attached) => attached,
        Err(fault) => {
            let detail = format!("{fault}; is adb (Android platform-tools) installed?");
            return Check::new("adb", CheckStatus::Fail, detail);
        }
    };
    let unusable: Vec<String> = attached
        .iter()
        .filter_map(|d| unusable(&d.state, &d.serial).map(|why| format!("{}: {why}", d.serial)))
        .collect();
    let ready = attached.len() - unusable.len();
    match (ready, unusable.is_empty()) {
        (_, false) => Check::new("adb", CheckStatus::Warn, unusable.join("; ")),
        (0, true) => Check::new(
            "adb",
            CheckStatus::Warn,
            "no phone attached: connect one by USB with USB debugging on",
        ),
        (1, true) => Check::new("adb", CheckStatus::Ok, "1 phone ready"),
        (n, true) => Check::new("adb", CheckStatus::Ok, format!("{n} phones ready")),
    }
}

fn policy(interfaces: &[InterfaceInfo]) -> Check {
    let allowed: Vec<String> = interfaces
        .iter()
        .filter(|i| i.ineligible.is_none())
        .map(|i| {
            if i.dhcp {
                format!("{} (DHCP)", i.name)
            } else {
                i.name.clone()
            }
        })
        .collect();
    if allowed.is_empty() {
        let detail = "no interface may carry phones: allow one in /etc/routedroid/helper.toml \
                      (`routedroid interfaces` says why each is refused)";
        Check::new("policy", CheckStatus::Fail, detail)
    } else {
        let detail = format!("phones may join through {}", allowed.join(", "));
        Check::new("policy", CheckStatus::Ok, detail)
    }
}
