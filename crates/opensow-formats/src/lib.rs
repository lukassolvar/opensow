//! Parsers for the data files of State of War (Cypron Studios, 2001).
//!
//! OpenSOW ships no game data: everything here reads files from a
//! legitimately installed copy of the game (see [`GameDir`]).

pub mod aim;
pub mod air;
pub mod aircraft;
pub mod anim;
pub mod facing;
pub mod fire;
pub mod font;
pub mod lang;
pub mod indeo5;
pub mod level;
pub mod advancer;
pub mod effects;
pub mod buildings;
pub mod config;
pub mod endscreen;
pub mod cursor;
pub mod curves;
pub mod sparks;
pub mod infobox;
pub mod messages;
pub mod production;
pub mod rally;
pub mod rng;
pub mod save;
pub mod saveobj;
pub mod sfx;
pub mod projectile;
pub mod recoil;
pub mod freecell;
pub mod netwalk;
pub mod rings;
pub mod rogon;
pub mod map;
pub mod net;
pub mod megafile;
pub mod mp2;
pub mod mpeg1;
pub mod msvideo1;
pub mod sprite;
pub mod srf;
pub mod strings;
pub mod units;
pub mod video;
pub mod walk;
pub mod wav;
pub mod wave;

mod reader;

use std::fmt;
use std::path::{Path, PathBuf};

pub use reader::Reader;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// The file ended before a structure was complete.
    Truncated { what: &'static str, offset: usize },
    /// The file contents do not match the expected format.
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Truncated { what, offset } => write!(f, "truncated {what} at offset {offset:#x}"),
            Error::Invalid(msg) => write!(f, "invalid data: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Environment variable pointing at the game's install directory
/// (the folder containing `State of War.exe` and `Data`).
pub const GAME_DIR_ENV: &str = "OPENSOW_GAME_DIR";

/// Default Steam install location on macOS (depot downloaded via the Steam console).
pub const DEFAULT_MAC_STEAM_DIR: &str = "Library/Application Support/Steam/Steam.AppBundle/Steam/Contents/MacOS/steamapps/content/app_748040/depot_748041/State of War";

/// A located game installation.
#[derive(Debug, Clone)]
pub struct GameDir {
    pub root: PathBuf,
}

impl GameDir {
    /// Find the game via `OPENSOW_GAME_DIR`, falling back to the default Steam path.
    pub fn locate() -> Option<GameDir> {
        let candidates = std::env::var_os(GAME_DIR_ENV).map(PathBuf::from).into_iter().chain(
            std::env::var_os("HOME").map(|h| Path::new(&h).join(DEFAULT_MAC_STEAM_DIR)),
        );
        candidates
            .filter(|p| p.join("Data").is_dir())
            .map(|root| GameDir { root })
            .next()
    }

    pub fn data(&self) -> PathBuf {
        self.root.join("Data")
    }

    /// Resolve a path as the game writes it (`data\levels\m01.map`), ignoring case.
    /// The original targets Windows, so stored names rarely match on-disk case.
    pub fn resolve(&self, game_path: &str) -> Option<PathBuf> {
        let mut cur = self.root.clone();
        for part in game_path.split(['\\', '/']).filter(|s| !s.is_empty()) {
            let exact = cur.join(part);
            cur = if exact.exists() {
                exact
            } else {
                std::fs::read_dir(&cur)
                    .ok()?
                    .filter_map(|e| e.ok())
                    .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))?
                    .path()
            };
        }
        Some(cur)
    }
}
