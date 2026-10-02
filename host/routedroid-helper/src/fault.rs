//! Crash injection for the kill matrix (`integration-tests/helper`): with
//! `--features testing`, a stage name written to the crash file makes the
//! helper SIGKILL itself when it reaches that stage. Without the feature the
//! hook compiles to nothing, so an installed helper has no kill switch.

#[cfg(feature = "testing")]
use std::path::PathBuf;

#[cfg(feature = "testing")]
pub const DEFAULT_CRASH_FILE: &str = "/run/routedroid/crash-at";

#[cfg(feature = "testing")]
pub struct CrashHook(pub PathBuf);

#[cfg(not(feature = "testing"))]
pub struct CrashHook;

impl CrashHook {
    #[cfg(feature = "testing")]
    pub fn at(&self, stage: &str) {
        let Ok(armed) = std::fs::read_to_string(&self.0) else {
            return;
        };
        if armed.trim() != stage {
            return;
        }
        // Consume the hook first: exactly one process dies per injected
        // stage, so the cleanup that systemd runs next is not hit as well.
        let _ = std::fs::remove_file(&self.0);
        tracing::warn!(stage, "crash hook: SIGKILL self");
        // SAFETY: plain signal to our own pid.
        unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
    }

    #[cfg(not(feature = "testing"))]
    pub fn at(&self, _stage: &str) {}

    /// A hook that never fires, whatever the build.
    #[cfg(test)]
    pub fn disabled() -> Self {
        #[cfg(feature = "testing")]
        return CrashHook(PathBuf::new());
        #[cfg(not(feature = "testing"))]
        return CrashHook;
    }
}
