use super::content_failure;

#[test]
fn a_missing_provider_means_the_app_is_missing() {
    let uri = "content://dev.routedroid.bootstrap/record";
    assert_eq!(content_failure(uri, " \n"), None);
    let missing = "Error while accessing provider:dev.routedroid.bootstrap\n\
        java.lang.IllegalStateException: Could not find provider: dev.routedroid.bootstrap\n\
        \tat com.android.commands.content.Content$Command.execute(Content.java:519)";
    let message = content_failure(uri, missing).unwrap();
    assert!(message.starts_with("nothing on the phone provides dev.routedroid.bootstrap: "));
    assert_eq!(
        content_failure(uri, "Permission Denial").unwrap(),
        "content write reported: Permission Denial"
    );
}
