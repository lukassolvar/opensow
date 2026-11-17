// The application icon: window icon pixels and, on Windows, the `.exe` icon resource (build_icon.rs).
include!("../opensow-launcher/build_icon.rs");

fn main() {
    let dir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    build_icon(&dir.join("../opensow-launcher/art/icon.png"));
}
