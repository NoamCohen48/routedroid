use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::commands::setup::block::Block;

fn choice(lan_if: &str, dhcp: bool, blocks: &[&str]) -> Choice {
    Choice {
        lan_if: lan_if.into(),
        dhcp,
        blocks: blocks.iter().map(|b| b.parse::<Block>().unwrap()).collect(),
    }
}

/// The file the packages install: comments only, allowing nothing.
const SHIPPED: &str = include_str!("../../../../../routedroid-helper/helper.toml");

#[test]
fn the_shipped_policy_gets_its_first_interface() {
    let mut file = PolicyFile::parse(SHIPPED).unwrap();
    assert!(file.allow(&choice("eno1", true, &[])));
    let text = file.render();
    assert!(text.starts_with("# Which LAN interfaces may carry phones"));
    assert!(
        text.ends_with("\n[[interface]]\nname = \"eno1\"\ndhcp = true\n"),
        "{text}"
    );
}

#[test]
fn other_interfaces_are_kept_and_the_chosen_one_replaced() {
    let old = "[[interface]]\nname = \"eno1\"\ndhcp = true\n\n[[interface]]\nname = \"wlan0\"\nphone_addresses = [\"10.0.0.200/29\"]\n";
    let mut file = PolicyFile::parse(old).unwrap();
    assert!(
        !file.allow(&choice("eno1", true, &[])),
        "already so: nothing to write"
    );
    assert!(file.allow(&choice("eno1", false, &["192.168.1.200/29"])));
    let text = file.render();
    assert!(
        text.contains("name = \"eno1\"\nphone_addresses = [\"192.168.1.200/29\"]\ndhcp = false\n"),
        "{text}"
    );
    assert!(
        text.contains("name = \"wlan0\"\nphone_addresses = [\"10.0.0.200/29\"]\ndhcp = false\n"),
        "{text}"
    );
    let again = PolicyFile::parse(&text).unwrap();
    assert_eq!(
        again.interfaces, file.interfaces,
        "what is written reads back the same"
    );
}

#[test]
fn a_policy_with_unknown_keys_is_not_rewritten_blindly() {
    assert!(PolicyFile::parse("[[interface]]\nname = \"eno1\"\nallow_all = true\n").is_err());
}

/// What setup writes for two interfaces; routedroid-helper's policy tests
/// parse this same file with the helper's own rules.
const RENDERED: &str = include_str!("rendered.toml");

#[test]
fn what_is_written_is_the_sample_the_helper_parses() {
    let mut file = PolicyFile::default();
    file.allow(&choice("eno1", true, &[]));
    file.allow(&choice("wlan0", false, &["10.1.0.200/29"]));
    assert_eq!(file.render(), RENDERED);
}

#[test]
fn writing_keeps_a_backup_and_leaves_no_temp_file() {
    let dir = std::env::temp_dir().join(format!("routedroid-setup-{}", std::process::id()));
    let path = dir.join("helper.toml");
    write(&path, "new\n", None).unwrap();
    assert!(!dir.join("helper.toml.bak").exists(), "nothing to back up");
    write(&path, "newer\n", Some("new\n")).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "newer\n");
    assert_eq!(
        fs::read_to_string(dir.join("helper.toml.bak")).unwrap(),
        "new\n"
    );
    assert!(!dir.join("helper.toml.tmp").exists());
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
    assert_eq!(read(&dir.join("absent.toml")).unwrap(), None);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn applying_says_what_changed_and_only_once() {
    let dir = std::env::temp_dir().join(format!("routedroid-apply-{}", std::process::id()));
    let path = dir.join("helper.toml");
    let shown = path.display();
    let dhcp = choice("eno1", true, &[]);
    assert_eq!(
        apply(&path, &dhcp).unwrap().unwrap(),
        format!("let phones join through eno1 (DHCP) in {shown}")
    );
    assert_eq!(apply(&path, &dhcp).unwrap(), None, "already so");
    let fixed = choice("eno1", false, &["192.168.1.200/29"]);
    assert_eq!(
        apply(&path, &fixed).unwrap().unwrap(),
        format!(
            "let phones join through eno1 (192.168.1.200/29) in {shown} (the old one is {shown}.bak)"
        )
    );
    fs::write(&path, "nonsense = [").unwrap();
    let broken = apply(&path, &dhcp).unwrap_err();
    assert!(format!("{broken:#}").contains("cannot be read; fix it, or move it away"));
    fs::remove_dir_all(dir).unwrap();
}
