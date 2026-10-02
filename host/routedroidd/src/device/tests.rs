use super::ports::*;
use super::Transport;
use crate::adb::parse_reverse_list;

#[test]
fn picks_first_free_from_seed_and_wraps() {
    let (first, last) = (*DEVICE_PORT_RANGE.start(), *DEVICE_PORT_RANGE.end());
    assert_eq!(pick_device_port(&[], 0), Some(first));
    assert_eq!(pick_device_port(&[first], 0), Some(first + 1));
    assert_eq!(pick_device_port(&[last], last - first), Some(first));
    assert!(last < 32_768, "stays below Android's ephemeral ports");
    let all: Vec<u16> = DEVICE_PORT_RANGE.collect();
    assert_eq!(pick_device_port(&all, 5), None);
}

#[test]
fn ownership_requires_exactly_one_matching_mapping() {
    let ours = ReservedPort {
        device_port: 9000,
        host_port: 41234,
    };
    let list = parse_reverse_list("UsbFfs tcp:9000 tcp:41234\nUsbFfs tcp:9001 tcp:41235\n");
    assert!(is_exactly_ours(&list, ours));
    assert!(!is_exactly_ours(
        &list,
        ReservedPort {
            device_port: 9000,
            host_port: 41235
        }
    ));
    assert!(!is_exactly_ours(
        &list,
        ReservedPort {
            device_port: 9002,
            host_port: 41234
        }
    ));
    assert!(!is_exactly_ours(&[], ours));
    assert!(!is_exactly_ours(
        &parse_reverse_list("X tcp:9000 tcp:41234\nX tcp:9000 tcp:1\n"),
        ours
    ));
}

#[test]
fn classifies_serials() {
    assert_eq!(Transport::classify("R58M12345AB"), Transport::Usb);
    assert_eq!(Transport::classify("0123456789ABCDEF"), Transport::Usb);
    assert_eq!(Transport::classify("192.168.1.5:5555"), Transport::Network);
    assert_eq!(
        Transport::classify("adb-R58M12345AB-abcdef._adb-tls-connect._tcp"),
        Transport::Network
    );
    assert_eq!(Transport::classify("emulator-5554"), Transport::Emulator);
    assert_eq!(Transport::classify("emulator-x"), Transport::Usb);
    assert_eq!(Transport::classify(""), Transport::Invalid);
    assert_eq!(Transport::classify("has space"), Transport::Invalid);
}

#[test]
fn network_is_refused_unless_allowed() {
    assert!(Transport::check("R58M12345AB", false).is_ok());
    assert!(Transport::check("emulator-5554", false).is_ok());
    assert_eq!(
        Transport::check("10.0.0.2:5555", false).unwrap_err().kind(),
        crate::fault::Kind::Transport
    );
    assert!(Transport::check("10.0.0.2:5555", true).is_ok());
    assert_eq!(
        Transport::check("", true).unwrap_err().kind(),
        crate::fault::Kind::Usage
    );
}
