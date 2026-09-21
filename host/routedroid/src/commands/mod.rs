//! One module per subcommand; `run` dispatches and yields the exit code.

pub mod devices;
pub mod events;
pub mod start;
pub mod status;
pub mod stop;
pub mod version;

use anyhow::Result;

use crate::cli::{Cli, Command};
use crate::connect::connect;

pub async fn run(cli: Cli) -> Result<i32> {
    let mut client = connect(&cli.socket).await?;
    match cli.command {
        Command::Devices => devices::run(&mut client, cli.json).await,
        Command::Start(args) => start::run(&mut client, &cli.socket, args).await,
        Command::Stop { serial } => stop::run(&mut client, &serial).await,
        Command::Status => status::run(&mut client, cli.json).await,
        Command::Events => events::run(&mut client).await,
        Command::Version => version::run(&mut client).await,
    }
}
