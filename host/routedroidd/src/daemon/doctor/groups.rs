//! Why the helper socket refused us, when it did: group `routedroid` is
//! missing from this process, though perhaps not from the user. A
//! `systemd --user` manager started before `usermod -aG` keeps its old
//! groups across logins, and so does every daemon it starts.

use std::fs;

const GROUP: &str = "routedroid";

/// What to do about a refused helper socket.
pub fn hint() -> String {
    let user = std::env::var("USER").unwrap_or_default();
    let etc = fs::read_to_string("/etc/group").unwrap_or_default();
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    advice(&user, &etc, &status)
}

fn advice(user: &str, etc_group: &str, proc_status: &str) -> String {
    let Some((gid, members)) = etc_group.lines().find_map(|line| entry(line)) else {
        return format!("group {GROUP} does not exist: run host/install.sh");
    };
    let held = proc_status
        .lines()
        .find_map(|l| l.strip_prefix("Groups:"))
        .is_some_and(|ids| ids.split_whitespace().any(|id| id == gid));
    if held {
        return "this daemon is in group routedroid; is routedroid-helper.socket enabled?".into();
    }
    if members.split(',').any(|m| m == user) {
        return format!(
            "{user} is in group {GROUP}, but this daemon started before that: log out \
             completely (or `sudo systemctl restart user@$(id -u)`), then start it again"
        );
    }
    format!(
        "add {user} to group {GROUP} (`sudo usermod -aG {GROUP} {user}`), then log out completely"
    )
}

/// `routedroid:x:989:dev,alice` → ("989", "dev,alice").
fn entry(line: &str) -> Option<(&str, &str)> {
    let mut fields = line.split(':');
    (fields.next()? == GROUP).then_some(())?;
    let _password = fields.next()?;
    Some((fields.next()?, fields.next().unwrap_or("")))
}

#[cfg(test)]
mod tests;
