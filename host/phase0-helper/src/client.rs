//! Unprivileged controller stand-in: starts a session, optionally pushes
//! ICMP echo requests through the relay (the host kernel answers them), can
//! kill itself at chosen moments, and stops cleanly otherwise.

use std::net::Ipv4Addr;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use tracing::{info, warn};

use crate::proto::{Reply, Request, KIND_CONTROL, KIND_PACKET, MAX_DATAGRAM};
use crate::seqpacket::SeqPacket;

pub struct ClientArgs {
    pub lan_if: String,
    pub phone_ip: Ipv4Addr,
    pub tun: String,
    pub mtu: u32,
    pub hold: Duration,
    pub bench: u32,
    pub crash_at: Option<String>,
    pub no_stop: bool,
}

fn crash(stage: &str, want: &Option<String>) {
    if want.as_deref() == Some(stage) {
        warn!(stage, "client crash hook: SIGKILL self");
        // SAFETY: signal to our own pid.
        unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
    }
}

async fn send_control(c: &SeqPacket, req: &Request) -> Result<()> {
    let mut b = vec![KIND_CONTROL];
    b.extend_from_slice(&serde_json::to_vec(req)?);
    c.send(&b).await.context("send")
}

async fn recv_reply(c: &SeqPacket, buf: &mut [u8]) -> Result<Reply> {
    loop {
        let n = c.recv(buf).await?;
        if n == 0 { bail!("helper closed the connection"); }
        if buf[0] == KIND_CONTROL {
            return serde_json::from_slice(&buf[1..n]).context("parse reply");
        }
    }
}

fn icmp_echo(src: Ipv4Addr, dst: Ipv4Addr, id: u16, seq: u16) -> Vec<u8> {
    let total = 20 + 8 + 32;
    let mut p = vec![0u8; total];
    p[0] = 0x45; p[2..4].copy_from_slice(&(total as u16).to_be_bytes()); p[8] = 64; p[9] = 1;
    p[12..16].copy_from_slice(&src.octets()); p[16..20].copy_from_slice(&dst.octets());
    let c = checksum(&p[..20]); p[10..12].copy_from_slice(&c.to_be_bytes());
    p[20] = 8; p[24..26].copy_from_slice(&id.to_be_bytes()); p[26..28].copy_from_slice(&seq.to_be_bytes());
    for (i, b) in p[28..].iter_mut().enumerate() { *b = i as u8; }
    let c = checksum(&p[20..]); p[22..24].copy_from_slice(&c.to_be_bytes());
    p
}

fn checksum(d: &[u8]) -> u16 {
    let mut s = 0u32;
    for ch in d.chunks(2) { s += u32::from(if ch.len() == 2 { u16::from_be_bytes([ch[0], ch[1]]) } else { u16::from(ch[0]) << 8 }); }
    while s >> 16 != 0 { s = (s & 0xffff) + (s >> 16); }
    !(s as u16)
}

pub async fn run(socket: &Path, a: ClientArgs) -> Result<()> {
    let c = SeqPacket::connect(socket).await?;
    let mut buf = vec![0u8; MAX_DATAGRAM];
    send_control(&c, &Request::Ping).await?;
    if recv_reply(&c, &mut buf).await? != Reply::Pong { bail!("no PONG"); }
    crash("before_start", &a.crash_at);
    send_control(&c, &Request::Start { lan_if: a.lan_if.clone(), phone_ip: a.phone_ip, tun: a.tun.clone(), mtu: a.mtu }).await?;
    let (host_ip, tun) = match recv_reply(&c, &mut buf).await? {
        Reply::Started { session, tun, host_ip, lan_prefix } => {
            info!(session, tun, %host_ip, lan_prefix, "started");
            println!("STARTED session={session} tun={tun} host_ip={host_ip}/{lan_prefix}");
            (host_ip, tun)
        }
        Reply::Error { code, message } => bail!("start refused: {code}: {message}"),
        other => bail!("unexpected {other:?}"),
    };
    let _ = tun;
    crash("after_start", &a.crash_at);

    if a.bench > 0 {
        // Wait for the kernel to finish bringing the TUN up before timing.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let id = (std::process::id() & 0xffff) as u16;
        let t0 = Instant::now();
        let mut replies = 0u32;
        let deadline = tokio::time::sleep(Duration::from_secs(5 + a.bench as u64 / 2000));
        tokio::pin!(deadline);
        let mut sent = 0u32;
        let mut pkt = vec![0u8; MAX_DATAGRAM];
        loop {
            if sent < a.bench {
                let echo = icmp_echo(a.phone_ip, host_ip, id, sent as u16);
                pkt[0] = KIND_PACKET; pkt[1..=echo.len()].copy_from_slice(&echo);
                c.send(&pkt[..=echo.len()]).await?;
                sent += 1;
                if sent == a.bench / 2 { crash("during_traffic", &a.crash_at); }
            }
            tokio::select! {
                biased;
                n = c.recv(&mut buf), if replies < a.bench => {
                    let n = n?;
                    if n == 0 { bail!("helper closed during bench"); }
                    if buf[0] == KIND_PACKET && n > 28 && buf[1 + 9] == 1 && buf[1 + 20] == 0 && u16::from_be_bytes([buf[1 + 24], buf[1 + 25]]) == id { replies += 1; }
                }
                _ = &mut deadline => break,
                _ = std::future::ready(()), if sent < a.bench => {}
            }
            if replies >= a.bench { break; }
        }
        let dt = t0.elapsed();
        println!("BENCH sent={sent} replies={replies} elapsed_ms={} rtt_avg_us={}", dt.as_millis(), if replies > 0 { dt.as_micros() / replies as u128 } else { 0 });
        if replies < a.bench { bail!("bench: {replies}/{} echo replies", a.bench); }
    }

    if !a.hold.is_zero() {
        info!(secs = a.hold.as_secs(), "holding session");
        tokio::time::sleep(a.hold).await;
    }
    crash("before_stop", &a.crash_at);
    if a.no_stop {
        println!("DISCONNECTING without Stop");
        return Ok(());
    }
    send_control(&c, &Request::Stop).await?;
    match recv_reply(&c, &mut buf).await? {
        Reply::Stopped => { println!("STOPPED"); Ok(()) }
        Reply::Error { code, message } => bail!("stop failed: {code}: {message}"),
        other => bail!("unexpected {other:?}"),
    }
}
