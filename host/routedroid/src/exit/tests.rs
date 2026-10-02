use std::collections::HashSet;

use super::*;

#[test]
fn every_kind_has_its_own_code() {
    let codes: HashSet<i32> = Kind::ALL.into_iter().map(for_kind).collect();
    assert_eq!(codes.len(), Kind::ALL.len());
    for reserved in [OK, DAEMON_UNREACHABLE, DAEMON_INCOMPATIBLE, ABANDONED] {
        assert!(!codes.contains(&reserved), "{reserved} is taken");
    }
}

#[test]
fn the_help_names_every_code() {
    let help = help();
    for kind in Kind::ALL {
        assert!(
            help.contains(&format!("{:<4} {kind}", for_kind(kind))),
            "{help}"
        );
    }
}

#[test]
fn a_daemon_error_exits_with_its_kind() {
    let error = anyhow::Error::from(DaemonError {
        kind: Kind::Helper,
        message: "no".into(),
    });
    assert_eq!(report(&error), 15);
    let clean = Outcome::Clean {
        reason: routedroid_ipc::EndReason::Stopped,
    };
    assert_eq!(for_outcome(&clean), OK);
    assert_eq!(for_outcome(&Outcome::failed(Kind::Vpn, "x")), 14);
}
