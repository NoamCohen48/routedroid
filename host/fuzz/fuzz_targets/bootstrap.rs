//! The bootstrap record: never panics; what decodes re-encodes identically.
#![no_main]

use libfuzzer_sys::fuzz_target;
use routedroid_proto::bootstrap;

fuzz_target!(|data: &[u8]| {
    if let Ok(record) = bootstrap::decode(data) {
        let again = bootstrap::encode(&record.session, record.device_port, &record.secret)
            .expect("a decoded record re-encodes");
        assert_eq!(again.as_slice(), data);
    }
});
