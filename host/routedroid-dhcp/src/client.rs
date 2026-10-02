//! The DHCP client proper: transactions (DISCOVER/REQUEST, RENEW, REBIND,
//! INIT-REBOOT, RELEASE), the lease record, the BOUND hold loop, and the
//! ARP responder for the lease address.
//!
//! The lease is never configured on the interface. Because of that, nobody
//! would answer ARP for it, and every server unicasts RENEW/REBIND replies
//! to `ciaddr` (RFC 2131 §4.1) -- so while a lease is known this client
//! answers ARP requests for it with the interface MAC. That is the same
//! proxy-presence the production host later provides for the phone.

use std::net::Ipv4Addr;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::signal::unix::{signal, Signal, SignalKind};
use tracing::{debug, info, warn};

use crate::dhcp::{
    self, Identity, Message, MessageType, Options, StaticRoute, CLIENT_PORT, SERVER_PORT,
};
use crate::packet::{self, fmt_mac, parse_mac, Mac, BROADCAST_MAC};
use crate::sock::{self, Iface, PacketSocket, RecvMeta, PACKET_OUTGOING};

/// RFC 2131 §4.1 retransmission: 4 s doubling to 64 s, plus jitter.
const RETRY_BASE: Duration = Duration::from_secs(4);
const RETRY_MAX: Duration = Duration::from_secs(64);
/// REQUEST retransmissions before falling back to DISCOVER.
const REQUEST_ATTEMPTS: u32 = 4;
/// Retry floor while RENEWING/REBINDING (RFC says 60 s; shorter for a probe).
const RENEW_RETRY_MIN: Duration = Duration::from_secs(10);
/// How long one RENEW/REBIND/INIT-REBOOT transmission waits for its reply.
const RENEW_REPLY_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lease {
    pub iface: String,
    pub client_id: String,
    pub address: Ipv4Addr,
    pub prefix: u8,
    pub subnet_mask: Ipv4Addr,
    pub router: Option<Ipv4Addr>,
    pub dns: Vec<Ipv4Addr>,
    pub static_routes: Vec<StaticRoute>,
    pub server_id: Ipv4Addr,
    /// Ethernet source of the ACK frame: the server or the relay/next hop
    /// through which it is reached. Used for unicast RENEW/RELEASE.
    pub server_mac: String,
    pub lease_secs: u32,
    pub t1: u32,
    pub t2: u32,
    /// Unix seconds when the ACK was received.
    pub acquired_at: u64,
    /// True if any reply frame carried VLAN offload metadata.
    pub vlan_tagged_replies: bool,
}

impl Lease {
    pub fn server_mac(&self) -> Result<Mac> {
        parse_mac(&self.server_mac)
            .ok_or_else(|| anyhow!("state file: bad server_mac {:?}", self.server_mac))
    }
    pub fn expires_at(&self) -> u64 {
        self.acquired_at.saturating_add(u64::from(self.lease_secs))
    }
    pub fn t1_at(&self) -> u64 {
        self.acquired_at.saturating_add(u64::from(self.t1))
    }
    pub fn t2_at(&self) -> u64 {
        self.acquired_at.saturating_add(u64::from(self.t2))
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("lease serialises")
    }
    pub fn load(path: &Path) -> Result<Self> {
        let s = std::fs::read_to_string(path)
            .with_context(|| format!("read state file {}", path.display()))?;
        serde_json::from_str(&s).with_context(|| format!("parse state file {}", path.display()))
    }
    /// Write atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, format!("{}\n", serde_json::to_string_pretty(self)?))
            .with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What a transaction ended with.
#[derive(Debug)]
pub enum Outcome {
    Bound(Lease),
    Nak(String),
    Timeout,
}

/// One validated server reply.
struct Reply {
    msg: Message,
    opts: Options,
    kind: MessageType,
    src_mac: Mac,
    src_ip: Ipv4Addr,
}

pub struct Client {
    sock: PacketSocket,
    pub iface: Iface,
    identity: Identity,
    client_id: String,
    /// Address we answer ARP for, once known.
    arp_ip: Option<Ipv4Addr>,
    arp_enabled: bool,
    vlan_seen: bool,
    vlan_logged: bool,
    buf: Vec<u8>,
    started: Instant,
    sigint: Signal,
    sigterm: Signal,
}

/// Why a wait ended early.
enum Interrupt {
    Signal,
}

/// REQUEST flavours sent from a lease-holding state.
#[derive(Debug, Clone, Copy)]
enum Tx {
    Renew,
    Rebind,
    InitReboot,
}

impl Client {
    pub fn new(iface_name: &str, client_id: &str, arp_enabled: bool) -> Result<Self> {
        if client_id.is_empty() || client_id.len() > 254 {
            bail!("--client-id must be 1..=254 bytes");
        }
        let iface = sock::lookup_iface(iface_name)?;
        let sock = PacketSocket::open(&iface)?;
        info!(
            iface = %iface.name,
            ifindex = iface.index,
            mac = %fmt_mac(&iface.mac),
            client_id,
            "AF_PACKET socket bound (BPF: IPv4/UDP dst 68 + ARP requests; PACKET_AUXDATA on)"
        );
        let sigint = signal(SignalKind::interrupt()).context("install SIGINT handler")?;
        let sigterm = signal(SignalKind::terminate()).context("install SIGTERM handler")?;
        Ok(Self {
            sock,
            identity: Identity::new(iface.mac, client_id),
            iface,
            client_id: client_id.to_string(),
            arp_ip: None,
            arp_enabled,
            vlan_seen: false,
            vlan_logged: false,
            buf: vec![0u8; 65536],
            started: Instant::now(),
            sigint,
            sigterm,
        })
    }

    /// Start (or stop, with `None`) answering ARP for `ip`.
    pub fn set_arp_ip(&mut self, ip: Option<Ipv4Addr>) {
        if self.arp_enabled {
            if ip != self.arp_ip {
                match ip {
                    Some(ip) => {
                        info!(%ip, "ARP responder: answering requests for the lease address")
                    }
                    None => info!("ARP responder: off"),
                }
            }
            self.arp_ip = ip;
        }
    }

    fn secs(&self) -> u16 {
        self.started.elapsed().as_secs().min(u64::from(u16::MAX)) as u16
    }

    async fn send_dhcp(
        &self,
        msg: &Message,
        dst_mac: &Mac,
        src_ip: Ipv4Addr,
        dst_ip: Ipv4Addr,
    ) -> Result<()> {
        let payload = msg.encode();
        let frame = packet::ipv4_udp_frame(
            &self.iface.mac,
            dst_mac,
            src_ip,
            dst_ip,
            CLIENT_PORT,
            SERVER_PORT,
            &payload,
        );
        let kind = msg
            .message_type()
            .map(|t| t.to_string())
            .unwrap_or_default();
        info!(
            kind = %kind,
            xid = format_args!("{:#010x}", msg.xid),
            dst_mac = %fmt_mac(dst_mac),
            ip = %format_args!("{src_ip} -> {dst_ip}"),
            ciaddr = %msg.ciaddr,
            secs = msg.secs,
            broadcast_flag = msg.flags & dhcp::FLAG_BROADCAST != 0,
            len = frame.len(),
            "tx"
        );
        self.sock.send(&frame).await
    }

    async fn send_broadcast(&self, msg: &Message) -> Result<()> {
        self.send_dhcp(
            msg,
            &BROADCAST_MAC,
            Ipv4Addr::UNSPECIFIED,
            Ipv4Addr::BROADCAST,
        )
        .await
    }

    fn note_meta(&mut self, meta: &RecvMeta) {
        if let Some((tci, tpid)) = meta.vlan {
            self.vlan_seen = true;
            if !self.vlan_logged {
                self.vlan_logged = true;
                info!(
                    vid = tci & 0x0fff,
                    pcp = tci >> 13,
                    tpid = tpid
                        .map(|t| format!("{t:#06x}"))
                        .unwrap_or_else(|| "n/a".into()),
                    "PACKET_AUXDATA: VLAN tag present on received frames"
                );
            }
        } else if !self.vlan_logged {
            self.vlan_logged = true;
            info!(
                aux = meta.aux_present,
                "PACKET_AUXDATA: no VLAN tag on received frames"
            );
        }
    }

    /// Answer an ARP request for our lease address.
    async fn handle_arp(&self, frame: &[u8]) -> Result<()> {
        let Some(arp) = packet::parse_arp(frame) else {
            return Ok(());
        };
        let Some(ip) = self.arp_ip else { return Ok(()) };
        if arp.op != packet::ARP_REQUEST || arp.tpa != ip || arp.sha == self.iface.mac {
            return Ok(());
        }
        if arp.spa == ip {
            warn!(from = %fmt_mac(&arp.sha), %ip, "ARP: another station claims our lease address (conflict)");
            return Ok(());
        }
        debug!(from = %fmt_mac(&arp.sha), spa = %arp.spa, %ip, "ARP: replying for lease address");
        let reply = packet::arp_reply_frame(&self.iface.mac, ip, &arp.sha, arp.spa);
        self.sock.send(&reply).await
    }

    /// Receive frames until `until`, answering ARP, returning the first DHCP
    /// reply that matches `xid`, is a BOOTREPLY for our MAC, and has a
    /// message type in `want`. `Ok(Ok(None))` on deadline; `Ok(Err(..))` if
    /// a signal arrived.
    async fn wait_reply(
        &mut self,
        xid: u32,
        want: &[MessageType],
        until: Instant,
    ) -> Result<std::result::Result<Option<Reply>, Interrupt>> {
        loop {
            let now = Instant::now();
            if now >= until {
                return Ok(Ok(None));
            }
            let remaining = until - now;
            let (n, meta) = {
                let Self {
                    sock,
                    sigint,
                    sigterm,
                    buf,
                    ..
                } = &mut *self;
                tokio::select! {
                    r = sock.recv(buf) => r?,
                    _ = sigint.recv() => return Ok(Err(Interrupt::Signal)),
                    _ = sigterm.recv() => return Ok(Err(Interrupt::Signal)),
                    _ = tokio::time::sleep(remaining) => continue,
                }
            };
            if n == 0 || meta.pkttype == PACKET_OUTGOING {
                continue;
            }
            self.note_meta(&meta);
            let frame = &self.buf[..n];
            if packet::ethertype(frame) == Some(packet::ETHERTYPE_ARP) {
                self.handle_arp(frame).await?;
                continue;
            }
            if let Some(reply) = self.validate(frame, &meta, xid) {
                if want.contains(&reply.kind) {
                    return Ok(Ok(Some(reply)));
                }
                debug!(kind = %reply.kind, "reply type not wanted now; ignored");
            }
        }
    }

    /// Strict receive validation (architecture.md §6.1).
    fn validate(&self, frame: &[u8], meta: &RecvMeta, xid: u32) -> Option<Reply> {
        let udp = match packet::parse_udp(frame, !meta.csum_not_ready) {
            Ok(u) => u,
            Err(e) => {
                debug!(error = %e, len = frame.len(), "ignored frame");
                return None;
            }
        };
        if udp.dst_port != CLIENT_PORT {
            return None;
        }
        if udp.src_port != SERVER_PORT {
            debug!(
                src_port = udp.src_port,
                "reply from a non-67 source port (accepted)"
            );
        }
        let msg = match Message::parse(udp.payload) {
            Ok(m) => m,
            Err(e) => {
                debug!(error = %e, from = %udp.src_ip, "ignored malformed DHCP payload");
                return None;
            }
        };
        if msg.xid != xid {
            debug!(
                xid = format_args!("{:#010x}", msg.xid),
                "ignored: foreign xid"
            );
            return None;
        }
        if msg.op != dhcp::BOOTREPLY {
            debug!(op = msg.op, "ignored: not BOOTREPLY");
            return None;
        }
        if msg.htype != dhcp::HTYPE_ETHERNET || msg.hlen != 6 || msg.chaddr_mac() != self.iface.mac
        {
            debug!(htype = msg.htype, hlen = msg.hlen, chaddr = %fmt_mac(&msg.chaddr_mac()), "ignored: chaddr is not ours");
            return None;
        }
        let opts = Options::from_message(&msg);
        let Some(kind) = opts.message_type else {
            debug!("ignored: no/invalid option 53");
            return None;
        };
        if !opts.malformed.is_empty() {
            warn!(options = ?opts.malformed, "reply carries malformed options (ignored individually)");
        }
        info!(
            kind = %kind,
            xid = format_args!("{:#010x}", msg.xid),
            from = %format_args!("{} ({})", udp.src_ip, fmt_mac(&udp.src_mac)),
            to = %format_args!("{} ({})", udp.dst_ip, fmt_mac(&udp.dst_mac)),
            yiaddr = %msg.yiaddr,
            server_id = ?opts.server_id,
            lease = ?opts.lease_secs,
            csum_verified = !meta.csum_not_ready,
            vlan = ?meta.vlan,
            "rx"
        );
        Some(Reply {
            msg,
            opts,
            kind,
            src_mac: udp.src_mac,
            src_ip: udp.src_ip,
        })
    }

    fn lease_from_ack(&self, r: &Reply) -> Result<Lease> {
        let address = r.msg.yiaddr;
        if address.is_unspecified() || address.is_broadcast() || address.is_multicast() {
            bail!("ACK yiaddr {address} is not a usable unicast address");
        }
        let Some(lease_secs) = r.opts.lease_secs else {
            bail!("ACK without lease time (option 51)");
        };
        let subnet_mask = match r.opts.subnet_mask {
            Some(m) => m,
            None => {
                let m = classful_mask(address);
                warn!(%address, mask = %m, "ACK without subnet mask (option 1); using classful default");
                m
            }
        };
        let prefix = mask_prefix(subnet_mask)
            .ok_or_else(|| anyhow!("non-contiguous subnet mask {subnet_mask}"))?;
        let server_id = match r.opts.server_id {
            Some(s) => s,
            None => {
                warn!(src = %r.src_ip, "ACK without server identifier (option 54); using IP source");
                r.src_ip
            }
        };
        let t1 = r.opts.t1.unwrap_or(lease_secs / 2);
        let t2 = r
            .opts
            .t2
            .unwrap_or_else(|| (u64::from(lease_secs) * 7 / 8) as u32);
        if !(t1 < t2 && t2 <= lease_secs) {
            warn!(t1, t2, lease_secs, "odd timer relation in ACK");
        }
        Ok(Lease {
            iface: self.iface.name.clone(),
            client_id: self.client_id.clone(),
            address,
            prefix,
            subnet_mask,
            router: r.opts.routers.first().copied(),
            dns: r.opts.dns.clone(),
            static_routes: r.opts.classless_routes.clone(),
            server_id,
            server_mac: fmt_mac(&r.src_mac),
            lease_secs,
            t1,
            t2,
            acquired_at: unix_now(),
            vlan_tagged_replies: self.vlan_seen,
        })
    }

    /// INIT -> SELECTING -> REQUESTING -> BOUND with RFC 2131 backoff.
    pub async fn acquire(&mut self, timeout: Duration, offer_window: Duration) -> Result<Outcome> {
        let deadline = Instant::now() + timeout;
        let mut attempt: u32 = 0;
        loop {
            if Instant::now() >= deadline {
                return Ok(Outcome::Timeout);
            }
            let xid = sock::random_u32();
            let wait = retry_delay(attempt).min(deadline.saturating_duration_since(Instant::now()));
            info!(
                attempt,
                xid = format_args!("{xid:#010x}"),
                wait_secs = wait.as_secs_f64(),
                "SELECTING: sending DISCOVER"
            );
            self.send_broadcast(&dhcp::discover(&self.identity, xid, self.secs()))
                .await?;

            // Collect OFFERs: wait up to `wait` for the first, then `offer_window` more.
            let mut offers: Vec<Reply> = Vec::new();
            let mut until = Instant::now() + wait;
            loop {
                match self.wait_reply(xid, &[MessageType::Offer], until).await? {
                    Err(Interrupt::Signal) => bail!("interrupted"),
                    Ok(None) => break,
                    Ok(Some(o)) => {
                        if offers.is_empty() {
                            until = (Instant::now() + offer_window).min(deadline);
                        }
                        info!(n = offers.len() + 1, yiaddr = %o.msg.yiaddr, server = ?o.opts.server_id, "OFFER");
                        offers.push(o);
                    }
                }
            }
            if offers.is_empty() {
                warn!(attempt, "no OFFER received");
                attempt += 1;
                continue;
            }
            if offers.len() > 1 {
                info!(count = offers.len(), "multiple OFFERs; taking the first");
            }
            let offer = offers.swap_remove(0);
            let Some(server) = offer.opts.server_id else {
                warn!("OFFER without server identifier; ignoring it");
                attempt += 1;
                continue;
            };
            let requested = offer.msg.yiaddr;

            // REQUESTING.
            let mut req_attempt = 0;
            let outcome = loop {
                if Instant::now() >= deadline {
                    break Outcome::Timeout;
                }
                let wait = retry_delay(req_attempt)
                    .min(deadline.saturating_duration_since(Instant::now()));
                info!(req_attempt, %requested, %server, "REQUESTING: sending REQUEST");
                self.send_broadcast(&dhcp::request_selecting(
                    &self.identity,
                    xid,
                    self.secs(),
                    requested,
                    server,
                ))
                .await?;
                match self
                    .wait_reply(
                        xid,
                        &[MessageType::Ack, MessageType::Nak],
                        Instant::now() + wait,
                    )
                    .await?
                {
                    Err(Interrupt::Signal) => bail!("interrupted"),
                    Ok(Some(r)) if r.kind == MessageType::Ack => {
                        break Outcome::Bound(self.lease_from_ack(&r)?)
                    }
                    Ok(Some(r)) => break Outcome::Nak(r.opts.message.unwrap_or_default()),
                    Ok(None) => {
                        req_attempt += 1;
                        if req_attempt >= REQUEST_ATTEMPTS {
                            warn!("no ACK/NAK after {REQUEST_ATTEMPTS} REQUESTs; restarting from DISCOVER");
                            break Outcome::Timeout;
                        }
                    }
                }
            };
            match outcome {
                Outcome::Bound(l) => {
                    self.set_arp_ip(Some(l.address));
                    return Ok(Outcome::Bound(l));
                }
                Outcome::Nak(m) => {
                    warn!(message = %m, "NAK in REQUESTING; backing off and restarting");
                    attempt += 1;
                    let pause = retry_delay(attempt)
                        .min(deadline.saturating_duration_since(Instant::now()));
                    if let Err(Interrupt::Signal) = self.idle(Instant::now() + pause).await? {
                        bail!("interrupted");
                    }
                }
                Outcome::Timeout => attempt += 1,
            }
        }
    }

    /// One REQUEST in a lease-holding state with a single reply wait.
    async fn request_once(&mut self, lease: &Lease, tx: Tx, wait: Duration) -> Result<Outcome> {
        let xid = sock::random_u32();
        match tx {
            Tx::Renew => {
                let mac = lease.server_mac()?;
                let msg = dhcp::request_renew(&self.identity, xid, self.secs(), lease.address);
                info!(xid = format_args!("{xid:#010x}"), server = %lease.server_id, server_mac = %lease.server_mac, "RENEWING: unicast REQUEST with ciaddr");
                self.send_dhcp(&msg, &mac, lease.address, lease.server_id)
                    .await?;
            }
            Tx::Rebind => {
                let msg = dhcp::request_renew(&self.identity, xid, self.secs(), lease.address);
                info!(
                    xid = format_args!("{xid:#010x}"),
                    "REBINDING: broadcast REQUEST with ciaddr"
                );
                self.send_dhcp(&msg, &BROADCAST_MAC, lease.address, Ipv4Addr::BROADCAST)
                    .await?;
            }
            Tx::InitReboot => {
                info!(xid = format_args!("{xid:#010x}"), requested = %lease.address, "INIT-REBOOT: broadcast REQUEST with option 50");
                self.send_broadcast(&dhcp::request_init_reboot(
                    &self.identity,
                    xid,
                    self.secs(),
                    lease.address,
                ))
                .await?;
            }
        }
        match self
            .wait_reply(
                xid,
                &[MessageType::Ack, MessageType::Nak],
                Instant::now() + wait,
            )
            .await?
        {
            Err(Interrupt::Signal) => bail!("interrupted"),
            Ok(Some(r)) if r.kind == MessageType::Ack => {
                let mut l = self.lease_from_ack(&r)?;
                if l.address != lease.address {
                    warn!(old = %lease.address, new = %l.address, "server changed our address");
                }
                l.client_id = lease.client_id.clone();
                Ok(Outcome::Bound(l))
            }
            Ok(Some(r)) => Ok(Outcome::Nak(r.opts.message.unwrap_or_default())),
            Ok(None) => Ok(Outcome::Timeout),
        }
    }

    /// Repeat `tx` with the RFC backoff sequence until a definite outcome or
    /// `timeout`.
    async fn request_retried(
        &mut self,
        lease: &Lease,
        tx: Tx,
        timeout: Duration,
    ) -> Result<Outcome> {
        self.set_arp_ip(Some(lease.address));
        let deadline = Instant::now() + timeout;
        let mut attempt = 0;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(Outcome::Timeout);
            }
            let wait = retry_delay(attempt).min(left);
            match self.request_once(lease, tx, wait).await? {
                Outcome::Timeout => attempt += 1,
                o => return Ok(o),
            }
        }
    }

    /// `renew` subcommand: unicast RENEW with retries inside `timeout`.
    pub async fn renew(&mut self, lease: &Lease, timeout: Duration) -> Result<Outcome> {
        self.request_retried(lease, Tx::Renew, timeout).await
    }

    /// INIT-REBOOT: broadcast REQUEST with option 50, no server id.
    pub async fn init_reboot(&mut self, lease: &Lease, timeout: Duration) -> Result<Outcome> {
        self.request_retried(lease, Tx::InitReboot, timeout).await
    }

    /// RELEASE: unicast to the server, no reply expected.
    pub async fn release(&mut self, lease: &Lease) -> Result<()> {
        let xid = sock::random_u32();
        let mac = lease.server_mac()?;
        info!(xid = format_args!("{xid:#010x}"), address = %lease.address, server = %lease.server_id, "RELEASE (unicast)");
        let msg = dhcp::release(&self.identity, xid, lease.address, lease.server_id);
        self.send_dhcp(&msg, &mac, lease.address, lease.server_id)
            .await?;
        self.set_arp_ip(None);
        Ok(())
    }

    /// Service ARP (and discard stray DHCP) until `until` or a signal.
    async fn idle(&mut self, until: Instant) -> Result<std::result::Result<(), Interrupt>> {
        // xid 0 with an empty want-list: nothing DHCP can match.
        match self.wait_reply(0, &[], until).await? {
            Ok(_) => Ok(Ok(())),
            Err(i) => Ok(Err(i)),
        }
    }

    /// BOUND for `hold`: RENEW at T1 (or `renew_after`), REBIND at T2,
    /// give up at expiry. Returns the current lease when the hold ends or a
    /// signal arrives; `Err` with `Outcome`-like exit semantics otherwise.
    pub async fn hold(
        &mut self,
        mut lease: Lease,
        hold: Duration,
        renew_after: Option<Duration>,
    ) -> Result<HoldEnd> {
        let end = Instant::now() + hold;
        self.set_arp_ip(Some(lease.address));
        let mut first_renew_at = renew_after.map(|d| unix_now() + d.as_secs());
        let mut next_retry: Option<u64> = None;
        loop {
            let now = unix_now();
            let t1 = first_renew_at.unwrap_or_else(|| lease.t1_at());
            let t2 = lease.t2_at();
            let expiry = lease.expires_at();
            if now >= expiry {
                warn!(address = %lease.address, "lease expired without a successful renewal");
                return Ok(HoldEnd::Expired);
            }
            // Which action is due, and when.
            let (due_at, rebind) = if now >= t2 {
                (next_retry.unwrap_or(now).max(t2), true)
            } else if now >= t1 {
                (next_retry.unwrap_or(now).max(t1), false)
            } else {
                (t1, false)
            };
            let due_in = Duration::from_secs(due_at.saturating_sub(now));
            let wake = (Instant::now() + due_in).min(end);
            if wake >= end {
                info!(
                    secs = end.saturating_duration_since(Instant::now()).as_secs(),
                    next_renew_in = due_at.saturating_sub(now),
                    "BOUND: holding"
                );
            } else {
                info!(in_secs = due_in.as_secs(), rebind, "BOUND: next renewal");
            }
            if let Err(Interrupt::Signal) = self.idle(wake).await? {
                info!("signal received; leaving hold");
                return Ok(HoldEnd::Signal(lease));
            }
            if Instant::now() >= end {
                return Ok(HoldEnd::Done(lease));
            }
            // Time to renew or rebind.
            let now = unix_now();
            if now < t1 {
                continue;
            }
            let bound = t2.max(now + 1).min(expiry);
            let wait = RENEW_REPLY_WAIT.min(Duration::from_secs(bound.saturating_sub(now).max(1)));
            match self
                .request_once(&lease, if rebind { Tx::Rebind } else { Tx::Renew }, wait)
                .await?
            {
                Outcome::Bound(l) => {
                    info!(address = %l.address, lease_secs = l.lease_secs, t1 = l.t1, t2 = l.t2, "renewal ACK; lease refreshed");
                    println!("{}", l.to_json());
                    lease = l;
                    first_renew_at = None;
                    next_retry = None;
                    self.set_arp_ip(Some(lease.address));
                }
                Outcome::Nak(m) => {
                    warn!(message = %m, "NAK on renewal; lease lost");
                    self.set_arp_ip(None);
                    return Ok(HoldEnd::Nak);
                }
                Outcome::Timeout => {
                    let now = unix_now();
                    let horizon = if rebind { expiry } else { t2 };
                    let half = Duration::from_secs(horizon.saturating_sub(now) / 2);
                    let delay = half.max(RENEW_RETRY_MIN);
                    warn!(
                        retry_in = delay.as_secs(),
                        rebind, "no reply to renewal; will retry"
                    );
                    next_retry = Some(now + delay.as_secs());
                }
            }
        }
    }
}

/// How the BOUND hold ended.
#[derive(Debug)]
pub enum HoldEnd {
    Done(Lease),
    Signal(Lease),
    Nak,
    Expired,
}

/// RFC 2131 §4.1: 4, 8, 16, 32, 64 s plus uniform jitter.
fn retry_delay(attempt: u32) -> Duration {
    let base = RETRY_BASE
        .saturating_mul(1u32 << attempt.min(4))
        .min(RETRY_MAX);
    // +0..1 s jitter (RFC 2131 says +-1 s; never shortening the wait keeps a
    // 3 s server-side ping check, as dnsmasq does, inside the first attempt).
    let jitter = sock::random_unit();
    let secs = (base.as_secs_f64() + jitter).max(1.0);
    Duration::from_secs_f64(secs)
}

fn classful_mask(a: Ipv4Addr) -> Ipv4Addr {
    let first = a.octets()[0];
    if first < 128 {
        Ipv4Addr::new(255, 0, 0, 0)
    } else if first < 192 {
        Ipv4Addr::new(255, 255, 0, 0)
    } else {
        Ipv4Addr::new(255, 255, 255, 0)
    }
}

/// Prefix length of a contiguous mask, `None` if it has holes.
pub fn mask_prefix(mask: Ipv4Addr) -> Option<u8> {
    let m = u32::from(mask);
    let ones = m.count_ones();
    (ones == 0 || m == u32::MAX << (32 - ones)).then_some(ones as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_prefix_contiguous_only() {
        assert_eq!(mask_prefix(Ipv4Addr::new(255, 255, 255, 0)), Some(24));
        assert_eq!(mask_prefix(Ipv4Addr::new(255, 255, 255, 255)), Some(32));
        assert_eq!(mask_prefix(Ipv4Addr::new(0, 0, 0, 0)), Some(0));
        assert_eq!(mask_prefix(Ipv4Addr::new(255, 0, 255, 0)), None);
        assert_eq!(
            classful_mask(Ipv4Addr::new(10, 1, 1, 1)),
            Ipv4Addr::new(255, 0, 0, 0)
        );
        assert_eq!(
            classful_mask(Ipv4Addr::new(192, 168, 1, 1)),
            Ipv4Addr::new(255, 255, 255, 0)
        );
    }

    #[test]
    fn retry_delay_is_bounded() {
        for a in 0..10 {
            let d = retry_delay(a).as_secs_f64();
            assert!((1.0..=65.0).contains(&d), "attempt {a}: {d}");
        }
        assert!(retry_delay(0).as_secs_f64() <= 5.0);
        assert!(retry_delay(9).as_secs_f64() >= 63.0);
    }

    #[test]
    fn lease_json_roundtrip() {
        let l = Lease {
            iface: "eth0".into(),
            client_id: "routedroid:x:y".into(),
            address: "192.168.50.101".parse().unwrap(),
            prefix: 24,
            subnet_mask: "255.255.255.0".parse().unwrap(),
            router: Some("192.168.50.1".parse().unwrap()),
            dns: vec!["192.168.50.1".parse().unwrap()],
            static_routes: vec![],
            server_id: "192.168.50.1".parse().unwrap(),
            server_mac: "de:ad:be:ef:00:01".into(),
            lease_secs: 3600,
            t1: 1800,
            t2: 3150,
            acquired_at: 1_700_000_000,
            vlan_tagged_replies: false,
        };
        let back: Lease = serde_json::from_str(&l.to_json()).unwrap();
        assert_eq!(back.address, l.address);
        assert_eq!(back.server_mac().unwrap(), [0xde, 0xad, 0xbe, 0xef, 0, 1]);
        assert_eq!(back.expires_at(), 1_700_003_600);
    }
}
