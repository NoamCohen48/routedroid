//! `routedroid stop`: disconnect one phone and wait for it to be gone. The
//! exit code says how the connection ended.

use anyhow::Result;
use routedroid_ipc::{Client, Request, Response};

use super::answer;
use crate::exit;
use crate::output::print_json;

pub async fn run(client: &Client, serial: &str, json: bool) -> Result<i32> {
    let request = Request::Stop {
        serial: serial.to_owned(),
    };
    let outcome =
        answer!(client.call_ok(request).await?, Response::Stopped { outcome, .. } => outcome);
    if json {
        print_json(&outcome)?;
    } else {
        println!("{serial}: {outcome}");
    }
    Ok(exit::for_outcome(&outcome))
}
