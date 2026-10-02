//! Non-persistent `IFF_TUN | IFF_NO_PI` interfaces via `/dev/net/tun`. The
//! [`Device`] owns the fd, so the interface lives exactly as long as it (and
//! any duplicate the relay holds). `IFF_TUN_EXCL` makes creation fail with
//! EBUSY when the name exists, instead of attaching to someone else's TUN.
//!
//! Packet semantics (architecture §8.5): every write is one whole packet; a
//! short write ends the packet path; a call that moved zero bytes because of
//! EINTR/EAGAIN is retried.

use std::io;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};

use anyhow::{bail, Context, Result};
use routedroid_helper_ipc::IfName;
use rustix::fs::{Mode, OFlags};
use tokio::io::unix::AsyncFd;

const IFF_TUN: libc::c_short = 0x0001;
const IFF_NO_PI: libc::c_short = 0x1000;
const IFF_TUN_EXCL: libc::c_short = 0x8000_u16 as libc::c_short;
/// `_IOW('T', 202, int)`.
const TUNSETIFF: libc::c_ulong = 0x4004_54ca;

pub struct Device(OwnedFd);

impl Device {
    pub fn create(name: &IfName) -> Result<Self> {
        let flags = OFlags::RDWR | OFlags::NONBLOCK | OFlags::CLOEXEC;
        let fd =
            rustix::fs::open("/dev/net/tun", flags, Mode::empty()).context("open /dev/net/tun")?;
        // SAFETY: ifreq is a plain C struct; all-zero is a valid value.
        let mut request: libc::ifreq = unsafe { std::mem::zeroed() };
        // IfName guarantees 1..=15 ASCII bytes, so the name stays NUL-terminated.
        for (dst, src) in request.ifr_name.iter_mut().zip(name.as_str().bytes()) {
            *dst = src as libc::c_char;
        }
        request.ifr_ifru.ifru_flags = IFF_TUN | IFF_NO_PI | IFF_TUN_EXCL;
        // SAFETY: TUNSETIFF reads and writes the ifreq we own for the whole call.
        let rc = unsafe {
            libc::ioctl(
                fd.as_raw_fd(),
                TUNSETIFF as _,
                &mut request as *mut libc::ifreq,
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error()).with_context(|| format!("create TUN {name}"));
        }
        // SAFETY: the kernel NUL-terminates ifr_name.
        let actual = unsafe { std::ffi::CStr::from_ptr(request.ifr_name.as_ptr()) };
        if actual.to_bytes() != name.as_str().as_bytes() {
            bail!("kernel named TUN {name} {actual:?}");
        }
        Ok(Self(fd))
    }

    /// A tokio handle on a duplicate of the fd, for the relay.
    pub fn open_async(&self) -> io::Result<AsyncTun> {
        Ok(AsyncTun(AsyncFd::new(
            self.0.as_fd().try_clone_to_owned()?,
        )?))
    }
}

pub struct AsyncTun(AsyncFd<OwnedFd>);

impl AsyncTun {
    /// Read one packet. Zero bytes means the device is gone.
    pub async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut guard = self.0.readable().await?;
            match guard.try_io(|fd| Ok(rustix::io::read(fd.get_ref(), &mut *buf)?)) {
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }

    /// Write exactly one packet; a short write is an error, never completed.
    pub async fn write(&self, packet: &[u8]) -> io::Result<()> {
        loop {
            let mut guard = self.0.writable().await?;
            match guard.try_io(|fd| Ok(rustix::io::write(fd.get_ref(), packet)?)) {
                Ok(Ok(n)) if n == packet.len() => return Ok(()),
                Ok(Ok(n)) => {
                    return Err(io::Error::other(format!(
                        "short TUN write: {n} of {} bytes",
                        packet.len()
                    )))
                }
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        }
    }
}
