//! Form editing, picking and validation.

use routedroid_ipc::{DnsChoice, InterfaceInfo};

use super::{Field, LineInput, StartForm};

fn interface(name: &str, default_route: bool, ineligible: Option<&str>) -> InterfaceInfo {
    InterfaceInfo {
        name: name.into(),
        up: true,
        addresses: vec![],
        default_route,
        phone_addresses: vec![],
        dhcp: false,
        ineligible: ineligible.map(Into::into),
    }
}

fn form() -> StartForm {
    StartForm::new("abc".into(), &[])
}

#[test]
fn the_cursor_edits_in_place() {
    let mut input = LineInput::new("10.0.5");
    input.left();
    input.left();
    input.insert(".0");
    assert_eq!((input.value(), input.cursor()), ("10.0.0.5", 6));
    input.home();
    input.delete();
    input.end();
    input.backspace();
    assert_eq!(input.value(), "0.0.0.");
    input.insert("é\u{7}x");
    assert_eq!(input.value(), "0.0.0.éx", "control characters are dropped");
}

#[test]
fn tab_cycles_through_fields_and_wraps() {
    let mut form = form();
    assert_eq!(form.focused, Field::LanIf);
    form.focus_previous();
    assert_eq!(form.focused, Field::NetworkAdb);
    form.focus_next();
    form.focus_next();
    assert_eq!(form.focused, Field::PhoneIp);
}

#[test]
fn the_interface_is_picked_from_the_eligible_ones() {
    let interfaces = [
        interface("docker0", false, Some("a bridge")),
        interface("wlan0", false, None),
        interface("eno1", true, None),
    ];
    let mut form = StartForm::new("abc".into(), &interfaces);
    assert_eq!(form.lan_if.value(), "eno1", "the default route first");
    assert!(form.picking());
    assert!(form.input(Field::LanIf).is_none());
    form.pick(true);
    assert_eq!(form.lan_if.value(), "wlan0");
    form.pick(true);
    assert_eq!(form.lan_if.value(), "eno1", "wraps, skipping docker0");
    form.offer(&interfaces[..2]);
    assert_eq!(form.lan_if.value(), "wlan0", "a vanished pick is replaced");
}

#[test]
fn empty_fields_leave_the_defaults_to_the_daemon() {
    let mut form = form();
    form.lan_if = LineInput::new(" eth0 ");
    let request = form.to_request().unwrap();
    assert_eq!(request.lan_if.as_deref(), Some("eth0"));
    assert_eq!(
        (request.phone_ip, request.mtu, request.tun),
        (None, None, None)
    );
    assert_eq!(request.dns, None);
    assert_eq!(request.connect_timeout_secs, None);
}

#[test]
fn filled_fields_become_the_request() {
    let mut form = form();
    form.lan_if = LineInput::new("eth0");
    form.phone_ip = LineInput::new("10.0.0.5");
    form.dns = LineInput::new("1.1.1.1, 8.8.8.8");
    form.mtu = LineInput::new("1280");
    form.timeout = LineInput::new("2m");
    form.reconnect_wait = LineInput::new("5m");
    form.allow_network_adb = true;
    let request = form.to_request().unwrap();
    assert_eq!(request.phone_ip.unwrap().to_string(), "10.0.0.5");
    assert!(matches!(request.dns, Some(DnsChoice::Servers(servers)) if servers.len() == 2));
    assert_eq!(
        (request.mtu, request.connect_timeout_secs),
        (Some(1280), Some(120))
    );
    assert_eq!(request.reconnect_secs, Some(300));
    assert!(request.allow_network_adb);
    form.dns = LineInput::new("none");
    assert_eq!(form.to_request().unwrap().dns, Some(DnsChoice::None));
}

#[test]
fn invalid_fields_are_named_in_the_error() {
    let mut form = form();
    let error = |form: &StartForm| form.to_request().unwrap_err().to_string();
    assert!(
        form.to_request().unwrap().lan_if.is_none(),
        "empty: the one allowed"
    );
    form.lan_if = LineInput::new("eth0");
    form.phone_ip = LineInput::new("nope");
    assert!(error(&form).contains("phone IP"));
    form.phone_ip = LineInput::default();
    form.dns = LineInput::new("1.1.1.1,bad");
    assert!(error(&form).contains("DNS"));
    form.dns = LineInput::default();
    form.timeout = LineInput::new("soon");
    assert!(error(&form).contains("timeout"));
    form.timeout = LineInput::default();
    form.reconnect_wait = LineInput::new("later");
    assert!(error(&form).contains("reconnect wait"));
}

#[test]
fn the_phone_field_hints_the_allowed_blocks() {
    let mut lan = interface("eno1", true, None);
    lan.phone_addresses = vec![routedroid_ipc::Ipv4Net {
        address: "10.0.0.200".parse().unwrap(),
        prefix: 29,
    }];
    let form = StartForm::new("abc".into(), &[lan, interface("wlan0", false, None)]);
    assert_eq!(form.phone_hint().as_deref(), Some("allowed: 10.0.0.200/29"));
    assert_eq!(StartForm::new("abc".into(), &[]).phone_hint(), None);
}
