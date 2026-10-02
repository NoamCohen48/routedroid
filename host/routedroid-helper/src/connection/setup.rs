//! Before a session: `Hello`, then any number of `Ping`s and `Interfaces`,
//! then `Start`, all within the deadline.

use std::sync::Arc;

use anyhow::Result;
use routedroid_helper_ipc::{Datagram, ErrorCode, Reply, Request, SeqPacket, VERSION};
use tokio::task::spawn_blocking;
use tokio::time::{Instant, timeout_at};
use tracing::warn;

use super::error;
use crate::env::Env;
use crate::kernel::System;
use crate::plan::Request as StartRequest;
use crate::policy::Policy;
use crate::survey::survey;

/// `Hello` then `Start`. `None`: the controller left, stopped, or was refused.
pub async fn setup(
    env: &Arc<Env<System>>,
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
            (true, Ok(Datagram::Control(Request::Interfaces))) => {
                conn.send_control(&interfaces(env).await).await?;
                continue;
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

/// The survey, read fresh: the policy and the links may have changed since
/// the last one.
async fn interfaces(env: &Arc<Env<System>>) -> Reply {
    let env = Arc::clone(env);
    let surveyed = spawn_blocking(move || survey(&env.kernel, &Policy::load(&env.policy))).await;
    match surveyed {
        Ok(Ok(interfaces)) => Reply::Interfaces { interfaces },
        Ok(Err(e)) => error(ErrorCode::QueryFailed, format!("{e:#}")),
        Err(e) => error(ErrorCode::QueryFailed, format!("survey panicked: {e}")),
    }
}
