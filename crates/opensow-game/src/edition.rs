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
