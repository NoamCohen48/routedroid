//! Structured logging: human-readable by default, JSON lines with `--log-json`.
//! Colour only on a terminal, so journald and log files get plain text.

use std::io::IsTerminal;

use clap::Args;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Args)]
pub struct LogOptions {
    /// Log filter, e.g. `info`, `debug`, `routedroid=trace` (also RUST_LOG).
    #[arg(long, global = true, env = "RUST_LOG", default_value = "info")]
    pub log: String,
    /// Emit one JSON object per line instead of text.
    #[arg(long, global = true)]
    pub log_json: bool,
}

/// Fails on a filter that does not parse: a typo must not silently log at `info`.
pub fn init(opts: &LogOptions) -> Result<(), String> {
    let filter = EnvFilter::try_new(&opts.log)
        .map_err(|e| format!("invalid --log / RUST_LOG {:?}: {e}", opts.log))?;
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false);
    if opts.log_json {
        builder.json().init();
    } else {
        builder.init();
    }
    Ok(())
}
