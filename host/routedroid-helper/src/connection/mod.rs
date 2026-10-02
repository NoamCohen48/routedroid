//! One controller's connection: `Hello`, then `Start` (both within
//! [`SETUP_DEADLINE`]), the phone's address settled (leased or probed),
//! then the relay, then the undo, which runs on every path out of the
//! relay. The first invalid request closes the connection: a controller
//! gets one well-formed attempt, not a retry loop.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use routedroid_helper_ipc::{ErrorCode, MAX_DATAGRAM, Reply, SeqPacket};
use tokio::sync::watch;
use tokio::task::spawn_blocking;
use tokio::time::Instant;
use tracing::info;

use crate::env::Env;
use crate::kernel::System;
use crate::session::Session;

mod address;
mod keeper;
mod lease;
mod relay;
mod setup;

use address::{Refusal, Settled};
use keeper::Kept;
use relay::{Ended, Relay};
use setup::setup;

pub const SETUP_DEADLINE: Duration = Duration::from_secs(10);

pub(crate) fn error(code: ErrorCode, message: impl Into<String>) -> Reply {
    Reply::Error {
        code,
        message: message.into(),
    }
}

pub async fn serve(
    env: Arc<Env<System>>,
    conn: SeqPacket,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let deadline = Instant::now() + SETUP_DEADLINE;
    let mut buf = vec![0u8; MAX_DATAGRAM];
    let Some(start) = setup(&env, &conn, &mut buf, deadline).await? else {
        return Ok(());
    };

    let Settled {
        plan,
        mut client,
        bound,
    } = match address::settle(&env, start).await {
        Ok(settled) => settled,
        Err(Refusal(code, reason)) => {
            info!(?code, reason = %format!("{reason:#}"), "start refused");
            let _ = conn.send_control(&error(code, format!("{reason:#}"))).await;
            return Ok(());
        }
    };
    let started = {
        let env = Arc::clone(&env);
        spawn_blocking(move || Session::start(env, plan)).await?
    };
    let session = match started {
        Ok(session) => session,
        Err(e) => {
            lease::give_back(&mut client, bound.as_ref()).await;
            let _ = conn
                .send_control(&error(ErrorCode::StartFailed, format!("{e:#}")))
                .await;
            bail!("start failed: {e:#}");
        }
    };
    let plan = session.plan();
    let phone_ip = plan.request().phone_ip;
    info!(session = %plan.session(), tun = %plan.request().tun, phone = %phone_ip, leased = bound.is_some(),
          gateway = ?plan.gateway(), "session active");
    env.hook.at("active");

    let lease = bound.as_ref().map(|b| keeper::report(&b.lease));
    let (keeper, mut kept) = keeper::spawn(client, bound, phone_ip);
    let ended = relay_session(&conn, &session, lease, shutdown, &mut kept).await;
    keeper.abort();
    let _ = keeper.await;
    let stopped = spawn_blocking(move || session.stop())
        .await
        .context("undo panicked")?;
    let reply = match (&ended, &stopped) {
        (Ended::Stop, Ok(())) => Some(Reply::Stopped),
        (Ended::Failed(why), Ok(())) => Some(error(ErrorCode::SessionEnded, why.clone())),
        (Ended::Stop | Ended::Failed(_), Err(e)) => {
            Some(error(ErrorCode::StopFailed, format!("{e:#}")))
        }
        (Ended::Disconnected | Ended::Shutdown, _) => None,
    };
    if let Some(reply) = reply {
        let _ = conn.send_control(&reply).await;
    }
    stopped
}

/// Announce the session, then relay. Nothing in here can skip the undo.
async fn relay_session(
    conn: &SeqPacket,
    session: &Session<System>,
    lease: Option<routedroid_helper_ipc::Lease>,
    shutdown: watch::Receiver<bool>,
    kept: &mut tokio::sync::mpsc::Receiver<Kept>,
) -> Ended {
    let plan = session.plan();
    let started = Reply::Started {
        session: plan.session().to_string(),
        tun: plan.request().tun.clone(),
        phone_ip: plan.request().phone_ip,
        host_ip: plan.host_ip(),
        lan_prefix: plan.lan_prefix(),
        lease,
    };
    if let Err(e) = conn.send_control(&started).await {
        return Ended::Failed(format!("send Started: {e}"));
    }
    let tun = match session.tun().map(|device| device.open_async()) {
        Some(Ok(tun)) => tun,
        Some(Err(e)) => return Ended::Failed(format!("open TUN: {e}")),
        None => return Ended::Failed("session has no TUN".into()),
    };
    let request = plan.request();
    Relay {
        conn,
        tun: &tun,
        phone_ip: request.phone_ip,
        mtu: request.mtu as usize,
    }
    .run(shutdown, kept)
    .await
}
