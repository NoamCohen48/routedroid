use std::sync::Mutex;
use std::sync::atomic::Ordering;

use super::*;

/// A UDP datagram with an empty payload: a 20-byte header and 8 bytes of UDP.
fn packet() -> Vec<u8> {
    let mut p = vec![
        0x45, 0, 0, 28, 0, 0, 0, 0, 64, 17, 0, 0, 10, 0, 0, 2, 10, 0, 0, 1,
    ];
    p.extend([0, 53, 0, 53, 0, 8, 0, 0]);
    p
}

fn uplink(result: fn() -> io::Result<bool>) -> (Uplink, Arc<Mutex<Vec<Vec<u8>>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let inject: Inject = Arc::new(move |p: &[u8]| {
        log.lock().unwrap().push(p.to_vec());
        result()
    });
    (
        Uplink {
            inject,
            counters: Arc::default(),
        },
        seen,
    )
}

#[test]
fn well_formed_packets_are_injected_and_counted() {
    let (up, seen) = uplink(|| Ok(true));
    up.forward(&packet()).unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![packet()]);
    assert_eq!(up.counters.packets_from_phone(), 1);
}

#[test]
fn malformed_packets_are_dropped_before_the_helper() {
    let (up, seen) = uplink(|| Ok(true));
    let mut bad = packet();
    bad[0] = 0x65;
    up.forward(&bad).unwrap();
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(up.counters.malformed(), 1);
}

#[test]
fn a_full_helper_drops_and_a_gone_helper_fails() {
    let (up, _) = uplink(|| Ok(false));
    up.forward(&packet()).unwrap();
    assert_eq!(
        (up.counters.congested(), up.counters.packets_from_phone()),
        (1, 0)
    );
    let (up, _) = uplink(|| Err(io::ErrorKind::BrokenPipe.into()));
    assert!(up.forward(&packet()).is_err());
    assert_eq!(
        up.counters.from_phone.load(Ordering::Relaxed),
        0,
        "counted only when delivered"
    );
}
