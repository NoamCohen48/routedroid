//! The one task that owns the daemon connection: runs each UI command as its
//! own call (a `stop` that waits for a teardown holds up neither events nor
//! other commands), forwards events, and reconnects every few seconds when
//! the daemon goes away.

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
                Ok(connected) => {
                    client = Some(connected);
                    if incoming.send(Incoming::Connected).await.is_err() {
                        return;
                    }
                }
                Err(_) => {
                    if !wait_disconnected(&mut commands, &incoming).await {
                        return;
                    }
                }
            }
            continue;
        };
        match serve(connected, &mut commands, &incoming).await {
            Ok(()) => return,
            Err(error) => {
                if incoming.send(Incoming::Disconnected { reason: format!("{error:#}") }).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Answers commands with a failure while waiting one reconnect interval; `false` when the UI quit.
async fn wait_disconnected(commands: &mut mpsc::Receiver<Command>, incoming: &mpsc::Sender<Incoming>) -> bool {
    let deadline = tokio::time::sleep(RECONNECT_EVERY);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return true,
            command = commands.recv() => match command {
                None => return false,
                Some(command) => {
                    let failed = Incoming::Failed { what: name(&command).into(), message: "not connected".into() };
                    if incoming.send(failed).await.is_err() {
                        return false;
                    }
                }
            },
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
    for command in [Command::RefreshDevices, Command::RefreshStatus] {
        let outcome = execute(&calls, command).await?;
        incoming.send(outcome).await.ok();
    }
    // Dropped (aborting what is still running) when the connection breaks:
    // their answers could not arrive anyway.
    let mut running = JoinSet::new();
    loop {
        tokio::select! {
            event = events.next() => match event? {
                Some(event) => {
                    incoming.send(Incoming::Event(event)).await.ok();
                }
                None => anyhow::bail!("routedroidd closed the connection"),
            },
            command = commands.recv() => match command {
                None => return Ok(()),
                Some(command) => {
                    let (calls, incoming) = (calls.clone(), incoming.clone());
                    running.spawn(async move {
                        let what = name(&command);
                        // A broken connection also ends `events`, which reconnects.
                        let outcome = execute(&calls, command).await.unwrap_or_else(|error| Incoming::Failed {
                            what: what.into(),
                            message: format!("{error:#}"),
                        });
                        incoming.send(outcome).await.ok();
                    });
                }
            },
            Some(_) = running.join_next() => {}
        };
    }
}

/// One call; a daemon `Error` becomes `Incoming::Failed`, a transport error propagates.
async fn execute(calls: &Calls, command: Command) -> Result<Incoming> {
    let what = name(&command);
    let request = match command {
        Command::RefreshDevices => Request::Devices,
        Command::RefreshStatus => Request::Status,
        Command::Start(request) => Request::Start(request),
        Command::Stop { serial } => Request::Stop { serial },
    };
    let serial_of_stop = match &request {
        Request::Stop { serial } => Some(serial.clone()),
        _ => None,
    };
    Ok(match calls.call(request).await? {
        Response::Error { kind, message } => {
            Incoming::Failed { what: what.into(), message: format!("{message} ({})", kind.as_str()) }
        }
        Response::Devices { devices } => Incoming::Devices(devices),
        Response::Status { connections } => Incoming::Connections(connections),
        Response::Started { serial } => Incoming::Started { serial },
        Response::Ok => Incoming::Stopped { serial: serial_of_stop.unwrap_or_default() },
        Response::Version { .. } => Incoming::Failed { what: what.into(), message: "unexpected version reply".into() },
    })
}

fn name(command: &Command) -> &'static str {
    match command {
        Command::RefreshDevices => "devices",
        Command::RefreshStatus => "status",
        Command::Start(_) => "start",
        Command::Stop { .. } => "stop",
    }
}
