//! One client's connection to the control socket: JSON lines in, responses
//! (and, after `subscribe`, events) out. Requests are handled one at a time
//! per connection.
//!
//! It holds the three components a client may reach and nothing else: the
//! attached devices, the device connections, and the event bus. There is no
//! handle here to the daemon itself.

mod answer;

use futures_util::StreamExt;
use routedroid_ipc::wire::{ClientMessage, ServerMessage, MAX_LINE};
use routedroid_ipc::{Event, Request};
use tokio::io::AsyncWriteExt;
use tokio::net::unix::OwnedReadHalf;
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc, watch};
use tokio_util::codec::{FramedRead, LinesCodec};
use tracing::{debug, warn};

use crate::daemon::{AttachedDevices, DeviceConnections, EventBus, Snapshot};

/// Outbound lines are queued to a writer task, so a slow client blocks its
/// own connection and never the daemon.
const WRITE_QUEUE: usize = 64;

pub struct ClientConnection {
    devices: AttachedDevices,
    connections: DeviceConnections,
    bus: EventBus,
    lines: FramedRead<OwnedReadHalf, LinesCodec>,
    out: mpsc::Sender<String>,
    writer: tokio::task::JoinHandle<()>,
    events: Option<broadcast::Receiver<Event>>,
    device_changes: Option<watch::Receiver<Snapshot>>,
}

impl ClientConnection {
    pub fn new(devices: AttachedDevices, connections: DeviceConnections, bus: EventBus, stream: UnixStream) -> Self {
        let (read_half, mut write_half) = stream.into_split();
        // The codec refuses a line over MAX_LINE while it is being read, so a
        // client cannot make the daemon buffer an unbounded line.
        let lines = FramedRead::new(read_half, LinesCodec::new_with_max_length(MAX_LINE));
        let (out, mut queued) = mpsc::channel::<String>(WRITE_QUEUE);
        let writer = tokio::spawn(async move {
            while let Some(mut line) = queued.recv().await {
                line.push('\n');
                if write_half.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        });
        Self { devices, connections, bus, lines, out, writer, events: None, device_changes: None }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                line = self.lines.next() => match line {
                    Some(Ok(line)) => if self.on_line(&line).await.is_err() { break },
                    Some(Err(error)) => { warn!(%error, "request line rejected; closing"); break; }
                    None => break,
                },
                event = Self::next_event(&mut self.events) => {
                    let event = match event {
                        Ok(event) => event,
                        // Tell the client rather than silently losing (possibly) an `ended`.
                        Err(broadcast::error::RecvError::Lagged(missed)) => Event::Lagged { missed },
                        Err(broadcast::error::RecvError::Closed) => break,
                    };
                    if self.send(ServerMessage::Event { event }).await.is_err() { break }
                }
                // The device list is a `watch`: this fires on the newest list,
                // so a slow client skips intermediate ones instead of lagging.
                changed = Self::next_device_change(&mut self.device_changes) => {
                    if changed.is_err() { break }
                    let devices = self.devices_view().await;
                    if self.send(ServerMessage::Event { event: Event::Devices { devices } }).await.is_err() { break }
                }
            }
        }
        self.close().await;
    }

    /// `Err(())` means this connection is finished (bad line or dead peer).
    async fn on_line(&mut self, line: &str) -> Result<(), ()> {
        let message: ClientMessage = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(error) => {
                warn!(%error, "bad request line; closing");
                return Err(());
            }
        };
        debug!(id = message.id, request = ?message.request, "request");
        if matches!(message.request, Request::Subscribe) && self.events.is_none() {
            self.events = Some(self.bus.subscribe());
            self.device_changes = Some(self.devices.changes());
        }
        let response = self.answer(message.request).await;
        self.send(ServerMessage::Response { id: message.id, response }).await
    }

    async fn next_event(events: &mut Option<broadcast::Receiver<Event>>) -> Result<Event, broadcast::error::RecvError> {
        match events {
            Some(events) => events.recv().await,
            // Not subscribed: this branch of the select never completes.
            None => std::future::pending().await,
        }
    }

    async fn next_device_change(
        changes: &mut Option<watch::Receiver<Snapshot>>,
    ) -> Result<(), watch::error::RecvError> {
        match changes {
            Some(changes) => changes.changed().await,
            None => std::future::pending().await,
        }
    }

    async fn send(&self, message: ServerMessage) -> Result<(), ()> {
        let line = serde_json::to_string(&message).map_err(|_| ())?;
        self.out.send(line).await.map_err(|_| ())
    }

    /// Let the writer drain what is already queued, then wait for it.
    async fn close(self) {
        drop(self.out);
        let _ = self.writer.await;
    }
}
