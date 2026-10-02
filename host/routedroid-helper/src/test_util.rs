//! Shared test scaffolding.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use routedroid_helper_ipc::IfName;

use crate::env::Env;
use crate::fault::CrashHook;
use crate::kernel::fake::Fake;
use crate::kernel::Address;
use crate::plan::{Facts, Plan, Request};
use crate::policy::Policy;
use crate::session_id::SessionId;

/// Keeps tests that spawn processes apart from tests that hold `flock`s.
/// A forked child shares its parent's open file descriptions, and so their
/// locks, until it execs: a journal dropped and re-taken during that window
/// reads as live. Reader-preferring, so a test may hold several `Scratch`es.
struct ForkGate {
    /// Live `Scratch`es; `None` while a process-spawning test runs.
    readers: Mutex<Option<usize>>,
    changed: Condvar,
}

static GATE: ForkGate = ForkGate {
    readers: Mutex::new(Some(0)),
    changed: Condvar::new(),
};

pub struct Spawning(());

/// Held by a test for as long as it spawns processes.
pub fn spawning() -> Spawning {
    let mut readers = GATE
        .changed
        .wait_while(GATE.readers.lock().unwrap(), |r| *r != Some(0))
        .unwrap();
    *readers = None;
    Spawning(())
}

impl Drop for Spawning {
    fn drop(&mut self) {
        *GATE.readers.lock().unwrap() = Some(0);
        GATE.changed.notify_all();
    }
}

fn enter() {
    let mut readers = GATE
        .changed
        .wait_while(GATE.readers.lock().unwrap(), |r| r.is_none())
        .unwrap();
    *readers = readers.map(|n| n + 1);
}

fn leave() {
    let mut readers = GATE.readers.lock().unwrap();
    *readers = readers.map(|n| n - 1);
    GATE.changed.notify_all();
}

/// A fresh directory under the system temp dir, removed on drop.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        enter();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("rd-helper-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        leave();
    }
}

/// A fake host: `lan0` (10.0.0.2/24) under a policy allowing 10.0.0.0/24,
/// with private state and policy files.
pub struct Lab {
    pub kernel: Fake,
    pub env: Arc<Env<Fake>>,
    /// Last, so it outlives the sessions that write into it.
    _scratch: Scratch,
}

impl Lab {
    pub fn new() -> Self {
        let scratch = Scratch::new();
        let kernel = Fake::default();
        {
            let mut state = kernel.lock();
            let lan = state.add_link("lan0", None);
            state.addresses.push(Address {
                index: lan,
                addr: "10.0.0.2".parse().unwrap(),
                prefix: 24,
            });
        }
        let policy = scratch.path().join("helper.toml");
        std::fs::write(
            &policy,
            "[[interface]]\nname = \"lan0\"\nphone_addresses = [\"10.0.0.0/24\"]\n",
        )
        .unwrap();
        let env = Env::new(
            kernel.clone(),
            &scratch.path().join("state"),
            policy,
            CrashHook::disabled(),
        );
        Self {
            kernel,
            env: Arc::new(env),
            _scratch: scratch,
        }
    }

    pub fn plan(&self, session: u64, tun: &str, phone_ip: &str) -> anyhow::Result<Plan> {
        let request = Request {
            lan_if: IfName::new("lan0").unwrap(),
            phone_ip: phone_ip.parse().unwrap(),
            tun: IfName::new(tun).unwrap(),
            mtu: 1400,
        };
        let facts = Facts::gather(&self.kernel, &request.lan_if, &request.tun)?;
        let policy = Policy::load(&self.env.policy)?;
        Plan::build(SessionId::from_raw(session), request, &policy, &facts)
    }

    pub fn journals(&self) -> Vec<PathBuf> {
        let dir = &self.env.journal_dir;
        let mut out: Vec<_> = std::fs::read_dir(dir)
            .map(|d| d.map(|e| e.unwrap().path()).collect())
            .unwrap_or_default();
        out.retain(|p| p.extension().is_some_and(|e| e == "journal"));
        out
    }
}
