//! Accept controllers and give each one its own session. Under systemd the
//! socket unit is `Accept=yes`, so a process serves exactly one connection;
//! with `--socket` (the test rigs) one process serves every connection.
//!
//! SIGTERM and SIGINT end every relay, and each session is undone before
//! the process exits; `cleanup` after an exit is the backstop, not the plan.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use routedroid_helper_ipc::{Activated, Activation, Listener, SeqPacket};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;
use tokio::task::{JoinSet, spawn_blocking};
use tracing::{info, warn};

use crate::connection;
use crate::env::Env;
use crate::kernel::System;
use crate::recovery;

const ACCEPT_BACKOFF: Duration = Duration::from_millis(200);

pub struct Options {
    pub socket: Option<PathBuf>,
    pub allow_uid: Option<u32>,
    /// Exit after the first session (what an `Accept=yes` instance does anyway).
    pub once: bool,
}

pub async fn serve(
    env: Env<System>,
    options: Options,
    activation: Option<Activation>,
) -> Result<()> {
    let env = Arc::new(env);
    // Orphans cannot collide with new sessions (their journals reserve their
    // names), so a failed cleanup is reported, not fatal.
    let recovering = Arc::clone(&env);
    if let Err(e) = spawn_blocking(move || recovery::cleanup(&recovering)).await? {
        warn!(error = %format!("{e:#}"), "startup cleanup incomplete");
    }
    let shutdown = shutdown_signal()?;

    let listener = match activation.map(Activation::register).transpose()? {
        Some(Activated::Connection(conn)) => {
            info!("serving one connection from systemd (Accept=yes)");
            return connection::serve(env, admit(&options, conn)?, shutdown).await;
        }
        Some(Activated::Listener(listener)) => listener,
        None => {
            let path = options
                .socket
                .as_deref()
                .context("--socket required without systemd activation")?;
            Listener::bind(path).with_context(|| format!("bind {}", path.display()))?
        }
    };
    info!("listening");
    let mut sessions = JoinSet::new();
    let mut stopping = shutdown.clone();
    loop {
        let accepted = tokio::select! {
            () = stopped(&mut stopping) => break,
            accepted = listener.accept() => accepted,
        };
        let conn = match accepted {
            Ok(conn) => conn,
            Err(error) => {
                warn!(%error, "accept failed");
                tokio::time::sleep(ACCEPT_BACKOFF).await;
                continue;
            }
        };
        let Ok(conn) = admit(&options, conn) else {
            continue;
        };
        if options.once {
            return connection::serve(env, conn, shutdown).await;
        }
        let (env, shutdown) = (Arc::clone(&env), shutdown.clone());
        sessions.spawn(async move {
            if let Err(error) = connection::serve(env, conn, shutdown).await {
                warn!(error = %format!("{error:#}"), "session failed");
            }
        });
    }
    info!(
        sessions = sessions.len(),
        "shutting down; undoing active sessions"
    );
    while sessions.join_next().await.is_some() {}
    Ok(())
}

/// Resolves once shutdown is signalled (or can no longer be).
pub async fn stopped(shutdown: &mut watch::Receiver<bool>) {
    let _ = shutdown.wait_for(|stop| *stop).await;
}

/// Flips to `true` on the first SIGTERM or SIGINT.
fn shutdown_signal() -> Result<watch::Receiver<bool>> {
    let (tx, rx) = watch::channel(false);
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;
    tokio::spawn(async move {
        tokio::select! {
            _ = term.recv() => info!("SIGTERM"),
            _ = int.recv() => info!("SIGINT"),
        }
        let _ = tx.send(true);
    });
    Ok(rx)
}

/// The uid gate; socket permissions are the primary one.
fn admit(options: &Options, conn: SeqPacket) -> Result<SeqPacket> {
    let uid = conn.peer_uid()?;
    if let Some(want) = options.allow_uid
        && uid != want
    {
        warn!(uid, "rejecting controller: uid not allowed");
        bail!("controller uid {uid} not allowed");
    }
    info!(uid, "controller connected");
    Ok(conn)
}
