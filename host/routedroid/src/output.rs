//! Text rendering shared by the commands: aligned tables and connection states.

use routedroid_ipc::ConnectionState;

/// Prints rows as left-aligned columns, each as wide as its widest cell.
pub fn print_table(rows: &[Vec<String>]) {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|column| rows.iter().filter_map(|row| row.get(column)).map(String::len).max().unwrap_or(0))
        .collect();
    for row in rows {
        let cells: Vec<String> =
            row.iter().enumerate().map(|(column, cell)| format!("{cell:<width$}", width = widths[column])).collect();
        println!("{}", cells.join("  ").trim_end());
    }
}

/// Short state word for tables.
pub fn state_word(state: &ConnectionState) -> String {
    match state {
        ConnectionState::Starting => "starting".into(),
        ConnectionState::WaitingForApp => "waiting for app".into(),
        ConnectionState::Handshaking => "handshaking".into(),
        ConnectionState::Active => "active".into(),
        ConnectionState::Stopping => "stopping".into(),
        ConnectionState::Ended(outcome) if outcome.ok => "ended".into(),
        ConnectionState::Ended(outcome) => format!("ended: {}", outcome.message),
    }
}

/// One status line for an attached `start`, telling the user what to do.
pub fn state_line(state: &ConnectionState) -> String {
    match state {
        ConnectionState::WaitingForApp => "waiting for the app to connect (answer the VPN dialog on the phone)".into(),
        ConnectionState::Ended(outcome) => format!("ended: {}", outcome.message),
        other => state_word(other),
    }
}

pub fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
