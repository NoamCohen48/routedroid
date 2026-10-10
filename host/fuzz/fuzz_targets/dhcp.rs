//! Everything parsed from a LAN frame: never panics on any bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use routedroid_dhcp::dhcp::{Message, Options, parse_classless_routes};
use routedroid_dhcp::packet::{parse_arp, parse_udp};

fuzz_target!(|data: &[u8]| {
    let _ = parse_arp(data);
    let _ = parse_classless_routes(data);
    for verify in [false, true] {
        if let Ok(udp) = parse_udp(data, verify) {
            let _ = Message::parse(udp.payload);
        }
    }
    if let Ok(m) = Message::parse(data) {
        let _ = Options::from_message(&m);
        let _ = m.message_type();
        let _ = Message::parse(&m.encode());
    }
});
