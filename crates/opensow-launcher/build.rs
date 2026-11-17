// The application icon: window icon pixels and, on Windows, the `.exe` icon resource (build_icon.rs).
include!("../opensow-launcher/build_icon.rs");

fn main() {
    let dir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    build_icon(&dir.join("../opensow-launcher/art/icon.png"));
    // this version's release notes in each language for the home page's "What's new" card (update::OWN_NOTES, NOTE_LANGS);
    // empty when not written yet
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    println!("cargo:rerun-if-changed={}", dir.join("../../docs/releases").display());
    for lang in ["", ".sk", ".cs", ".zh-Hans", ".zh-Hant"] {
        let notes = dir.join(format!("../../docs/releases/v{version}{lang}.md"));
        println!("cargo:rerun-if-changed={}", notes.display());
        std::fs::write(out.join(format!("notes{lang}.md")), std::fs::read_to_string(&notes).unwrap_or_default()).expect("write notes");
    }
}
