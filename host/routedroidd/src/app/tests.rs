use super::{BundledApp, Need, version_code, version_name};

#[test]
fn version_codes_match_the_gradle_build() {
    assert_eq!(version_code("0.1.0"), Some(1_000));
    assert_eq!(version_code("1.2.3"), Some(1_002_003));
    assert_eq!(version_name(1_002_003), "1.2.3");
    for bad in ["1.2", "1.2.3.4", "1.2.x", "1.1000.0", "1.2.3-rc1"] {
        assert_eq!(version_code(bad), None, "{bad}");
    }
}

#[test]
fn a_phone_needs_the_app_when_it_lacks_it_or_has_an_older_one() {
    let app = BundledApp::new(b"", 1_002_003);
    assert_eq!(app.need(None), Need::Install);
    assert_eq!(app.need(Some(1_002_002)), Need::Upgrade { from: 1_002_002 });
    assert_eq!(app.need(Some(1_002_003)), Need::Nothing);
    assert_eq!(
        app.need(Some(2_000_000)),
        Need::Nothing,
        "never a downgrade"
    );
}
