//! Accept controllers and give each one its own session. Under systemd the
//! socket unit is `Accept=yes`, so a process serves exactly one connection;
//! with `--socket` (the test rigs) one process serves every connection.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tracing::{info, warn};

use crate::connection;
use crate::recovery;
use crate::seqpacket::{Activated, Listener, SeqPacket};

pub struct ServeConfig {
    pub socket: Option<PathBuf>,
    pub journal_dir: PathBuf,
    pub crash_file: PathBuf,
    pub allow_uid: Option<u32>,
    /// Exit after the first session (what an `Accept=yes` instance does anyway).
    pub once: bool,
}

pub async fn serve(cfg: ServeConfig) -> Result<()> {
    recovery::check(&cfg.journal_dir).context("refusing to start")?;
    let cfg = Arc::new(cfg);

    let listener = match Activated::from_systemd()? {
        Some(Activated::Connection(conn)) => {
            info!("serving one connection from systemd (Accept=yes)");
            return connection::serve(&cfg, admit(&cfg, conn)?).await;
        }
        Some(Activated::Listener(listener)) => {
            info!("listening on socket from systemd");
            listener
        }
        None => {
            let path = cfg.socket.as_deref().context("--socket required without systemd activation")?;
            let listener = Listener::bind(path)?;
            info!(path = %path.display(), "listening");
            listener
        }
    };
    loop {
        let conn = match listener.accept().await {
            Ok(conn) => conn,
            Err(error) => {
                warn!(%error, "accept failed");
                continue;
            }
        };
        let conn = match admit(&cfg, conn) {
            Ok(conn) => conn,
            Err(_) => continue,
        };
        if cfg.once {
            return connection::serve(&cfg, conn).await;
        }
        let cfg = Arc::clone(&cfg);
        tokio::spawn(async move {
            if let Err(error) = connection::serve(&cfg, conn).await {
                warn!(error = %format!("{error:#}"), "session failed");
            }
        });
    }
}

/// The uid gate; socket permissions are the primary one.
fn admit(cfg: &ServeConfig, conn: SeqPacket) -> Result<SeqPacket> {
    let uid = conn.peer_uid()?;
    if let Some(want) = cfg.allow_uid {
        if uid != want {
            warn!(uid, "rejecting controller: uid not allowed");
            bail!("controller uid {uid} not allowed");
        }
    }
    info!(uid, "controller connected");
    Ok(conn)
}
