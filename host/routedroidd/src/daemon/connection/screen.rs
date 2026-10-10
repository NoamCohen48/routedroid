//! While the connection waits on the phone, whether its screen is what it
//! waits on: read every couple of seconds and put in the state, so the user
//! is told to unlock it instead of watching a wait time out.

use std::convert::Infallible;
use std::time::Duration;

use super::sink::StateSink;
use crate::adb::AdbDevice;

const EVERY: Duration = Duration::from_secs(2);

/// Runs until dropped; a phone that cannot be asked is left as it was.
pub async fn watch(adb: AdbDevice, sink: &StateSink) -> Infallible {
    let mut every = tokio::time::interval(EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        every.tick().await;
        if let Ok(screen) = adb.screen().await {
            sink.set_screen(screen);
        }
    }
}
