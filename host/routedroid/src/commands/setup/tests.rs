use clap::Parser;

use super::*;
use crate::cli::{Cli, Command};

fn parse(args: &[&str]) -> Result<SetupArgs, clap::Error> {
    let cli = Cli::try_parse_from([&["routedroid", "setup"], args].concat())?;
    match cli.command {
        Command::Setup(args) => Ok(args),
        other => panic!("parsed as {other:?}"),
    }
}

#[test]
fn flags_choose_without_asking() {
    let args = parse(&[
        "--lan-if",
        "eno1",
        "--phone-addresses",
        "192.168.1.200/29",
        "--phone-addresses",
        "192.168.1.240/30",
    ])
    .unwrap();
    assert_eq!(args.lan_if.as_deref(), Some("eno1"));
    let blocks: Vec<String> = args
        .phone_addresses
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(blocks, ["192.168.1.200/29", "192.168.1.240/30"]);
    assert!(!args.dhcp && !args.yes);
    assert_eq!(args.policy, PathBuf::from("/etc/routedroid/helper.toml"));
}

#[test]
fn a_bad_block_is_refused_with_why() {
    let error = parse(&["--phone-addresses", "192.168.1.201/29"])
        .unwrap_err()
        .to_string();
    assert!(error.contains("did you mean 192.168.1.200/29?"), "{error}");
}

#[test]
fn it_is_for_whoever_ran_sudo_unless_told() {
    let user = |flag: Option<&str>, sudo: Option<&str>| {
        system::target_user(flag.map(Into::into), sudo.map(Into::into)).map_err(|e| e.to_string())
    };
    assert_eq!(user(None, Some("noam")).unwrap(), "noam");
    assert_eq!(user(Some("ana"), Some("noam")).unwrap(), "ana");
    assert!(
        user(None, Some("root"))
            .unwrap_err()
            .contains("--user NAME")
    );
    assert!(user(None, None).unwrap_err().contains("--user NAME"));
    assert_eq!(
        user(Some("root"), None).unwrap(),
        "root",
        "asked for by name"
    );
}

#[test]
fn a_session_has_the_group_only_if_its_manager_does() {
    let status = "Name:\tsystemd\nUid:\t1000\t1000\t1000\t1000\nGroups:\t4 27 968 1000 \n";
    assert!(system::has_gid(status, "968"));
    assert!(!system::has_gid(status, "96"), "whole numbers only");
    assert!(!system::has_gid("Name:\tsystemd\n", "968"));
}

#[tokio::test]
async fn without_root_it_changes_nothing_and_says_how() {
    if system::is_root() {
        return;
    }
    let args = parse(&["--yes"]).unwrap();
    let error = setup(args).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "setup changes system files: run it with sudo: sudo routedroid setup"
    );
    assert!(error.downcast_ref::<Usage>().is_some(), "exit 2");
}
