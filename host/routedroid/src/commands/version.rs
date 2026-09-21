//! `routedroid version`: this binary's version and the daemon's.

use anyhow::{bail, Result};
use routedroid_ipc::{Client, Request, Response};

pub async fn run(client: &mut Client) -> Result<i32> {
    println!("routedroid {}", env!("CARGO_PKG_VERSION"));
    match client.call_ok(Request::Version).await? {
        Response::Version { daemon, api } => println!("routedroidd {daemon} (api {api})"),
        other => bail!("unexpected answer to version: {other:?}"),
    }
    Ok(0)
}
