//! Non-persistent `IFF_TUN | IFF_NO_PI` interface via `/dev/net/tun`.
//!
//! Exact semantics (architecture.md §8.5): every write is one whole packet;
//! a short write terminates the packet path (the suffix is never retried);
//! an interrupted call that transferred zero bytes may be retried.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::ipv4;
use crate::session::{TunEndpoints, QUEUE_DEPTH};
use crate::stats::Stats;

const IFF_TUN: libc::c_short = 0x0001;
const IFF_NO_PI: libc::c_short = 0x1000;
/// `_IOW('T', 202, int)`.
const TUNSETIFF: libc::c_ulong = 0x4004_54ca;

pub struct Tun {
    fd: AsyncFd<OwnedFd>,
    name: String,
}

impl Tun {
    /// Create (or attach to) a non-persistent TUN interface named `name`.
    /// Needs CAP_NET_ADMIN. The interface disappears when the fd is closed.
    pub fn create(name: &str) -> Result<Self> {
        if name.is_empty() || name.len() >= libc::IFNAMSIZ {
            bail!("TUN name must be 1..{} chars", libc::IFNAMSIZ - 1);
        }
        // SAFETY: plain libc open of a well-known device path.
        let raw = unsafe {
            libc::open(c"/dev/net/tun".as_ptr(), libc::O_RDWR | libc::O_NONBLOCK | libc::O_CLOEXEC)
        };
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
        Ok(Self { fd, name: actual })
    }

    pub fn name(&self) -> &str {
        &self.name
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

/// Configure MTU and bring the link up with iproute2 (acceptable for Phase 0).
pub fn link_up(name: &str, mtu: u32) -> Result<()> {
    let status = std::process::Command::new("ip")
        .args(["link", "set", "dev", name, "mtu", &mtu.to_string(), "up"])
        .status()
        .context("run `ip link set`")?;
    if !status.success() {
        bail!("`ip link set dev {name} mtu {mtu} up` failed: {status}");
    }
    Ok(())
}

/// Spawn reader/writer tasks and return the bounded endpoints the session
/// driver consumes. Both tasks end when their channel counterpart closes or on
/// an I/O error (including a short write).
pub fn spawn_pumps(tun: Arc<Tun>, mtu: u32, stats: Arc<Stats>) -> TunEndpoints {
    let (to_tun_tx, mut to_tun_rx) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
    let (from_tun_tx, from_tun_rx) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);

    // Session -> TUN writer.
    let wtun = tun.clone();
    tokio::spawn(async move {
        while let Some(pkt) = to_tun_rx.recv().await {
            if let Err(e) = wtun.write(&pkt).await {
                warn!(error = %e, "TUN write failed; terminating packet path");
                break;
            }
        }
        debug!("TUN writer task ended");
    });

    // TUN reader -> session. Buffer larger than MTU so oversize packets are
    // detected and dropped rather than silently truncated.
    let rtun = tun;
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65536];
        loop {
            let n = match rtun.read(&mut buf).await {
                Ok(0) => {
                    warn!("TUN read returned 0 bytes; device gone");
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    warn!(error = %e, "TUN read failed");
                    break;
                }
            };
            let pkt = &buf[..n];
            if n > mtu as usize {
                stats.drop_tun_oversize.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                continue;
            }
            if ipv4::validate(pkt).is_err() {
                stats.drop_tun_invalid.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                continue;
            }
            // Bounded: suspends TUN reads (kernel queue absorbs/drops) when full.
            if from_tun_tx.send(pkt.to_vec()).await.is_err() {
                break;
            }
        }
        debug!("TUN reader task ended");
    });

    TunEndpoints { to_tun: to_tun_tx, from_tun: from_tun_rx }
}
