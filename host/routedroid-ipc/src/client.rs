//! Async client: one connection, sequential calls, optional event stream.
//! Reads go through `Lines::next_line`, which is cancel-safe, so callers may
//! use `next_event` inside `select!`.

use std::path::Path;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;

use crate::api::{Event, Request, Response};
use crate::wire::{ClientMessage, ServerMessage};
use crate::API_VERSION;

pub struct Client {
    lines: Lines<BufReader<OwnedReadHalf>>,
    writer: OwnedWriteHalf,
    next_id: u64,
    /// Events read while waiting for a response; drained by `next_event`.
    pending: std::collections::VecDeque<Event>,
}

impl Client {
    pub async fn connect(path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(path)
            .await
            .with_context(|| format!("connect to routedroidd at {} (is it running?)", path.display()))?;
        let (rd, writer) = stream.into_split();
        let mut client = Self { lines: BufReader::new(rd).lines(), writer, next_id: 1, pending: Default::default() };
        match client.call(Request::Version).await? {
            Response::Version { api, .. } if api == API_VERSION => Ok(client),
            Response::Version { daemon, api } => {
                bail!("routedroidd {daemon} speaks API {api}; this client needs {API_VERSION}")
            }
            other => bail!("unexpected answer to version: {other:?}"),
        }
    }

    /// Send one request and wait for its response. Events that arrive in
    /// between are queued for [`Self::next_event`].
    pub async fn call(&mut self, request: Request) -> Result<Response> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = serde_json::to_string(&ClientMessage { id, request })?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await.context("send to routedroidd")?;
        loop {
            match self.read_message().await? {
                ServerMessage::Response { id: got, response } if got == id => return Ok(response),
                ServerMessage::Response { id: got, .. } => bail!("response for unknown request id {got}"),
                ServerMessage::Event { event } => self.pending.push_back(event),
            }
        }
    }

    /// Like `call`, but an `Error` response becomes an `Err`.
    pub async fn call_ok(&mut self, request: Request) -> Result<Response> {
        match self.call(request).await? {
            Response::Error { kind, message } => Err(crate::fault::Fault::msg(kind, message).into()),
            other => Ok(other),
        }
    }

    /// Next event (after `Request::Subscribe`); `None` when the daemon closed.
    pub async fn next_event(&mut self) -> Result<Option<Event>> {
        if let Some(e) = self.pending.pop_front() {
            return Ok(Some(e));
        }
        loop {
            match self.read_message().await {
                Ok(ServerMessage::Event { event }) => return Ok(Some(event)),
                Ok(ServerMessage::Response { .. }) => continue,
                Err(e) if e.downcast_ref::<Closed>().is_some() => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }

    async fn read_message(&mut self) -> Result<ServerMessage> {
        let Some(line) = self.lines.next_line().await.context("read from routedroidd")? else {
            return Err(Closed.into());
        };
        serde_json::from_str(&line).with_context(|| format!("bad line from routedroidd: {}", line.trim()))
    }
}

#[derive(Debug)]
pub struct Closed;

impl std::fmt::Display for Closed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("routedroidd closed the connection")
    }
}

impl std::error::Error for Closed {}
