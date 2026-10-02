use super::*;

pub fn sample() -> Lease {
    Lease {
        iface: "eth0".into(),
        client_id: "routedroid:0123456789abcdef:020000000001".into(),
        address: "192.168.50.101".parse().unwrap(),
        prefix: 24,
        subnet_mask: "255.255.255.0".parse().unwrap(),
        router: Some("192.168.50.1".parse().unwrap()),
        dns: vec!["192.168.50.1".parse().unwrap()],
        static_routes: vec![],
        server_id: "192.168.50.1".parse().unwrap(),
        server_mac: "de:ad:be:ef:00:01".into(),
        lease_secs: 3600,
        t1: 1800,
        t2: 3150,
        acquired_at: 1_700_000_000,
        vlan_tagged_replies: false,
    }
}

#[test]
fn json_roundtrip() {
    let l = sample();
    let back: Lease = serde_json::from_str(&l.to_json()).unwrap();
    assert_eq!(back, l);
    assert_eq!(back.server_mac().unwrap(), [0xde, 0xad, 0xbe, 0xef, 0, 1]);
    assert_eq!(back.client_id().unwrap().as_str(), l.client_id);
    assert_eq!(back.expires_at(), 1_700_003_600);
}

#[test]
fn save_replaces_durably_and_leaves_no_temp_file() {
    let dir = std::env::temp_dir().join(format!("rd-lease-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("lease.json");
    let mut l = sample();
    l.save(&path).unwrap();
    l.lease_secs = 7200;
    l.save(&path).unwrap();
    assert_eq!(Lease::load(&path).unwrap(), l);
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["lease.json"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn schedule_is_monotonic_from_the_ack() {
    let received = Instant::now();
    let s = Schedule::new(&sample(), received);
    assert_eq!(s.t1 - received, Duration::from_secs(1800));
    assert_eq!(s.t2 - received, Duration::from_secs(3150));
    assert_eq!(s.expiry - received, Duration::from_secs(3600));
}

#[test]
fn a_restored_schedule_counts_what_is_left() {
    let mut l = sample();
    l.acquired_at = unix_now() - 1000;
    let before = Instant::now();
    let s = Schedule::restored(&l);
    let left = s.t1 - before;
    assert!(
        (Duration::from_secs(799)..=Duration::from_secs(801)).contains(&left),
        "{left:?}"
    );
    l.acquired_at = 1; // long expired
    let s = Schedule::restored(&l);
    assert!(s.expiry <= Instant::now());
}
