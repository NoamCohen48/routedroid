//! One module per subcommand; `run` dispatches and yields the exit code.

pub mod devices;
pub mod docs;
pub mod doctor;
pub mod events;
pub mod interfaces;
pub mod start;
pub mod status;
pub mod stop;
pub mod version;

use anyhow::Result;
use routedroid_ipc::Client;

use crate::cli::{Cli, Command};

pub async fn run(cli: Cli) -> Result<i32> {
    match &cli.command {
        // Our own version needs no daemon, so it shows even without one.
        Command::Version => return version::run(&cli.socket, cli.json).await,
        Command::Completions { shell } => return Ok(docs::completions(*shell)),
        Command::Manpages { dir } => return docs::manpages(dir),
        _ => {}
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
        Command::Version | Command::Completions { .. } | Command::Manpages { .. } => {
            unreachable!("answered above")
        }
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
