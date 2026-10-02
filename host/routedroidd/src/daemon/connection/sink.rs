//! Where a connection's task says how it is doing: the handle's watches,
//! for status, and the event bus, for subscribers. Owned by the task.

use routedroid_ipc::{ConnectionState, Event, NetworkInfo};
use tokio::sync::watch;

use crate::daemon::events::EventBus;

pub struct StateSink {
    pub serial: String,
    events: EventBus,
    state: watch::Sender<ConnectionState>,
    network: watch::Sender<Option<NetworkInfo>>,
}

type Watches = (
    watch::Receiver<ConnectionState>,
    watch::Receiver<Option<NetworkInfo>>,
);

impl StateSink {
    pub fn new(serial: String, events: EventBus) -> (Self, Watches) {
        let (state, state_rx) = watch::channel(ConnectionState::Starting);
        let (network, network_rx) = watch::channel(None);
        let sink = Self {
            serial,
            events,
            state,
            network,
        };
        sink.publish_state(ConnectionState::Starting);
        (sink, (state_rx, network_rx))
    }

    pub fn set(&self, state: ConnectionState) {
        tracing::info!(serial = %self.serial, %state, "connection state");
        let _ = self.state.send(state.clone());
        self.publish_state(state);
    }

    pub fn set_network(&self, network: NetworkInfo) {
        let _ = self.network.send(Some(network.clone()));
        let serial = self.serial.clone();
        self.events.publish(Event::Network { serial, network });
    }

    fn publish_state(&self, state: ConnectionState) {
        let serial = self.serial.clone();
        self.events.publish(Event::Connection { serial, state });
    }
}
