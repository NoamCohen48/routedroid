use super::*;

fn current() -> (u32, String) {
    let out = std::process::Command::new("id")
        .arg("-gn")
        .output()
        .unwrap();
    let group = String::from_utf8(out.stdout).unwrap().trim().to_string();
    // SAFETY: getuid cannot fail.
    (unsafe { libc::getuid() }, group)
}

fn group(name: &str) -> Gate {
    Gate {
        uid: None,
        group: Some(name.into()),
    }
}

#[tokio::test]
async fn members_and_root_are_admitted() {
    let (uid, primary) = current();
    assert_eq!(check(&group(&primary), uid).await, Ok(()));
    assert_eq!(check(&group("no-such-group"), 0).await, Ok(()), "root");
    assert_eq!(check(&Gate::default(), uid).await, Ok(()), "no gate");
}

#[tokio::test]
async fn others_are_told_why_not() {
    let (uid, _) = current();
    assert_eq!(
        check(&group("no-such-group"), uid).await,
        Err(format!(
            "uid {uid} is not in group no-such-group; `sudo routedroid setup` adds it"
        ))
    );
    let unknown = 3_999_999_999;
    assert!(
        check(&group("root"), unknown).await.is_err(),
        "no such user"
    );
    let other = Gate {
        uid: Some(uid + 1),
        group: None,
    };
    assert_eq!(
        check(&other, uid).await,
        Err(format!("uid {uid} is not allowed"))
    );
}

#[test]
fn supplementary_groups_count() {
    // Root is in group root by its primary gid; the current user is in
    // every group `id -Gn` lists, from the database's member lists too.
    assert!(members::is_member(0, "root").unwrap());
    let (uid, _) = current();
    let out = std::process::Command::new("id")
        .arg("-Gn")
        .output()
        .unwrap();
    for name in String::from_utf8(out.stdout).unwrap().split_whitespace() {
        assert!(members::is_member(uid, name).unwrap(), "{name}");
    }
}
