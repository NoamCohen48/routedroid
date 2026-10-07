use super::version_code;

const INSTALLED: &str = "\
Activity Resolver Table:
  Non-Data Actions:
Packages:
  Package [dev.routedroid] (4b1c2f0):
    userId=10245
    pkg=Package{9a8e1d3 dev.routedroid}
    versionCode=1002003 minSdk=26 targetSdk=35
    versionName=1.2.3
  Package [dev.routedroid.other] (77aa001):
    versionCode=9 minSdk=26 targetSdk=35
";

#[test]
fn the_installed_version_is_the_packages_own() {
    assert_eq!(version_code(INSTALLED, "dev.routedroid"), Some(1_002_003));
    assert_eq!(version_code(INSTALLED, "dev.routedroid.other"), Some(9));
}

#[test]
fn a_missing_package_has_no_version() {
    let missing = "Dexopt state:\n  versionCode=5\nUnable to find package: dev.routedroid\n";
    assert_eq!(version_code(missing, "dev.routedroid"), None);
    assert_eq!(version_code("", "dev.routedroid"), None);
}
