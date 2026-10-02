//! An interface's index and MAC, asked of the packet socket itself
//! (`SIOCGIFINDEX` / `SIOCGIFHWADDR` work on any socket family, so the
//! helper needs no IP socket for them).

use std::io;
use std::os::fd::{AsRawFd, OwnedFd};

use anyhow::{Context, Result, bail};

use crate::packet::Mac;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iface {
    pub name: String,
    pub index: i32,
    pub mac: Mac,
}

fn ifreq_for(name: &str) -> Result<libc::ifreq> {
    if name.is_empty() || name.len() >= libc::IFNAMSIZ || name.contains('\0') {
        bail!(
            "interface name {name:?} must be 1..{} bytes",
            libc::IFNAMSIZ - 1
        );
    }
    // SAFETY: ifreq is a plain C struct; zeroed is a valid value.
    let mut req: libc::ifreq = unsafe { std::mem::zeroed() };
    for (dst, src) in req.ifr_name.iter_mut().zip(name.bytes()) {
        *dst = libc::c_char::from_ne_bytes([src]);
    }
    Ok(req)
}

fn ioctl(fd: &OwnedFd, request: libc::Ioctl, name: &str) -> io::Result<libc::ifreq> {
    let mut req = ifreq_for(name).map_err(io::Error::other)?;
    // SAFETY: both requests take a pointer to an ifreq we own.
    if unsafe { libc::ioctl(fd.as_raw_fd(), request, &raw mut req) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(req)
}

pub fn lookup(fd: &OwnedFd, name: &str) -> Result<Iface> {
    let req = ioctl(fd, libc::SIOCGIFINDEX, name)
        .with_context(|| format!("{name}: no such interface"))?;
    // SAFETY: the kernel filled ifru_ifindex for SIOCGIFINDEX.
    let index = unsafe { req.ifr_ifru.ifru_ifindex };
    let req =
        ioctl(fd, libc::SIOCGIFHWADDR, name).with_context(|| format!("{name}: SIOCGIFHWADDR"))?;
    // SAFETY: the kernel filled ifru_hwaddr for SIOCGIFHWADDR.
    let hw = unsafe { req.ifr_ifru.ifru_hwaddr };
    if hw.sa_family != libc::ARPHRD_ETHER {
        bail!(
            "{name}: hardware type {} is not Ethernet; only Ethernet-like netdevices are supported",
            hw.sa_family
        );
    }
    let mut mac = [0u8; 6];
    for (dst, src) in mac.iter_mut().zip(hw.sa_data) {
        *dst = src.to_ne_bytes()[0];
    }
    Ok(Iface {
        name: name.to_owned(),
        index,
        mac,
    })
}
