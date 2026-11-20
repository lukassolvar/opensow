//! The two campaigns (docs/warmonger.md): which one was selected last, remembered in a one-line file of the per-user data folder
//! (tests never touch the disk). The Warmonger install itself is found next to the base game or via `OPENSOW_WARMONGER_DIR` /
//! `--warmonger`; a later launcher will set up all paths.

use opensow_formats::Edition;
use std::path::PathBuf;

fn dir() -> PathBuf {
    opensow_formats::launcher::data_dir()
}

const EDITION_FILE: &str = "edition.txt";

fn read(name: &str) -> Option<String> {
    let s = std::fs::read_to_string(dir().join(name)).ok()?;
    Some(s.trim().to_owned()).filter(|s| !s.is_empty())
}

fn write(name: &str, text: &str) {
    if cfg!(test) {
        return;
    }
    let d = dir();
    if std::fs::create_dir_all(&d).and_then(|_| std::fs::write(d.join(name), text)).is_err() {
        eprintln!("could not remember {name}");
    }
}

/// The edition of the last session (Warmonger only counts while its install is known).
pub fn saved() -> Edition {
    if cfg!(test) {
        return Edition::Original;
    }
    if read(EDITION_FILE).as_deref() == Some("warmonger") {
        Edition::Warmonger
    } else {
        Edition::Original
    }
}

/// Remembers the chosen campaign for the next start.
pub fn remember(e: Edition) {
    write(EDITION_FILE, if e == Edition::Warmonger { "warmonger" } else { "original" });
}

const INTROS_FILE: &str = "intros.txt";

/// Whose intro was played already (original, Warmonger), one edition per line of the file.
pub fn intros_seen() -> [bool; 2] {
    if cfg!(test) {
        return [false; 2];
    }
    intros_seen_in(&dir())
}

/// Remembers that this edition's intro was played, so it never plays again.
pub fn remember_intro(e: Edition) {
    if !cfg!(test) {
        remember_intro_in(&dir(), e);
    }
}

fn intros_seen_in(d: &std::path::Path) -> [bool; 2] {
    let s = std::fs::read_to_string(d.join(INTROS_FILE)).unwrap_or_default();
    let seen = |name| s.lines().any(|l| l.trim() == name);
    [seen("original"), seen("warmonger")]
}

fn remember_intro_in(d: &std::path::Path, e: Edition) {
    let mut seen = intros_seen_in(d);
    seen[usize::from(e == Edition::Warmonger)] = true;
    let names: Vec<_> = ["original", "warmonger"].into_iter().zip(seen).filter(|(_, s)| *s).map(|(n, _)| n).collect();
    if std::fs::create_dir_all(d).and_then(|_| std::fs::write(d.join(INTROS_FILE), names.join("\n"))).is_err() {
        eprintln!("could not remember {INTROS_FILE}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_played_intro_stays_played_after_a_restart() {
        let d = std::env::temp_dir().join(format!("opensow-intros-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(intros_seen_in(&d), [false, false], "a new player has seen none");
        remember_intro_in(&d, Edition::Warmonger);
        assert_eq!(intros_seen_in(&d), [false, true], "read back as the next start does");
        remember_intro_in(&d, Edition::Original);
        assert_eq!(intros_seen_in(&d), [true, true], "the second one keeps the first");
        let _ = std::fs::remove_dir_all(&d);
    }
}
