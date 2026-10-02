//! The one way the helper runs another program: an absolute path chosen from
//! a fixed list (never `$PATH`), an empty environment, stdin fed from memory,
//! and a deadline after which the child is killed.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

const POLL: Duration = Duration::from_millis(5);

/// The first of `candidates` that exists, as an absolute path.
pub fn locate(candidates: &[&str]) -> Result<PathBuf> {
    candidates
        .iter()
        .map(Path::new)
        .find(|path| path.is_absolute() && path.is_file())
        .map(Path::to_path_buf)
        .with_context(|| format!("none of {} exists", candidates.join(", ")))
}

/// Run `program` to completion within `timeout`; its stdout on success.
pub fn run(
    program: &Path,
    args: &[&str],
    stdin: Option<&str>,
    timeout: Duration,
) -> Result<String> {
    let shown = format!("{} {}", program.display(), args.join(" "));
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn {shown}"))?;
    // Drain both pipes on their own threads, so a chatty child cannot block
    // on a full pipe while we wait for it.
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(input.as_bytes())
            .with_context(|| format!("write stdin of {shown}"))?;
    }
    let status = wait(&mut child, timeout).with_context(|| format!("wait for {shown}"))?;
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if !status.success() {
        bail!("{shown} failed ({status}): {}", stderr.trim());
    }
    Ok(stdout)
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    })
}

fn wait(child: &mut Child, timeout: Duration) -> Result<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("timed out after {timeout:?}; killed");
        }
        thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Also holds the fork gate (see `test_util`) for the calling test.
    fn sh() -> (crate::test_util::Spawning, PathBuf) {
        (
            crate::test_util::spawning(),
            locate(&["/bin/sh", "/usr/bin/sh"]).unwrap(),
        )
    }

    #[test]
    fn output_input_and_environment() {
        let (_gate, sh) = sh();
        let out = run(
            &sh,
            &["-c", "read x; echo \"$x:${HOME-unset}\""],
            Some("hi\n"),
            Duration::from_secs(5),
        );
        assert_eq!(out.unwrap(), "hi:unset\n");
    }

    #[test]
    fn failure_carries_stderr() {
        let (_gate, sh) = sh();
        let err = run(
            &sh,
            &["-c", "echo boom >&2; exit 3"],
            None,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("boom"), "{err:#}");
    }

    #[test]
    fn a_hung_child_is_killed() {
        let started = Instant::now();
        let (_gate, sh) = sh();
        assert!(run(&sh, &["-c", "sleep 10"], None, Duration::from_millis(100)).is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn only_absolute_existing_paths() {
        assert!(locate(&["sh", "/nonexistent/sh"]).is_err());
    }
}
