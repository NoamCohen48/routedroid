use super::advice;

const ETC: &str = "root:x:0:\nroutedroid:x:989:dev,alice\n";

#[test]
fn a_member_whose_daemon_predates_the_group_is_told_to_log_out() {
    let hint = advice("dev", ETC, "Name:\troutedroidd\nGroups:\t46 1000 \n");
    assert!(hint.contains("dev is in group routedroid, but this daemon started before that"));
}

#[test]
fn a_non_member_is_told_how_to_join() {
    let hint = advice("bob", ETC, "Groups:\t1000\n");
    assert!(hint.starts_with("add bob to group routedroid"));
}

#[test]
fn holding_the_group_points_at_the_socket_and_no_group_at_install() {
    assert!(advice("dev", ETC, "Groups:\t46 989 1000\n").contains("routedroid-helper.socket"));
    assert!(advice("dev", "root:x:0:\n", "").contains("does not exist"));
}
