//! `adb -s SERIAL <command>`: the per-device half of the binding.

use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, bail};
use tokio::process::Command;

use super::Adb;
use crate::fault::{Fault, FaultExt, Kind, Result};

#[derive(Debug, Clone)]
pub struct AdbDevice {
    adb: Adb,
    serial: String,
}

impl AdbDevice {
    pub(super) fn new(adb: Adb, serial: &str) -> Self {
        Self {
            adb,
            serial: serial.to_string(),
        }
    }

    pub fn serial(&self) -> &str {
        &self.serial
    }

    /// Run `adb -s SERIAL <args>`; stdout on success, or an error carrying
    /// both streams. Never passes secrets: those go through [`Self::shell`]'s stdin.
    pub(super) async fn run(&self, args: &[&str]) -> Result<String> {
        Ok(self.run_with_stdin(args, None).await?.0)
    }

    /// [`Self::run`] for a command that may take longer than adb's usual timeout.
    pub(super) async fn run_for(&self, args: &[&str], timeout: Duration) -> Result<String> {
        Ok(self.exec(args, None, timeout).await?.0)
    }

    /// `adb -s SERIAL shell <args>` with optional bytes on stdin. Returns
    /// `(stdout, stderr)`: many shell tools exit 0 and print their errors.
    pub(super) async fn shell(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<(String, String)> {
        let mut full = vec!["shell"];
        full.extend_from_slice(args);
        self.run_with_stdin(&full, stdin).await
    }

    async fn run_with_stdin(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<(String, String)> {
        self.exec(args, stdin, self.adb.timeout).await
    }

    async fn exec(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<(String, String)> {
        let mut cmd = Command::new(&self.adb.binary);
        cmd.arg("-s")
            .arg(&self.serial)
            .args(args)
            .kill_on_drop(true);
        cmd.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let desc = format!("adb -s {} {}", self.serial, args.join(" "));
        let fut = async {
            let mut child = cmd
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .with_context(|| format!("spawn {desc}"))?;
            if let Some(bytes) = stdin {
                use tokio::io::AsyncWriteExt;
                let mut pipe = child.stdin.take().expect("piped stdin");
                pipe.write_all(bytes).await.context("write to adb stdin")?;
                drop(pipe);
            }
            let out = child
                .wait_with_output()
                .await
                .with_context(|| format!("wait {desc}"))?;
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            if !out.status.success() {
                bail!(
                    "{desc} failed ({}): {}{}",
                    out.status,
                    stdout.trim(),
                    stderr.trim()
                );
            }
            Ok::<_, anyhow::Error>((stdout, stderr))
        };
        match tokio::time::timeout(timeout, fut).await {
            Ok(r) => r.fault(Kind::Adb),
            Err(_) => Err(Fault::msg(
                Kind::Adb,
                format!("{desc} timed out after {timeout:?}"),
            )),
        }
    }
}
