//! Why the helper refused us, when it did. It admits root and the members
//! of group `routedroid`, as the group database has them when we connect;
//! a socket that refuses outright (EACCES) is one from before that, still
//! limited to the group's members as their sessions had it at login.

use std::fs;

const GROUP: &str = "routedroid";

/// What to do about a refusal.
pub fn hint() -> String {
    let user = std::env::var("USER").unwrap_or_default();
    let etc = fs::read_to_string("/etc/group").unwrap_or_default();
    advice(&user, &etc)
}

fn advice(user: &str, etc_group: &str) -> String {
    let Some(members) = etc_group.lines().find_map(|line| members(line)) else {
        return format!(
            "group {GROUP} does not exist: install Routedroid (its package, or install.sh)"
        );
    };
    if members.split(',').any(|m| m == user) {
        return format!(
            "{user} is in group {GROUP}, so the helper's socket predates this version: \
             sudo systemctl restart routedroid-helper.socket"
        );
    }
    format!("add {user} to group {GROUP}: sudo routedroid setup")
}

/// `routedroid:x:989:dev,alice` → "dev,alice".
fn members(line: &str) -> Option<&str> {
    let mut fields = line.split(':');
    (fields.next()? == GROUP).then_some(())?;
    Some(fields.nth(2).unwrap_or(""))
}

#[cfg(test)]
mod tests;
