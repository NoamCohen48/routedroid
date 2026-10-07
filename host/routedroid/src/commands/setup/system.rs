//! What `setup` changes outside the policy: who it is for, their group, and
//! their user daemon. Each step is a system command (`id`, `usermod`,
//! `systemctl`), the way an administrator would do it by hand.

use std::process::{Command, Output};

use anyhow::{Context, Result};

use super::Usage;

pub const GROUP: &str = "routedroid";

pub fn is_root() -> bool {
    rustix::process::geteuid().is_root()
}

/// `--user`, else whoever ran sudo; root's own phones need `--user root`.
pub fn target_user(flag: Option<String>, sudo_user: Option<String>) -> Result<String> {
    flag.or(sudo_user.filter(|user| !user.is_empty() && user != "root"))
        .ok_or_else(|| {
            Usage(
                "say who will connect phones: --user NAME (or run it with sudo from their account)"
                    .into(),
            )
            .into()
        })
}

fn run(program: &str, args: &[&str]) -> Result<Output> {
    Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("run {program}"))
}

pub fn group_exists() -> Result<bool> {
    Ok(run("getent", &["group", GROUP])?.status.success())
}

/// Whether `user` is in the group (by /etc/group; a session started before
/// they joined does not have it yet).
pub fn in_group(user: &str) -> Result<bool> {
    let out = run("id", &["-nG", user])?;
    if !out.status.success() {
        return Err(Usage(format!("there is no user {user:?}")).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .any(|g| g == GROUP))
}

pub fn add_to_group(user: &str) -> Result<()> {
    let out = run("usermod", &["-aG", GROUP, user])?;
    anyhow::ensure!(
        out.status.success(),
        "usermod -aG {GROUP} {user} failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(())
}

/// What became of the user's daemon.
#[derive(Debug, PartialEq, Eq)]
pub enum Daemon {
    /// Enabled (and started, if asked) just now.
    Enabled {
        started: bool,
    },
    AlreadyEnabled,
    /// Their systemd manager is not running (not logged in): enabled later.
    NoManager,
    /// Not booted with systemd: nothing to enable.
    NoSystemd,
}

/// Enable `routedroid.service` in `user`'s manager; `start` it too when
/// their session already has the group.
pub fn enable_daemon(user: &str, start: bool) -> Daemon {
    if !std::path::Path::new("/run/systemd/system").exists() {
        return Daemon::NoSystemd;
    }
    let machine = format!("{user}@");
    let user_ctl = |args: &[&str]| {
        let mut all = vec!["--user", "-M", machine.as_str()];
        all.extend_from_slice(args);
        run("systemctl", &all)
    };
    match user_ctl(&["is-enabled", "routedroid"]) {
        Ok(out) if out.status.success() => return Daemon::AlreadyEnabled,
        Ok(out) if String::from_utf8_lossy(&out.stdout).trim().is_empty() => {
            return Daemon::NoManager;
        }
        Err(_) => return Daemon::NoManager,
        Ok(_) => {}
    }
    let args: &[&str] = if start {
        &["enable", "--now", "routedroid"]
    } else {
        &["enable", "routedroid"]
    };
    match user_ctl(args) {
        Ok(out) if out.status.success() => Daemon::Enabled { started: start },
        _ => Daemon::NoManager,
    }
}
