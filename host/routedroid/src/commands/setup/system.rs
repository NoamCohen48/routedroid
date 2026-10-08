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

/// Not so in a container: there are units to install but no managers.
fn booted_with_systemd() -> bool {
    std::path::Path::new("/run/systemd/system").exists()
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

/// Whether `user` is in the group, by the group database. The helper asks
/// the same database on each connection, so a session started before they
/// joined needs no new login.
pub fn in_group(user: &str) -> Result<bool> {
    let out = run("id", &["-nG", user])?;
    if !out.status.success() {
        return Err(Usage(format!("there is no user {user:?}")).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .any(|g| g == GROUP))
}

/// Whether a systemd manager of `user`'s is running.
pub fn manager_running(user: &str) -> Result<bool> {
    if !booted_with_systemd() {
        return Ok(false);
    }
    let text = |out: Output| String::from_utf8_lossy(&out.stdout).trim().to_string();
    let uid = text(run("id", &["-u", user])?);
    let unit = format!("user@{uid}.service");
    let pid = text(run(
        "systemctl",
        &["show", "-p", "MainPID", "--value", &unit],
    )?);
    Ok(!pid.is_empty() && pid != "0")
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
    /// Could not be enabled now (no manager, and an older systemd): the
    /// user enables it once logged in.
    NoManager,
    /// Not booted with systemd: nothing to enable.
    NoSystemd,
}

/// Enable `routedroid.service` for `user`. A running manager of theirs
/// (`running`) is asked, and starts the daemon too. Otherwise the link is written as the user, offline, as `systemctl
/// enable` would: asking a manager that is not running starts one.
pub fn enable_daemon(user: &str, running: bool) -> Daemon {
    if !booted_with_systemd() {
        return Daemon::NoSystemd;
    }
    let machine = format!("{user}@");
    let offline = [
        "-u",
        user,
        "--",
        "env",
        "SYSTEMD_OFFLINE=1",
        "systemctl",
        "--user",
    ];
    let user_ctl = |args: &[&str]| match running {
        true => run("systemctl", &[&["--user", "-M", &machine], args].concat()),
        false => run("runuser", &[&offline[..], args].concat()),
    };
    if user_ctl(&["is-enabled", "routedroid"]).is_ok_and(|out| out.status.success()) {
        return Daemon::AlreadyEnabled;
    }
    let start = running;
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
