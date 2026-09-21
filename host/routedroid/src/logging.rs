//! Structured logging: human-readable by default, JSON lines with `--log-json`.

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

pub fn init(opts: &LogOptions) {
    let filter = EnvFilter::try_new(&opts.log).unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).with_target(false);
    if opts.log_json {
        builder.json().init();
    } else {
        builder.init();
    }
}
