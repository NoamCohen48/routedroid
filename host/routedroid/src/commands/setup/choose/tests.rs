use routedroid_helper_ipc::Net;

use super::super::ask::ask;
use super::*;

fn link(
    name: &str,
    address: Option<[u8; 4]>,
    default_route: bool,
    ineligible: Option<&str>,
) -> Interface {
    Interface {
        name: name.into(),
        up: true,
        addresses: address
            .map(|a| Net {
                address: a.into(),
                prefix: 24,
            })
            .into_iter()
            .collect(),
        default_route,
        phone_addresses: vec![],
        dhcp: false,
        ineligible: ineligible.map(Into::into),
    }
}

fn host() -> Vec<Interface> {
    vec![
        link("lo", Some([127, 0, 0, 1]), false, Some("loopback")),
        link(
            "eno1",
            Some([192, 168, 1, 10]),
            true,
            Some("not in the helper policy"),
        ),
        link(
            "wlan0",
            Some([10, 0, 0, 5]),
            false,
            Some("not in the helper policy"),
        ),
        link("eth9", None, false, Some("no IPv4 address")),
        link(
            "phone0",
            Some([192, 168, 1, 77]),
            false,
            Some("a phone's TUN"),
        ),
    ]
}

fn block(s: &str) -> Block {
    s.parse().unwrap()
}

#[test]
fn only_links_the_policy_holds_back_are_candidates() {
    let host = host();
    let names: Vec<&str> = candidates(&host).iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["eno1", "wlan0"]);
    let unreadable = [link(
        "eno1",
        Some([192, 168, 1, 10]),
        true,
        Some("the helper policy cannot be read: x"),
    )];
    assert_eq!(
        candidates(&unreadable).len(),
        1,
        "setup is how a missing policy gets written"
    );
}

#[test]
fn the_default_route_is_offered_first() {
    let host = host();
    assert_eq!(preferred(&candidates(&host)).unwrap().name, "eno1");
    let none = [
        link("a", Some([10, 0, 0, 1]), false, None),
        link("b", Some([10, 0, 1, 1]), false, None),
    ];
    assert!(
        preferred(&candidates(&none)).is_none(),
        "two without a default route: ask"
    );
}

#[test]
fn flags_are_checked_against_the_survey() {
    let host = host();
    let dhcp = from_flags(&host, "eno1", false, &[]).unwrap();
    assert_eq!(
        (dhcp.dhcp, dhcp.addresses()),
        (true, "DHCP".into()),
        "no block means DHCP"
    );
    let fixed = from_flags(&host, "eno1", false, &[block("192.168.1.200/29")]).unwrap();
    assert_eq!(fixed.addresses(), "192.168.1.200/29");
    let both = from_flags(&host, "eno1", true, &[block("192.168.1.200/29")]).unwrap();
    assert_eq!(both.addresses(), "DHCP, 192.168.1.200/29");

    let refused = |lan_if: &str, blocks: &[Block]| {
        from_flags(&host, lan_if, false, blocks)
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        refused("eth9", &[]),
        "phones cannot join through eth9: it has no IPv4 address"
    );
    assert_eq!(
        refused("lo", &[]),
        "phones cannot join through lo: loopback"
    );
    assert_eq!(
        refused("nope", &[]),
        "phones cannot join through nope: there is no such interface"
    );
    assert!(
        refused("eno1", &[block("10.0.0.0/29")]).starts_with("10.0.0.0/29 is not on eno1's LAN")
    );
}

#[test]
fn defaults_need_one_clear_choice() {
    assert_eq!(defaults(&host()).unwrap().lan_if, "eno1");
    let two = [
        link("a", Some([10, 0, 0, 1]), false, None),
        link("b", Some([10, 0, 1, 1]), false, None),
    ];
    assert!(
        defaults(&two)
            .unwrap_err()
            .to_string()
            .contains("choose with --lan-if")
    );
    let none = [link("eth9", None, false, Some("no IPv4 address"))];
    assert!(
        defaults(&none)
            .unwrap_err()
            .to_string()
            .contains("eth9: no IPv4 address")
    );
}

#[test]
fn blocks_parse_like_the_policy() {
    assert_eq!(block("192.168.1.200/29").to_string(), "192.168.1.200/29");
    let bad = |s: &str| s.parse::<Block>().unwrap_err();
    assert_eq!(
        bad("192.168.1.201/29"),
        "192.168.1.201/29 does not start a block: did you mean 192.168.1.200/29?"
    );
    assert!(bad("192.168.1.0").contains("not a block like"));
    assert!(bad("192.168.1.0/33").contains("not a prefix length"));
    let eno1 = &host()[1];
    assert!(block("192.168.1.200/29").inside(eno1));
    assert!(!block("192.168.0.0/16").inside(eno1), "wider than the LAN");
    assert_eq!(
        Block::example(eno1).unwrap().to_string(),
        "192.168.1.200/29"
    );
}

fn asked(answers: &str) -> (Result<Choice>, String) {
    let mut out = Vec::new();
    let choice = ask(&host(), &mut answers.as_bytes(), &mut out);
    (choice, String::from_utf8(out).unwrap())
}

#[test]
fn enter_takes_the_defaults() {
    let (choice, screen) = asked("\n\n");
    assert_eq!(
        choice.unwrap(),
        Choice {
            lan_if: "eno1".into(),
            dhcp: true,
            blocks: vec![]
        }
    );
    assert!(
        screen.contains("eno1         192.168.1.10/24  default route"),
        "{screen}"
    );
    assert!(screen.contains("Let phones join the LAN through eno1? [Y/n]"));
}

#[test]
fn another_interface_and_a_block_are_asked_until_right() {
    let (choice, screen) = asked("n\nwlan9\nwlan0\nn\n192.168.1.200/29\n10.0.0.200/29\n");
    assert_eq!(
        choice.unwrap(),
        Choice {
            lan_if: "wlan0".into(),
            dhcp: false,
            blocks: vec![block("10.0.0.200/29")]
        }
    );
    assert!(screen.contains("\"wlan9\" is not one of them"));
    assert!(screen.contains("192.168.1.200/29 is not on wlan0's LAN"));
}

#[test]
fn the_input_ending_is_an_answer_not_a_loop() {
    let (choice, _) = asked("maybe\n");
    assert_eq!(
        choice.unwrap_err().to_string(),
        "no answer: the input ended"
    );
}
