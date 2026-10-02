//! `routedroid version`: ours first, then the daemon's, so ours shows even
//! when no daemon answers.

use std::path::Path;

use anyhow::Result;
use routedroid_ipc::{Client, Request, Response, API_VERSION};
use serde_json::json;

use super::answer;
use crate::output::print_json;

pub async fn run(socket: &Path, json: bool) -> Result<i32> {
    let cli = env!("CARGO_PKG_VERSION");
    if !json {
        println!("routedroid {cli} (api {API_VERSION})");
    }
    let client = Client::connect(socket).await?;
    let (daemon, api) = answer!(client.call_ok(Request::Version).await?, Response::Version { daemon, api } => (daemon, api));
    if json {
        print_json(&json!({ "cli": cli, "daemon": daemon, "api": api }))?;
    } else {
        println!("routedroidd {daemon} (api {api})");
    }
    Ok(0)
}
