//! Every control body parser: never panics on any bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use routedroid_proto::messages::{
    self, Auth, ConfigureVpn, ErrorBody, Hello, HelloAck, VpnReady,
};

fuzz_target!(|data: &[u8]| {
    let _ = messages::parse::<Hello>(data);
    let _ = messages::parse::<HelloAck>(data);
    let _ = messages::parse::<Auth>(data);
    let _ = messages::parse::<ConfigureVpn>(data);
    let _ = messages::parse::<VpnReady>(data);
    let _ = messages::parse::<ErrorBody>(data);
});
