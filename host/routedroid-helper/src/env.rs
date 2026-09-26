//! What every session and every recovery run shares: the kernel, where state
//! lives, the operator's policy file, and the crash hook.

use std::path::{Path, PathBuf};

use crate::claims::Claims;
use crate::fault::CrashHook;

pub const DEFAULT_STATE_DIR: &str = "/var/lib/routedroid";

pub struct Env<K> {
    pub kernel: K,
    pub journal_dir: PathBuf,
    pub claims: Claims,
    pub policy: PathBuf,
    pub hook: CrashHook,
}

impl<K> Env<K> {
    /// Journals in `<state>/journal`, sysctl claims in `<state>/sysctl`.
    pub fn new(kernel: K, state_dir: &Path, policy: PathBuf, hook: CrashHook) -> Self {
        Self {
            kernel,
            journal_dir: state_dir.join("journal"),
            claims: Claims::new(state_dir.join("sysctl")),
            policy,
            hook,
        }
    }
}
