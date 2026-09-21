//! `routedroid-tui`: a keyboard-driven terminal UI for `routedroidd`.
//!
//! One task owns the daemon connection (`client_task`); the render loop here
//! folds its messages and key presses into the `App` state and redraws.

mod app;
mod client_task;
mod describe;
mod form;
mod keys;
mod messages;
mod ui;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{Event as TerminalEvent, EventStream};
use futures_util::StreamExt;
use ratatui::DefaultTerminal;
use routedroid_ipc::Client;
use tokio::sync::mpsc;

use app::{App, Level};
use messages::{Command, Incoming};

/// Exit code when the daemon is not reachable at start.
const EXIT_NO_DAEMON: i32 = 3;

#[derive(Debug, Parser)]
#[command(name = "routedroid-tui", version, about = "Routedroid terminal UI: control routedroidd from the keyboard")]
struct Args {
    /// Control socket of routedroidd.
    #[arg(long, env = routedroid_ipc::socket::SOCKET_ENV, default_value_os_t = routedroid_ipc::socket::default_path())]
    socket: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let client = match Client::connect(&args.socket).await {
        Ok(client) => client,
        Err(_) => {
            eprintln!("routedroidd is not running: systemctl --user start routedroid");
            std::process::exit(EXIT_NO_DAEMON);
        }
    };

    let (command_sender, command_receiver) = mpsc::channel::<Command>(32);
    let (incoming_sender, incoming_receiver) = mpsc::channel::<Incoming>(256);
    let client_task = tokio::spawn(client_task::run(args.socket, client, command_receiver, incoming_sender));

    let terminal = ratatui::init();
    let outcome = run_ui(terminal, command_sender, incoming_receiver).await;
    ratatui::restore();
    client_task.abort();
    if let Err(error) = outcome {
        eprintln!("routedroid-tui: {error:#}");
        std::process::exit(1);
    }
}

async fn run_ui(
    mut terminal: DefaultTerminal,
    commands: mpsc::Sender<Command>,
    mut incoming: mpsc::Receiver<Incoming>,
) -> Result<()> {
    let mut app = App::new();
    let mut terminal_events = EventStream::new();
    loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;
        let followups = tokio::select! {
            message = incoming.recv() => match message {
                Some(message) => app.apply(message),
                None => anyhow::bail!("connection task ended"),
            },
            terminal_event = terminal_events.next() => match terminal_event {
                Some(Ok(TerminalEvent::Key(key))) => keys::handle(&mut app, key),
                Some(Ok(_)) => vec![],
                Some(Err(error)) => return Err(error.into()),
                None => return Ok(()),
            },
        };
        for command in followups {
            // Never block rendering on the connection task (a `stop` can take seconds).
            match commands.try_send(command) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => app.push_log(Level::Error, "busy; try again".into()),
                Err(mpsc::error::TrySendError::Closed(_)) => anyhow::bail!("connection task ended"),
            }
        }
        if app.quit {
            return Ok(());
        }
    }
}
