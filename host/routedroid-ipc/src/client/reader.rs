//! The task that owns the read half: responses to the calls waiting for
//! them, events to the queue, and on the way out, the reason to everyone.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::unix::OwnedReadHalf;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::api::{Event, Response};
use crate::wire::ServerMessage;

/// The daemon closed the connection.
#[derive(Debug)]
pub struct Closed;

impl std::fmt::Display for Closed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("routedroidd closed the connection")
    }
}

impl std::error::Error for Closed {}

#[derive(Default)]
pub struct Shared {
    state: Mutex<State>,
}

/// Held by the client's parts (never by the task): the last one dropped
/// stops the reader.
pub struct Reader(JoinHandle<()>);

impl Drop for Reader {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Default)]
struct State {
    waiting: HashMap<u64, oneshot::Sender<Response>>,
    /// Set once, when the reader ends; no call can wait after that.
    closed: Option<String>,
    eof: bool,
}

impl Shared {
    pub fn start(read: OwnedReadHalf) -> (Arc<Self>, Arc<Reader>, mpsc::UnboundedReceiver<Event>) {
        let shared = Arc::new(Self::default());
        let (events, queue) = mpsc::unbounded_channel();
        let task = tokio::spawn(read_all(read, Arc::clone(&shared), events));
        (shared, Arc::new(Reader(task)), queue)
    }

    pub fn expect(&self, id: u64, answer: oneshot::Sender<Response>) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if state.closed.is_some() {
            drop(state);
            return Err(self.why_closed());
        }
        state.waiting.insert(id, answer);
        Ok(())
    }

    pub fn forget(&self, id: u64) {
        self.state.lock().unwrap().waiting.remove(&id);
    }

    pub fn why_closed(&self) -> anyhow::Error {
        let state = self.state.lock().unwrap();
        match &state.closed {
            Some(_) if state.eof => Closed.into(),
            Some(why) => anyhow!("{why}"),
            None => anyhow!("routedroidd dropped the answer"),
        }
    }

    fn close(&self, eof: bool, why: String) {
        let mut state = self.state.lock().unwrap();
        state.closed = Some(why);
        state.eof = eof;
        // Dropping the senders wakes every waiting call.
        state.waiting.clear();
    }
}

async fn read_all(read: OwnedReadHalf, shared: Arc<Shared>, events: mpsc::UnboundedSender<Event>) {
    let mut lines = BufReader::new(read).lines();
    let (eof, why) = loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break (true, Closed.to_string()),
            Err(error) => break (false, format!("read from routedroidd: {error}")),
        };
        match serde_json::from_str(&line) {
            Ok(ServerMessage::Response { id, response }) => {
                let waiting = shared.state.lock().unwrap().waiting.remove(&id);
                // No one waiting: that call was dropped.
                if let Some(answer) = waiting {
                    let _ = answer.send(response);
                }
            }
            Ok(ServerMessage::Event { event }) => {
                let _ = events.send(event);
            }
            Err(error) => {
                break (
                    false,
                    format!("bad line from routedroidd ({error}): {}", line.trim()),
                )
            }
        }
    };
    shared.close(eof, why);
}
