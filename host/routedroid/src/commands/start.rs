//! `routedroid start` — placeholder until the session driver lands.

use clap::Args;

use crate::fault::{Fault, Kind, Result};

#[derive(Debug, Args)]
pub struct StartArgs {
    /// ADB serial of the phone (see `routedroid devices`).
    #[arg(long, short = 's', env = "ANDROID_SERIAL")]
    pub serial: String,
}

pub async fn run(_adb: &str, _args: StartArgs) -> Result<()> {
    Err(Fault::msg(Kind::Internal, "start is not implemented yet"))
}
