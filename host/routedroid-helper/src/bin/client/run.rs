//! The session script: hello, start, optional relay benchmark, optional
//! hold, then stop, with a self-SIGKILL at the stage `--crash-at` names.

use std::net::Ipv4Addr;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use routedroid_helper_ipc::{DeviceId, IfName, Reply, Request, SeqPacket, VERSION};
use tracing::{info, warn};

use crate::bench;
use crate::link::{Incoming, Link, print_lease};

pub struct ClientArgs {
    pub lan_if: IfName,
    /// `None` leases one.
    pub phone_ip: Option<Ipv4Addr>,
    pub device: DeviceId,
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
        device: args.device,
        tun: args.tun.clone(),
        mtu: args.mtu,
    };
    let (host_ip, phone_ip) = match link.request(&start).await? {
        Reply::Started {
            session,
            tun,
            phone_ip,
            host_ip,
            lan_prefix,
            lease,
        } => {
            info!(session, %tun, %phone_ip, %host_ip, lan_prefix, "started");
            println!(
                "STARTED session={session} tun={tun} phone_ip={phone_ip} host_ip={host_ip}/{lan_prefix}"
            );
            if let Some(lease) = &lease {
                print_lease(lease);
            }
            (host_ip, phone_ip)
        }
        Reply::Error { code, message } => bail!("start refused: {code:?}: {message}"),
        other => bail!("unexpected {other:?}"),
    };
    args.crash("after_start");

    if args.bench > 0 {
        let target = args.bench_target.unwrap_or(host_ip);
        let replies = bench::run(&mut link, &args, phone_ip, target).await?;
        if replies < args.bench {
            bail!("bench: {replies}/{} echo replies", args.bench);
        }
    }
    if !args.hold.is_zero() {
        hold(&mut link, args.hold).await?;
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

/// Keep the session for `d`, printing renewals; the helper ending it is an
/// error.
async fn hold(link: &mut Link, d: Duration) -> Result<()> {
    info!(secs = d.as_secs(), "holding session (Ctrl-C stops early)");
    let until = tokio::time::Instant::now() + d;
    loop {
        tokio::select! {
            () = tokio::time::sleep_until(until) => return Ok(()),
            _ = tokio::signal::ctrl_c() => {
                info!("interrupted; stopping");
                return Ok(());
            }
            incoming = link.recv() => match incoming? {
                Incoming::Reply(Reply::Lease { lease }) => print_lease(&lease),
                Incoming::Reply(Reply::Error { code, message }) => {
                    println!("ENDED {code:?}: {message}");
                    bail!("the helper ended the session: {code:?}: {message}");
                }
                Incoming::Reply(other) => bail!("unexpected {other:?}"),
                Incoming::Packet(_) => {}
            },
        }
    }
}
