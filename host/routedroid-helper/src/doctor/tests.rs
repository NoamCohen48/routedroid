use std::sync::Arc;

use super::*;
use crate::kernel::{ForwardDrop, HostFirewall, Rule};
use crate::session::Session;
use crate::test_util::Lab;

fn crash(lab: &Lab, phone_ip: &str) {
    let plan = lab.plan(1, "phone0", phone_ip).unwrap();
    Session::start(Arc::clone(&lab.env), plan).unwrap().crash();
}

fn subjects(findings: &[Finding]) -> Vec<&str> {
    findings.iter().map(|f| f.subject.as_str()).collect()
}

#[test]
fn a_clean_host_has_nothing_to_report() {
    let lab = Lab::new();
    assert!(inspect(&lab.env).unwrap().is_empty());
    let (done, remaining) = repair(&lab.env).unwrap();
    assert!(done.is_empty() && remaining.is_empty());
}

#[test]
fn an_orphaned_journal_is_reported_with_its_undo_and_repaired() {
    let lab = Lab::new();
    let baseline = lab.kernel.snapshot();
    crash(&lab, "10.0.0.5");
    let found = inspect(&lab.env).unwrap();
    assert_eq!(subjects(&found), ["session 0000000000000001"]);
    assert_eq!(found[0].repair[0], "undo route:10.0.0.5/32@phone0");
    assert_eq!(found[0].repair.len(), 7, "{:?}", found[0].repair);
    assert_eq!(
        inspect(&lab.env).unwrap(),
        found,
        "inspecting changes nothing"
    );

    let (done, remaining) = repair(&lab.env).unwrap();
    assert_eq!(done, found[0].repair);
    assert!(remaining.is_empty(), "{remaining:?}");
    assert_eq!(lab.kernel.snapshot(), baseline);
}

#[test]
fn objects_whose_journal_was_deleted_are_leftovers() {
    let lab = Lab::new();
    let baseline = lab.kernel.snapshot();
    crash(&lab, "10.0.0.5");
    for journal in lab.journals() {
        std::fs::remove_file(journal).unwrap();
    }
    let found = inspect(&lab.env).unwrap();
    assert_eq!(
        subjects(&found),
        [
            "nft table inet routedroid_phone0",
            "egress of 10.0.0.5",
            "sysctl net.ipv4.conf.lan0.forwarding",
            "sysctl net.ipv4.conf.lan0.proxy_arp",
            "sysctl net.ipv4.conf.phone0.forwarding",
        ]
    );
    let (done, remaining) = repair(&lab.env).unwrap();
    assert_eq!(done.len(), 5, "{done:?}");
    assert!(remaining.is_empty(), "{remaining:?}");
    assert_eq!(lab.kernel.snapshot(), baseline);
}

#[test]
fn foreign_objects_are_never_leftovers_and_the_firewall_needs_a_person() {
    let lab = Lab::new();
    {
        let mut state = lab.kernel.lock();
        state.add_link("phone7", Some("someone else's"));
        state.rules.push(Rule {
            priority: 1082,
            table: 77,
            src: Some(("10.0.0.9".parse().unwrap(), 32)),
            protocol: 0,
        });
        state.forward_drops.push(ForwardDrop {
            family: "inet".into(),
            table: "firewalld".into(),
            chain: "filter_FORWARD".into(),
            firewall: HostFirewall::Firewalld,
        });
    }
    let found = inspect(&lab.env).unwrap();
    assert_eq!(
        subjects(&found),
        ["nft inet firewalld chain filter_FORWARD"]
    );
    assert!(found[0].repair.is_empty() && found[0].warning);
    assert!(found[0].problem.contains("firewalld zone that forwards"));
    let (done, remaining) = repair(&lab.env).unwrap();
    assert!(done.is_empty());
    assert_eq!(remaining, found);
    assert_eq!(lab.kernel.lock().rules.len(), 1);
    // A warning alone is no failure: scripts (uninstall) go on.
    run(&lab.env, false).unwrap();
    run(&lab.env, true).unwrap();
}
