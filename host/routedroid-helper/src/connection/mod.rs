//! One controller's connection: `Hello`, then `Start` (both within
//! [`SETUP_DEADLINE`]), then the relay, then the undo, which runs on every
//! path out of the relay. The first invalid request closes the connection:
//! a controller gets one well-formed attempt, not a retry loop.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use routedroid_helper_ipc::{ErrorCode, Reply, SeqPacket, MAX_DATAGRAM};
use tokio::sync::watch;
use tokio::task::spawn_blocking;
use tokio::time::Instant;
use tracing::info;

use crate::env::Env;
use crate::kernel::System;
use crate::plan::{Facts, Plan, Request as StartRequest};
use crate::policy::Policy;
use crate::session::Session;
use crate::session_id::SessionId;

mod relay;
mod setup;

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
    let Some(request) = setup(&env, &conn, &mut buf, deadline).await? else {
        return Ok(());
    };

    let prepared = {
        let env = Arc::clone(&env);
        spawn_blocking(move || prepare(&env, request)).await?
    };
    let plan = match prepared {
        Ok(plan) => plan,
        Err(e) => {
            info!(reason = %format!("{e:#}"), "start refused");
            let _ = conn
                .send_control(&error(ErrorCode::Refused, format!("{e:#}")))
                .await;
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
            let _ = conn
                .send_control(&error(ErrorCode::StartFailed, format!("{e:#}")))
                .await;
            bail!("start failed: {e:#}");
        }
    };
    let plan = session.plan();
    info!(session = %plan.session(), tun = %plan.request().tun, phone = %plan.request().phone_ip, "session active");
    env.hook.at("active");

    let ended = relay_session(&conn, &session, shutdown).await;
    let stopped = spawn_blocking(move || session.stop())
        .await
        .context("undo panicked")?;
    if ended == Ended::Stop {
        let reply = match &stopped {
            Ok(()) => Reply::Stopped,
            Err(e) => error(ErrorCode::StopFailed, format!("{e:#}")),
        };
        let _ = conn.send_control(&reply).await;
    }
    stopped
}

/// Announce the session, then relay. Nothing in here can skip the undo.
async fn relay_session(
    conn: &SeqPacket,
    session: &Session<System>,
    shutdown: watch::Receiver<bool>,
) -> Ended {
    let plan = session.plan();
    let started = Reply::Started {
        session: plan.session().to_string(),
        tun: plan.request().tun.clone(),
        host_ip: plan.host_ip(),
        lan_prefix: plan.lan_prefix(),
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
    .run(shutdown)
    .await
}

/// Read the operator's policy and the kernel, and decide.
fn prepare(env: &Env<System>, request: StartRequest) -> Result<Plan> {
    let policy = Policy::load(&env.policy)?;
    let facts = Facts::gather(&env.kernel, &request.lan_if, &request.tun)?;
    Plan::build(SessionId::random()?, request, &policy, &facts)
}
