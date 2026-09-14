//! Per-direction packet/byte counters and drop counters.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

#[derive(Debug, Default)]
pub struct Stats {
    /// Android VPN read -> IP_PACKET -> host TUN write.
    pub peer_to_tun_packets: AtomicU64,
    pub peer_to_tun_bytes: AtomicU64,
    /// Linux TUN read -> IP_PACKET -> Android VPN write.
    pub tun_to_peer_packets: AtomicU64,
    pub tun_to_peer_bytes: AtomicU64,
    /// TUN packets discarded because the session was not Active yet.
    pub drop_tun_not_active: AtomicU64,
    /// TUN packets discarded because they were not valid IPv4 (e.g. IPv6 ND).
    pub drop_tun_invalid: AtomicU64,
    /// TUN packets discarded because they exceeded the negotiated MTU.
    pub drop_tun_oversize: AtomicU64,
    /// Control frames exchanged (both directions).
    pub control_frames: AtomicU64,
}

impl Stats {
    pub fn add_peer_to_tun(&self, bytes: usize) {
        self.peer_to_tun_packets.fetch_add(1, Relaxed);
        self.peer_to_tun_bytes.fetch_add(bytes as u64, Relaxed);
    }

    pub fn add_tun_to_peer(&self, bytes: usize) {
        self.tun_to_peer_packets.fetch_add(1, Relaxed);
        self.tun_to_peer_bytes.fetch_add(bytes as u64, Relaxed);
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            peer_to_tun_packets: self.peer_to_tun_packets.load(Relaxed),
            peer_to_tun_bytes: self.peer_to_tun_bytes.load(Relaxed),
            tun_to_peer_packets: self.tun_to_peer_packets.load(Relaxed),
            tun_to_peer_bytes: self.tun_to_peer_bytes.load(Relaxed),
            drop_tun_not_active: self.drop_tun_not_active.load(Relaxed),
            drop_tun_invalid: self.drop_tun_invalid.load(Relaxed),
            drop_tun_oversize: self.drop_tun_oversize.load(Relaxed),
            control_frames: self.control_frames.load(Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub peer_to_tun_packets: u64,
    pub peer_to_tun_bytes: u64,
    pub tun_to_peer_packets: u64,
    pub tun_to_peer_bytes: u64,
    pub drop_tun_not_active: u64,
    pub drop_tun_invalid: u64,
    pub drop_tun_oversize: u64,
    pub control_frames: u64,
}

impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "android->tun {} pkts/{} B, tun->android {} pkts/{} B, drops: not_active={} invalid_ipv4={} oversize={}, control frames {}",
            self.peer_to_tun_packets,
            self.peer_to_tun_bytes,
            self.tun_to_peer_packets,
            self.tun_to_peer_bytes,
            self.drop_tun_not_active,
            self.drop_tun_invalid,
            self.drop_tun_oversize,
            self.control_frames,
        )
    }
}
