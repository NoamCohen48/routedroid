//! `AF_PACKET` socket bound to one interface index with a classic BPF filter
//! attached before bind (so nothing unfiltered is ever queued), plus
//! `PACKET_AUXDATA` so VLAN offload metadata and checksum state are visible.

use std::io;
use std::mem;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use anyhow::{Context, Result, bail};
use tokio::io::unix::AsyncFd;
use tracing::debug;

use crate::packet::{ETHERTYPE_ARP, ETHERTYPE_IPV4};

/// Not in the libc crate.
const PACKET_AUXDATA: libc::c_int = 8;
const TP_STATUS_CSUMNOTREADY: u32 = 1 << 3;
const TP_STATUS_VLAN_VALID: u32 = 1 << 4;
const TP_STATUS_VLAN_TPID_VALID: u32 = 1 << 6;
pub const PACKET_OUTGOING: u8 = 4;

/// `struct tpacket_auxdata` (linux/if_packet.h).
#[repr(C)]
#[derive(Clone, Copy)]
struct TpacketAuxdata {
    tp_status: u32,
    tp_len: u32,
    tp_snaplen: u32,
    tp_mac: u16,
    tp_net: u16,
    tp_vlan_tci: u16,
    tp_vlan_tpid: u16,
}

// Classic BPF opcodes (linux/filter.h).
const BPF_LD: u16 = 0x00;
const BPF_LDX: u16 = 0x01;
const BPF_JMP: u16 = 0x05;
const BPF_RET: u16 = 0x06;
const BPF_H: u16 = 0x08;
const BPF_B: u16 = 0x10;
const BPF_ABS: u16 = 0x20;
const BPF_IND: u16 = 0x40;
const BPF_MSH: u16 = 0xa0;
const BPF_JEQ: u16 = 0x10;
const BPF_JSET: u16 = 0x40;
const BPF_K: u16 = 0x00;

const fn stmt(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}
const fn jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

/// Accept `IPv4 / UDP / not-a-fragment / dst port 68` and `ARP request`;
/// drop everything else. Frame offsets are for an untagged Ethernet header
/// (the kernel strips a hardware/accelerated VLAN tag before running the
/// socket filter and reports it via auxdata instead).
const FILTER: [libc::sock_filter; 14] = [
    /* 0 */ stmt(BPF_LD | BPF_H | BPF_ABS, 12), // A = ethertype
    /* 1 */ jump(BPF_JMP | BPF_JEQ | BPF_K, ETHERTYPE_ARP as u32, 8, 0), // ARP -> 10
    /* 2 */
    jump(BPF_JMP | BPF_JEQ | BPF_K, ETHERTYPE_IPV4 as u32, 0, 10), // !IPv4 -> drop (13)
    /* 3 */ stmt(BPF_LD | BPF_B | BPF_ABS, 23), // A = ip proto
    /* 4 */ jump(BPF_JMP | BPF_JEQ | BPF_K, 17, 0, 8), // !UDP -> drop
    /* 5 */ stmt(BPF_LD | BPF_H | BPF_ABS, 20), // A = flags+frag offset
    /* 6 */ jump(BPF_JMP | BPF_JSET | BPF_K, 0x3fff, 6, 0), // MF or offset -> drop
    /* 7 */ stmt(BPF_LDX | BPF_B | BPF_MSH, 14), // X = IHL*4
    /* 8 */ stmt(BPF_LD | BPF_H | BPF_IND, 16), // A = udp dst port (14 + X + 2)
    /* 9 */ jump(BPF_JMP | BPF_JEQ | BPF_K, 68, 2, 3), // == 68 -> accept, else drop
    /* 10 */ stmt(BPF_LD | BPF_H | BPF_ABS, 20), // A = arp opcode
    /* 11 */ jump(BPF_JMP | BPF_JEQ | BPF_K, 1, 0, 1), // request -> accept, else drop
    /* 12 */ stmt(BPF_RET | BPF_K, 0x0004_0000), // accept (whole frame)
    /* 13 */ stmt(BPF_RET | BPF_K, 0), // drop
];

#[derive(Debug, Clone)]
pub struct Iface {
    pub name: String,
    pub index: i32,
    pub mac: [u8; 6],
}

fn ifreq_for(name: &str) -> Result<libc::ifreq> {
    if name.is_empty() || name.len() >= libc::IFNAMSIZ {
        bail!("interface name must be 1..{} chars", libc::IFNAMSIZ - 1);
    }
    // SAFETY: ifreq is a plain C struct; zeroed is a valid value.
    let mut req: libc::ifreq = unsafe { mem::zeroed() };
    for (dst, src) in req.ifr_name.iter_mut().zip(name.bytes()) {
        *dst = src as libc::c_char;
    }
    Ok(req)
}

/// Interface index and MAC via `SIOCGIFINDEX` / `SIOCGIFHWADDR`.
pub fn lookup_iface(name: &str) -> Result<Iface> {
    // SAFETY: plain socket() call; result checked below.
    let raw = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if raw < 0 {
        return Err(io::Error::last_os_error()).context("socket(AF_INET, SOCK_DGRAM)");
    }
    // SAFETY: freshly created, owned descriptor.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };

    let mut req = ifreq_for(name)?;
    // SAFETY: SIOCGIFINDEX takes a pointer to an ifreq we own.
    if unsafe {
        libc::ioctl(
            fd.as_raw_fd(),
            libc::SIOCGIFINDEX as _,
            &mut req as *mut libc::ifreq,
        )
    } < 0
    {
        return Err(io::Error::last_os_error())
            .context(format!("ioctl(SIOCGIFINDEX, {name}): no such interface?"));
    }
    // SAFETY: the kernel filled ifru_ivalue for SIOCGIFINDEX.
    let index = unsafe { req.ifr_ifru.ifru_ifindex };

    let mut req = ifreq_for(name)?;
    // SAFETY: SIOCGIFHWADDR takes a pointer to an ifreq we own.
    if unsafe {
        libc::ioctl(
            fd.as_raw_fd(),
            libc::SIOCGIFHWADDR as _,
            &mut req as *mut libc::ifreq,
        )
    } < 0
    {
        return Err(io::Error::last_os_error()).context(format!("ioctl(SIOCGIFHWADDR, {name})"));
    }
    // SAFETY: the kernel filled ifru_hwaddr for SIOCGIFHWADDR.
    let hw = unsafe { req.ifr_ifru.ifru_hwaddr };
    if i32::from(hw.sa_family) != i32::from(libc::ARPHRD_ETHER) {
        bail!(
            "{name}: hardware type {} is not Ethernet (ARPHRD_ETHER); only Ethernet-like netdevices are supported",
            hw.sa_family
        );
    }
    let mut mac = [0u8; 6];
    for (dst, src) in mac.iter_mut().zip(hw.sa_data.iter()) {
        *dst = *src as u8;
    }
    Ok(Iface {
        name: name.to_string(),
        index,
        mac,
    })
}

/// Per-frame metadata from `PACKET_AUXDATA` and `sockaddr_ll`.
#[derive(Debug, Clone, Copy, Default)]
pub struct RecvMeta {
    pub pkttype: u8,
    /// `Some((tci, tpid))` when the kernel stripped a VLAN tag.
    pub vlan: Option<(u16, Option<u16>)>,
    /// Checksum not yet computed (locally generated packet, offload pending).
    pub csum_not_ready: bool,
    pub aux_present: bool,
}

pub struct PacketSocket {
    fd: AsyncFd<OwnedFd>,
    ifindex: i32,
}

impl PacketSocket {
    /// Create, filter, enable auxdata, then bind to `iface` with `ETH_P_ALL`.
    /// Needs CAP_NET_RAW.
    pub fn open(iface: &Iface) -> Result<Self> {
        // Protocol 0: the socket receives nothing until bind() sets one, so
        // the filter is in place before the first frame can be queued.
        // SAFETY: plain socket() call; result checked below.
        let raw = unsafe {
            libc::socket(
                libc::AF_PACKET,
                libc::SOCK_RAW | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                0,
            )
        };
        if raw < 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::EPERM) || e.raw_os_error() == Some(libc::EACCES) {
                bail!(
                    "socket(AF_PACKET, SOCK_RAW): {e}. This needs CAP_NET_RAW: run under sudo, or `setcap cap_net_raw+ep` on the binary"
                );
            }
            return Err(e).context("socket(AF_PACKET, SOCK_RAW)");
        }
        // SAFETY: freshly created, owned descriptor.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };

        let mut prog = FILTER;
        let fprog = libc::sock_fprog {
            len: prog.len() as u16,
            filter: prog.as_mut_ptr(),
        };
        // SAFETY: fprog points at a live array for the duration of the call.
        let rc = unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_ATTACH_FILTER,
                &fprog as *const libc::sock_fprog as *const libc::c_void,
                size_of::<libc::sock_fprog>() as libc::socklen_t,
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error()).context("setsockopt(SO_ATTACH_FILTER)");
        }

        let one: libc::c_int = 1;
        // SAFETY: `one` is a valid c_int for the duration of the call.
        let rc = unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                libc::SOL_PACKET,
                PACKET_AUXDATA,
                &one as *const libc::c_int as *const libc::c_void,
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error()).context("setsockopt(PACKET_AUXDATA)");
        }

        // SAFETY: sockaddr_ll is a plain C struct; zeroed is a valid value.
        let mut sll: libc::sockaddr_ll = unsafe { mem::zeroed() };
        sll.sll_family = libc::AF_PACKET as u16;
        sll.sll_protocol = (libc::ETH_P_ALL as u16).to_be();
        sll.sll_ifindex = iface.index;
        // SAFETY: sll is a properly initialised sockaddr_ll of the stated size.
        let rc = unsafe {
            libc::bind(
                fd.as_raw_fd(),
                &sll as *const libc::sockaddr_ll as *const libc::sockaddr,
                size_of::<libc::sockaddr_ll>() as libc::socklen_t,
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error())
                .context(format!("bind(AF_PACKET, ifindex {})", iface.index));
        }
        Ok(Self {
            fd: AsyncFd::new(fd)?,
            ifindex: iface.index,
        })
    }

    /// Receive one frame. Returns the byte count and its metadata.
    pub async fn recv(&self, buf: &mut [u8]) -> Result<(usize, RecvMeta)> {
        loop {
            let mut guard = self.fd.readable().await?;
            match guard.try_io(|inner| recvmsg_once(inner.as_raw_fd(), buf)) {
                Ok(r) => return r.context("recvmsg(AF_PACKET)"),
                Err(_would_block) => continue,
            }
        }
    }

    /// Transmit one complete Ethernet frame on the bound interface.
    pub async fn send(&self, frame: &[u8]) -> Result<()> {
        // SAFETY: sockaddr_ll is a plain C struct; zeroed is a valid value.
        let mut sll: libc::sockaddr_ll = unsafe { mem::zeroed() };
        sll.sll_family = libc::AF_PACKET as u16;
        sll.sll_ifindex = self.ifindex;
        sll.sll_halen = 6;
        sll.sll_addr[..6].copy_from_slice(&frame[..6]);
        if let Some(et) = crate::packet::ethertype(frame) {
            sll.sll_protocol = et.to_be();
        }
        loop {
            let mut guard = self.fd.writable().await?;
            let res = guard.try_io(|inner| {
                // SAFETY: frame and sll are valid for the duration of the call.
                let n = unsafe {
                    libc::sendto(
                        inner.as_raw_fd(),
                        frame.as_ptr() as *const libc::c_void,
                        frame.len(),
                        0,
                        &sll as *const libc::sockaddr_ll as *const libc::sockaddr,
                        size_of::<libc::sockaddr_ll>() as libc::socklen_t,
                    )
                };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            });
            match res {
                Ok(Ok(n)) if n == frame.len() => return Ok(()),
                Ok(Ok(n)) => bail!("short send: {n} of {} bytes", frame.len()),
                Ok(Err(e)) => return Err(e).context("sendto(AF_PACKET)"),
                Err(_would_block) => continue,
            }
        }
    }
}

fn recvmsg_once(fd: libc::c_int, buf: &mut [u8]) -> io::Result<(usize, RecvMeta)> {
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr() as *mut libc::c_void,
        iov_len: buf.len(),
    };
    // SAFETY: plain C structs; zeroed is a valid initial value.
    let mut from: libc::sockaddr_ll = unsafe { mem::zeroed() };
    let mut cmsg_buf = [0u8; 64];
    // SAFETY: msghdr is a plain C struct; zeroed is a valid value.
    let mut msg: libc::msghdr = unsafe { mem::zeroed() };
    msg.msg_name = &mut from as *mut libc::sockaddr_ll as *mut libc::c_void;
    msg.msg_namelen = size_of::<libc::sockaddr_ll>() as libc::socklen_t;
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
    msg.msg_controllen = cmsg_buf.len() as _;
    // SAFETY: all pointers in msg are valid for the call.
    let n = unsafe { libc::recvmsg(fd, &mut msg, libc::MSG_TRUNC) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut meta = RecvMeta {
        pkttype: from.sll_pkttype,
        ..Default::default()
    };
    // SAFETY: the CMSG_* helpers only read within msg_control as filled by the kernel.
    unsafe {
        let mut c = libc::CMSG_FIRSTHDR(&msg);
        while !c.is_null() {
            if (*c).cmsg_level == libc::SOL_PACKET && (*c).cmsg_type == PACKET_AUXDATA {
                let aux: TpacketAuxdata =
                    std::ptr::read_unaligned(libc::CMSG_DATA(c) as *const TpacketAuxdata);
                meta.aux_present = true;
                meta.csum_not_ready = aux.tp_status & TP_STATUS_CSUMNOTREADY != 0;
                if aux.tp_status & TP_STATUS_VLAN_VALID != 0 {
                    let tpid = (aux.tp_status & TP_STATUS_VLAN_TPID_VALID != 0)
                        .then_some(aux.tp_vlan_tpid);
                    meta.vlan = Some((aux.tp_vlan_tci, tpid));
                }
            }
            c = libc::CMSG_NXTHDR(&msg, c);
        }
    }
    let n = n as usize;
    if n > buf.len() {
        debug!(len = n, cap = buf.len(), "truncated frame dropped");
        return Ok((0, meta));
    }
    Ok((n, meta))
}

pub fn random_u32() -> u32 {
    let mut b = [0u8; 4];
    // SAFETY: b is a valid 4-byte buffer.
    let n = unsafe { libc::getrandom(b.as_mut_ptr() as *mut libc::c_void, b.len(), 0) };
    if n != 4 {
        // Fallback; only reachable on exotic kernels.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        return (t as u32) ^ std::process::id().rotate_left(16);
    }
    u32::from_le_bytes(b)
}

/// Uniform in `[0, 1)`.
pub fn random_unit() -> f64 {
    f64::from(random_u32() >> 8) / f64::from(1u32 << 24)
}
