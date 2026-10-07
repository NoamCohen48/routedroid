//! Against this desktop's notification server, by hand:
//! `cargo test -p routedroidd -- --ignored on_this_desktop`.

use super::notice::Notice;
use super::show;

#[tokio::test]
#[ignore = "shows a notification on the desktop it runs on"]
async fn on_this_desktop() {
    let bus = zbus::Connection::session().await.unwrap();
    let notice = |summary: &str| Notice {
        serial: "R58M".into(),
        summary: summary.into(),
        body: "Its address is 192.168.1.201.".into(),
        urgent: false,
    };
    let id = show(&bus, 0, &notice("pixel is on the LAN")).await.unwrap();
    assert_ne!(id, 0);
    let again = show(&bus, id, &notice("pixel is back on the LAN"))
        .await
        .unwrap();
    assert_eq!(again, id, "replaced, not stacked");
}
