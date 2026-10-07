//! Whether the phone's screen keeps it from going on: off, or showing the
//! lock screen. `dumpsys` says so in words that differ across Android
//! versions: `mWakefulness=Asleep` (or `Dozing`) in `power`, and
//! `mKeyguardShowing=true` (Android 10) or `KeyguardShowing=true` (14) in
//! `activity activities`.

use routedroid_ipc::Screen;

use super::super::AdbDevice;
use crate::fault::Result;

impl AdbDevice {
    /// `None`: on and unlocked, or the phone does not say.
    pub async fn screen(&self) -> Result<Option<Screen>> {
        let (power, _) = self.shell(&["dumpsys", "power"], None).await?;
        if asleep(&power) {
            return Ok(Some(Screen::Off));
        }
        let (activities, _) = self
            .shell(&["dumpsys", "activity", "activities"], None)
            .await?;
        Ok(keyguard(&activities).then_some(Screen::Locked))
    }
}

fn asleep(power: &str) -> bool {
    power
        .lines()
        .map(str::trim)
        .any(|line| matches!(line, "mWakefulness=Asleep" | "mWakefulness=Dozing"))
}

fn keyguard(activities: &str) -> bool {
    activities.split_whitespace().any(|word| {
        matches!(
            word,
            "mKeyguardShowing=true" | "KeyguardShowing=true" | "isKeyguardShowing=true"
        )
    })
}

#[cfg(test)]
mod tests;
