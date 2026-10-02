//! The one task that owns the daemon connection: runs each UI command as its
//! own call (a `stop` that waits for a teardown holds up neither events nor
//! other commands), forwards events, and reconnects every few seconds when
//! the daemon goes away. It never refreshes on its own: `Connected` tells
//! the UI, which asks for what it shows.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use routedroid_ipc::{Calls, Client, Request, Response};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::messages::{Command, Incoming};

pub const RECONNECT_EVERY: Duration = Duration::from_secs(2);

/// Runs until the command channel closes (the UI quit).
pub async fn run(
    socket: PathBuf,
    client: Client,
    mut commands: mpsc::Receiver<Command>,
    incoming: mpsc::Sender<Incoming>,
) {
    let mut client = Some(client);
    loop {
        let Some(connected) = client.take() else {
            match Client::connect(&socket).await {
                Ok(connected) => client = Some(connected),
                Err(_) if wait_disconnected(&mut commands, &incoming).await => {}
                Err(_) => return,
            }
            continue;
        };
        let Err(error) = serve(connected, &mut commands, &incoming).await else {
            return;
        };
        let reason = format!("{error:#}");
        if incoming
            .send(Incoming::Disconnected { reason })
            .await
            .is_err()
        {
            return;
        }
    }
}

/// Answers commands with a failure while waiting one reconnect interval;
/// `false` when the UI quit.
async fn wait_disconnected(
    commands: &mut mpsc::Receiver<Command>,
    incoming: &mpsc::Sender<Incoming>,
) -> bool {
    let deadline = tokio::time::sleep(RECONNECT_EVERY);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return true,
            command = commands.recv() => {
                let Some(command) = command else { return false };
                let (what, serial) = name(&command);
                let failed = Incoming::Failed { what, serial, message: "not connected".into() };
                if incoming.send(failed).await.is_err() {
                    return false;
                }
            }
        }
    }
}

/// `Ok(())` when the UI quit; `Err` when the connection broke.
async fn serve(
    client: Client,
    commands: &mut mpsc::Receiver<Command>,
    incoming: &mpsc::Sender<Incoming>,
) -> Result<()> {
    let (calls, mut events) = client.into_parts();
    calls.call_ok(Request::Subscribe).await?;
    incoming.send(Incoming::Connected).await?;
    // Dropped (aborting what is still running) when the connection breaks:
    // their answers could not arrive anyway.
    let mut running = JoinSet::new();
    loop {
        tokio::select! {
            event = events.next() => match event? {
                Some(event) => incoming.send(Incoming::Event(event)).await?,
                None => anyhow::bail!("routedroidd closed the connection"),
            },
            command = commands.recv() => {
                let Some(command) = command else { return Ok(()) };
                let (calls, incoming) = (calls.clone(), incoming.clone());
                running.spawn(async move {
                    let (what, serial) = name(&command);
                    // A broken connection also ends `events`, which reconnects.
                    let outcome = execute(&calls, command).await.unwrap_or_else(|error| {
                        Incoming::Failed { what, serial, message: format!("{error:#}") }
                    });
                    let _ = incoming.send(outcome).await;
                });
            }
            Some(_) = running.join_next() => {}
        };
    }
}

/// One call; a daemon `Error` becomes `Incoming::Failed`, a transport error propagates.
async fn execute(calls: &Calls, command: Command) -> Result<Incoming> {
    let (what, serial) = name(&command);
    let request = match command {
        Command::RefreshDevices => Request::Devices,
        Command::RefreshStatus => Request::Status,
        Command::RefreshInterfaces => Request::Interfaces,
        Command::Start(request) => Request::Start(request),
        Command::Stop { serial } => Request::Stop { serial },
    };
    Ok(match calls.call(request).await? {
        Response::Error { kind, message } => Incoming::Failed {
            what,
            serial,
            message: format!("{message} ({kind})"),
        },
        Response::Devices { devices } => Incoming::Devices(devices),
        Response::Status { connections } => Incoming::Connections(connections),
        Response::Interfaces { interfaces } => Incoming::Interfaces(interfaces),
        Response::Started { serial, tun } => Incoming::Started { serial, tun },
        Response::Stopped { serial, outcome } => Incoming::Stopped { serial, outcome },
        other @ (Response::Version { .. } | Response::Subscribed) => Incoming::Failed {
            what,
            serial,
            message: format!("unexpected answer {other:?}"),
        },
    })
}

/// What a command is called in the log, and the phone it is about.
fn name(command: &Command) -> (&'static str, Option<String>) {
    match command {
        Command::RefreshDevices => ("devices", None),
        Command::RefreshStatus => ("status", None),
        Command::RefreshInterfaces => ("interfaces", None),
        Command::Start(request) => ("start", Some(request.serial.clone())),
        Command::Stop { serial } => ("stop", Some(serial.clone())),
    }
}
