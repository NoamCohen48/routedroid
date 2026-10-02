use super::*;

#[test]
fn accepts_real_device_names() {
    for name in [
        "eno1",
        "phone0",
        "eth0.100",
        "wlp3s0",
        "br-lan",
        "a_b",
        "0123456789abcde",
    ] {
        assert_eq!(IfName::new(name).unwrap().as_str(), name);
    }
}

#[test]
fn rejects_what_is_not_a_device() {
    for name in [
        "",
        "lo",
        ".",
        "..",
        "all",
        "default",
        "-x",
        "a b",
        "x/y",
        "a:b",
        "0123456789abcdef",
    ] {
        assert!(IfName::new(name).is_err(), "{name:?} accepted");
    }
}

#[test]
fn validates_while_deserializing() {
    assert_eq!(
        serde_json::from_str::<IfName>(r#""eno1""#).unwrap(),
        IfName::new("eno1").unwrap()
    );
    assert!(serde_json::from_str::<IfName>(r#""../x""#).is_err());
}
