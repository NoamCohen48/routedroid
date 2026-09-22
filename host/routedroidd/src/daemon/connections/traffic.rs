//! Counters for the active connections, once a second. The table publishes
//! them because they are its own business; nothing else has to poll it.

use std::time::Duration;

use routedroid_ipc::{ConnectionState, Event};

use super::{DeviceConnections, Live};
use crate::daemon::events::EventBus;

const TICK: Duration = Duration::from_secs(1);

pub(super) async fn ticker(live: Live, events: EventBus) {
    loop {
        tokio::time::sleep(TICK).await;
        for connection in DeviceConnections::snapshot(&live).await {
            if connection.state != ConnectionState::Active {
                continue;
            }
            events.publish(Event::Traffic {
                serial: connection.serial,
                packets_to_phone: connection.packets_to_phone,
                packets_from_phone: connection.packets_from_phone,
            });
        }
    }
}
