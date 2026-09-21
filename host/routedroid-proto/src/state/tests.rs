use super::*;
use crate::fixtures::STATES;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn allowlist_matches_fixture_exactly() {
    let f: serde_json::Value = serde_json::from_str(STATES).unwrap();
    let states: Vec<&str> = f["states"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    assert_eq!(states, State::ALL.map(State::name).to_vec());
    for (role, key) in [(Role::Host, "host_receives"), (Role::Android, "android_receives")] {
        let table: BTreeMap<String, Vec<String>> = serde_json::from_value(f[key].clone()).unwrap();
        for st in State::ALL {
            let want: BTreeSet<&str> = table[st.name()].iter().map(String::as_str).collect();
            let got: BTreeSet<&str> = allowed(role, st).iter().map(|m| m.name()).collect();
            assert_eq!(got, want, "{role:?} in {}", st.name());
        }
    }
    let types: BTreeMap<String, u8> = serde_json::from_value(f["types"].clone()).unwrap();
    for t in MessageType::ALL {
        assert_eq!(types[t.name()], t as u8);
    }
    assert_eq!(types.len(), MessageType::ALL.len());
}

#[test]
fn nothing_is_allowed_when_closed_and_packets_only_when_active() {
    for role in [Role::Host, Role::Android] {
        assert!(allowed(role, State::Closed).is_empty());
        for st in State::ALL {
            assert_eq!(is_allowed(role, st, MessageType::IpPacket), st == State::Active, "{role:?} {st:?}");
            assert_eq!(is_allowed(role, st, MessageType::Ping), st == State::Active);
        }
    }
}
