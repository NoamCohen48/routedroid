//! One `recvmsg` with its `PACKET_AUXDATA`: VLAN offload metadata and the
//! checksum state of locally generated frames.

use std::io;
use std::mem;

/// Not in the libc crate.
pub const PACKET_AUXDATA: libc::c_int = 8;
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

/// Control-message space aligned for `cmsghdr`, which `CMSG_FIRSTHDR`
/// points into and the loop below dereferences.
#[repr(C, align(8))]
struct CmsgSpace([u8; 64]);

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

#[derive(Debug, Clone, Copy)]
pub enum Received {
    Frame(usize, RecvMeta),
    /// Longer than the buffer (`MSG_TRUNC` gives the real length); dropped.
    Truncated(usize),
}

pub fn recvmsg_once(fd: libc::c_int, buf: &mut [u8]) -> io::Result<Received> {
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    };
    // SAFETY: plain C structs; zeroed is a valid value for each.
    let (mut from, mut msg): (libc::sockaddr_ll, libc::msghdr) =
        unsafe { (mem::zeroed(), mem::zeroed()) };
    let mut control = CmsgSpace([0; 64]);
    msg.msg_name = (&raw mut from).cast();
    msg.msg_namelen = socklen::<libc::sockaddr_ll>();
    msg.msg_iov = &raw mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.0.as_mut_ptr().cast();
    msg.msg_controllen = control.0.len();
    // SAFETY: every pointer in msg is valid for the call.
    let n = unsafe { libc::recvmsg(fd, &raw mut msg, libc::MSG_TRUNC) };
    let n = usize::try_from(n).map_err(|_| io::Error::last_os_error())?;
    if n > buf.len() {
        return Ok(Received::Truncated(n));
    }
    let mut meta = RecvMeta {
        pkttype: from.sll_pkttype,
        ..RecvMeta::default()
    };
    // SAFETY: the CMSG_* helpers stay within msg_control as the kernel filled
    // it; `control` is aligned for cmsghdr, and the payload is read unaligned.
    unsafe {
        let mut c = libc::CMSG_FIRSTHDR(&raw const msg);
        while !c.is_null() {
            if (*c).cmsg_level == libc::SOL_PACKET && (*c).cmsg_type == PACKET_AUXDATA {
                let aux: TpacketAuxdata =
                    libc::CMSG_DATA(c).cast::<TpacketAuxdata>().read_unaligned();
                meta.aux_present = true;
                meta.csum_not_ready = aux.tp_status & TP_STATUS_CSUMNOTREADY != 0;
                if aux.tp_status & TP_STATUS_VLAN_VALID != 0 {
                    let tpid = (aux.tp_status & TP_STATUS_VLAN_TPID_VALID != 0)
                        .then_some(aux.tp_vlan_tpid);
                    meta.vlan = Some((aux.tp_vlan_tci, tpid));
                }
            }
            c = libc::CMSG_NXTHDR(&raw const msg, c);
        }
    }
    Ok(Received::Frame(n, meta))
}

/// `size_of::<T>()` as a `socklen_t`; every sockaddr is far below its limit.
pub fn socklen<T>() -> libc::socklen_t {
    libc::socklen_t::try_from(size_of::<T>()).expect("sockaddr size fits socklen_t")
}
