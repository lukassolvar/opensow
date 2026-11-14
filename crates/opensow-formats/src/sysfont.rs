//! Finding the system fonts the launcher and the menus' GDI stand-in draw with (not in the exe: it asks GDI by face name).
//!
//! Callers list the files they prefer by full path (macOS, Windows, Debian/Ubuntu, Arch spellings). Distributions keep the same
//! files in other folders (Fedora `/usr/share/fonts/dejavu-sans-fonts/`, per-user `~/.local/share/fonts`, Windows' per-user
//! `%LOCALAPPDATA%\Microsoft\Windows\Fonts`), so a listed file that is not at its path is also looked up by its file name in
//! the font folders (docs/launcher.md).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// How deep the font folders are walked (`/usr/share/fonts/<package>/<subfolder>/file.ttf` is three levels).
const DEPTH: usize = 4;

/// The folders that hold system and user fonts on this machine, in the platform's search order.
fn font_dirs() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let home = env("HOME").or_else(|| env("USERPROFILE"));
    let mut dirs = Vec::new();
    if cfg!(windows) {
        dirs.extend(env("WINDIR").or_else(|| env("SystemRoot")).map(|w| w.join("Fonts")));
        dirs.extend(env("LOCALAPPDATA").map(|l| l.join("Microsoft").join("Windows").join("Fonts")));
    } else if cfg!(target_os = "macos") {
        dirs.extend(home.iter().map(|h| h.join("Library").join("Fonts")));
        dirs.extend(["/Library/Fonts", "/System/Library/Fonts"].map(PathBuf::from));
    } else {
        dirs.extend(env("XDG_DATA_HOME").or_else(|| home.as_ref().map(|h| h.join(".local").join("share"))).map(|d| d.join("fonts")));
        dirs.extend(home.iter().map(|h| h.join(".fonts")));
        let data = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        dirs.extend(data.split(':').filter(|s| !s.is_empty()).map(|d| Path::new(d).join("fonts")));
    }
    dirs
}

/// Every font file below `dirs` (sorted per folder, so the pick does not depend on the file system's order).
fn index(dirs: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                if depth > 0 {
                    walk(&p, depth - 1, out);
                }
            } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| ["ttf", "ttc", "otf"].iter().any(|x| e.eq_ignore_ascii_case(x))) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        walk(d, DEPTH, &mut out);
    }
    out
}

/// The font folders' files, read once per run.
fn system_index() -> &'static [PathBuf] {
    static INDEX: OnceLock<Vec<PathBuf>> = OnceLock::new();
    INDEX.get_or_init(|| index(&font_dirs()))
}

/// The file name of a listed path, whichever separator it uses (`C:\Windows\Fonts\arial.ttf` -> `arial.ttf`).
fn file_name(listed: &str) -> &str {
    listed.rsplit(['/', '\\']).next().unwrap_or(listed)
}

/// `candidates` resolved against `index`: each listed file at its own path, else the first file of that name in the index.
fn resolve(candidates: &[&str], index: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for c in candidates {
        let exact = Path::new(c);
        let found = if exact.is_file() {
            Some(exact.to_path_buf())
        } else {
            let name = file_name(c);
            index.iter().find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))).cloned()
        };
        if let Some(f) = found.filter(|f| !out.contains(f)) {
            out.push(f);
        }
    }
    out
}

/// The listed font files that exist on this machine, best first: each at its listed path or, failing that, under the same
/// file name in a font folder. The caller reads them in order and keeps the first that parses.
pub fn find(candidates: &[&str]) -> Vec<PathBuf> {
    resolve(candidates, system_index())
}

/// Plain regular sans-serif faces in `index` (no bold, italic, mono, condensed, ...): the last resort when no listed font exists.
fn plain_sans(index: &[PathBuf]) -> Vec<PathBuf> {
    const STYLED: [&str; 10] = ["bold", "italic", "oblique", "mono", "condensed", "narrow", "light", "thin", "black", "semi"];
    index
        .iter()
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            n.contains("sans") && !n.contains("serif") && !STYLED.iter().any(|s| n.contains(s))
        })
        .cloned()
        .collect()
}

/// Any plain sans-serif font on this machine, for UI that cannot work without text when none of its listed fonts is there.
pub fn any_sans() -> Vec<PathBuf> {
    plain_sans(system_index())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(tag: &str, files: &[&str]) -> PathBuf {
        let base = std::env::temp_dir().join(format!("opensow-sysfont-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for f in files {
            let p = base.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, f).unwrap();
        }
        base
    }

    #[test]
    fn a_listed_font_is_found_by_name_in_another_folder() {
        // Fedora's layout: one folder per font package
        let base = tree(
            "fedora",
            &["usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf", "usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf", "usr/share/fonts/README"],
        );
        let idx = index(&[base.join("usr/share/fonts")]);
        assert_eq!(idx.len(), 2, "only font files are indexed");
        // the listed paths must not exist on the machine running the test (a Debian host has the real DejaVu at the Debian path)
        let gone = |rel: &str| base.join("gone").join(rel).to_string_lossy().into_owned();
        let listed = [gone("Arial Unicode.ttf"), gone("truetype/dejavu/DejaVuSans.ttf"), gone("TTF/DejaVuSans.ttf")];
        let listed: Vec<&str> = listed.iter().map(String::as_str).collect();
        assert_eq!(resolve(&listed, &idx), [base.join("usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf")], "found once, by file name");
        let windows = format!("{}\\dejavusans-BOLD.TTF", gone("Windows\\Fonts"));
        assert_eq!(resolve(&[&windows], &idx).len(), 1, "Windows spelling and any case");
        assert!(resolve(&[&gone("LiberationSerif-Bold.ttf")], &idx).is_empty());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn the_listed_order_wins_and_exact_paths_come_first() {
        let base = tree("order", &["fonts/b/Second.ttf", "fonts/a/First.otf", "exact/Second.ttf"]);
        let idx = index(&[base.join("fonts")]);
        let exact = base.join("exact/Second.ttf");
        let listed = [exact.to_str().unwrap(), "/gone/First.otf"];
        assert_eq!(resolve(&listed, &idx), [exact.clone(), base.join("fonts/a/First.otf")]);
        let listed = ["/gone/First.otf", exact.to_str().unwrap()];
        assert_eq!(resolve(&listed, &idx)[0], base.join("fonts/a/First.otf"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn the_last_resort_is_a_plain_sans_face() {
        let base = tree(
            "sans",
            &[
                "f/noto/NotoSans-Bold.ttf",
                "f/noto/NotoSansMono-Regular.ttf",
                "f/noto/NotoSerif-Regular.ttf",
                "f/noto/NotoSans-Regular.ttf",
                "f/x/LiberationSans-Regular.ttf",
                "f/x/DejaVuSansCondensed.ttf",
            ],
        );
        let idx = index(&[base.join("f")]);
        assert_eq!(plain_sans(&idx), [base.join("f/noto/NotoSans-Regular.ttf"), base.join("f/x/LiberationSans-Regular.ttf")]);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn deep_folders_stop_at_the_depth_limit() {
        let base = tree("deep", &["f/1/2/3/4/A.ttf", "f/1/2/3/4/5/B.ttf"]);
        let idx = index(&[base.join("f")]);
        assert_eq!(idx, [base.join("f/1/2/3/4/A.ttf")]);
        let _ = std::fs::remove_dir_all(base);
    }
}
