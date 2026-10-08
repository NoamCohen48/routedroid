//! The user and group databases, through NSS as they are now: a user added
//! to a group a moment ago is a member, whatever their session holds.

use std::ffi::{CStr, CString};
use std::io;

/// Lookups give up on records larger than this.
const MAX_BUFFER: usize = 1 << 20;

/// Whether `uid` is in `group`, as its primary group or by the database's
/// member lists. A user or group the database does not know has no members.
pub fn is_member(uid: u32, group: &str) -> io::Result<bool> {
    let Some((name, primary)) = user(uid)? else {
        return Ok(false);
    };
    let Some(gid) = group_id(group)? else {
        return Ok(false);
    };
    Ok(primary == gid || groups(&name, primary)?.contains(&gid))
}

/// The user's name and primary gid.
fn user(uid: u32) -> io::Result<Option<(CString, u32)>> {
    lookup(|buf, found| {
        // SAFETY: an all-zero passwd is valid.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result = std::ptr::null_mut();
        // SAFETY: every pointer is live for the call; `buf.len()` is its size.
        let rc = unsafe {
            libc::getpwuid_r(
                uid,
                &mut entry,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        };
        if rc == 0 && !result.is_null() {
            // SAFETY: a found entry's name points into `buf`, still intact.
            let name = unsafe { CStr::from_ptr(entry.pw_name) }.to_owned();
            *found = Some((name, entry.pw_gid));
        }
        rc
    })
}

fn group_id(name: &str) -> io::Result<Option<u32>> {
    let Ok(name) = CString::new(name) else {
        return Ok(None);
    };
    lookup(|buf, found| {
        // SAFETY: an all-zero group is valid.
        let mut entry: libc::group = unsafe { std::mem::zeroed() };
        let mut result = std::ptr::null_mut();
        // SAFETY: as in `user`; only the gid is copied out.
        let rc = unsafe {
            libc::getgrnam_r(
                name.as_ptr(),
                &mut entry,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        };
        if rc == 0 && !result.is_null() {
            *found = Some(entry.gr_gid);
        }
        rc
    })
}

/// Runs a `get*_r` call, growing its buffer while it says `ERANGE`.
fn lookup<T>(
    mut call: impl FnMut(&mut [u8], &mut Option<T>) -> libc::c_int,
) -> io::Result<Option<T>> {
    let mut buf = vec![0u8; 4096];
    loop {
        let mut found = None;
        match call(&mut buf, &mut found) {
            0 => return Ok(found),
            // "Not found", as some NSS modules say it.
            libc::ENOENT | libc::ESRCH => return Ok(None),
            libc::ERANGE if buf.len() < MAX_BUFFER => buf.resize(buf.len() * 2, 0),
            rc => return Err(io::Error::from_raw_os_error(rc)),
        }
    }
}

/// Every group `name` is in, its primary one included.
fn groups(name: &CStr, primary: u32) -> io::Result<Vec<u32>> {
    let mut count: libc::c_int = 64;
    loop {
        let asked = usize::try_from(count).map_err(|_| io::Error::other("getgrouplist failed"))?;
        let mut gids: Vec<libc::gid_t> = vec![0; asked];
        // SAFETY: `gids` has room for `count` gids; on -1 glibc sets
        // `count` to the number it needs.
        let rc =
            unsafe { libc::getgrouplist(name.as_ptr(), primary, gids.as_mut_ptr(), &mut count) };
        let needed = usize::try_from(count).unwrap_or(0);
        if rc >= 0 {
            gids.truncate(needed);
            return Ok(gids);
        }
        if needed <= asked || needed > MAX_BUFFER {
            return Err(io::Error::other("getgrouplist failed"));
        }
    }
}
