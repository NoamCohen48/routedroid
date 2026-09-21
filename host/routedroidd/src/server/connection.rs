//! One client connection: JSON lines in, responses (and, after `subscribe`,
//! events) out. Requests are handled one at a time per connection.

use std::sync::Arc;

use futures_util::StreamExt;
use routedroid_ipc::wire::{ClientMessage, ServerMessage, MAX_LINE};
use routedroid_ipc::{Event, Request};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc};
use tokio_util::codec::{FramedRead, LinesCodec};
use tracing::{debug, warn};

use crate::daemon::Daemon;

pub async fn run(daemon: Arc<Daemon>, stream: UnixStream) {
    let (rd, mut wr) = stream.into_split();
    // The codec refuses a line over MAX_LINE while it is being read, so a
    // client cannot make the daemon buffer an unbounded line.
    let mut lines = FramedRead::new(rd, LinesCodec::new_with_max_length(MAX_LINE));
    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
    let writer = tokio::spawn(async move {
        while let Some(mut line) = out_rx.recv().await {
            line.push('\n');
            if wr.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    let mut events: Option<broadcast::Receiver<Event>> = None;

    loop {
        tokio::select! {
            line = lines.next() => match line {
                Some(Ok(line)) => {
                    let msg: ClientMessage = match serde_json::from_str(&line) {
                        Ok(msg) => msg,
                        Err(e) => { warn!(error = %e, "bad request line; closing"); break; }
                    };
                    debug!(id = msg.id, request = ?msg.request, "request");
                    if matches!(msg.request, Request::Subscribe) && events.is_none() {
                        events = Some(daemon.subscribe());
                    }
                    let response = crate::daemon::handle(&daemon, msg.request).await;
                    if send(&out_tx, ServerMessage::Response { id: msg.id, response }).await.is_err() { break; }
                }
                Some(Err(e)) => { warn!(error = %e, "request line rejected; closing"); break; }
                None => break,
            },
            ev = recv_event(&mut events) => {
                let event = match ev {
                    Ok(event) => event,
                    // Tell the client rather than silently losing (possibly) an `ended`.
                    Err(broadcast::error::RecvError::Lagged(missed)) => Event::Lagged { missed },
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                if send(&out_tx, ServerMessage::Event { event }).await.is_err() { break; }
            }
        }
    }
    drop(out_tx);
    let _ = writer.await;
}

async fn recv_event(events: &mut Option<broadcast::Receiver<Event>>) -> Result<Event, broadcast::error::RecvError> {
    match events {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn send(tx: &mpsc::Sender<String>, msg: ServerMessage) -> Result<(), ()> {
    let line = serde_json::to_string(&msg).map_err(|_| ())?;
    tx.send(line).await.map_err(|_| ())
}
