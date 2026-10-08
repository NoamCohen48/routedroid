//! One module per subcommand; `run` dispatches and yields the exit code.

pub mod devices;
pub mod docs;
pub mod doctor;
pub mod events;
pub mod interfaces;
mod options;
pub mod phones;
pub mod setup;
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
    if let Command::Setup(args) = cli.command {
        // Run as root, so it does not need (or reach) the user's daemon.
        return setup::run(args).await;
    }
    if let Command::Start(args) = cli.command {
        return start_first(&cli.socket, args, cli.json).await;
    }
    let mut client = Client::connect(&cli.socket).await?;
    let json = cli.json;
    match cli.command {
        Command::Devices => devices::run(&client, json).await,
        Command::Interfaces => interfaces::run(&client, json).await,
        Command::Stop { phone, serial } => stop::run(&client, phone.or(serial), json).await,
        Command::Phones => phones::list(&client, json).await,
        Command::Remember(args) => phones::run_remember(&client, args, json).await,
        Command::Forget { phone } => phones::forget(&client, &phone, json).await,
        Command::Status => status::run(&client, json).await,
        Command::Events => events::run(&mut client, json).await,
        Command::Doctor { repair } => doctor::run(&client, repair, json).await,
        Command::Version
        | Command::Setup(_)
        | Command::Start(_)
        | Command::Completions { .. }
        | Command::Manpages { .. } => unreachable!("answered above"),
    }
}

/// `start`, offering setup first on a PC that is not set up.
async fn start_first(socket: &std::path::Path, args: start::StartArgs, json: bool) -> Result<i32> {
    use start::first_time::{self, After};
    let client = match Client::connect(socket).await {
        Ok(client) => Some(client),
        Err(routedroid_ipc::ConnectError::Unreachable { .. }) => None,
        Err(error) => return Err(error.into()),
    };
    match first_time::offer(client.as_ref(), json).await? {
        After::Exit(code) => Ok(code),
        after => {
            let client = match client {
                Some(client) => client,
                None if after == After::SetUp => first_time::reconnect(socket).await?,
                None => Client::connect(socket).await?,
            };
            start::run(client, args, json).await
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
