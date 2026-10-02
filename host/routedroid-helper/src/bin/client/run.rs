//! The session script: hello, start, optional relay benchmark, optional
//! hold, then stop, with a self-SIGKILL at the stage `--crash-at` names.

use std::net::Ipv4Addr;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use routedroid_helper_ipc::{IfName, Reply, Request, SeqPacket, VERSION};
use tracing::{info, warn};

use crate::bench;
use crate::link::Link;

pub struct ClientArgs {
    pub lan_if: IfName,
    pub phone_ip: Ipv4Addr,
    pub tun: IfName,
    pub mtu: u32,
    pub hold: Duration,
    pub bench: u32,
    /// Echo target for `bench`; the host itself when unset.
    pub bench_target: Option<Ipv4Addr>,
    pub crash_at: Option<String>,
    pub no_stop: bool,
}

impl ClientArgs {
    pub fn crash(&self, stage: &str) {
        if self.crash_at.as_deref() == Some(stage) {
            warn!(stage, "client crash hook: SIGKILL self");
            // SAFETY: signal to our own pid.
            unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
        }
    }
}

pub async fn run(socket: &Path, args: ClientArgs) -> Result<()> {
    let conn =
        SeqPacket::connect(socket).with_context(|| format!("connect {}", socket.display()))?;
    let mut link = Link::new(conn);
    match link.request(&Request::Hello { version: VERSION }).await? {
        Reply::Hello { .. } => {}
        other => bail!("handshake refused: {other:?}"),
    }
    if link.request(&Request::Ping).await? != Reply::Pong {
        bail!("no Pong");
    }
    args.crash("before_start");
    let start = Request::Start {
        lan_if: args.lan_if.clone(),
        phone_ip: args.phone_ip,
        tun: args.tun.clone(),
        mtu: args.mtu,
    };
    let host_ip = match link.request(&start).await? {
        Reply::Started {
            session,
            tun,
            host_ip,
            lan_prefix,
        } => {
            info!(session, %tun, %host_ip, lan_prefix, "started");
            println!("STARTED session={session} tun={tun} host_ip={host_ip}/{lan_prefix}");
            host_ip
        }
        Reply::Error { code, message } => bail!("start refused: {code:?}: {message}"),
        other => bail!("unexpected {other:?}"),
    };
    args.crash("after_start");

    if args.bench > 0 {
        let target = args.bench_target.unwrap_or(host_ip);
        let replies = bench::run(&mut link, &args, target).await?;
        if replies < args.bench {
            bail!("bench: {replies}/{} echo replies", args.bench);
        }
    }
    if !args.hold.is_zero() {
        info!(
            secs = args.hold.as_secs(),
            "holding session (Ctrl-C stops early)"
        );
        tokio::select! {
            _ = tokio::time::sleep(args.hold) => {}
            _ = tokio::signal::ctrl_c() => info!("interrupted; stopping"),
        }
    }
    args.crash("before_stop");
    if args.no_stop {
        println!("DISCONNECTING without Stop");
        return Ok(());
    }
    match link.request(&Request::Stop).await? {
        Reply::Stopped => {
            println!("STOPPED");
            Ok(())
        }
        Reply::Error { code, message } => bail!("stop failed: {code:?}: {message}"),
        other => bail!("unexpected {other:?}"),
    }
}
