//! The per-session daemon: accept one controller, run one session, exit.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use tracing::{info, warn};

use crate::proto::{Reply, Request, KIND_CONTROL, KIND_PACKET, MAX_DATAGRAM};
use crate::seqpacket::{Listener, SeqPacket};
use crate::session::{self, CrashHook, Plan};

pub struct ServeConfig {
    pub socket: Option<PathBuf>,
    pub journal_dir: PathBuf,
    pub crash_file: PathBuf,
    pub allow_uid: Option<u32>,
}

async fn send_reply(conn: &SeqPacket, reply: &Reply) -> Result<()> {
    let mut buf = vec![KIND_CONTROL];
    buf.extend_from_slice(&serde_json::to_vec(reply)?);
    conn.send(&buf).await.context("send reply")
}

fn ipv4_ok(p: &[u8]) -> bool {
    p.len() > 20 && p[0] >> 4 == 4 && (p[0] & 15) >= 5 && u16::from_be_bytes([p[2], p[3]]) as usize == p.len()
}

pub async fn serve(cfg: ServeConfig) -> Result<()> {
    let hook = CrashHook(cfg.crash_file.clone());
    session::check(&cfg.journal_dir).context("refusing to start")?;

    let listener = match Listener::from_systemd()? {
        Some(l) => {
            info!("listening on socket from systemd");
            l
        }
        None => {
            let p = cfg.socket.as_deref().context("--socket required without systemd activation")?;
            let l = Listener::bind(p)?;
            info!(path = %p.display(), "listening");
            l
        }
    };
    let conn = listener.accept().await?;
    drop(listener); // one session per process; systemd keeps the socket unit itself
    let uid = Listener::peer_uid(&conn)?;
    if let Some(want) = cfg.allow_uid {
        if uid != want {
            warn!(uid, "rejecting controller: uid not allowed");
            bail!("controller uid {uid} not allowed");
        }
    }
    info!(uid, "controller connected");

    let mut buf = vec![0u8; MAX_DATAGRAM];
    // Wait for Start.
    let plan = loop {
        let n = conn.recv(&mut buf).await?;
        if n == 0 {
            info!("controller left before Start");
            return Ok(());
        }
        match parse_control(&buf[..n]) {
            Some(Request::Ping) => send_reply(&conn, &Reply::Pong).await?,
            Some(Request::Start { lan_if, phone_ip, tun, mtu }) => {
                let session = format!("s{}-{}", std::process::id(), nanos());
                match Plan::build(&session, &lan_if, phone_ip, &tun, mtu) {
                    Ok(p) => break p,
                    Err(e) => {
                        send_reply(&conn, &Reply::Error { code: "invalid".into(), message: e.to_string() }).await?
                    }
                }
            }
            Some(Request::Stop) => {
                send_reply(&conn, &Reply::Stopped).await?;
                return Ok(());
            }
            None => {
                send_reply(&conn, &Reply::Error { code: "bad_request".into(), message: "expected control JSON".into() })
                    .await?
            }
        }
    };

    let active = match session::start(&plan, &cfg.journal_dir, &hook) {
        Ok(a) => a,
        Err(e) => {
            send_reply(&conn, &Reply::Error { code: "start_failed".into(), message: format!("{e:#}") }).await?;
            bail!("start failed: {e:#}");
        }
    };
    send_reply(
        &conn,
        &Reply::Started {
            session: plan.session.clone(),
            tun: plan.tun.clone(),
            host_ip: plan.host_ip,
            lan_prefix: plan.lan_prefix,
        },
    )
    .await?;
    info!(session = %plan.session, tun = %plan.tun, phone = %plan.phone_ip, "session active");
    hook.at("active");

    // Relay until Stop or disconnect.
    let mut tun_buf = vec![0u8; plan.mtu as usize + 1];
    let mut pkt_out = vec![0u8; MAX_DATAGRAM];
    let mut stop_requested = false;
    let mut relayed = (0u64, 0u64);
    loop {
        tokio::select! {
            r = active.tun.read(&mut tun_buf) => {
                let n = match r { Ok(n) => n, Err(e) => { warn!(error = %e, "tun read failed"); break; } };
                if n == 0 || n > plan.mtu as usize || !ipv4_ok(&tun_buf[..n]) { continue; }
                pkt_out[0] = KIND_PACKET;
                pkt_out[1..=n].copy_from_slice(&tun_buf[..n]);
                if conn.send(&pkt_out[..=n]).await.is_err() { break; }
                relayed.0 += 1;
            }
            r = conn.recv(&mut buf) => {
                let n = match r { Ok(0) => { info!("controller disconnected"); break; } Ok(n) => n, Err(e) => { warn!(error = %e, "recv failed"); break; } };
                match buf[0] {
                    KIND_PACKET => {
                        let p = &buf[1..n];
                        if p.len() <= plan.mtu as usize && ipv4_ok(p) {
                            if let Err(e) = active.tun.write(p).await { warn!(error = %e, "tun write failed"); break; }
                            relayed.1 += 1;
                        }
                    }
                    KIND_CONTROL => match parse_control(&buf[..n]) {
                        Some(Request::Stop) => { stop_requested = true; break; }
                        Some(Request::Ping) => send_reply(&conn, &Reply::Pong).await?,
                        _ => send_reply(&conn, &Reply::Error { code: "out_of_state".into(), message: "session active".into() }).await?,
                    },
                    _ => {}
                }
            }
        }
    }
    info!(tun_to_controller = relayed.0, controller_to_tun = relayed.1, "relay ended");
    let r = active.stop(&hook);
    if stop_requested {
        let _ = send_reply(
            &conn,
            &match &r {
                Ok(()) => Reply::Stopped,
                Err(e) => Reply::Error { code: "stop_failed".into(), message: e.to_string() },
            },
        )
        .await;
    }
    r
}

fn parse_control(d: &[u8]) -> Option<Request> {
    if d.first() != Some(&KIND_CONTROL) {
        return None;
    }
    serde_json::from_slice(&d[1..]).ok()
}

fn nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}
