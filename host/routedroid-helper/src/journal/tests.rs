use std::fs;

use routedroid_helper_ipc::IfName;

use super::*;
use crate::op::Leaf;
use crate::test_util::Scratch;

fn reservation(tun: &str, ip: &str) -> Reservation {
    Reservation {
        tun: IfName::new(tun).unwrap(),
        phone_ip: ip.parse().unwrap(),
    }
}

fn op(n: u8) -> Op {
    Op::Sysctl {
        ifname: IfName::new(format!("lan{n}")).unwrap(),
        leaf: Leaf::Forwarding,
    }
}

fn orphan(path: &Path) -> Journal {
    match Journal::take(path).unwrap() {
        Taken::Orphan(journal) => journal,
        Taken::Live => panic!("live"),
        Taken::Gone => panic!("gone"),
    }
}

fn open_steps(journal: &Journal) -> Vec<(u32, Phase)> {
    journal
        .outstanding()
        .into_iter()
        .map(|(seq, step)| (seq, step.phase))
        .collect()
}

#[test]
fn records_replay_after_a_crash() {
    let scratch = Scratch::new();
    let mut j = Journal::create(
        scratch.path(),
        SessionId::from_raw(1),
        reservation("phone0", "10.0.0.5"),
    )
    .unwrap();
    let a = j.intend(op(1)).unwrap();
    j.advance(a, Phase::Done).unwrap();
    let b = j.intend(op(2)).unwrap();
    j.advance(b, Phase::Done).unwrap();
    let c = j.intend(op(3)).unwrap(); // crashed after intent
    j.advance(a, Phase::UndoPending).unwrap();
    j.advance(a, Phase::Undone).unwrap();
    let path = j.path().to_owned();

    assert!(
        matches!(Journal::take(&path).unwrap(), Taken::Live),
        "the writer's lock marks it live"
    );
    drop(j); // the crash
    let j = orphan(&path);
    assert_eq!(j.session(), SessionId::from_raw(1));
    assert_eq!(open_steps(&j), vec![(c, Phase::Pending), (b, Phase::Done)]);
}

#[test]
fn illegal_transitions_are_refused_before_writing() {
    let scratch = Scratch::new();
    let mut j = Journal::create(
        scratch.path(),
        SessionId::from_raw(1),
        reservation("phone0", "10.0.0.5"),
    )
    .unwrap();
    let a = j.intend(op(1)).unwrap();
    assert!(j.advance(a, Phase::Undone).is_err());
    assert!(j.advance(a + 1, Phase::Done).is_err());
    j.advance(a, Phase::Done).unwrap();
    assert!(j.advance(a, Phase::Done).is_err());
    let path = j.path().to_owned();
    drop(j);
    assert_eq!(open_steps(&orphan(&path)), vec![(a, Phase::Done)]);
}

#[test]
fn reservations_are_exclusive_until_resolved() {
    let scratch = Scratch::new();
    let dir = scratch.path();
    let j = Journal::create(
        dir,
        SessionId::from_raw(1),
        reservation("phone0", "10.0.0.5"),
    )
    .unwrap();
    assert!(Journal::create(
        dir,
        SessionId::from_raw(2),
        reservation("phone0", "10.0.0.6")
    )
    .is_err());
    assert!(Journal::create(
        dir,
        SessionId::from_raw(3),
        reservation("phone1", "10.0.0.5")
    )
    .is_err());
    let other = Journal::create(
        dir,
        SessionId::from_raw(4),
        reservation("phone1", "10.0.0.6"),
    )
    .unwrap();
    assert!(
        Journal::create(
            dir,
            SessionId::from_raw(4),
            reservation("phone2", "10.0.0.7")
        )
        .is_err(),
        "same id"
    );
    drop(j); // orphaned journals still reserve
    assert!(Journal::create(
        dir,
        SessionId::from_raw(5),
        reservation("phone0", "10.0.0.8")
    )
    .is_err());
    orphan(&dir.join("0000000000000001.journal"))
        .resolve()
        .unwrap();
    Journal::create(
        dir,
        SessionId::from_raw(5),
        reservation("phone0", "10.0.0.8"),
    )
    .unwrap();
    drop(other);
}

#[test]
fn an_unreadable_journal_blocks_reservations_but_not_other_recovery() {
    let scratch = Scratch::new();
    let dir = scratch.path();
    drop(
        Journal::create(
            dir,
            SessionId::from_raw(1),
            reservation("phone0", "10.0.0.5"),
        )
        .unwrap(),
    );
    fs::write(dir.join("0000000000000002.journal"), "garbage\n").unwrap();
    fs::write(dir.join("0000000000000003.journal.tmp"), "").unwrap();
    assert!(Journal::create(
        dir,
        SessionId::from_raw(4),
        reservation("phone9", "10.0.0.9")
    )
    .is_err());

    let taken = take_all(dir).unwrap();
    assert_eq!(taken.len(), 2);
    assert!(matches!(taken[0].1, Ok(Taken::Orphan(_))));
    assert!(taken[1].1.is_err());
    assert!(
        !dir.join("0000000000000003.journal.tmp").exists(),
        "never-created journals are removed"
    );
}

#[test]
fn a_torn_tail_is_dropped_and_garbage_elsewhere_fails_closed() {
    let scratch = Scratch::new();
    let mut j = Journal::create(
        scratch.path(),
        SessionId::from_raw(1),
        reservation("phone0", "10.0.0.5"),
    )
    .unwrap();
    let a = j.intend(op(1)).unwrap();
    let path = j.path().to_owned();
    drop(j);
    let intact = fs::read(&path).unwrap();

    fs::write(&path, [intact.as_slice(), b"{\"seq\":1,\"pha"].concat()).unwrap();
    let mut j = orphan(&path);
    assert_eq!(
        fs::read(&path).unwrap(),
        intact,
        "the torn tail is truncated away"
    );
    j.advance(a, Phase::UndoPending).unwrap();
    drop(j);
    assert_eq!(open_steps(&orphan(&path)), vec![(a, Phase::UndoPending)]);

    for bad in [
        &b"{\"seq\":1,\"pha\n"[..],
        b"{\"seq\":9,\"phase\":\"done\"}\n",
    ] {
        fs::write(&path, [intact.as_slice(), bad].concat()).unwrap();
        assert!(
            Journal::take(&path).is_err(),
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
    fs::write(
        &path,
        String::from_utf8(intact)
            .unwrap()
            .replace("\"version\":1", "\"version\":2"),
    )
    .unwrap();
    assert!(format!("{:#}", Journal::take(&path).err().unwrap()).contains("journal format 2"));
}

#[test]
fn resolution_requires_every_step_undone() {
    let scratch = Scratch::new();
    let mut j = Journal::create(
        scratch.path(),
        SessionId::from_raw(1),
        reservation("phone0", "10.0.0.5"),
    )
    .unwrap();
    let a = j.intend(op(1)).unwrap();
    let path = j.path().to_owned();
    j.advance(a, Phase::UndoPending).unwrap();
    let j = match j.resolve() {
        Err(_) => orphan(&path),
        Ok(()) => panic!("resolved with a step outstanding"),
    };
    let mut j = j;
    j.advance(a, Phase::Undone).unwrap();
    j.resolve().unwrap();
    assert!(!path.exists());
    assert!(sessions(scratch.path()).unwrap().is_empty());
}

#[test]
fn journals_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let dir = scratch.path().join("journal");
    let j = Journal::create(
        &dir,
        SessionId::from_raw(1),
        reservation("phone0", "10.0.0.5"),
    )
    .unwrap();
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(j.path()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        sessions(&dir).unwrap().into_iter().collect::<Vec<_>>(),
        vec![SessionId::from_raw(1)]
    );
}
