//! One client connection: JSON lines in, responses (and, after `subscribe`,
//! events) out. Requests are handled one at a time per connection.

use std::sync::Arc;

use routedroid_ipc::wire::{ClientMessage, ServerMessage, MAX_LINE};
use routedroid_ipc::Request;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, warn};

use crate::daemon::Daemon;

pub async fn run(daemon: Arc<Daemon>, stream: UnixStream) {
    let (rd, mut wr) = stream.into_split();
    let mut lines = BufReader::new(rd).lines();
    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
    let writer = tokio::spawn(async move {
        while let Some(mut line) = out_rx.recv().await {
            line.push('\n');
            if wr.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    let mut events: Option<broadcast::Receiver<routedroid_ipc::Event>> = None;

    loop {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) if line.len() <= MAX_LINE => {
                    let msg: ClientMessage = match serde_json::from_str(&line) {
                        Ok(m) => m,
                        Err(e) => { warn!(error = %e, "bad request line; closing"); break; }
                    };
                    debug!(id = msg.id, request = ?msg.request, "request");
                    if matches!(msg.request, Request::Subscribe) && events.is_none() {
                        events = Some(daemon.subscribe());
                    }
                    let response = crate::daemon::handle(&daemon, msg.request).await;
                    if send(&out_tx, ServerMessage::Response { id: msg.id, response }).await.is_err() { break; }
                }
                Ok(Some(_)) => { warn!("request line too long; closing"); break; }
                Ok(None) | Err(_) => break,
            },
            ev = recv_event(&mut events) => match ev {
                Ok(event) => { if send(&out_tx, ServerMessage::Event { event }).await.is_err() { break; } }
                Err(broadcast::error::RecvError::Lagged(n)) => warn!(missed = n, "slow subscriber"),
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }
    drop(out_tx);
    let _ = writer.await;
}

async fn recv_event(
    events: &mut Option<broadcast::Receiver<routedroid_ipc::Event>>,
) -> Result<routedroid_ipc::Event, broadcast::error::RecvError> {
    match events {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn send(tx: &mpsc::Sender<String>, msg: ServerMessage) -> Result<(), ()> {
    let line = serde_json::to_string(&msg).map_err(|_| ())?;
    tx.send(line).await.map_err(|_| ())
}
