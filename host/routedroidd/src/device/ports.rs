//! Device-port reservation on top of `adb reverse`. The host port is whatever
//! the loopback listener got; the device port is picked at random from a
//! range below Android's ephemeral ports, skipping anything `adb reverse
//! --list` shows, and added with `--no-rebind`: the adb server is shared, so
//! another controller may take the same port between our list and our add.
//! Released only while the mapping is still exactly ours.

use tracing::{info, warn};

use crate::adb::{AdbDevice, ReverseMapping};
use routedroid_ipc::fault::{Fault, Kind, Result};

/// Below 32768, where Android's ephemeral range starts.
pub const DEVICE_PORT_RANGE: std::ops::RangeInclusive<u16> = 20_000..=32_767;

/// Ports tried before giving up when others keep taking them first.
const ATTEMPTS: u16 = 8;

/// A reverse mapping this process created. Released explicitly (never on
/// drop: releasing runs adb, which is async and may be refused).
#[derive(Debug, Clone, Copy)]
pub struct ReservedPort {
    pub device_port: u16,
    pub host_port: u16,
}

pub struct DevicePorts {
    adb: AdbDevice,
}

impl DevicePorts {
    pub fn new(adb: AdbDevice) -> Self {
        Self { adb }
    }

    /// Map a free device port in [`DEVICE_PORT_RANGE`] to `host_port`. A
    /// failed add is a collision if the port shows up in the list right
    /// after (then the next port is tried), and an adb failure otherwise.
    pub async fn reserve(&self, host_port: u16, seed: u16) -> Result<ReservedPort> {
        let mut used = self.used().await?;
        for attempt in 0..ATTEMPTS {
            let device_port = pick_device_port(&used, seed.wrapping_add(attempt.wrapping_mul(7919)))
                .ok_or_else(|| Fault::msg(Kind::Adb, "no free device port in the Routedroid range"))?;
            let Err(error) = self.adb.reverse_add(device_port, host_port).await else {
                info!(device_port, host_port, "adb reverse mapping added");
                return Ok(ReservedPort { device_port, host_port });
            };
            used = self.used().await?;
            if !used.contains(&device_port) {
                return Err(error);
            }
            warn!(device_port, "device port taken by someone else meanwhile; trying another");
        }
        Err(Fault::msg(Kind::Adb, format!("device ports kept being taken; gave up after {ATTEMPTS} attempts")))
    }

    async fn used(&self) -> Result<Vec<u16>> {
        Ok(self.adb.reverse_list().await?.iter().filter_map(ReverseMapping::device_port).collect())
    }

    /// Remove the mapping if it is still exactly ours. Another controller (or
    /// the user) may have replaced it meanwhile; theirs is never removed.
    /// Errors are logged: cleanup must not mask the session's own outcome.
    pub async fn release(&self, port: ReservedPort) {
        let list = match self.adb.reverse_list().await {
            Ok(l) => l,
            Err(e) => {
                warn!(error = %e, "could not list reverse mappings; not removing");
                return;
            }
        };
        if !is_exactly_ours(&list, port) {
            warn!(
                device_port = port.device_port,
                host_port = port.host_port,
                ?list,
                "reverse mapping is not ours any more; not removing"
            );
            return;
        }
        match self.adb.reverse_remove(port.device_port).await {
            Ok(()) => info!(device_port = port.device_port, "adb reverse mapping removed"),
            Err(e) => warn!(error = %e, "failed to remove adb reverse mapping"),
        }
    }
}

/// First port in the range not in `used`, starting from a random offset so
/// two controllers racing for the same device rarely collide.
pub(super) fn pick_device_port(used: &[u16], seed: u16) -> Option<u16> {
    let len = DEVICE_PORT_RANGE.end() - DEVICE_PORT_RANGE.start() + 1;
    (0..len).map(|i| DEVICE_PORT_RANGE.start() + (seed.wrapping_add(i)) % len).find(|p| !used.contains(p))
}

/// Exactly one mapping for the device port, and it points at our host port.
pub(super) fn is_exactly_ours(list: &[ReverseMapping], port: ReservedPort) -> bool {
    let ours: Vec<_> = list.iter().filter(|m| m.device_port() == Some(port.device_port)).collect();
    ours.len() == 1 && ours[0].local == format!("tcp:{}", port.host_port)
}
