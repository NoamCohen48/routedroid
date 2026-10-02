use super::*;

const DEVICE: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
const MAC: Mac = [0x02, 0, 0, 0, 0, 0x01];

#[test]
fn built_from_device_and_interface() {
    let id = ClientId::new(&DEVICE, &MAC);
    assert_eq!(id.as_str(), "routedroid:0123456789abcdef:020000000001");
    let option = id.option();
    assert_eq!(option[0], 0, "type 0: not a hardware address");
    assert_eq!(&option[1..], id.as_str().as_bytes());
}

#[test]
fn parses_only_its_own_shape() {
    let id = ClientId::new(&DEVICE, &MAC);
    assert_eq!(ClientId::parse(id.as_str()), Ok(id));
    for bad in [
        "",
        "test",
        "routedroid:lab:1:a",
        "routedroid:0123456789abcdef",
        "routedroid:0123456789ABCDEF:020000000001",
        "routedroid:0123456789abcdef:020000000001:x",
        "Routedroid:0123456789abcdef:020000000001",
        "routedroid:0123456789abcde:0200000000011",
    ] {
        assert!(ClientId::parse(bad).is_err(), "{bad:?}");
    }
}
