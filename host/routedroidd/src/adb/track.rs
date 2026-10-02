//! `adb track-devices -l`: the adb server pushes the whole device list
//! whenever it changes, each as a 4-hex-digit length and the list without
//! its header. One process for the daemon's life instead of a poll.

use std::process::Stdio;

use anyhow::Context;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, ChildStdout, Command};

use super::devices::{parse_list, Device};
use super::Adb;
use crate::fault::{Fault, FaultExt, Kind, Result};

/// adb's own limit on one message; a longer length is not adb talking.
const MAX_BLOCK: usize = 0xffff;

pub struct Tracker {
    /// Killed when the tracker is dropped.
    _child: Child,
    stdout: ChildStdout,
}

impl Adb {
    pub fn track(&self) -> Result<Tracker> {
        let mut child = Command::new(&self.binary)
            .args(["track-devices", "-l"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("spawn adb track-devices")
            .fault(Kind::Adb)?;
        let stdout = child.stdout.take().expect("stdout is piped");
        Ok(Tracker {
            _child: child,
            stdout,
        })
    }
}

impl Tracker {
    /// The next list; `None` when adb stopped sending (its server died).
    pub async fn next(&mut self) -> Result<Option<Vec<Device>>> {
        next_block(&mut self.stdout).await
    }
}

async fn next_block(reader: &mut (impl AsyncRead + Unpin)) -> Result<Option<Vec<Device>>> {
    let mut length = [0u8; 4];
    match reader.read_exact(&mut length).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(Fault::new(Kind::Adb, e)),
    }
    let length = std::str::from_utf8(&length)
        .ok()
        .and_then(|hex| usize::from_str_radix(hex, 16).ok())
        .filter(|length| *length <= MAX_BLOCK)
        .ok_or_else(|| Fault::msg(Kind::Adb, "adb track-devices sent a bad length"))?;
    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .await
        .context("adb track-devices ended mid-list")
        .fault(Kind::Adb)?;
    Ok(Some(parse_list(&String::from_utf8_lossy(&body))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::DeviceState;

    #[tokio::test]
    async fn blocks_are_whole_lists() {
        let first = "emulator-5554          device product:x model:sdk transport_id:11\n";
        let stream = format!("{:04x}{first}0000", first.len());
        let mut reader = stream.as_bytes();
        let list = next_block(&mut reader).await.unwrap().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].state, DeviceState::Device);
        assert_eq!(list[0].model.as_deref(), Some("sdk"));
        assert_eq!(next_block(&mut reader).await.unwrap(), Some(vec![]));
        assert_eq!(next_block(&mut reader).await.unwrap(), None);
        // A partial length is a stream that ended, not a list.
        assert_eq!(next_block(&mut "00".as_bytes()).await.unwrap(), None);
        assert!(next_block(&mut "00zzabc".as_bytes()).await.is_err());
        assert!(next_block(&mut "0009abc".as_bytes()).await.is_err());
    }
}
