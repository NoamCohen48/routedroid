//! Why the helper refused us, when it did. It admits root and the members
//! of group `routedroid`, as the group database has them when we connect;
//! a socket that refuses outright (EACCES) is one from before that, still
//! limited to the group's members as their sessions had it at login.
//!
//! The database is asked through `getent` and `id`, as the helper asks it
//! through NSS, so users and groups from LDAP or SSSD count the same.

use std::process::Command;

const GROUP: &str = "routedroid";

/// What to do about a refusal.
pub fn hint() -> String {
    let user = std::env::var("USER").unwrap_or_default();
    let exists = Command::new("getent")
        .args(["group", GROUP])
        .output()
        .is_ok_and(|out| out.status.success());
    let groups = Command::new("id")
        .args(["-nG", &user])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();
    advice(&user, exists, &groups)
}

/// `groups`: the user's groups by name, as `id -nG` lists them.
fn advice(user: &str, exists: bool, groups: &str) -> String {
    if !exists {
        return format!(
            "group {GROUP} does not exist: install Routedroid (its package, or install.sh)"
        );
    }
    if groups.split_whitespace().any(|g| g == GROUP) {
        return format!(
            "{user} is in group {GROUP}, so the helper's socket predates this version: \
             sudo systemctl restart routedroid-helper.socket"
        );
    }
    format!("add {user} to group {GROUP}: sudo routedroid setup")
}

#[cfg(test)]
mod tests;
