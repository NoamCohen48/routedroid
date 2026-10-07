use super::*;

fn phone(serial: &str, name: Option<&str>) -> Phone {
    Phone {
        serial: serial.into(),
        name: name.map(Into::into),
        auto: true,
        ..Phone::default()
    }
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rdd-phones-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("routedroid").join("phones.toml")
}

#[test]
fn remembered_phones_survive_a_restart() {
    let path = scratch();
    let phones = Phones::load(path.clone());
    assert!(phones.all().is_empty(), "no file yet");
    phones.remember(phone("R58", Some("pixel"))).unwrap();
    phones.remember(phone("ZX1", None)).unwrap();
    let again = Phones::load(path.clone());
    assert_eq!(again.all(), phones.all());
    assert_eq!(again.find("pixel").unwrap().serial, "R58");
    assert_eq!(again.serial("pixel"), "R58");
    assert_eq!(again.serial("unknown"), "unknown");
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn remembering_again_replaces_and_forgetting_removes() {
    let phones = Phones::default();
    phones.remember(phone("R58", Some("pixel"))).unwrap();
    phones.remember(phone("R58", Some("work"))).unwrap();
    assert_eq!(phones.all().len(), 1);
    assert!(phones.find("pixel").is_none());
    assert_eq!(phones.forget("work").unwrap().serial, "R58");
    assert!(phones.all().is_empty());
    let error = phones.forget("work").unwrap_err();
    assert_eq!(error.to_string(), "work is not a remembered phone");
}

#[test]
fn names_are_words_nobody_else_has() {
    let phones = Phones::default();
    phones.remember(phone("R58", Some("pixel"))).unwrap();
    let taken = phones.remember(phone("ZX1", Some("pixel"))).unwrap_err();
    assert_eq!(taken.to_string(), "pixel already names R58");
    let a_serial = phones.remember(phone("ZX1", Some("R58"))).unwrap_err();
    assert_eq!(a_serial.to_string(), "R58 already names R58");
    let spaced = phones.remember(phone("ZX1", Some("my phone"))).unwrap_err();
    assert!(
        spaced.to_string().starts_with("name \"my phone\""),
        "{spaced}"
    );
    assert_eq!(spaced.kind(), Kind::Usage);
}

#[test]
fn a_broken_file_is_an_empty_list() {
    let path = scratch();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "[[phone]]\nserial = 3\n").unwrap();
    assert!(Phones::load(path.clone()).all().is_empty());
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}
