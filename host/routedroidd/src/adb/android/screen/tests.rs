//! Lines as real phones print them: a Samsung on Android 10 at its lock
//! screen, and an Android 14 emulator.

use super::{asleep, keyguard};

#[test]
fn asleep_is_off_awake_is_on() {
    assert!(asleep("POWER MANAGER\n  mWakefulness=Asleep\n"));
    assert!(asleep("  mWakefulness=Dozing\n"));
    assert!(!asleep(
        "  mWakefulness=Awake\n  mWakefulnessChanging=false\n"
    ));
    assert!(!asleep(""));
}

#[test]
fn the_lock_screen_in_both_dialects() {
    let samsung = "  isHomeRecentsComponent=true  KeyguardController:\n    \
        mKeyguardShowing=true\n    mKeyguardGoingAway=false\n";
    assert!(keyguard(samsung));
    let android14 = "    isKeyguardShowing=false\n    mKeyguardShowing=false\n   \
        KeyguardShowing=false AodShowing=false KeyguardGoingAway=false\n";
    assert!(!keyguard(android14));
    assert!(keyguard(
        &android14.replace("   KeyguardShowing=false", "   KeyguardShowing=true")
    ));
    assert!(!keyguard("mKeyguardGoingAway=true"));
}
