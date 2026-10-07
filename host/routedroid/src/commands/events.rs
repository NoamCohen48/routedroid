//! `routedroid events`: what the daemon reports as it happens, one line each
//! (`--json`: the events themselves, as JSON lines, for scripts).

use anyhow::Result;
use routedroid_ipc::{Client, Event, Request};

use crate::output::{network_line, print_json_line};

pub async fn run(client: &mut Client, json: bool) -> Result<i32> {
    client.call_ok(Request::Subscribe).await?;
    let mut last = None;
    loop {
        let event = tokio::select! {
            event = client.next_event() => event?,
            _ = tokio::signal::ctrl_c() => return Ok(0),
        };
        match event {
            Some(event) if json => print_json_line(&event)?,
            Some(event) => {
                let Some(line) = line(&event) else { continue };
                // The phone list comes again on any change in adb's view of it.
                if matches!(event, Event::Devices { .. })
                    && last.replace(line.clone()) == Some(line.clone())
                {
                    continue;
                }
                println!("{} {line}", chrono::Local::now().format("%H:%M:%S"));
            }
            None => return Ok(0),
        }
    }
}

/// What a person reads for `event`; `None` for traffic, which comes every
/// second (`routedroid status` has the counters).
fn line(event: &Event) -> Option<String> {
    match event {
        Event::Connection { serial, state } => Some(format!("{serial}: {state:#}")),
        Event::Network { serial, network } => Some(format!("{serial}: {}", network_line(network))),
        Event::Traffic { .. } => None,
        Event::Devices { devices } if devices.is_empty() => Some("phones attached: none".into()),
        Event::Devices { devices } => {
            let phones: Vec<String> = devices
                .iter()
                .map(|d| match d.state.as_str() {
                    "device" => d.serial.clone(),
                    state => format!("{} ({state})", d.serial),
                })
                .collect();
            Some(format!("phones attached: {}", phones.join(", ")))
        }
        Event::Shutdown => Some("the daemon is shutting down".into()),
        Event::Lagged { missed } => Some(format!("missed {missed} events (too slow to read them)")),
    }
}

#[cfg(test)]
mod tests;
