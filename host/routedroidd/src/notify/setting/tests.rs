use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rdd-settings-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("settings.toml")
}

#[test]
fn off_is_kept_across_a_restart() {
    let path = scratch("restart");
    let setting = Setting::load(path.clone(), true);
    assert!(setting.on());
    setting.set(false).unwrap();
    assert!(!setting.on());
    assert!(!Setting::load(path.clone(), true).on());
    setting.set(true).unwrap();
    assert!(Setting::load(path, true).on());
}

#[test]
fn notify_false_wins() {
    let setting = Setting::in_memory(false);
    assert!(!setting.on());
    assert!(setting.set(false).is_ok());
    let refused = setting.set(true).unwrap_err();
    assert!(refused.to_string().contains("--notify false"));
}

#[test]
fn an_unreadable_file_leaves_them_on() {
    let path = scratch("unreadable");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "notify = ").unwrap();
    assert!(Setting::load(path, true).on());
}
