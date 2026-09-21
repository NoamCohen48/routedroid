//! Routedroid Phase 0 §3.1 host probe: one TUN <-> one `adb reverse` TCP
//! session, packets framed per protocol/phase0-draft.md. Throwaway quality.

mod adb;
mod auth;
mod fake_client;
mod frame;
mod ipv4;
mod messages;
mod session;
mod stats;
mod tun;

use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use messages::Prefix;
use session::{Machine, SessionConfig, SessionEnd, TunEndpoints};
use stats::Stats;

#[derive(Parser, Debug)]
#[command(name = "phase0-tunnel", about = "Routedroid Phase 0 minimal packet tunnel (host side)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Create a TUN, accept one Android session over adb reverse, pump packets.
    Run(RunArgs),
    /// Exercise framing + state machine against an in-process fake Android over loopback.
    Selftest,
    /// Print the 80-byte bootstrap record (for `adb shell content write` in --no-adb setups).
    BootstrapRecord(BootstrapRecordArgs),
}

#[derive(Parser, Debug, Clone)]
struct BootstrapRecordArgs {
    #[arg(long)]
    session: String,
    /// File holding the 32-byte secret as 64 hex characters.
    #[arg(long)]
    secret_file: PathBuf,
}

#[derive(Parser, Debug, Clone)]
struct RunArgs {
    /// ADB device serial (required unless --no-adb).
    #[arg(long, required_unless_present = "no_adb")]
    serial: Option<String>,
    /// TUN interface name to create (non-persistent).
    #[arg(long, default_value = "phone0")]
    tun: String,
    /// Phone address(es) as A.B.C.D/N, first one is the primary (repeatable).
    /// §3.2 compares /32 aliases against actual-LAN-prefix aliases.
    #[arg(long = "address", value_parser = parse_prefix, required = true)]
    addresses: Vec<Prefix>,
    #[arg(long, default_value_t = frame::DEFAULT_MTU)]
    mtu: u32,
    /// Route(s) pushed to Android, e.g. 0.0.0.0/0 (repeatable; default 0.0.0.0/0).
    #[arg(long = "route", value_parser = parse_prefix)]
    routes: Vec<Prefix>,
    /// DNS server(s) pushed to Android (repeatable).
    #[arg(long = "dns")]
    dns: Vec<String>,
    /// Port the Android app connects to on the device (adb reverse remote side).
    #[arg(long, default_value_t = 9000)]
    device_port: u16,
    /// Skip adb entirely: just print the host port and wait for a client.
    #[arg(long)]
    no_adb: bool,
    /// Session id passed to the app; random when omitted. With --no-adb and no
    /// --session the HELLO session string is not enforced.
    #[arg(long)]
    session: Option<String>,
    /// Session name shown in Android's VPN dialog.
    #[arg(long, default_value = "Routedroid Phase 0")]
    session_name: String,
    /// File holding the session secret as 64 hex characters. Required with
    /// --no-adb (the operator delivers the record); otherwise random and
    /// delivered by this process over `adb shell content write` stdin.
    #[arg(long, required_if_eq("no_adb", "true"))]
    secret_file: Option<PathBuf>,
}

fn parse_prefix(s: &str) -> std::result::Result<Prefix, String> {
    let (addr, len) = s.split_once('/').ok_or_else(|| format!("{s}: expected A.B.C.D/N"))?;
    let ip: Ipv4Addr = addr.parse().map_err(|e| format!("{s}: {e}"))?;
    let prefix: u8 = len.parse().map_err(|e| format!("{s}: {e}"))?;
    if prefix > 32 {
        return Err(format!("{s}: prefix > 32"));
    }
    Ok(Prefix { address: ip.to_string(), prefix })
}

fn random_session() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let pid = std::process::id();
    // FNV-1a over nanos+pid; not cryptographic, Phase 0 has no auth anyway.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in nanos.to_le_bytes().iter().chain(pid.to_le_bytes().iter()) {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("p0-{h:016x}")
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();
    let cli = Cli::parse();
    let rt = tokio::runtime::Runtime::new()?;
    match cli.cmd {
        Cmd::Run(args) => rt.block_on(run(args)),
        Cmd::Selftest => rt.block_on(selftest()),
        Cmd::BootstrapRecord(a) => {
            let secret = load_secret(&a.secret_file)?;
            let record = auth::bootstrap_record(&a.session, &secret)?;
            std::io::stdout().write_all(&record[..])?;
            Ok(())
        }
    }
}

fn load_secret(path: &Path) -> Result<auth::Secret> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read secret file {}", path.display()))?;
    auth::secret_from_hex(&text)
}

/// Everything that must be undone on exit, in the order it was set up.
struct Cleanup {
    adb: Option<(String, u16, u16)>,
    tun: Option<Arc<tun::Tun>>,
}

impl Cleanup {
    fn run(&mut self) {
        if let Some((serial, device_port, host_port)) = self.adb.take() {
            adb::reverse_remove_if_ours(&serial, device_port, host_port);
        }
        if let Some(t) = self.tun.take() {
            let name = t.name().to_string();
            // Non-persistent: closing the last fd removes the interface. The
            // pump tasks hold clones; aborting the runtime drops them.
            drop(t);
            info!(tun = %name, "TUN handle released (interface vanishes with the last fd)");
        }
    }
}

async fn run(args: RunArgs) -> Result<()> {
    if args.addresses.iter().any(|a| a.prefix == 0 || a.prefix > 32) {
        bail!("--address prefix must be 1..=32");
    }
    if args.mtu < 576 || args.mtu > 65535 {
        bail!("--mtu must be within 576..=65535");
    }
    let routes = if args.routes.is_empty() {
        vec![Prefix { address: "0.0.0.0".into(), prefix: 0 }]
    } else {
        args.routes.clone()
    };
    let session_id = args.session.clone().unwrap_or_else(random_session);
    let expected_session = if args.no_adb && args.session.is_none() { None } else { Some(session_id.clone()) };

    let stats = Arc::new(Stats::default());
    let mut cleanup = Cleanup { adb: None, tun: None };

    let result = run_inner(&args, routes, session_id, expected_session, stats.clone(), &mut cleanup).await;

    cleanup.run();
    info!(stats = %stats.snapshot(), "final counters");
    match result {
        Ok(end) => {
            info!(end = %end, "exit");
            match end {
                SessionEnd::LocalStop | SessionEnd::PeerStop | SessionEnd::PeerClosed => Ok(()),
                other => Err(anyhow!("session ended abnormally: {other}")),
            }
        }
        Err(e) => {
            error!(error = %e, "exit");
            Err(e)
        }
    }
}

async fn run_inner(
    args: &RunArgs,
    routes: Vec<Prefix>,
    session_id: String,
    expected_session: Option<String>,
    stats: Arc<Stats>,
    cleanup: &mut Cleanup,
) -> Result<SessionEnd> {
    // 1. TUN first: if we cannot create it there is no point touching adb.
    let tun_dev = Arc::new(tun::Tun::create(&args.tun).context("create TUN (needs CAP_NET_ADMIN)")?);
    cleanup.tun = Some(tun_dev.clone());
    tun::link_up(tun_dev.name(), args.mtu)?;
    info!(tun = tun_dev.name(), mtu = args.mtu, "TUN created (IFF_TUN|IFF_NO_PI, non-persistent)");

    // 2. Loopback listener on a random port.
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.context("bind 127.0.0.1:0")?;
    let host_port = listener.local_addr()?.port();

    // 3. Secret: operator-provided file, or fresh random for adb delivery.
    let secret = match &args.secret_file {
        Some(p) => load_secret(p)?,
        None => auth::random_secret()?,
    };

    // 4. adb reverse + record + launch, or tell the operator what to connect to.
    if args.no_adb {
        info!(host_port, session = %session_id, "--no-adb: waiting for a client on 127.0.0.1:{host_port}");
        println!("HOST_PORT={host_port}");
        println!("SESSION={session_id}");
    } else {
        let serial = args.serial.as_deref().expect("clap enforces --serial without --no-adb");
        adb::reverse_add(serial, args.device_port, host_port).context("adb reverse")?;
        cleanup.adb = Some((serial.to_string(), args.device_port, host_port));
        info!(serial, device_port = args.device_port, host_port, "adb reverse installed");
        // Record first (stdin, never an argument), then the activity that consumes it.
        let record = auth::bootstrap_record(&session_id, &secret)?;
        adb::write_bootstrap_record(serial, &record[..]).context("deliver bootstrap record")?;
        drop(record);
        adb::launch_bootstrap(serial, &session_id, args.device_port).context("adb shell am start")?;
    }

    // 5. Exactly one connection.
    let (stream, peer) = tokio::select! {
        r = listener.accept() => r.context("accept")?,
        _ = tokio::signal::ctrl_c() => {
            info!("interrupted while waiting for the client");
            return Ok(SessionEnd::LocalStop);
        }
    };
    drop(listener);
    stream.set_nodelay(true).ok();
    info!(%peer, "client connected");

    let cfg = SessionConfig {
        mtu: args.mtu,
        addresses: args.addresses.clone(),
        routes,
        dns: args.dns.clone(),
        session_name: args.session_name.clone(),
        expected_session,
        secret,
    };
    let machine = Machine::new(cfg, auth::random_bytes()?);
    let endpoints = tun::spawn_pumps(tun_dev.clone(), args.mtu, stats.clone());

    // 6. Ctrl-C -> graceful STOP; periodic counters.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            info!("Ctrl-C: stopping session");
            let _ = shutdown_tx.send(true);
        }
    });
    let stats_task = {
        let stats = stats.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            tick.tick().await;
            loop {
                tick.tick().await;
                info!(stats = %stats.snapshot(), "counters");
            }
        })
    };

    let summary = session::run_session(stream, machine, endpoints, stats, shutdown_rx).await;
    stats_task.abort();
    if !summary.reached_active {
        warn!("session ended before reaching Active");
    }
    Ok(summary.end)
}

/// Loopback selftest: no ADB, no root. Fakes the TUN with channels.
async fn selftest() -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let addr: SocketAddr = listener.local_addr()?;
    let session = "selftest-session";
    let mtu = frame::DEFAULT_MTU;

    let cfg = SessionConfig {
        mtu,
        addresses: vec![Prefix { address: "192.168.10.74".into(), prefix: 32 }],
        routes: vec![Prefix { address: "0.0.0.0".into(), prefix: 0 }],
        dns: vec!["192.168.10.1".into()],
        session_name: "Routedroid Phase 0 selftest".into(),
        expected_session: Some(session.into()),
        secret: auth::random_secret()?,
    };

    // --- Check 1: happy path with a packet round trip -------------------
    let probe = fake_client::icmp_echo([192, 168, 10, 74], [192, 168, 10, 1], 40);
    let client = tokio::spawn(fake_client::run_fake_android(addr, session, 9000, cfg.secret.clone(), probe.clone()));

    let (stream, _) = listener.accept().await?;
    let stats = Arc::new(Stats::default());
    let (to_tun_tx, mut to_tun_rx) = mpsc::channel::<Vec<u8>>(session::QUEUE_DEPTH);
    let (from_tun_tx, from_tun_rx) = mpsc::channel::<Vec<u8>>(session::QUEUE_DEPTH);
    // Fake "Linux": whatever lands on the TUN is bounced back with swapped addresses.
    let fake_linux = tokio::spawn(async move {
        let mut seen = Vec::new();
        while let Some(mut pkt) = to_tun_rx.recv().await {
            seen.push(pkt.clone());
            fake_client::swap_addresses(&mut pkt);
            if from_tun_tx.send(pkt).await.is_err() {
                break;
            }
        }
        seen
    });
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    let summary = session::run_session(
        stream,
        Machine::new(cfg.clone(), auth::random_bytes()?),
        TunEndpoints { to_tun: to_tun_tx, from_tun: from_tun_rx },
        stats.clone(),
        shutdown_rx,
    )
    .await;
    let report = client.await??;
    let seen = fake_linux.await?;

    check("handshake reached Active", summary.reached_active)?;
    check("session ended by peer STOP", summary.end == SessionEnd::PeerStop)?;
    check("HELLO_ACK carried mtu", report.hello_ack.mtu == mtu)?;
    check("CONFIGURE_VPN matched config", report.configure == cfg.configure_vpn())?;
    check("PING answered with PONG", report.got_pong)?;
    check("host TUN received exactly the probe packet", seen.len() == 1 && seen[0] == probe)?;
    let mut expect = probe.clone();
    fake_client::swap_addresses(&mut expect);
    check("Android received the bounced packet unchanged", report.echoed == expect)?;
    let s = stats.snapshot();
    check(
        "counters: 1 packet each direction, no drops",
        s.peer_to_tun_packets == 1
            && s.tun_to_peer_packets == 1
            && s.peer_to_tun_bytes == probe.len() as u64
            && s.tun_to_peer_bytes == probe.len() as u64
            && s.drop_tun_not_active == 0,
    )?;

    // --- Check 2: IP_PACKET before HELLO is rejected with ERROR + close ---
    let bad = tokio::spawn(fake_client::run_misbehaving_client(addr));
    let (stream, _) = listener.accept().await?;
    let stats2 = Arc::new(Stats::default());
    let (to_tun_tx, _to_tun_rx) = mpsc::channel::<Vec<u8>>(1);
    let (_from_tun_tx, from_tun_rx) = mpsc::channel::<Vec<u8>>(1);
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    let summary2 = session::run_session(
        stream,
        Machine::new(cfg, auth::random_bytes()?),
        TunEndpoints { to_tun: to_tun_tx, from_tun: from_tun_rx },
        stats2,
        shutdown_rx,
    )
    .await;
    let (err_body, closed) = bad.await??;
    check(
        "IP_PACKET before Active closes with out_of_state ERROR",
        matches!(summary2.end, SessionEnd::ProtocolViolation(ref s) if s.starts_with("out_of_state"))
            && closed
            && err_body.as_deref().is_some_and(|b| b.contains("out_of_state")),
    )?;

    println!("selftest: PASS");
    Ok(())
}

fn check(name: &str, ok: bool) -> Result<()> {
    if ok {
        println!("PASS  {name}");
        Ok(())
    } else {
        println!("FAIL  {name}");
        Err(anyhow!("selftest check failed: {name}"))
    }
}
