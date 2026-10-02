//! `routedroid events`: the daemon's event stream as JSON lines, for scripts.

use anyhow::Result;
use routedroid_ipc::{Client, Request};

use crate::output::print_json_line;

pub async fn run(client: &mut Client) -> Result<i32> {
    client.call_ok(Request::Subscribe).await?;
    loop {
        let event = tokio::select! {
            event = client.next_event() => event?,
            _ = tokio::signal::ctrl_c() => return Ok(0),
        };
        match event {
            Some(event) => print_json_line(&event)?,
            None => return Ok(0),
        }
    }
}
