//! Launcher settings (`opensow-launcher`): the language and the install folder of each game, kept in one small text file of the
//! per-user data folder (`launcher.cfg`, `name=value` lines). The game reads it too: [`crate::GameDir::locate`] uses the saved
//! folders when the environment variables are not set, and [`crate::GameDir::language`] the saved language before `lang.dat`.

use crate::lang::Language;
use std::path::PathBuf;

pub const FILE: &str = "launcher.cfg";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// Language of the texts and movies; `None` = whatever the install's `lang.dat` says.
    pub language: Option<Language>,
    /// Folder of `State of War` (contains `Data`).
    pub original: Option<PathBuf>,
    /// Folder of `State of War Warmonger`.
    pub warmonger: Option<PathBuf>,
}

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            match k.trim() {
                "language" => s.language = Language::from_code(v),
                "original" => s.original = Some(PathBuf::from(v)),
                "warmonger" => s.warmonger = Some(PathBuf::from(v)),
                _ => {}
            }
        }
        s
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        if let Some(l) = self.language {
            out += &format!("language={}\n", l.code());
        }
        if let Some(p) = &self.original {
            out += &format!("original={}\n", p.display());
        }
        if let Some(p) = &self.warmonger {
            out += &format!("warmonger={}\n", p.display());
        }
        out
    }
}

/// The folder of this port's own files: `$OPENSOW_SAVE_DIR`, else the per-user data folder.
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("OPENSOW_SAVE_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/OpenSOW")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join("OpenSOW")
    } else {
        home.join(".local/share/opensow")
    }
}

/// The saved settings (all unset when there is no file).
pub fn load() -> Settings {
    std::fs::read_to_string(data_dir().join(FILE)).map(|t| Settings::parse(&t)).unwrap_or_default()
}

pub fn save(s: &Settings) -> std::io::Result<()> {
    let d = data_dir();
    std::fs::create_dir_all(&d)?;
    std::fs::write(d.join(FILE), s.to_text())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_junk() {
        let s = Settings { language: Some(Language::Czech), original: Some("/g/State of War".into()), warmonger: None };
        assert_eq!(s.to_text(), "language=CZ\noriginal=/g/State of War\n");
        assert_eq!(Settings::parse(&s.to_text()), s);
        assert_eq!(Settings::parse("language=xx\nnonsense\n=\nwarmonger =  /w \n"), Settings { language: None, original: None, warmonger: Some("/w".into()) });
        assert_eq!(Settings::parse(""), Settings::default());
    }
}
