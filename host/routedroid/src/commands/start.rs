//! `routedroid start`: one phone, one statically chosen address, until Ctrl-C.
//!
//! Order matters for safety: helper first (so a failure leaves nothing on the
//! phone), then the reverse mapping, then the secret, then the launch.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;

use clap::Args;
use routedroid_proto::auth::{self, Secret};
use routedroid_proto::bootstrap;
use routedroid_proto::frame::DEFAULT_MTU;
use routedroid_proto::messages::Prefix;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::adb::{Adb, DEFAULT_TIMEOUT};
use crate::fault::{Fault, FaultExt, Kind, Result};
use crate::helper::{HelperSession, DEFAULT_SOCKET};
use crate::session::{run_session, Machine, SessionConfig, SessionEnd};
use crate::{listener, ports};

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
    #[arg(long, default_value_t = DEFAULT_MTU)]
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

pub async fn run(adb_bin: &str, a: StartArgs) -> Result<()> {
    let adb = Adb::new(adb_bin, &a.serial, DEFAULT_TIMEOUT)?;
    let (listener, host_port) = ports::bind_loopback().await?;
    let used = adb.reverse_used_device_ports().await?;
    let seed = u16::from_le_bytes(auth::random_bytes::<2>().fault(Kind::Internal)?);
    let device_port = ports::pick_device_port(&used, seed)
        .ok_or_else(|| Fault::msg(Kind::Adb, "no free device port in the Routedroid range"))?;
    let session = hex::encode(auth::random_bytes::<8>().fault(Kind::Internal)?);
    let secret = Secret::random().fault(Kind::Internal)?;
    let host_nonce = auth::random_nonce().fault(Kind::Internal)?;
    let record = bootstrap::encode(&session, &secret).expect("session id is valid hex");

    let helper = HelperSession::start(&a.helper_socket, &a.lan_if, a.phone_ip, &a.tun, a.mtu).await?;
    info!(serial = adb.serial(), tun = %helper.tun, host_ip = %helper.host_ip, lan_prefix = helper.lan_prefix,
          phone_ip = %a.phone_ip, helper_session = %helper.session, "host network ready");
    let outcome = async {
        adb.reverse_add(device_port, host_port).await?;
        adb.write_bootstrap_record(record.as_slice()).await?;
        adb.launch_bootstrap(&session, device_port).await?;
        let stream = listener::accept_one(&listener, a.connect_timeout).await?;
        info!(host_port, device_port, "app connected");

        let cfg = SessionConfig {
            mtu: a.mtu,
            addresses: vec![Prefix::new(a.phone_ip, 32)],
            routes: vec![Prefix::new(Ipv4Addr::UNSPECIFIED, 0)],
            dns: a.dns.iter().map(ToString::to_string).collect(),
            session_name: "Routedroid".into(),
            expected_session: session.clone(),
            expected_device_port: device_port,
            secret,
        };
        let (stop_tx, stop_rx) = watch::channel(false);
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                info!("stop requested");
                let _ = stop_tx.send(true);
            }
        });
        let summary = run_session(stream, Machine::new(cfg, host_nonce), helper.relay(), stop_rx).await;
        info!(to_phone = summary.packets_to_phone, from_phone = summary.packets_from_phone, dropped = summary.bad_packets, "traffic");
        match summary.end {
            SessionEnd::LocalStop | SessionEnd::PeerStop | SessionEnd::PeerClosed if summary.reached_active => Ok(()),
            SessionEnd::VpnError(e) => Err(Fault::msg(Kind::Vpn, format!("{}: {}", e.code, e.message))),
            SessionEnd::Refused(e) if e.code == "auth_failed" => Err(Fault::msg(Kind::Auth, e.message)),
            SessionEnd::Refused(e) => Err(Fault::msg(Kind::Protocol, format!("{}: {}", e.code, e.message))),
            other => Err(Fault::msg(Kind::Vpn, other.to_string())),
        }
    }
    .await;

    adb.reverse_remove_if_ours(device_port, host_port).await;
    helper.stop().await;
    if outcome.is_err() {
        warn!("session did not complete cleanly");
    }
    outcome
}
