//! Frame decoding: never panics, and anything it accepts re-encodes to the
//! bytes it consumed.
#![no_main]

use libfuzzer_sys::fuzz_target;
use routedroid_proto::frame;

fuzz_target!(|data: &[u8]| {
    if let Ok((frame, used)) = frame::decode(data, 1500) {
        assert_eq!(frame.encode(), data[..used]);
    }
});
