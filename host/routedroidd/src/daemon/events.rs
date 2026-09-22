//! The event bus: one broadcast channel, cloned into whatever publishes or
//! listens. Holding it gives no access to the daemon's state.

use routedroid_ipc::Event;
use tokio::sync::broadcast;

/// Enough that a client doing something slow between `recv`s still sees a
/// connection's whole life; past it the client is told it lagged.
const CAPACITY: usize = 256;

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CAPACITY);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    pub fn publish(&self, event: Event) {
        // No subscribers is not an error.
        let _ = self.tx.send(event);
    }
}
