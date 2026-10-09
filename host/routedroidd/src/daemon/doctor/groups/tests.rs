use super::advice;

#[test]
fn a_non_member_is_told_to_run_setup() {
    assert_eq!(
        advice("bob", true, "bob users\n"),
        "add bob to group routedroid: sudo routedroid setup"
    );
}

#[test]
fn a_refused_member_points_at_an_old_socket() {
    let hint = advice("dev", true, "dev plugdev routedroid\n");
    assert!(hint.contains("sudo systemctl restart routedroid-helper.socket"));
    assert!(
        !advice("dev", true, "routedroid-old\n").contains("restart"),
        "whole names only"
    );
}

#[test]
fn no_group_points_at_install() {
    assert!(advice("dev", false, "").contains("does not exist"));
}
