//! A first `routedroid start` on a PC nobody has set up: instead of an error
//! and a command to copy, offer to run `sudo routedroid setup` (the user
//! types their password), then go on with the start. Only at a terminal;
//! scripts get the daemon's error, which names the same command.

use std::io::{BufRead, IsTerminal, Write};
use std::process::Command;
use std::time::Duration;

use anyhow::Result;
use routedroid_ipc::{Client, InterfaceInfo, Request, Response};

use crate::commands::setup::{GROUP, in_group};

/// What the start does after the offer.
#[derive(Debug, PartialEq, Eq)]
pub enum After {
    /// Go on: nothing was missing.
    Start,
    /// Go on: set up just now (the daemon may be starting).
    SetUp,
    /// End with this code: declined, or set up but only usable after the
    /// user logs in again (setup said so).
    Exit(i32),
}

/// Why this PC is not set up for `user`, if it is not. `enabled`: whether
/// the daemon is enabled, asked only when it is not running. `interfaces`:
/// the helper's survey, when the daemon could get it.
pub fn missing(
    joined: bool,
    enabled: Option<bool>,
    interfaces: Option<&[InterfaceInfo]>,
) -> Option<String> {
    if !joined {
        return Some(format!("you are not in group {GROUP} yet"));
    }
    if enabled == Some(false) {
        return Some("the daemon is not enabled".into());
    }
    let allowed = |list: &[InterfaceInfo]| list.iter().any(|i| i.ineligible.is_none());
    match interfaces {
        Some(list) if !allowed(list) => Some("no interface may carry phones yet".into()),
        _ => None,
    }
}

/// Enter, "y" or "yes".
pub fn yes(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "" | "y" | "yes")
}

/// Check, and offer setup if something is missing. `client`: `None` when
/// the daemon is not running.
pub async fn offer(client: Option<&Client>, json: bool) -> Result<After> {
    let terminal = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if json || !terminal {
        return Ok(After::Start);
    }
    let user = std::env::var("USER").unwrap_or_default();
    let joined = user.is_empty() || in_group(&user).unwrap_or(true);
    let enabled = match client {
        Some(_) => None,
        None => Some(daemon_enabled()),
    };
    let interfaces = match client {
        Some(client) => match client.call_ok(Request::Interfaces).await {
            Ok(Response::Interfaces { interfaces }) => Some(interfaces),
            _ => None,
        },
        None => None,
    };
    let Some(why) = missing(joined, enabled, interfaces.as_deref()) else {
        return Ok(After::Start);
    };
    eprint!("Routedroid is not set up yet: {why}.\nSet it up now (sudo routedroid setup)? [Y/n] ");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    if !yes(&answer) {
        eprintln!("run `sudo routedroid setup` when you are ready");
        return Ok(After::Exit(2));
    }
    let program = std::env::current_exe().unwrap_or_else(|_| "routedroid".into());
    let status = match Command::new("sudo")
        .arg("--")
        .arg(program)
        .args(["setup", "--then-start"])
        .status()
    {
        Ok(status) => status,
        Err(error) => {
            eprintln!("error: could not run sudo: {error}; as root, run `routedroid setup`");
            return Ok(After::Exit(2));
        }
    };
    match status.code() {
        Some(0) if joined => Ok(After::SetUp),
        // A group joined just now reaches this session only at the next login.
        Some(0) => Ok(After::Exit(0)),
        code => Ok(After::Exit(code.unwrap_or(1))),
    }
}

fn daemon_enabled() -> bool {
    let status = Command::new("systemctl")
        .args(["--user", "--quiet", "is-enabled", "routedroid"])
        .status();
    // No systemctl to ask: not for us to say.
    status.map_or(true, |s| s.success())
}

/// The daemon setup just started may take a moment to listen.
pub async fn reconnect(socket: &std::path::Path) -> Result<Client> {
    for _ in 0..20 {
        if let Ok(client) = Client::connect(socket).await {
            return Ok(client);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(Client::connect(socket).await?)
}

#[cfg(test)]
mod tests;
