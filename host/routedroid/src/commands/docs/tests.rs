use super::*;

#[test]
fn completions_cover_every_shell_we_package() {
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        let mut out = Vec::new();
        clap_complete::generate(shell, &mut Cli::command(), "routedroid", &mut out);
        let script = String::from_utf8(out).unwrap();
        assert!(script.contains("doctor"), "{shell}: {script}");
    }
}

#[test]
fn a_page_per_visible_subcommand() {
    let dir = std::env::temp_dir().join(format!("routedroid-man-{}", std::process::id()));
    manpages(&dir).unwrap();
    let main = std::fs::read_to_string(dir.join("routedroid.1")).unwrap();
    assert!(main.contains(".TH routedroid 1"), "{main}");
    let start = std::fs::read_to_string(dir.join("routedroid-start.1")).unwrap();
    assert!(start.contains("lan\\-if"), "{start}");
    assert!(
        !dir.join("routedroid-manpages.1").exists(),
        "hidden stays hidden"
    );
    assert!(!dir.join("routedroid-help.1").exists());
    std::fs::remove_dir_all(dir).unwrap();
}
