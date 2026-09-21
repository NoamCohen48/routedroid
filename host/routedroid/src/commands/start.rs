//! `routedroid start`: one phone, one statically chosen address, until Ctrl-C.
//!
//! Order matters for safety: helper first (so a failure leaves nothing on the
//! phone), then the reverse mapping, then the secret, then the launch.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;

use clap::Args;
use routedroid_proto::frame::DEFAULT_MTU;
use routedroid_proto::messages::Prefix;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::adb::{Adb, DEFAULT_TIMEOUT};
use crate::device::DeviceSession;
use crate::fault::{Fault, Kind, Result};
use crate::helper::{HelperSession, DEFAULT_SOCKET};
use crate::listener;
use crate::session::{run_session, Machine, SessionConfig, SessionEnd};

#[derive(Debug, Args)]
pub struct StartArgs {
    /// ADB serial of the phone (see `routedroid devices`).
    #[arg(long, short = 's', env = "ANDROID_SERIAL")]
    pub serial: String,
    /// LAN interface the phone joins (e.g. eno1).
    #[arg(long)]
    pub lan_if: String,
    /// Address the phone gets on that LAN (must be free; automatic DHCP comes in Phase 3).
    #[arg(long)]
    pub phone_ip: Ipv4Addr,
    /// TUN interface name the helper creates.
    #[arg(long, default_value = "phone0")]
    pub tun: String,
    /// Packet MTU offered in HELLO_ACK (576..=65535).
    #[arg(long, default_value_t = DEFAULT_MTU, value_parser = clap::value_parser!(u32).range(576..=65535))]
    pub mtu: u32,
    /// DNS server(s) to hand the phone; defaults to none.
    #[arg(long = "dns")]
    pub dns: Vec<Ipv4Addr>,
    #[arg(long, default_value = DEFAULT_SOCKET)]
    pub helper_socket: PathBuf,
    /// How long to wait for the app to connect after launch.
    #[arg(long, default_value = "90s", value_parser = humantime_secs)]
    pub connect_timeout: Duration,
}

fn humantime_secs(s: &str) -> std::result::Result<Duration, String> {
    let s = s.trim_end_matches('s');
    s.parse::<u64>().map(Duration::from_secs).map_err(|e| e.to_string())
}

pub async fn run(adb_bin: &str, args: StartArgs) -> Result<()> {
    let adb = Adb::new(adb_bin, &args.serial, DEFAULT_TIMEOUT)?;
    let (listener, host_port) = listener::bind_loopback().await?;

    // Ctrl-C from here on: everything below is undone at the bottom of `run`.
    let (stop_tx, mut stop_rx) = watch::channel(false);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            info!("stop requested");
            let _ = stop_tx.send(true);
        }
    });

    let mut helper =
        HelperSession::start(&args.helper_socket, &args.lan_if, args.phone_ip, &args.tun, args.mtu).await?;
    info!(serial = adb.serial(), tun = %helper.tun, host_ip = %helper.host_ip, lan_prefix = helper.lan_prefix,
          phone_ip = %args.phone_ip, helper_session = %helper.session, "host network ready");
    let mut device = match DeviceSession::open(adb, host_port).await {
        Ok(d) => d,
        Err(e) => {
            helper.stop().await;
            return Err(e);
        }
    };

    let outcome = async {
        device.bootstrap().await?;
        let stream = tokio::select! {
            r = listener::accept_one(&listener, args.connect_timeout) => r?,
            _ = stop_rx.changed() => return Err(Fault::msg(Kind::Usage, "stopped before the app connected")),
        };
        drop(listener); // one session per run: refuse anything else that connects
        info!(host_port, device_port = device.device_port(), "app connected");

        let cfg = SessionConfig {
            mtu: args.mtu,
            addresses: vec![Prefix::new(args.phone_ip, 32)],
            routes: vec![Prefix::new(Ipv4Addr::UNSPECIFIED, 0)],
            dns: args.dns.iter().map(ToString::to_string).collect(),
            session_name: "Routedroid".into(),
            expected_session: device.session.clone(),
            expected_device_port: device.device_port(),
            secret: device.take_secret(),
        };
        let summary = run_session(stream, Machine::new(cfg, device.host_nonce), helper.relay(), stop_rx).await;
        info!(
            to_phone = summary.packets_to_phone,
            from_phone = summary.packets_from_phone,
            dropped = summary.bad_packets,
            "traffic"
        );
        match summary.end {
            SessionEnd::LocalStop | SessionEnd::PeerStop | SessionEnd::PeerClosed if summary.reached_active => Ok(()),
            // Peer-supplied text: `{:?}` escapes control characters before it reaches a terminal.
            SessionEnd::VpnError(e) => Err(Fault::msg(Kind::Vpn, format!("{}: {:?}", e.code, e.message))),
            SessionEnd::Refused(e) if e.code == "auth_failed" => Err(Fault::msg(Kind::Auth, e.message)),
            SessionEnd::Refused(e) => Err(Fault::msg(Kind::Protocol, format!("{}: {:?}", e.code, e.message))),
            other => Err(Fault::msg(Kind::Vpn, other.to_string())),
        }
    }
    .await;

    device.close().await;
    helper.stop().await;
    if outcome.is_err() {
        warn!("session did not complete cleanly");
    }
    outcome
}
