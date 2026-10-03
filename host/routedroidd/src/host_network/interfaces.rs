//! The helper's survey of the host's links, in the control API's terms.

use std::path::Path;
use std::time::Duration;

use routedroid_helper_ipc::{Net, Reply, Request};
use routedroid_ipc::{InterfaceInfo, Ipv4Net};

use super::helper::{self, request, unexpected};
use crate::fault::{Fault, FaultExt, Kind, Result};

const SURVEY_TIMEOUT: Duration = Duration::from_secs(10);

/// Every link, and whether a phone may join the LAN through it.
pub async fn interfaces(socket: &Path) -> Result<Vec<InterfaceInfo>> {
    let survey = async {
        let conn = helper::connect(socket).await?;
        match request(&conn, &Request::Interfaces)
            .await
            .fault(Kind::Helper)?
        {
            Reply::Interfaces { interfaces } => Ok(interfaces),
            Reply::Error { code, message } => Err(Fault::msg(
                Kind::Helper,
                format!("helper could not list interfaces: {code:?}: {message}"),
            )),
            other => Err(unexpected(&other)),
        }
    };
    let interfaces = tokio::time::timeout(SURVEY_TIMEOUT, survey)
        .await
        .map_err(|_| {
            Fault::msg(
                Kind::Helper,
                format!("helper did not list interfaces within {SURVEY_TIMEOUT:?}"),
            )
        })??;
    Ok(interfaces
        .into_iter()
        .map(|i| InterfaceInfo {
            name: i.name,
            up: i.up,
            addresses: i.addresses.into_iter().map(net).collect(),
            default_route: i.default_route,
            phone_addresses: i.phone_addresses.into_iter().map(net).collect(),
            dhcp: i.dhcp,
            ineligible: i.ineligible,
        })
        .collect())
}

fn net(net: Net) -> Ipv4Net {
    Ipv4Net {
        address: net.address,
        prefix: net.prefix,
    }
}
