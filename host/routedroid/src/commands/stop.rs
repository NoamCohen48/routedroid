//! `routedroid stop`: end the session on one phone and wait for it to be gone.

use anyhow::Result;
use routedroid_ipc::{Client, Request};

pub async fn run(client: &mut Client, serial: &str) -> Result<i32> {
    client.call_ok(Request::Stop { serial: serial.to_owned() }).await?;
    println!("stopped");
    Ok(0)
}
