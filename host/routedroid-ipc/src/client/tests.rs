use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use super::*;

/// A client and the daemon's end of its connection.
fn pair() -> (Client, BufReader<UnixStream>) {
    let (ours, theirs) = UnixStream::pair().unwrap();
    (Client::over(ours), BufReader::new(theirs))
}

async fn request_id(daemon: &mut BufReader<UnixStream>) -> u64 {
    let mut line = String::new();
    daemon.read_line(&mut line).await.unwrap();
    serde_json::from_str::<serde_json::Value>(&line).unwrap()["id"].as_u64().unwrap()
}

async fn say(daemon: &mut BufReader<UnixStream>, line: &str) {
    daemon.get_mut().write_all(format!("{line}\n").as_bytes()).await.unwrap();
}

#[tokio::test]
async fn answers_find_their_calls_in_any_order_and_events_flow_meanwhile() {
    let (client, mut daemon) = pair();
    let (calls, mut events) = client.into_parts();
    let slow = tokio::spawn({
        let calls = calls.clone();
        async move { calls.call(Request::Stop { serial: "s".into() }).await }
    });
    let stop_id = request_id(&mut daemon).await;
    let quick = tokio::spawn({
        let calls = calls.clone();
        async move { calls.call(Request::Status).await }
    });
    let status_id = request_id(&mut daemon).await;
    say(&mut daemon, &format!(r#"{{"id":{status_id},"type":"status","connections":[]}}"#)).await;
    assert!(matches!(quick.await.unwrap().unwrap(), Response::Status { .. }));
    say(&mut daemon, r#"{"event":"shutdown"}"#).await;
    assert!(matches!(events.next().await.unwrap(), Some(Event::Shutdown)));
    assert!(!slow.is_finished());
    say(&mut daemon, &format!(r#"{{"id":{stop_id},"type":"ok"}}"#)).await;
    assert!(matches!(slow.await.unwrap().unwrap(), Response::Ok));
}

#[tokio::test]
async fn a_closed_daemon_fails_waiting_calls_and_ends_events() {
    let (client, mut daemon) = pair();
    let (calls, mut events) = client.into_parts();
    let waiting = tokio::spawn({
        let calls = calls.clone();
        async move { calls.call(Request::Status).await }
    });
    request_id(&mut daemon).await;
    drop(daemon);
    let error = waiting.await.unwrap().unwrap_err();
    assert!(error.downcast_ref::<Closed>().is_some(), "{error:#}");
    assert!(events.next().await.unwrap().is_none());
    assert!(calls.call(Request::Status).await.unwrap_err().downcast_ref::<Closed>().is_some());
}

#[tokio::test]
async fn garbage_from_the_daemon_is_an_error_not_a_clean_close() {
    let (mut client, mut daemon) = pair();
    say(&mut daemon, "{not json").await;
    let error = client.next_event().await.unwrap_err();
    assert!(error.to_string().contains("bad line"), "{error:#}");
}

#[tokio::test]
async fn an_other_api_is_incompatible_not_unreachable() {
    let dir = std::env::temp_dir().join(format!("rd-ipc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("control.sock");
    let _ = std::fs::remove_file(&path);
    assert!(matches!(Client::connect(&path).await, Err(ConnectError::Unreachable { .. })));
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(async move {
        let mut daemon = BufReader::new(listener.accept().await.unwrap().0);
        let id = request_id(&mut daemon).await;
        say(&mut daemon, &format!(r#"{{"id":{id},"type":"version","daemon":"9.9","api":999}}"#)).await;
    });
    let result = Client::connect(&path).await;
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(matches!(result, Err(ConnectError::Incompatible { api: 999, .. })));
}
