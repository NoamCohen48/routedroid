//! Form editing and validation.

use super::{Field, StartForm};

fn type_text(form: &mut StartForm, text: &str) {
    text.chars().for_each(|character| form.insert(character));
}

#[test]
fn tab_cycles_through_fields_and_wraps() {
    let mut form = StartForm::new("abc".into());
    assert_eq!(form.focused, Field::LanIf);
    form.focus_next();
    assert_eq!(form.focused, Field::PhoneIp);
    form.focus_next();
    form.focus_next();
    assert_eq!(form.focused, Field::LanIf);
    form.focus_previous();
    assert_eq!(form.focused, Field::Dns);
}

#[test]
fn typing_goes_to_the_focused_field() {
    let mut form = StartForm::new("abc".into());
    type_text(&mut form, "eth0");
    form.focus_next();
    type_text(&mut form, "10.0.0.5x");
    form.backspace();
    assert_eq!(form.lan_if, "eth0");
    assert_eq!(form.phone_ip, "10.0.0.5");
    assert_eq!(form.dns, "");
}

#[test]
fn valid_form_becomes_a_start_request() {
    let mut form = StartForm::new("abc".into());
    form.lan_if = " eth0 ".into();
    form.phone_ip = "10.0.0.5".into();
    form.dns = "1.1.1.1, 8.8.8.8".into();
    let request = form.to_request().unwrap();
    assert_eq!(request.serial, "abc");
    assert_eq!(request.lan_if, "eth0");
    assert_eq!(request.phone_ip.to_string(), "10.0.0.5");
    assert_eq!(request.dns.len(), 2);
    assert!(request.tun.is_none());
}

#[test]
fn invalid_fields_are_named_in_the_error() {
    let mut form = StartForm::new("abc".into());
    assert!(form.to_request().unwrap_err().to_string().contains("LAN interface"));
    form.lan_if = "eth0".into();
    form.phone_ip = "nope".into();
    assert!(form.to_request().unwrap_err().to_string().contains("phone IP"));
    form.phone_ip = "10.0.0.5".into();
    form.dns = "1.1.1.1,bad".into();
    assert!(form.to_request().unwrap_err().to_string().contains("DNS"));
}
