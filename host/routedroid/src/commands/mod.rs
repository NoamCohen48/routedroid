//! One module per subcommand; `run` dispatches and yields the exit code.

pub mod devices;
pub mod doctor;
pub mod events;
pub mod interfaces;
pub mod setup;
pub mod start;
pub mod status;
pub mod stop;
pub mod version;

use anyhow::Result;
use routedroid_ipc::Client;

use crate::cli::{Cli, Command};

pub async fn run(cli: Cli) -> Result<i32> {
    if let Command::Version = cli.command {
        // Our own version needs no daemon, so it shows even without one.
        return version::run(&cli.socket, cli.json).await;
    }
    if let Command::Setup(args) = cli.command {
        // Run as root, so it does not need (or reach) the user's daemon.
        return setup::run(args).await;
    }
    let mut client = Client::connect(&cli.socket).await?;
    let json = cli.json;
    match cli.command {
        Command::Devices => devices::run(&client, json).await,
        Command::Interfaces => interfaces::run(&client, json).await,
        Command::Start(args) => start::run(client, args, json).await,
        Command::Stop { serial } => stop::run(&client, &serial, json).await,
        Command::Status => status::run(&client, json).await,
        Command::Events => events::run(&mut client, json).await,
        Command::Doctor { repair } => doctor::run(&client, repair, json).await,
        Command::Version | Command::Setup(_) => unreachable!("answered above"),
    }
}

/// The answer to a request, or an error naming what came instead.
macro_rules! answer {
    ($response:expr, $pattern:pat => $value:expr) => {
        match $response {
            $pattern => $value,
            other => anyhow::bail!("unexpected answer from routedroidd: {other:?}"),
        }
    };
}
pub(crate) use answer;
