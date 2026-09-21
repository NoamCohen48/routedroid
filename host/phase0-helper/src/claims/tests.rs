//! Claim bookkeeping against a scratch directory; the sysctl side needs
//! root and is covered by the multi-session rig.

use super::*;

fn scratch() -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("rd-claims-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn claim_files_round_trip_and_track_holders() {
    let dir = scratch();
    let path = dir.join("net.ipv4.conf.lan0.proxy_arp.json");
    assert_eq!(read_claim(&path).unwrap(), None);
    let mut claim = Claim { baseline: "0".into(), value: "1".into(), holders: vec!["a".into()] };
    write_claim(&path, &claim).unwrap();
    assert_eq!(read_claim(&path).unwrap(), Some(claim.clone()));
    claim.holders.push("b".into());
    write_claim(&path, &claim).unwrap();
    assert_eq!(read_claim(&path).unwrap().unwrap().holders, vec!["a", "b"]);
    assert!(!dir.join("net.ipv4.conf.lan0.proxy_arp.tmp").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn only_plain_keys_name_claim_files() {
    assert!(claim_path("net.ipv4.conf.lan0.proxy_arp").is_ok());
    assert!(claim_path("../etc/passwd").is_err());
    assert!(claim_path("a b").is_err());
    assert!(claim_path("").is_err());
}
