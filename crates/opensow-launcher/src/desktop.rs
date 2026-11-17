//! Linux desktop entry (not in the exe): Wayland desktops take an application's icon and name only from an installed
//! `.desktop` file whose name matches the window's app id, so the launcher installs one for itself at start
//! (docs/release.md "Icons"). Both programs set the app id / X11 class [`APP_ID`], so the game's window groups under
//! the same entry.
//!
//! Files, under `$XDG_DATA_HOME` (default `~/.local/share`):
//! - `applications/opensow.desktop`, whose `Exec` is this launcher's own path;
//! - `icons/hicolor/{64x64,512x512}/apps/opensow.png`.
//!
//! Each file is written only when its content differs, so a moved folder updates the entry on the next start and an
//! unchanged one is left alone.

use std::io;
use std::path::{Path, PathBuf};

/// Wayland app id and X11 `WM_CLASS` of both windows, and the `.desktop` file's name.
pub const APP_ID: &str = "opensow";

const ICON_64: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon64.png"));
const ICON_512: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon512.png"));

/// `$XDG_DATA_HOME`, else `~/.local/share` (None without a home folder).
pub fn data_home() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    var("XDG_DATA_HOME").filter(|p| p.is_absolute()).or_else(|| var("HOME").map(|h| h.join(".local").join("share")))
}

/// One argument of an `Exec` line: quoted, with `"`, `` ` ``, `$` and `\` escaped, and `%` doubled (Desktop Entry
/// Specification, "The Exec key").
fn exec_arg(path: &Path) -> String {
    let mut s = String::from("\"");
    for c in path.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                s.push('\\');
                s.push(c);
            }
            '%' => s.push_str("%%"),
            _ => s.push(c),
        }
    }
    s.push('"');
    s
}

/// The `.desktop` file that starts `launcher`.
pub fn entry(launcher: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=OpenSOW\n\
         GenericName=Real-time strategy\n\
         Comment=Play State of War with the OpenSOW engine\n\
         Exec={}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Game;StrategyGame;\n\
         StartupWMClass={APP_ID}\n",
        exec_arg(launcher)
    )
}

/// Writes `bytes` to `path` unless it already holds them; true when it wrote.
fn write_if_changed(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

/// Installs the entry and the icons under `data` for the launcher at `launcher`; true when anything changed.
pub fn install_into(data: &Path, launcher: &Path) -> io::Result<bool> {
    let icons = data.join("icons").join("hicolor");
    let mut changed = false;
    for (size, png) in [("64x64", ICON_64), ("512x512", ICON_512)] {
        changed |= write_if_changed(&icons.join(size).join("apps").join(format!("{APP_ID}.png")), png)?;
    }
    let desktop = data.join("applications").join(format!("{APP_ID}.desktop"));
    changed |= write_if_changed(&desktop, entry(launcher).as_bytes())?;
    Ok(changed)
}

/// Installs the entry for the running launcher on Linux (no-op elsewhere); failures only go to stderr.
pub fn install() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let (Some(data), Ok(exe)) = (data_home(), std::env::current_exe()) else { return };
    if let Err(e) = install_into(&data, &exe) {
        eprintln!("desktop entry: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exec_line_is_quoted_and_escaped() {
        assert_eq!(exec_arg(Path::new("/opt/Open SOW/opensow-launcher")), "\"/opt/Open SOW/opensow-launcher\"");
        assert_eq!(exec_arg(Path::new("/a\"b$c`d\\e%f")), "\"/a\\\"b\\$c\\`d\\\\e%%f\"");
        let e = entry(Path::new("/home/u/OpenSOW/opensow-launcher"));
        assert!(e.starts_with("[Desktop Entry]\n"));
        assert!(e.contains("\nExec=\"/home/u/OpenSOW/opensow-launcher\"\n"));
        assert!(e.contains("\nIcon=opensow\n") && e.contains("\nStartupWMClass=opensow\n"));
    }

    #[test]
    fn install_writes_once_and_follows_a_moved_launcher() {
        let data = std::env::temp_dir().join(format!("opensow-desktop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let a = Path::new("/a/opensow-launcher");
        assert!(install_into(&data, a).unwrap(), "first start installs");
        assert!(!install_into(&data, a).unwrap(), "same place: nothing to write");
        let png = std::fs::read(data.join("icons/hicolor/512x512/apps/opensow.png")).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert!(install_into(&data, Path::new("/b/opensow-launcher")).unwrap(), "moved folder rewrites the entry");
        let desktop = std::fs::read_to_string(data.join("applications/opensow.desktop")).unwrap();
        assert!(desktop.contains("Exec=\"/b/opensow-launcher\""));
        std::fs::remove_dir_all(&data).unwrap();
    }
}
