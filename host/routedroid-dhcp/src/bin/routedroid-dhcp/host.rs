//! The host's own IPv4 addresses, which a lease must never be.

use std::io;
use std::net::Ipv4Addr;

use anyhow::{Context, Result};

pub fn addresses() -> Result<Vec<Ipv4Addr>> {
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs writes a list head we free below.
    if unsafe { libc::getifaddrs(&raw mut head) } < 0 {
        return Err(io::Error::last_os_error()).context("getifaddrs");
    }
    let mut out = Vec::new();
    let mut cur = head;
    while !cur.is_null() {
        // SAFETY: cur is a node of the list getifaddrs returned, not yet freed.
        let ifa = unsafe { &*cur };
        if !ifa.ifa_addr.is_null()
            // SAFETY: a non-null ifa_addr points at a sockaddr.
            && i32::from(unsafe { (*ifa.ifa_addr).sa_family }) == libc::AF_INET
        {
            // SAFETY: an AF_INET sockaddr is a sockaddr_in.
            let sin = unsafe { &*ifa.ifa_addr.cast::<libc::sockaddr_in>() };
            out.push(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)));
        }
        cur = ifa.ifa_next;
    }
    // SAFETY: head came from getifaddrs and is freed exactly once.
    unsafe { libc::freeifaddrs(head) };
    Ok(out)
}
