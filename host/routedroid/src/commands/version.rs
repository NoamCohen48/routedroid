//! `routedroid version`: the daemon's version (the caller printed ours
//! before connecting, so it shows even with no daemon).

use anyhow::{bail, Result};
use routedroid_ipc::{Client, Request, Response};

pub async fn run(client: &mut Client) -> Result<i32> {
    match client.call_ok(Request::Version).await? {
        Response::Version { daemon, api } => println!("routedroidd {daemon} (api {api})"),
        other => bail!("unexpected answer to version: {other:?}"),
    }
    Ok(0)
}
