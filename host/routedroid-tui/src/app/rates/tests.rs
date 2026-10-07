use std::time::Duration;

use super::*;

fn traffic(to: u64, from: u64) -> Traffic {
    Traffic {
        bytes_to_phone: to,
        bytes_from_phone: from,
        ..Traffic::default()
    }
}

#[test]
fn rates_are_bytes_a_second_between_readings() {
    let start = Instant::now();
    let mut rates = Rates::default();
    rates.record(start, &traffic(1000, 100));
    assert_eq!(rates.now(), None, "one reading is only a baseline");
    rates.record(start + Duration::from_secs(1), &traffic(3000, 600));
    assert_eq!(rates.now(), Some((2000, 500)));
    rates.record(start + Duration::from_secs(3), &traffic(7000, 600));
    assert_eq!(rates.now(), Some((2000, 0)), "over two seconds");
    assert_eq!(rates.to_phone, [2000, 2000]);
}

#[test]
fn a_minute_is_kept() {
    let start = Instant::now();
    let mut rates = Rates::default();
    for second in 0..=100 {
        rates.record(
            start + Duration::from_secs(second),
            &traffic(second * 10, 0),
        );
    }
    assert_eq!(rates.to_phone.len(), SAMPLES);
    assert!(rates.to_phone.iter().all(|&rate| rate == 10));
}

#[test]
fn counters_going_back_start_over() {
    let start = Instant::now();
    let mut rates = Rates::default();
    rates.record(start, &traffic(0, 0));
    rates.record(start + Duration::from_secs(1), &traffic(5000, 5000));
    rates.record(start + Duration::from_secs(2), &traffic(10, 10));
    assert_eq!(rates.now(), None);
    rates.record(start + Duration::from_secs(3), &traffic(110, 20));
    assert_eq!(rates.now(), Some((100, 10)));
}

#[test]
fn a_repeated_instant_adds_nothing() {
    let start = Instant::now();
    let mut rates = Rates::default();
    rates.record(start, &traffic(0, 0));
    rates.record(start, &traffic(500, 0));
    assert_eq!(rates.now(), None);
}
