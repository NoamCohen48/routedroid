//! Non-persistent `IFF_TUN | IFF_NO_PI` interface via `/dev/net/tun`.
//! Copied from phase0-tunnel (Phase 0 throwaway); the helper owns the fd.
//!
//! Exact semantics (architecture.md §8.5): every write is one whole packet;
//! a short write terminates the packet path (the suffix is never retried);
//! an interrupted call that transferred zero bytes may be retried.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use anyhow::{bail, Context, Result};
use tokio::io::unix::AsyncFd;

const IFF_TUN: libc::c_short = 0x0001;
const IFF_NO_PI: libc::c_short = 0x1000;
/// `_IOW('T', 202, int)`.
const TUNSETIFF: libc::c_ulong = 0x4004_54ca;

pub struct Tun {
    fd: AsyncFd<OwnedFd>,
}

impl Tun {
    /// Create (or attach to) a non-persistent TUN interface named `name`.
    /// Needs CAP_NET_ADMIN. The interface disappears when the fd is closed.
    pub fn create(name: &str) -> Result<Self> {
        if name.is_empty() || name.len() >= libc::IFNAMSIZ {
            bail!("TUN name must be 1..{} chars", libc::IFNAMSIZ - 1);
        }
        // SAFETY: plain libc open of a well-known device path.
        let raw = unsafe { libc::open(c"/dev/net/tun".as_ptr(), libc::O_RDWR | libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if raw < 0 {
            return Err(io::Error::last_os_error()).context("open /dev/net/tun");
        }
        // SAFETY: `raw` is a freshly opened, owned descriptor.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };

        // SAFETY: ifreq is a plain C struct; zeroed is a valid value.
        let mut req: libc::ifreq = unsafe { std::mem::zeroed() };
        for (dst, src) in req.ifr_name.iter_mut().zip(name.bytes()) {
            *dst = src as libc::c_char;
        }
        req.ifr_ifru.ifru_flags = IFF_TUN | IFF_NO_PI;
        // SAFETY: TUNSETIFF takes a pointer to an ifreq, which we own.
        let rc = unsafe { libc::ioctl(fd.as_raw_fd(), TUNSETIFF as _, &mut req as *mut libc::ifreq) };
        if rc < 0 {
            return Err(io::Error::last_os_error()).context(format!("ioctl(TUNSETIFF, {name})"));
        }
        let actual = {
            // SAFETY: the kernel NUL-terminates ifr_name.
            let cstr = unsafe { std::ffi::CStr::from_ptr(req.ifr_name.as_ptr()) };
            cstr.to_string_lossy().into_owned()
        };
        let fd = AsyncFd::new(fd).context("register TUN fd with tokio")?;
        if actual != name {
            bail!("kernel renamed TUN {name} to {actual}");
        }
        Ok(Self { fd })
    }

    /// Read one packet. EINTR/EAGAIN transfer nothing and are retried.
    pub async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut guard = self.fd.readable().await?;
            match guard.try_io(|inner| {
                // SAFETY: buf is a valid writable slice for the duration of the call.
                let n = unsafe { libc::read(inner.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }) {
                Ok(Ok(n)) => return Ok(n),
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        }
    }

    /// Write exactly one packet. A short write is an error and is never
    /// completed with a second write of the suffix.
    pub async fn write(&self, pkt: &[u8]) -> io::Result<()> {
        loop {
            let mut guard = self.fd.writable().await?;
            match guard.try_io(|inner| {
                // SAFETY: pkt is a valid readable slice for the duration of the call.
                let n = unsafe { libc::write(inner.as_raw_fd(), pkt.as_ptr().cast(), pkt.len()) };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }) {
                Ok(Ok(n)) if n == pkt.len() => return Ok(()),
                Ok(Ok(n)) => {
                    return Err(io::Error::other(format!("short TUN write: {n} of {} bytes", pkt.len())));
                }
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        }
    }
}

/// Configure MTU and bring the link up with iproute2.
pub fn link_up(name: &str, mtu: u32) -> Result<()> {
    let status = std::process::Command::new("ip")
        .args(["link", "set", "dev", name, "mtu", &mtu.to_string(), "up"])
        .status()
        .context("spawn ip")?;
    if !status.success() {
        bail!("ip link set {name} up failed: {status}");
    }
    Ok(())
}
