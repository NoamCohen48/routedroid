//! The classic BPF program every packet socket carries from before bind:
//! IPv4/UDP to port 68, unfragmented, and every ARP frame (probes and
//! conflict detection need replies too, not only requests).

use crate::packet::{ETHERTYPE_ARP, ETHERTYPE_IPV4, IPPROTO_UDP};

// linux/filter.h
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

/// Jump offsets count from the next instruction.
const fn jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

const ACCEPT_ALL: u32 = 0x0004_0000;

/// Offsets are for an untagged Ethernet header: the kernel strips an
/// offloaded VLAN tag before the filter runs and reports it in auxdata.
#[allow(clippy::cast_lossless)] // `u32::from` is not const
pub const FILTER: [libc::sock_filter; 12] = [
    /* 0 */ stmt(BPF_LD | BPF_H | BPF_ABS, 12), // A = ethertype
    /* 1 */ jump(BPF_JMP | BPF_JEQ | BPF_K, ETHERTYPE_ARP as u32, 8, 0), // ARP -> 10
    /* 2 */ jump(BPF_JMP | BPF_JEQ | BPF_K, ETHERTYPE_IPV4 as u32, 0, 8), // else -> 11
    /* 3 */ stmt(BPF_LD | BPF_B | BPF_ABS, 23), // A = IP protocol
    /* 4 */ jump(BPF_JMP | BPF_JEQ | BPF_K, IPPROTO_UDP as u32, 0, 6), // !UDP -> 11
    /* 5 */ stmt(BPF_LD | BPF_H | BPF_ABS, 20), // A = flags + fragment offset
    /* 6 */ jump(BPF_JMP | BPF_JSET | BPF_K, 0x3fff, 4, 0), // MF or offset -> 11
    /* 7 */ stmt(BPF_LDX | BPF_B | BPF_MSH, 14), // X = IHL * 4
    /* 8 */ stmt(BPF_LD | BPF_H | BPF_IND, 16), // A = UDP dst port (14 + X + 2)
    /* 9 */ jump(BPF_JMP | BPF_JEQ | BPF_K, 68, 0, 1), // 68 -> 10, else -> 11
    /* 10 */ stmt(BPF_RET | BPF_K, ACCEPT_ALL),
    /* 11 */ stmt(BPF_RET | BPF_K, 0),
];
