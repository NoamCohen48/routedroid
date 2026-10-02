use std::net::Ipv4Addr;

use super::Watch;
use super::arp::conflicts;
use crate::packet::{ARP_REPLY, ARP_REQUEST, Arp};

const OURS: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 50);
const OTHER: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 7);

fn arp(op: u16, spa: Ipv4Addr, tpa: Ipv4Addr) -> Arp {
    Arp {
        op,
        sha: [2, 0, 0, 0, 0, 9],
        spa,
        tha: [0; 6],
        tpa,
    }
}

#[test]
fn a_sender_using_the_address_conflicts() {
    for probing in [false, true] {
        let w = Watch {
            addr: OURS,
            probing,
        };
        assert!(conflicts(&arp(ARP_REPLY, OURS, OTHER), w), "reply from it");
        assert!(
            conflicts(&arp(ARP_REQUEST, OURS, OURS), w),
            "its announcement"
        );
        assert!(
            !conflicts(&arp(ARP_REQUEST, OTHER, OURS), w),
            "someone asking for it"
        );
        assert!(!conflicts(&arp(ARP_REPLY, OTHER, OURS), w));
    }
}

#[test]
fn another_probe_conflicts_only_while_probing() {
    let probe = arp(ARP_REQUEST, Ipv4Addr::UNSPECIFIED, OURS);
    assert!(conflicts(
        &probe,
        Watch {
            addr: OURS,
            probing: true
        }
    ));
    assert!(!conflicts(
        &probe,
        Watch {
            addr: OURS,
            probing: false
        }
    ));
    let elsewhere = arp(ARP_REQUEST, Ipv4Addr::UNSPECIFIED, OTHER);
    assert!(!conflicts(
        &elsewhere,
        Watch {
            addr: OURS,
            probing: true
        }
    ));
}
