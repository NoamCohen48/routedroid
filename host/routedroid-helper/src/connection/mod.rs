//! One controller's connection: `Hello`, then `Start` (both within
//! [`SETUP_DEADLINE`]), then the relay, then the undo, which runs on every
//! path out of the relay. The first invalid request closes the connection:
//! a controller gets one well-formed attempt, not a retry loop.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use routedroid_helper_ipc::{
    Datagram, ErrorCode, Reply, Request, SeqPacket, MAX_DATAGRAM, VERSION,
};
use tokio::sync::watch;
use tokio::task::spawn_blocking;
use tokio::time::{timeout_at, Instant};
use tracing::{info, warn};

use crate::env::Env;
use crate::kernel::System;
use crate::plan::{Facts, Plan, Request as StartRequest};
use crate::policy::Policy;
use crate::session::Session;
use crate::session_id::SessionId;

mod relay;

use relay::{Ended, Relay};

pub const SETUP_DEADLINE: Duration = Duration::from_secs(10);

fn error(code: ErrorCode, message: impl Into<String>) -> Reply {
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
    let Some(request) = setup(&conn, &mut buf, deadline).await? else {
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

/// `Hello` then `Start`. `None`: the controller left, stopped, or was refused.
async fn setup(
    conn: &SeqPacket,
    buf: &mut [u8],
    deadline: Instant,
) -> Result<Option<StartRequest>> {
    let mut greeted = false;
    loop {
        let received = match timeout_at(deadline, conn.recv(buf)).await {
            Ok(received) => received?,
            Err(_) => {
                warn!("controller did not start a session in time");
                let _ = conn
                    .send_control(&error(
                        ErrorCode::BadRequest,
                        "no Start within the deadline",
                    ))
                    .await;
                return Ok(None);
            }
        };
        let Some(datagram) = received else {
            return Ok(None);
        };
        let reply = match (greeted, Datagram::<Request>::decode(datagram)) {
            (false, Ok(Datagram::Control(Request::Hello { version }))) if version == VERSION => {
                greeted = true;
                conn.send_control(&Reply::Hello { version: VERSION })
                    .await?;
                continue;
            }
            (false, Ok(Datagram::Control(Request::Hello { version }))) => error(
                ErrorCode::VersionMismatch,
                format!("controller speaks helper IPC {version}, helper speaks {VERSION}"),
            ),
            (false, _) => error(ErrorCode::BadRequest, "expected Hello"),
            (
                true,
                Ok(Datagram::Control(Request::Start {
                    lan_if,
                    phone_ip,
                    tun,
                    mtu,
                })),
            ) => {
                return Ok(Some(StartRequest {
                    lan_if,
                    phone_ip,
                    tun,
                    mtu,
                }));
            }
            (true, Ok(Datagram::Control(Request::Ping))) => {
                conn.send_control(&Reply::Pong).await?;
                continue;
            }
            (true, Ok(Datagram::Control(Request::Stop))) => Reply::Stopped,
            (true, Ok(_)) => error(ErrorCode::OutOfState, "expected Start"),
            (true, Err(e)) => error(ErrorCode::BadRequest, e.to_string()),
        };
        conn.send_control(&reply).await?;
        return Ok(None);
    }
}
