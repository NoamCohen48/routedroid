//! Desktop notifications, over the user's session bus (freedesktop
//! Notifications), for the moments [`notice`] picks. Each phone has one
//! notification, replaced as its connection changes. Without a session bus
//! (a server, no desktop) there are none, and nothing else changes; nor
//! while [`Setting`] has them off.

mod notice;
mod setting;

use std::collections::HashMap;

use routedroid_ipc::Event;
use tokio::sync::broadcast::{self, error::RecvError};
use tracing::{debug, info};
use zbus::zvariant::Value;

use crate::daemon::Phones;
use notice::{Notice, Notices};
pub use setting::Setting;

const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

pub async fn run(mut events: broadcast::Receiver<Event>, phones: Phones, setting: Setting) {
    let bus = match zbus::Connection::session().await {
        Ok(bus) => bus,
        Err(error) => {
            info!(%error, "no session bus: no desktop notifications");
            return;
        }
    };
    let phone = |serial: &str| {
        let name = phones.find(serial).and_then(|phone| phone.name);
        name.unwrap_or_else(|| format!("Phone {serial}"))
    };
    let mut notices = Notices::default();
    let mut shown = HashMap::<String, u32>::new();
    loop {
        let event = match events.recv().await {
            Ok(event) => event,
            Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => return,
        };
        // Followed even while off, so turning them on shows the next change.
        let Some(notice) = notices.on(&event, phone) else {
            continue;
        };
        if !setting.on() {
            continue;
        }
        let replaces = shown.get(&notice.serial).copied().unwrap_or(0);
        match show(&bus, replaces, &notice).await {
            Ok(id) => drop(shown.insert(notice.serial, id)),
            Err(error) => debug!(%error, "notification not shown"),
        }
    }
}

async fn show(bus: &zbus::Connection, replaces: u32, notice: &Notice) -> zbus::Result<u32> {
    let urgency: u8 = if notice.urgent { 2 } else { 1 };
    let hints = HashMap::from([("urgency", Value::from(urgency))]);
    let actions: Vec<&str> = Vec::new();
    let arguments = (
        "Routedroid",
        replaces,
        "phone",
        notice.summary.as_str(),
        notice.body.as_str(),
        actions,
        hints,
        -1_i32,
    );
    let reply = bus
        .call_method(Some(SERVICE), PATH, Some(SERVICE), "Notify", &arguments)
        .await?;
    reply.body().deserialize()
}

#[cfg(test)]
mod tests;
