//! `ROUTEDROID_APK=PATH cargo build` embeds that APK (the release app) in
//! routedroidd, which installs it on phones that lack the app or have an
//! older one. Without it the daemon carries no app: dev builds and tests.
//! The APK must be built from the same tree: the daemon takes its version
//! to be its own.

use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-env-changed=ROUTEDROID_APK");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let apk = match std::env::var_os("ROUTEDROID_APK").filter(|p| !p.is_empty()) {
        Some(path) => {
            let path = PathBuf::from(path);
            // Relative would mean relative to host/routedroidd: surprising.
            assert!(
                path.is_absolute(),
                "ROUTEDROID_APK must be an absolute path"
            );
            println!("cargo::rerun-if-changed={}", path.display());
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|e| panic!("ROUTEDROID_APK={}: {e}", path.display()));
            assert!(
                bytes.starts_with(b"PK\x03\x04"),
                "ROUTEDROID_APK={} is not an APK",
                path.display()
            );
            // Phones refuse unsigned APKs: embedding one would fail every install.
            assert!(
                bytes.windows(16).any(|w| w == b"APK Sig Block 42"),
                "ROUTEDROID_APK={} is not signed (APK Signature Scheme v2+)",
                path.display()
            );
            bytes
        }
        None => Vec::new(),
    };
    std::fs::write(out.join("app.apk"), apk).expect("write the embedded APK");
}
