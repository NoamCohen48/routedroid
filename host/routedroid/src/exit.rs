//! Exit codes: one per failure kind, so scripts can branch on why a command
//! failed, plus the two ways of not getting an answer at all.

use routedroid_ipc::{ConnectError, DaemonError, Kind, Outcome};

pub const OK: i32 = 0;
/// `doctor` found something that fails.
pub const PROBLEMS: i32 = 1;
/// Nothing is listening at the control socket.
pub const DAEMON_UNREACHABLE: i32 = 3;
/// The daemon speaks another API version.
pub const DAEMON_INCOMPATIBLE: i32 = 4;
/// A second Ctrl-C: we stopped waiting, the daemon goes on stopping.
pub const ABANDONED: i32 = 130;

pub fn for_kind(kind: Kind) -> i32 {
    match kind {
        Kind::Usage => 2,
        Kind::Adb => 10,
        Kind::Transport => 11,
        Kind::Protocol => 12,
        Kind::Auth => 13,
        Kind::Vpn => 14,
        Kind::Helper => 15,
        Kind::Timeout => 16,
        Kind::Internal => 70,
    }
}

pub fn for_outcome(outcome: &Outcome) -> i32 {
    match outcome {
        Outcome::Clean { .. } => OK,
        Outcome::Failed { kind, .. } => for_kind(*kind),
    }
}

/// Prints the error the way scripts and humans expect and picks its code.
pub fn report(error: &anyhow::Error) -> i32 {
    if let Some(error) = error.downcast_ref::<ConnectError>() {
        eprintln!("error: {error}");
        return match error {
            ConnectError::Unreachable { .. } => {
                eprintln!("hint: start the daemon with `systemctl --user start routedroid`");
                DAEMON_UNREACHABLE
            }
            ConnectError::Incompatible { .. } => {
                eprintln!(
                    "hint: routedroid and routedroidd are from different releases; restart the \
                     daemon after an upgrade (`systemctl --user restart routedroid`)"
                );
                DAEMON_INCOMPATIBLE
            }
            ConnectError::Handshake(_) => for_kind(Kind::Internal),
        };
    }
    if let Some(error) = error.downcast_ref::<DaemonError>() {
        eprintln!("error: {error}");
        return for_kind(error.kind);
    }
    eprintln!("error: {error:#}");
    for_kind(Kind::Internal)
}

/// The `--help` epilogue, built from the table above so it cannot drift,
/// in numeric order.
pub fn help() -> String {
    let mut lines = vec![
        (
            OK,
            "success (for `start`: the connection ended cleanly)".to_string(),
        ),
        (PROBLEMS, "`doctor` found a failing check".into()),
        (
            DAEMON_UNREACHABLE,
            "daemon unreachable; start it with `systemctl --user start routedroid`".into(),
        ),
        (
            DAEMON_INCOMPATIBLE,
            "daemon speaks another API version; restart it after an upgrade".into(),
        ),
        (
            ABANDONED,
            "Ctrl-C twice: stopped waiting, the daemon finishes the stop".into(),
        ),
    ];
    lines.extend(
        Kind::ALL
            .into_iter()
            .map(|kind| (for_kind(kind), kind.to_string())),
    );
    lines.sort_by_key(|(code, _)| *code);
    let lines: Vec<String> = lines
        .iter()
        .map(|(code, meaning)| format!("  {code:<4} {meaning}"))
        .collect();
    format!("Exit codes:\n{}", lines.join("\n"))
}

#[cfg(test)]
mod tests;
