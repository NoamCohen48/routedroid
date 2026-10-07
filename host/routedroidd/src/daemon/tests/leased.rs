//! Sessions whose address the helper leases.

use std::net::Ipv4Addr;

use routedroid_ipc::{ConnectionState, DnsChoice, Kind, Lease, Outcome};

use super::fake_app::FakeApp;
use super::fake_replies::{ENDED, LEASE_DNS, LEASED_IP};
use super::{Lab, request};

const PHONE: &str = "R58FAKE01";
const OTHER: &str = "R58FAKE02";

fn leased() -> routedroid_ipc::StartRequest {
    let mut start = request(PHONE, [0; 4]);
    start.phone_ip = None;
    start.dns = Some(DnsChoice::Auto);
    start
}

#[tokio::test]
async fn a_leased_phone_gets_the_leased_address_and_dns() {
    let mut lab = Lab::new(&[PHONE, OTHER]).await;
    lab.connections.start(leased()).await.unwrap();
    let app = FakeApp::connect(&lab.adb, PHONE).await;
    assert_eq!(
        app.configure.addresses[0].address,
        Ipv4Addr::from(LEASED_IP)
    );
    assert_eq!(app.configure.dns, vec![Ipv4Addr::from(LEASE_DNS)]);
    lab.until(PHONE, |s| *s == ConnectionState::Active).await;
    let network = lab.connections.info().remove(0).network.unwrap();
    assert_eq!(network.phone_ip, Ipv4Addr::from(LEASED_IP));
    let server = Ipv4Addr::new(10, 0, 0, 254);
    assert_eq!(
        network.lease,
        Some(Lease {
            server,
            expires_at: 1000
        })
    );

    // A start asking for the leased address is a duplicate.
    let again = lab.connections.start(request(OTHER, LEASED_IP)).await;
    let refusal = again.unwrap_err().to_string();
    assert_eq!(refusal, "10.0.0.50 is already used by R58FAKE01");
    assert!(lab.connections.stop(PHONE).await.unwrap().is_clean());
}

#[tokio::test]
async fn renewals_are_published_and_the_helpers_end_fails_the_connection() {
    let mut lab = Lab::new(&[PHONE]).await;
    lab.connections.start(leased()).await.unwrap();
    let _app = FakeApp::connect(&lab.adb, PHONE).await;
    lab.until(PHONE, |s| *s == ConnectionState::Active).await;
    lab.helper.kick.notify_one();
    match lab.ended(PHONE).await {
        Outcome::Failed { kind, message } => {
            assert_eq!(kind, Kind::Helper);
            assert!(message.contains(ENDED), "{message}");
        }
        other => panic!("expected a failure, got {other:?}"),
    }
    assert_eq!(lab.helper.stops(), 0, "nothing left to stop");
}
