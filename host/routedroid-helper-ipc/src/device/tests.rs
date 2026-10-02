use super::*;

#[test]
fn derived_from_the_serial_one_way() {
    let id = DeviceId::from_serial("R58M123ABC");
    assert_eq!(id, DeviceId::from_serial("R58M123ABC"), "stable");
    assert_ne!(id, DeviceId::from_serial("R58M123ABD"));
    let shown = id.to_string();
    assert_eq!(shown.len(), 16);
    assert!(!shown.contains("R58M"));
    // Pinned: a phone keeps its LAN identity across releases.
    assert_eq!(DeviceId::from_serial("emulator-5554").to_string(), PINNED);
}

/// SHA-256("routedroid device id v1\0emulator-5554"), first 8 bytes.
const PINNED: &str = "caf60be925035877";

#[test]
fn wire_form_is_strict_hex() {
    let id = DeviceId::from_serial("x");
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, format!("\"{id}\""));
    assert_eq!(serde_json::from_str::<DeviceId>(&json).unwrap(), id);
    for bad in [
        "\"\"",
        "\"0123\"",
        "\"0123456789ABCDEF\"",
        "\"0123456789abcdeg\"",
        "\"0123456789abcdef0\"",
    ] {
        assert!(serde_json::from_str::<DeviceId>(bad).is_err(), "{bad}");
    }
}
