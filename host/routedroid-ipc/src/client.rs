//! Async client over one connection. A reader task routes each response to
//! the call waiting for its id and queues events, so calls may overlap each
//! other and the event stream: a `stop` that waits for a teardown does not
//! hold up anything else on the connection.
//!
//! [`Client::into_parts`] splits it for a caller that must read events while
//! calls are in flight; [`Events::next`] is cancel-safe, so it may sit in a
//! `select!`.

mod connect;
mod reader;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::net::unix::OwnedWriteHalf;
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::api::{Event, Request, Response};
use crate::wire::ClientMessage;

pub use connect::ConnectError;
pub use reader::Closed;
use reader::{Reader, Shared};

pub struct Client {
    calls: Calls,
    events: Events,
}

/// Makes calls; cheap to clone, and clones may call concurrently.
#[derive(Clone)]
pub struct Calls {
    writer: Arc<Mutex<OwnedWriteHalf>>,
    next_id: Arc<AtomicU64>,
    shared: Arc<Shared>,
    _reader: Arc<Reader>,
}

/// The events of a subscribed connection, in order.
pub struct Events {
    queue: mpsc::UnboundedReceiver<Event>,
    shared: Arc<Shared>,
    _reader: Arc<Reader>,
}

impl Client {
    pub fn into_parts(self) -> (Calls, Events) {
        (self.calls, self.events)
    }

    pub async fn call(&self, request: Request) -> Result<Response> {
        self.calls.call(request).await
    }

    pub async fn call_ok(&self, request: Request) -> Result<Response> {
        self.calls.call_ok(request).await
    }

    /// Next event (after `Request::Subscribe`); `None` when the daemon closed.
    pub async fn next_event(&mut self) -> Result<Option<Event>> {
        self.events.next().await
    }
}

impl Calls {
    /// Send one request and wait for its response.
    pub async fn call(&self, request: Request) -> Result<Response> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (answer, answered) = oneshot::channel();
        self.shared.expect(id, answer)?;
        let mut line = serde_json::to_string(&ClientMessage { id, request })?;
        line.push('\n');
        let sent = self.writer.lock().await.write_all(line.as_bytes()).await;
        if let Err(error) = sent {
            self.shared.forget(id);
            return Err(error).context("send to routedroidd");
        }
        answered.await.map_err(|_| self.shared.why_closed())
    }

    /// Like `call`, but an `Error` response becomes an `Err`.
    pub async fn call_ok(&self, request: Request) -> Result<Response> {
        match self.call(request).await? {
            Response::Error { kind, message } => Err(crate::fault::Fault::msg(kind, message).into()),
            other => Ok(other),
        }
    }
}

impl Events {
    /// `None` when the daemon closed the connection; `Err` when it broke.
    pub async fn next(&mut self) -> Result<Option<Event>> {
        match self.queue.recv().await {
            Some(event) => Ok(Some(event)),
            None => {
                let why = self.shared.why_closed();
                if why.downcast_ref::<Closed>().is_some() {
                    Ok(None)
                } else {
                    Err(why)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
