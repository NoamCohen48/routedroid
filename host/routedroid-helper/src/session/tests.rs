use super::*;
use crate::claims::ENABLED;
use crate::kernel::fake::Fake;
use crate::op::{Leaf, SysctlKey};
use crate::test_util::Lab;

fn key(ifname: &str, leaf: Leaf) -> SysctlKey {
    SysctlKey::new(routedroid_helper_ipc::IfName::new(ifname).unwrap(), leaf)
}

fn start(lab: &Lab, id: u64, tun: &str, phone_ip: &str) -> Result<Session<Fake>> {
    Session::start(Arc::clone(&lab.env), lab.plan(id, tun, phone_ip)?)
}

#[test]
fn start_applies_tagged_state_and_stop_restores_the_baseline() {
    let lab = Lab::new();
    let baseline = lab.kernel.snapshot();
    let session = start(&lab, 1, "phone0", "10.0.0.5").unwrap();
    {
        let state = lab.kernel.lock();
        let tun = &state.links["phone0"];
        assert_eq!(tun.alias.as_deref(), Some("routedroid:0000000000000001"));
        assert_eq!(
            state.tables["routedroid_phone0"].0.comment.as_deref(),
            Some("routedroid:0000000000000001")
        );
        assert!(state
            .routes
            .iter()
            .any(|r| r.dst.to_string() == "10.0.0.5" && r.oif == Some(tun.index)));
        assert_eq!(state.sysctls[&key("lan0", Leaf::ProxyArp)], ENABLED);
        assert_eq!(state.sysctls[&key("phone0", Leaf::Forwarding)], ENABLED);
    }
    assert_eq!(lab.journals().len(), 1);
    assert!(session.tun().is_some());

    session.stop().unwrap();
    assert_eq!(lab.kernel.snapshot(), baseline);
    assert!(lab.journals().is_empty());
}

#[test]
fn a_failure_at_any_step_rolls_everything_back() {
    for failing in ["create_tun", "create_firewall", "sysctl_write", "add_route"] {
        let lab = Lab::new();
        let baseline = lab.kernel.snapshot();
        lab.kernel.lock().failing.insert(failing);
        let error = start(&lab, 1, "phone0", "10.0.0.5").err().unwrap();
        assert!(
            format!("{error:#}").contains(failing),
            "{failing}: {error:#}"
        );
        assert_eq!(lab.kernel.snapshot(), baseline, "{failing}");
        assert!(
            lab.journals().is_empty(),
            "{failing}: rollback succeeded, so the journal resolves"
        );
    }
}

#[test]
fn a_failed_rollback_keeps_the_journal() {
    let lab = Lab::new();
    {
        let mut state = lab.kernel.lock();
        state.failing.insert("add_route");
        state.failing.insert("delete_nft_table");
    }
    let error = start(&lab, 1, "phone0", "10.0.0.5").err().unwrap();
    assert!(
        format!("{error:#}").contains("rollback failed too"),
        "{error:#}"
    );
    assert_eq!(lab.journals().len(), 1);
    // Undo stopped at the firewall; the table stays, and the journal says so.
    assert!(lab.kernel.lock().tables.contains_key("routedroid_phone0"));
}

#[test]
fn dropping_a_session_undoes_it() {
    let lab = Lab::new();
    let baseline = lab.kernel.snapshot();
    drop(start(&lab, 1, "phone0", "10.0.0.5").unwrap());
    assert_eq!(lab.kernel.snapshot(), baseline);
    assert!(lab.journals().is_empty());
}

#[test]
fn names_and_addresses_are_reserved_while_a_journal_exists() {
    let lab = Lab::new();
    start(&lab, 1, "phone0", "10.0.0.5").unwrap().crash();
    // The crashed session's TUN is gone, but its journal still reserves both.
    let tun = start(&lab, 2, "phone0", "10.0.0.6").err().unwrap();
    assert!(
        format!("{tun:#}").contains("held by session 0000000000000001"),
        "{tun:#}"
    );
    let ip = start(&lab, 3, "phone1", "10.0.0.5").err().unwrap();
    assert!(
        format!("{ip:#}").contains("held by session 0000000000000001"),
        "{ip:#}"
    );
}

#[test]
fn foreign_objects_with_our_names_are_never_touched() {
    let lab = Lab::new();
    let foreign = crate::kernel::Firewall {
        tun: routedroid_helper_ipc::IfName::new("phone0").unwrap(),
        lan_if: routedroid_helper_ipc::IfName::new("lan0").unwrap(),
        phone_ip: "10.0.0.99".parse().unwrap(),
        host_ip: "10.0.0.2".parse().unwrap(),
        tag: "someone-else".into(),
    };
    lab.kernel.create_firewall(&foreign).unwrap();
    let baseline = lab.kernel.snapshot();
    let error = start(&lab, 1, "phone0", "10.0.0.5").err().unwrap();
    assert!(
        format!("{error:#}").contains("EEXIST"),
        "never merged into: {error:#}"
    );
    assert_eq!(
        lab.kernel.snapshot(),
        baseline,
        "and never deleted by the rollback"
    );
}
