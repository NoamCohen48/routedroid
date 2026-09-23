//! One controller's connection: `Hello`, then wait for `Start`, apply the
//! session, relay packets between the TUN and the controller until `Stop`
//! or disconnect, then undo the session.

use anyhow::{bail, Result};
use routedroid_helper_ipc::{Datagram, ErrorCode, Reply, Request, SeqPacket, MAX_DATAGRAM, VERSION};
use tracing::{info, warn};

use crate::serve::ServeConfig;
use crate::session::{self, Plan};

fn error(code: ErrorCode, message: impl Into<String>) -> Reply {
    Reply::Error { code, message: message.into() }
}

fn ipv4_ok(p: &[u8]) -> bool {
    p.len() > 20 && p[0] >> 4 == 4 && (p[0] & 15) >= 5 && u16::from_be_bytes([p[2], p[3]]) as usize == p.len()
}

/// The first datagram must be `Hello` with our version.
async fn handshake(conn: &SeqPacket, buf: &mut [u8]) -> Result<bool> {
    let Some(datagram) = conn.recv(buf).await? else { return Ok(false) };
    match Datagram::<Request>::decode(datagram) {
        Ok(Datagram::Control(Request::Hello { version })) if version == VERSION => {
            conn.send_control(&Reply::Hello { version: VERSION }).await?;
            Ok(true)
        }
        Ok(Datagram::Control(Request::Hello { version })) => {
            let message = format!("controller speaks helper IPC {version}, helper speaks {VERSION}");
            conn.send_control(&error(ErrorCode::VersionMismatch, message)).await?;
            Ok(false)
        }
        _ => {
            conn.send_control(&error(ErrorCode::BadRequest, "expected Hello")).await?;
            Ok(false)
        }
    }
}

pub async fn serve(cfg: &ServeConfig, conn: SeqPacket) -> Result<()> {
    let hook = &cfg.hook;
    let mut buf = vec![0u8; MAX_DATAGRAM];
    if !handshake(&conn, &mut buf).await? {
        info!("controller failed the handshake");
        return Ok(());
    }
    // Wait for Start.
    let plan = loop {
        let Some(datagram) = conn.recv(&mut buf).await? else {
            info!("controller left before Start");
            return Ok(());
        };
        match Datagram::<Request>::decode(datagram) {
            Ok(Datagram::Control(Request::Ping)) => conn.send_control(&Reply::Pong).await?,
            Ok(Datagram::Control(Request::Start { lan_if, phone_ip, tun, mtu })) => {
                let session = format!("s{}-{}", std::process::id(), nanos());
                match Plan::build(&session, lan_if.as_str(), phone_ip, tun.as_str(), mtu) {
                    Ok(p) => break p,
                    Err(e) => conn.send_control(&error(ErrorCode::Refused, format!("{e:#}"))).await?,
                }
            }
            Ok(Datagram::Control(Request::Stop)) => {
                conn.send_control(&Reply::Stopped).await?;
                return Ok(());
            }
            Ok(Datagram::Control(Request::Hello { .. }) | Datagram::Packet(_)) => {
                conn.send_control(&error(ErrorCode::OutOfState, "expected Start")).await?
            }
            Err(e) => conn.send_control(&error(ErrorCode::BadRequest, e.to_string())).await?,
        }
    };

    let active = match session::start(&plan, &cfg.journal_dir, hook) {
        Ok(a) => a,
        Err(e) => {
            conn.send_control(&error(ErrorCode::StartFailed, format!("{e:#}"))).await?;
            bail!("start failed: {e:#}");
        }
    };
    conn.send_control(&Reply::Started {
        session: plan.session.clone(),
        tun: routedroid_helper_ipc::IfName::new(plan.tun.clone())?,
        host_ip: plan.host_ip,
        lan_prefix: plan.lan_prefix,
    })
    .await?;
    info!(session = %plan.session, tun = %plan.tun, phone = %plan.phone_ip, "session active");
    hook.at("active");

    // Relay until Stop or disconnect.
    let mut tun_buf = vec![0u8; plan.mtu as usize + 1];
    let mut stop_requested = false;
    let mut relayed = (0u64, 0u64);
    loop {
        tokio::select! {
            r = active.tun.read(&mut tun_buf) => {
                let n = match r { Ok(n) => n, Err(e) => { warn!(error = %e, "tun read failed"); break; } };
                if n == 0 || n > plan.mtu as usize || !ipv4_ok(&tun_buf[..n]) { continue; }
                if conn.send_packet(&tun_buf[..n]).await.is_err() { break; }
                relayed.0 += 1;
            }
            r = conn.recv(&mut buf) => {
                let datagram = match r { Ok(Some(d)) => d, Ok(None) => { info!("controller disconnected"); break; } Err(e) => { warn!(error = %e, "recv failed"); break; } };
                match Datagram::<Request>::decode(datagram) {
                    Ok(Datagram::Packet(p)) => {
                        if p.len() <= plan.mtu as usize && ipv4_ok(p) {
                            if let Err(e) = active.tun.write(p).await { warn!(error = %e, "tun write failed"); break; }
                            relayed.1 += 1;
                        }
                    }
                    Ok(Datagram::Control(Request::Stop)) => { stop_requested = true; break; }
                    Ok(Datagram::Control(Request::Ping)) => conn.send_control(&Reply::Pong).await?,
                    Ok(Datagram::Control(_)) => conn.send_control(&error(ErrorCode::OutOfState, "session active")).await?,
                    Err(e) => conn.send_control(&error(ErrorCode::BadRequest, e.to_string())).await?,
                }
            }
        }
    }
    info!(tun_to_controller = relayed.0, controller_to_tun = relayed.1, "relay ended");
    let r = active.stop(hook);
    if stop_requested {
        let reply = match &r {
            Ok(()) => Reply::Stopped,
            Err(e) => error(ErrorCode::StopFailed, format!("{e:#}")),
        };
        let _ = conn.send_control(&reply).await;
    }
    r
}

fn nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}
