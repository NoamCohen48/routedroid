use std::fs;
use std::sync::Arc;

use super::*;
use crate::claims::ENABLED;
use crate::op::{Leaf, SysctlKey};
use crate::session::Session;
use crate::test_util::Lab;

fn key(ifname: &str, leaf: Leaf) -> SysctlKey {
    SysctlKey::new(routedroid_helper_ipc::IfName::new(ifname).unwrap(), leaf)
}

fn start(lab: &Lab, id: u64, tun: &str, phone_ip: &str) -> Session<crate::kernel::fake::Fake> {
    Session::start(Arc::clone(&lab.env), lab.plan(id, tun, phone_ip).unwrap()).unwrap()
}

#[test]
fn a_crashed_session_is_undone_and_its_names_freed() {
    let lab = Lab::new();
    let baseline = lab.kernel.snapshot();
    start(&lab, 1, "phone0", "10.0.0.5").crash();
    assert!(check(&lab.env.journal_dir).is_err());

    cleanup(&lab.env).unwrap();
    assert_eq!(lab.kernel.snapshot(), baseline);
    check(&lab.env.journal_dir).unwrap();
    start(&lab, 2, "phone0", "10.0.0.5").stop().unwrap();
}

#[test]
fn a_crash_leaves_other_sessions_alone() {
    let lab = Lab::new();
    start(&lab, 1, "phone0", "10.0.0.5").crash();
    let live = start(&lab, 2, "phone1", "10.0.0.6");
    cleanup(&lab.env).unwrap();
    {
        let state = lab.kernel.lock();
        assert!(!state.tables.contains_key("routedroid_phone0"));
        assert!(
            state.tables.contains_key("routedroid_phone1") && state.links.contains_key("phone1")
        );
        assert_eq!(
            state.sysctls[&key("lan0", Leaf::ProxyArp)],
            ENABLED,
            "still claimed by the live session"
        );
    }
    check(&lab.env.journal_dir).unwrap();
    live.stop().unwrap();
    assert!(lab
        .kernel
        .lock()
        .sysctls
        .get(&key("lan0", Leaf::ProxyArp))
        .is_none_or(|v| v == "0"));
}

#[test]
fn a_failing_undo_is_retried_by_the_next_cleanup() {
    let lab = Lab::new();
    let baseline = lab.kernel.snapshot();
    start(&lab, 1, "phone0", "10.0.0.5").crash();
    lab.kernel.lock().failing.insert("nft_table");
    assert!(cleanup(&lab.env).is_err());
    assert_eq!(lab.journals().len(), 1);
    lab.kernel.lock().failing.clear();
    cleanup(&lab.env).unwrap();
    assert_eq!(lab.kernel.snapshot(), baseline);
}

#[test]
fn one_corrupt_journal_does_not_block_the_others() {
    let lab = Lab::new();
    let baseline = lab.kernel.snapshot();
    start(&lab, 1, "phone0", "10.0.0.5").crash();
    fs::write(
        lab.env.journal_dir.join("00000000000000ff.journal"),
        "not json\n",
    )
    .unwrap();
    assert!(cleanup(&lab.env).is_err(), "the corrupt one is reported");
    assert_eq!(
        lab.kernel.snapshot(),
        baseline,
        "the readable one is replayed"
    );
    assert_eq!(lab.journals().len(), 1);
}

#[test]
fn claims_without_any_journal_are_collected() {
    let lab = Lab::new();
    start(&lab, 1, "phone0", "10.0.0.5").crash();
    for journal in lab.journals() {
        fs::remove_file(journal).unwrap(); // lost by hand
    }
    cleanup(&lab.env).unwrap();
    let state = lab.kernel.lock();
    for leaf in [Leaf::Forwarding, Leaf::ProxyArp] {
        assert_eq!(state.sysctls[&key("lan0", leaf)], "0", "{leaf:?} restored");
    }
}
