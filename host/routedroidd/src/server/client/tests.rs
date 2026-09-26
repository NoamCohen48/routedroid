use std::sync::Arc;
use std::time::Duration;

use routedroid_ipc::{ConnectionState, DeviceInfo, Event, Request, Response};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::sync::{watch, Notify};
use tokio::time::timeout;

use super::{Answer, ClientConnection};
use crate::daemon::{EventBus, Snapshot};

/// Answers at once, except `Stop`, which waits for `release`.
#[derive(Clone)]
struct Fake {
    release: Arc<Notify>,
    changes: watch::Receiver<Snapshot>,
}

impl Answer for Fake {
    async fn answer(&self, request: Request) -> Response {
        if matches!(request, Request::Stop { .. }) {
            self.release.notified().await;
        }
        Response::Ok
    }

    async fn devices_view(&self) -> Vec<DeviceInfo> {
        vec![]
    }

    fn device_changes(&self) -> watch::Receiver<Snapshot> {
        self.changes.clone()
    }
}

struct Peer {
    lines: Lines<BufReader<OwnedReadHalf>>,
    writer: OwnedWriteHalf,
    bus: EventBus,
    release: Arc<Notify>,
    _devices: watch::Sender<Snapshot>,
}

impl Peer {
    fn start() -> Self {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let (devices, changes) = watch::channel(Snapshot::default());
        let (release, bus) = (Arc::new(Notify::new()), EventBus::new());
        let fake = Fake { release: Arc::clone(&release), changes };
        tokio::spawn(ClientConnection::new(fake, bus.clone(), theirs).run());
        let (rd, writer) = ours.into_split();
        Self { lines: BufReader::new(rd).lines(), writer, bus, release, _devices: devices }
    }

    async fn send(&mut self, line: &str) {
        self.writer.write_all(format!("{line}\n").as_bytes()).await.unwrap();
    }

    async fn next(&mut self) -> Value {
        let line = timeout(Duration::from_secs(2), self.lines.next_line()).await.expect("a line in time");
        serde_json::from_str(&line.unwrap().expect("not closed")).unwrap()
    }
}

fn stopping() -> Event {
    Event::Connection { serial: "s".into(), state: ConnectionState::Stopping }
}

#[tokio::test]
async fn a_pending_stop_holds_up_neither_events_nor_other_requests() {
    let mut peer = Peer::start();
    peer.send(r#"{"id":1,"type":"subscribe"}"#).await;
    assert_eq!(peer.next().await["id"], 1);
    peer.send(r#"{"id":2,"type":"stop","serial":"s"}"#).await;
    peer.send(r#"{"id":3,"type":"status"}"#).await;
    assert_eq!(peer.next().await["id"], 3);
    peer.bus.publish(stopping());
    assert_eq!(peer.next().await["event"], "connection");
    peer.release.notify_one();
    assert_eq!(peer.next().await["id"], 2);
}

#[tokio::test]
async fn a_bad_line_is_answered_and_the_connection_stays() {
    let mut peer = Peer::start();
    peer.send(r#"{"id":7,"type":"teleport"}"#).await;
    let answer = peer.next().await;
    assert_eq!((answer["id"].as_u64(), answer["type"].as_str()), (Some(7), Some("error")));
    peer.send("not json").await;
    assert_eq!(peer.next().await["id"], 0);
    peer.send(r#"{"id":8,"type":"version"}"#).await;
    assert_eq!(peer.next().await["id"], 8);
}

#[tokio::test]
async fn a_client_that_half_closes_still_gets_its_answer() {
    let mut peer = Peer::start();
    peer.send(r#"{"id":1,"type":"stop","serial":"s"}"#).await;
    peer.writer.shutdown().await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    peer.release.notify_one();
    assert_eq!(peer.next().await["id"], 1);
    assert!(peer.lines.next_line().await.unwrap().is_none(), "closed after the answer");
}
