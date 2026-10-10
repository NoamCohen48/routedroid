use clap::Parser;

use routedroid_ipc::DnsChoice;

use super::*;

#[derive(Parser)]
struct Wrapper {
    #[command(flatten)]
    args: StartArgs,
}

fn parse(extra: &[&str]) -> Result<StartRequest, clap::Error> {
    let argv = ["routedroid", "-s", "R58M", "--lan-if", "eno1"]
        .iter()
        .chain(extra);
    Wrapper::try_parse_from(argv).map(|wrapper| wrapper.args.request())
}

#[test]
fn options_left_out_are_not_sent() {
    let request = parse(&[]).unwrap();
    assert_eq!(
        (request.phone_ip, request.mtu, request.tun),
        (None, None, None)
    );
    assert_eq!(request.connect_timeout_secs, None);
    assert_eq!(request.dns, None);
}

#[test]
fn the_phone_is_a_word_or_a_flag_or_left_to_the_daemon() {
    let parse_bare = |argv: &[&str]| {
        Wrapper::try_parse_from(argv)
            .map(|wrapper| wrapper.args.request())
            .unwrap()
    };
    let named = parse_bare(&["routedroid", "pixel"]);
    assert_eq!(named.serial.as_deref(), Some("pixel"));
    assert_eq!(named.lan_if, None);
    let flagged = parse_bare(&["routedroid", "-s", "R58M", "--lan-if", "eno1"]);
    assert_eq!(flagged.serial.as_deref(), Some("R58M"));
    assert_eq!(flagged.lan_if.as_deref(), Some("eno1"));
}

#[test]
fn dns_is_auto_listed_or_none() {
    let listed = parse(&["--dns", "9.9.9.9", "--dns", "1.1.1.1"]).unwrap();
    assert!(matches!(listed.dns, Some(DnsChoice::Servers(servers)) if servers.len() == 2));
    assert_eq!(parse(&["--no-dns"]).unwrap().dns, Some(DnsChoice::None));
    assert!(parse(&["--no-dns", "--dns", "9.9.9.9"]).is_err());
}

#[test]
fn timeouts_read_like_durations() {
    assert_eq!(
        parse(&["--connect-timeout", "2m"])
            .unwrap()
            .connect_timeout_secs,
        Some(120)
    );
    assert_eq!(
        parse(&["--connect-timeout", "90s"])
            .unwrap()
            .connect_timeout_secs,
        Some(90)
    );
    assert_eq!(
        parse(&["--connect-timeout", "500ms"])
            .unwrap()
            .connect_timeout_secs,
        Some(1)
    );
    assert!(parse(&["--connect-timeout", "90sss"]).is_err());
    let reconnect = |arg| parse(&["--reconnect-wait", arg]).unwrap().reconnect_secs;
    assert_eq!(reconnect("5m"), Some(300));
    // Zero is a choice (end at once), not "the default".
    assert_eq!(reconnect("0s"), Some(0));
    assert_eq!(parse(&[]).unwrap().reconnect_secs, None);
}
