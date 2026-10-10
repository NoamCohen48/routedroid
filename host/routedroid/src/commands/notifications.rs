//! `routedroid notifications [on|off]`: the daemon's desktop notifications.

use anyhow::Result;
use clap::ValueEnum;
use routedroid_ipc::{Client, Request, Response};

use super::answer;
use crate::output::print_json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Switch {
    On,
    Off,
}

pub async fn run(client: &Client, set: Option<Switch>, json: bool) -> Result<i32> {
    let request = Request::Notifications {
        on: set.map(|set| set == Switch::On),
    };
    let on = answer!(client.call_ok(request).await?, Response::Notifications { on } => on);
    if json {
        print_json(&serde_json::json!({ "on": on }))?;
    } else if on {
        println!("notifications are on (`routedroid notifications off` turns them off)");
    } else {
        println!("notifications are off (`routedroid notifications on` turns them on)");
    }
    Ok(0)
}
