//! Following one connection until it ends. The first Ctrl-C asks the daemon
//! to stop it and keeps following, so the exit code reflects how it ended;
//! a second one stops waiting (the daemon finishes the stop on its own).

use anyhow::{Result, bail};
use routedroid_ipc::{Calls, Client, ConnectionState, Event, Events, Outcome, Request, Response};
use tokio::sync::mpsc;

use crate::exit;
use crate::output::{network_line, print_json_line};

/// Every Ctrl-C from now on, as a message.
pub fn interrupts() -> mpsc::UnboundedReceiver<()> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(
        async move { while tokio::signal::ctrl_c().await.is_ok() && tx.send(()).is_ok() {} },
    );
    rx
}

pub struct Follow {
    calls: Calls,
    events: Events,
    serial: String,
    json: bool,
}

impl Follow {
    pub fn new(client: Client, serial: String, json: bool) -> Self {
        let (calls, events) = client.into_parts();
        Self {
            calls,
            events,
            serial,
            json,
        }
    }

    pub async fn run(mut self, mut interrupts: mpsc::UnboundedReceiver<()>) -> Result<i32> {
        let mut stopping = false;
        loop {
            let event = tokio::select! {
                event = self.events.next() => event?,
                Some(()) = interrupts.recv() => {
                    if stopping {
                        eprintln!("no longer waiting; routedroidd finishes disconnecting {}", self.serial);
                        return Ok(exit::ABANDONED);
                    }
                    stopping = true;
                    eprintln!("disconnecting {} (Ctrl-C again to stop waiting)", self.serial);
                    self.stop();
                    continue;
                }
            };
            let Some(event) = event else {
                bail!("routedroidd closed the connection")
            };
            if let Some(outcome) = self.on_event(event).await? {
                return Ok(exit::for_outcome(&outcome));
            }
        }
    }

    /// Prints what concerns our connection; `Some` once it has ended.
    async fn on_event(&mut self, event: Event) -> Result<Option<Outcome>> {
        let ours = match &event {
            Event::Connection { serial, .. } | Event::Network { serial, .. } => {
                serial == &self.serial
            }
            Event::Shutdown => true,
            Event::Lagged { .. } => return self.ended_meanwhile().await,
            Event::Traffic { .. } | Event::Devices { .. } => false,
        };
        if !ours {
            return Ok(None);
        }
        if self.json {
            print_json_line(&event)?;
        }
        match event {
            Event::Connection {
                state: ConnectionState::Ended { outcome },
                ..
            } => {
                if !self.json {
                    println!("ended: {outcome}");
                }
                Ok(Some(outcome))
            }
            _ if self.json => Ok(None),
            Event::Connection { state, .. } => {
                println!("{state:#}");
                Ok(None)
            }
            Event::Network { network, .. } => {
                println!("{}", network_line(&network));
                Ok(None)
            }
            Event::Shutdown => {
                eprintln!("routedroidd is shutting down");
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Ask for the stop on the same connection: events keep flowing while
    /// the daemon tears down, and a refusal is said, not swallowed.
    fn stop(&self) {
        let (calls, serial) = (self.calls.clone(), self.serial.clone());
        tokio::spawn(async move {
            if let Err(error) = calls.call_ok(Request::Stop { serial }).await {
                eprintln!("error: stop failed: {error:#}");
            }
        });
    }

    /// After missed events: `Some(outcome)` if our phone is no longer connected.
    async fn ended_meanwhile(&self) -> Result<Option<Outcome>> {
        let response = self.calls.call_ok(Request::Status).await?;
        let Response::Status { connections } = response else {
            bail!("unexpected answer to status: {response:?}");
        };
        if connections.iter().any(|c| c.serial == self.serial) {
            return Ok(None);
        }
        let message = "the connection ended while events were missed; its reason was lost";
        eprintln!("ended: {message}");
        Ok(Some(Outcome::failed(
            routedroid_ipc::Kind::Internal,
            message,
        )))
    }
}
