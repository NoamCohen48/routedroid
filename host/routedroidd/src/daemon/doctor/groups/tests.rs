use super::advice;

const ETC: &str = "root:x:0:\nroutedroid:x:989:dev,alice\n";

#[test]
fn a_non_member_is_told_to_run_setup() {
    assert_eq!(
        advice("bob", ETC),
        "add bob to group routedroid: sudo routedroid setup"
    );
}

#[test]
fn a_refused_member_points_at_an_old_socket() {
    assert!(advice("dev", ETC).contains("sudo systemctl restart routedroid-helper.socket"));
}

#[test]
fn no_group_points_at_install() {
    assert!(advice("dev", "root:x:0:\n").contains("does not exist"));
}
