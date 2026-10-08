//! The fake kernel's routes and rules, with the kernel's refusals: no
//! route replaced, no egress installed twice.

use std::net::Ipv4Addr;

use anyhow::{Result, bail};

use super::State;
use crate::kernel::{Egress, HostRoute, MAIN_TABLE, ROUTE_PROTOCOL, Route};

impl State {
    pub(super) fn add_route(&mut self, route: &HostRoute) -> Result<()> {
        if self
            .routes
            .iter()
            .any(|r| r.table == MAIN_TABLE && r.dst == route.dst && r.prefix == 32)
        {
            bail!("add route {}/32: EEXIST", route.dst);
        }
        let entry = Route {
            table: MAIN_TABLE,
            dst: route.dst,
            prefix: 32,
            gateway: None,
            oif: Some(route.oif),
            protocol: ROUTE_PROTOCOL,
            metric: 0,
        };
        self.routes.push(entry);
        Ok(())
    }

    pub(super) fn delete_route(&mut self, route: &HostRoute) -> Result<()> {
        let before = self.routes.len();
        self.routes.retain(|r| !route.matches(r));
        if self.routes.len() == before {
            bail!("delete route {}/32: ESRCH", route.dst);
        }
        Ok(())
    }

    pub(super) fn add_egress(&mut self, egress: &Egress, lan_index: u32) -> Result<()> {
        let rule = egress.rule();
        if self.rules.contains(&rule) || self.routes.iter().any(|r| r.table == egress.table) {
            bail!("add egress table {}: EEXIST", egress.table);
        }
        let route = |dst, prefix, gateway, oif| Route {
            table: egress.table,
            dst,
            prefix,
            gateway,
            oif,
            protocol: ROUTE_PROTOCOL,
            metric: 0,
        };
        let any = Ipv4Addr::UNSPECIFIED;
        self.routes
            .push(route(egress.lan_net, egress.prefix, None, Some(lan_index)));
        if let Some(gateway) = egress.gateway {
            self.routes
                .push(route(any, 0, Some(gateway), Some(lan_index)));
        }
        self.routes.push(route(any, 0, None, None));
        self.rules.push(rule);
        Ok(())
    }

    pub(super) fn delete_egress(&mut self, egress: &Egress) {
        let rule = egress.rule();
        self.rules.retain(|r| *r != rule);
        self.routes.retain(|r| !egress.owns(r));
    }
}
