//! One client's connection to the control socket: JSON lines in, responses
//! (and, after `subscribe`, events) out. Each request runs on its own task
//! and its response carries the request's id, so a `stop` that waits for a
//! teardown holds up neither events nor the client's other requests.
//!
//! It reaches the daemon only through [`Answer`] (the attached devices and
//! the device connections) and the event bus. There is no handle here to
//! the daemon itself.

mod answer;

use futures_util::StreamExt;
use routedroid_ipc::wire::{ServerMessage, MAX_LINE};
use routedroid_ipc::{Event, Request, Response};
use tokio::io::AsyncWriteExt;
use tokio::net::unix::OwnedReadHalf;
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinSet;
use tokio_util::codec::{FramedRead, LinesCodec};
use tracing::{debug, warn};

use crate::daemon::{EventBus, Snapshot};

pub use answer::{Answer, Handles};

/// Outbound lines are queued to a writer task, so a slow client blocks its
/// own connection and never the daemon.
const WRITE_QUEUE: usize = 64;
/// Requests running at once; past it the connection stops reading lines.
const IN_FLIGHT: usize = 8;

pub struct ClientConnection<A> {
    handles: A,
    bus: EventBus,
    lines: FramedRead<OwnedReadHalf, LinesCodec>,
    out: mpsc::Sender<String>,
    writer: tokio::task::JoinHandle<()>,
    requests: JoinSet<()>,
    events: Option<broadcast::Receiver<Event>>,
    device_changes: Option<watch::Receiver<Snapshot>>,
}

impl<A: Answer> ClientConnection<A> {
    pub fn new(handles: A, bus: EventBus, stream: UnixStream) -> Self {
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
        let requests = JoinSet::new();
        Self { handles, bus, lines, out, writer, requests, events: None, device_changes: None }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                line = self.lines.next(), if self.requests.len() < IN_FLIGHT => match line {
                    Some(Ok(line)) => if self.on_line(&line).await.is_err() { break },
                    Some(Err(error)) => { warn!(%error, "request line rejected; closing"); break; }
                    None => break,
                },
                Some(_) = self.requests.join_next() => {}
                event = Self::next_event(&mut self.events) => {
                    let event = match event {
                        Ok(event) => event,
                        // Tell the client rather than silently losing (possibly) an `ended`.
                        Err(broadcast::error::RecvError::Lagged(missed)) => Event::Lagged { missed },
                        Err(broadcast::error::RecvError::Closed) => break,
                    };
                    if send(&self.out, ServerMessage::Event { event }).await.is_err() { break }
                }
                // The device list is a `watch`: this fires on the newest list,
                // so a slow client skips intermediate ones instead of lagging.
                changed = Self::next_device_change(&mut self.device_changes) => {
                    if changed.is_err() { break }
                    let devices = self.handles.devices_view().await;
                    if send(&self.out, ServerMessage::Event { event: Event::Devices { devices } }).await.is_err() { break }
                }
            }
        }
        self.close().await;
    }

    /// `Err(())` means the peer is gone.
    async fn on_line(&mut self, line: &str) -> Result<(), ()> {
        let (id, request) = match answer::parse(line) {
            Ok(message) => (message.id, message.request),
            Err((id, response)) => {
                warn!(id, "bad request line");
                return send(&self.out, ServerMessage::Response { id, response }).await;
            }
        };
        debug!(id, ?request, "request");
        if matches!(request, Request::Subscribe) {
            if self.events.is_none() {
                self.events = Some(self.bus.subscribe());
                self.device_changes = Some(self.handles.device_changes());
            }
            return send(&self.out, ServerMessage::Response { id, response: Response::Ok }).await;
        }
        let (handles, out) = (self.handles.clone(), self.out.clone());
        self.requests.spawn(async move {
            let response = handles.answer(request).await;
            let _ = send(&out, ServerMessage::Response { id, response }).await;
        });
        Ok(())
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

    /// Finish the requests already taken (a client that half-closed after
    /// sending still gets its answers), let the writer drain, then wait for it.
    async fn close(mut self) {
        while self.requests.join_next().await.is_some() {}
        drop(self.out);
        let _ = self.writer.await;
    }
}

async fn send(out: &mpsc::Sender<String>, message: ServerMessage) -> Result<(), ()> {
    let line = serde_json::to_string(&message).map_err(|_| ())?;
    out.send(line).await.map_err(|_| ())
}

#[cfg(test)]
mod tests;
