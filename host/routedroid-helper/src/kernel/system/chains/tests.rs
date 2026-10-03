use super::*;

fn chain(family: &str, table: &str, name: &str, hook: &str, policy: &str) -> String {
    format!(
        r#"{{"chain": {{"family": "{family}", "table": "{table}", "name": "{name}", "handle": 1,
            "type": "filter", "hook": "{hook}", "prio": 0, "policy": "{policy}"}}}}"#
    )
}

/// A rule as `nft -j` lists `ufw route allow in on phone+` (iptables-nft).
fn accept(family: &str, table: &str, key: &str) -> String {
    format!(
        r#"{{"rule": {{"family": "{family}", "table": "{table}", "chain": "ufw-user-forward",
            "handle": 161, "expr": [{{"match": {{"op": "==", "left": {{"meta": {{"key": "{key}"}}}},
            "right": "phone*"}}}}, {{"counter": {{"packets": 35, "bytes": 4689}}}}, {{"accept": null}}]}}}}"#
    )
}

fn ruleset(items: &[String]) -> String {
    format!(
        r#"{{"nftables": [{{"metainfo": {{"version": "1.0.9"}}}}, {}]}}"#,
        items.join(", ")
    )
}

fn ufw() -> Vec<String> {
    vec![
        chain("ip", "filter", "FORWARD", "forward", "drop"),
        r#"{"chain": {"family": "ip", "table": "filter", "name": "ufw-user-forward", "handle": 9}}"#
            .into(),
        chain("ip6", "filter", "FORWARD", "forward", "drop"),
    ]
}

#[test]
fn ufw_gets_ufw_advice_and_ip6_is_never_the_phones_problem() {
    let found = parse(&ruleset(&ufw())).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].subject(), "nft ip filter chain FORWARD");
    assert_eq!(found[0].firewall, HostFirewall::Ufw);
    assert!(
        found[0]
            .advice()
            .starts_with("sudo ufw route allow in on phone+")
    );
}

#[test]
fn a_table_that_lets_phones_through_both_ways_is_fine() {
    let mut items = ufw();
    items.push(accept("ip", "filter", "iifname"));
    assert_eq!(
        parse(&ruleset(&items)).unwrap().len(),
        1,
        "one way is not enough"
    );
    items.push(accept("ip", "filter", "oifname"));
    assert!(parse(&ruleset(&items)).unwrap().is_empty());
}

#[test]
fn each_manager_is_named_and_ours_and_accepting_chains_are_not_reported() {
    let items = [
        chain("inet", "firewalld", "filter_FORWARD", "forward", "drop"),
        chain("ip", "filter", "FORWARD", "forward", "drop"),
        chain("inet", "filter", "forward", "forward", "drop"),
        chain("inet", "routedroid_phone0", "forward", "forward", "drop"),
        chain("inet", "other", "forward", "forward", "accept"),
        chain("inet", "other", "input", "input", "drop"),
    ];
    let found = parse(&ruleset(&items)).unwrap();
    let kinds: Vec<_> = found.iter().map(|d| d.firewall).collect();
    use HostFirewall::*;
    assert_eq!(kinds, [Firewalld, Iptables, Nftables]);
    assert!(
        found[1]
            .advice()
            .contains("iptables -I FORWARD -i phone+ -j ACCEPT")
    );
    assert!(
        found[2]
            .advice()
            .contains("nft insert rule inet filter forward iifname \"phone*\"")
    );
    assert!(parse("").is_err());
}
