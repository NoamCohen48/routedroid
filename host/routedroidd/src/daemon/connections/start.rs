//! Accepting a start: fill it in, check it, and spawn its connection.

use routedroid_helper_ipc::{IfName, TUN_PREFIX};
use routedroid_ipc::StartRequest;

use super::{Accepted, DeviceConnection, DeviceConnections, admit, resolve, usage};
use crate::daemon::spec::{ConnectionSpec, StartSpec};
use crate::fault::Result;

impl DeviceConnections {
    /// Connect one phone; returns what it was given once its task runs.
    /// Progress arrives as events. What the request leaves out is filled in
    /// first (see `resolve`). Refuses a bad request, an unattached phone,
    /// one the helper's policy would refuse, or a serial, address or TUN
    /// another connection already uses, and picks a free `phoneN` if none
    /// was asked for. The checks against the table and the insert happen
    /// under one lock, so two starts cannot race.
    pub async fn start(&self, request: StartRequest) -> Result<Accepted> {
        let interfaces = self.interfaces().await.ok();
        let attached = match request.serial {
            Some(_) => self.devices.current(),
            None => self
                .devices
                .refresh()
                .await
                .unwrap_or_else(|_| self.devices.current()),
        };
        let connected: Vec<String> = self.lock().keys().cloned().collect();
        let known = resolve::Known {
            phones: &self.phones.all(),
            attached: &attached,
            connected: &connected,
            interfaces: interfaces.as_deref(),
        };
        let start = StartSpec::parse(resolve::resolve(request, &known)?)?;
        self.check_attached(&start.serial).await?;
        admit::check_policy(interfaces.as_deref(), &start)?;
        let mut live = self.lock();
        if live.contains_key(&start.serial) {
            return Err(usage(format!("{} is already connected", start.serial)));
        }
        if let Some(ip) = start.phone_ip
            && let Some(other) = live.values().find(|c| c.phone_ip() == Some(ip))
        {
            let serial = &other.spec().serial;
            return Err(usage(format!("{ip} is already used by {serial}")));
        }
        let taken = |name: &IfName| live.values().any(|c| &c.spec().tun == name);
        let tun = match &start.tun {
            Some(name) if taken(name) => {
                return Err(usage(format!(
                    "TUN {name} is already used by another connection"
                )));
            }
            Some(name) => name.clone(),
            None => (0..)
                .filter_map(|number| IfName::new(format!("{TUN_PREFIX}{number}")).ok())
                .find(|name| !taken(name))
                .expect("a free phoneN"),
        };
        let accepted = Accepted {
            serial: start.serial.clone(),
            lan_if: start.lan_if.to_string(),
            phone_ip: start.phone_ip,
            tun: tun.clone(),
        };
        let connection = DeviceConnection::spawn(self, ConnectionSpec::new(start, tun));
        live.insert(accepted.serial.clone(), connection);
        Ok(accepted)
    }
}
