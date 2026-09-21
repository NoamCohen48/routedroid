use super::ports::*;
use crate::adb::parse_reverse_list;

#[test]
fn picks_first_free_from_seed_and_wraps() {
    assert_eq!(pick_device_port(&[], 0), Some(17_000));
    assert_eq!(pick_device_port(&[17_000], 0), Some(17_001));
    assert_eq!(pick_device_port(&[17_999], 999), Some(17_000));
    let all: Vec<u16> = DEVICE_PORT_RANGE.collect();
    assert_eq!(pick_device_port(&all, 5), None);
}

#[test]
fn ownership_requires_exactly_one_matching_mapping() {
    let ours = ReservedPort { device_port: 9000, host_port: 41234 };
    let list = parse_reverse_list("UsbFfs tcp:9000 tcp:41234\nUsbFfs tcp:9001 tcp:41235\n");
    assert!(is_exactly_ours(&list, ours));
    assert!(!is_exactly_ours(&list, ReservedPort { device_port: 9000, host_port: 41235 }));
    assert!(!is_exactly_ours(&list, ReservedPort { device_port: 9002, host_port: 41234 }));
    assert!(!is_exactly_ours(&[], ours));
    assert!(!is_exactly_ours(&parse_reverse_list("X tcp:9000 tcp:41234\nX tcp:9000 tcp:1\n"), ours));
}
