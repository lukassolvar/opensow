//! Parsers for the data files of State of War (Cypron Studios, 2001).
//!
//! OpenSOW ships no game data: everything here reads files from a
//! legitimately installed copy of the game (see [`GameDir`]).

pub mod advancer;
pub mod aim;
pub mod air;
pub mod aircraft;
pub mod anim;
pub mod buildings;
pub mod cheats;
pub mod clouds;
pub mod config;
pub mod cursor;
pub mod curves;
pub mod custommap;
pub mod effects;
pub mod endscreen;
pub mod facing;
pub mod fire;
pub mod font;
pub mod freecell;
pub mod indeo5;
pub mod infobox;
pub mod kinds;
pub mod lang;
pub mod launcher;
pub mod level;
pub mod map;
pub mod maptweak;
pub mod megafile;
pub mod messages;
pub mod mp2;
pub mod mpeg1;
pub mod msvideo1;
pub mod net;
pub mod netsync;
pub mod netwalk;
pub mod production;
pub mod projectile;
pub mod rally;
pub mod recoil;
pub mod rings;
pub mod rng;
pub mod rogon;
pub mod save;
pub mod saveobj;
pub mod savespark;
pub mod scatter;
pub mod sfx;
pub mod sparks;
pub mod sprite;
pub mod srf;
pub mod strings;
pub mod sysfont;
pub mod turretai;
pub mod units;
pub mod video;
pub mod walk;
pub mod warmonger;
pub mod wav;
pub mod wave;

mod reader;

use std::fmt;
use std::path::{Path, PathBuf};

pub use reader::Reader;

/// Why a file could not be read or parsed.
#[derive(Debug)]
pub enum Error {
    /// The file could not be opened or read.
    Io(std::io::Error),
    /// The file ended before a structure was complete.
    Truncated {
        /// The structure that was being read.
        what: &'static str,
        /// Byte offset in the file where the read started.
        offset: usize,
    },
    /// The file contents do not match the expected format.
    Invalid(String),
}

/// The result of reading or parsing a game file.
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
pub const DEFAULT_MAC_STEAM_DIR: &str =
    "Library/Application Support/Steam/Steam.AppBundle/Steam/Contents/MacOS/steamapps/content/app_748040/depot_748041/State of War";

/// Steam's app id for State of War; its depot `748041` holds both games' folders (docs/launcher.md).
const STEAM_APP: &str = "748040";

/// Steam's own folders (the ones with `steamapps`) in their default places: macOS, Linux (native, the legacy
/// `~/.steam` link, Flatpak) and Windows (`Program Files (x86)`/`Program Files`).
fn steam_roots(home: Option<&Path>, program_files: &[PathBuf]) -> Vec<PathBuf> {
    let in_home = ["Library/Application Support/Steam", ".local/share/Steam", ".steam/steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"];
    home.into_iter().flat_map(|h| in_home.map(|d| join_parts(h, d))).chain(program_files.iter().map(|p| p.join("Steam"))).collect()
}

/// `base` joined with each `/`-separated part of `rel` in turn, so Windows paths keep `\` throughout.
fn join_parts(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, part| p.join(part))
}

/// The `"path"` values of a `libraryfolders.vdf`: every Steam library on the machine.
fn vdf_values(text: &str, key: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|l| {
            let parts: Vec<&str> = l.trim().split('"').collect();
            (parts.len() == 5 && parts[1].eq_ignore_ascii_case(key) && parts[0].is_empty()).then(|| PathBuf::from(parts[3].replace("\\\\", "\\")))
        })
        .collect()
}

/// Install folders of the base game below Steam `roots`, in this order per root: the console download
/// (`steamapps/content/app_748040/depot_748041`, plus the macOS app-bundle one), then a normal library install
/// (`steamapps/common/<installdir>` from `appmanifest_748040.acf`, else any `common` folder named "State of War").
/// A folder whose `State of War` subfolder holds the game counts as that subfolder. Only folders with `Data` are returned.
fn steam_game_dirs(home: Option<&Path>, roots: &[PathBuf]) -> Vec<PathBuf> {
    let depot = format!("steamapps/content/app_{STEAM_APP}/depot_748041");
    let mut dirs: Vec<PathBuf> = home.map(|h| join_parts(h, DEFAULT_MAC_STEAM_DIR)).into_iter().collect();
    for root in roots {
        dirs.push(join_parts(root, &depot));
        let vdf = std::fs::read_to_string(join_parts(root, "steamapps/libraryfolders.vdf")).unwrap_or_default();
        for lib in std::iter::once(root.clone()).chain(vdf_values(&vdf, "path")) {
            let common = join_parts(&lib, "steamapps/common");
            let acf = std::fs::read_to_string(join_parts(&lib, &format!("steamapps/appmanifest_{STEAM_APP}.acf"))).unwrap_or_default();
            dirs.extend(vdf_values(&acf, "installdir").into_iter().map(|d| common.join(d)));
            let mut named: Vec<PathBuf> = std::fs::read_dir(&common)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                    n.contains("state of war") && !n.contains("warmonger")
                })
                .map(|e| e.path())
                .collect();
            named.sort();
            dirs.extend(named);
        }
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for d in dirs {
        let found = [d.join("State of War"), d].into_iter().find(|p| p.join("Data").is_dir() && !GameDir::is_warmonger_dir(p));
        if let Some(f) = found.filter(|f| !out.contains(f)) {
            out.push(f);
        }
    }
    out
}

/// Every base-game install Steam has put in its default places on this machine (macOS, Linux, Windows), best first.
pub fn default_steam_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let pf: Vec<PathBuf> = ["ProgramFiles(x86)", "ProgramFiles"].iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
    steam_game_dirs(home.as_deref(), &steam_roots(home.as_deref(), &pf))
}

/// Environment variable pointing at the Warmonger install (the folder containing `State of War Warmonger.exe` and `Data`).
pub const WARMONGER_DIR_ENV: &str = "OPENSOW_WARMONGER_DIR";

/// Which campaign the game plays. Warmonger is a standalone expansion that reuses every asset of the base game (docs/warmonger.md);
/// only its mission files `M01..M22` (map, tiles, picture, level script, briefing movie) and its skirmish maps (`*.net`) differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Edition {
    /// The base game, `State of War`.
    #[default]
    Original,
    /// The expansion, `State of War Warmonger`.
    Warmonger,
}

/// A located game installation, optionally with the Warmonger install next to it.
#[derive(Debug, Clone)]
pub struct GameDir {
    /// The install folder of the base game (the one that holds `Data`).
    pub root: PathBuf,
    /// The Warmonger install, when found; only its mission data is used.
    pub warmonger: Option<PathBuf>,
    /// The campaign whose missions `Data\Levels\M01..M22` resolve to.
    pub edition: Edition,
    /// The language chosen for this run (`set_language`); `None` = ask the launcher / `lang.dat`. Its `DiffData` pack
    /// (`SK`, `CZ`, ...) supplies speech, tutorial voice and pictures (docs/text.md).
    pub chosen: Option<lang::Language>,
}

/// `m01`..`m22` (any case): a campaign mission file stem.
fn is_mission_stem(stem: &str) -> bool {
    let b = stem.as_bytes();
    b.len() == 3
        && b[0].eq_ignore_ascii_case(&b'm')
        && b[1].is_ascii_digit()
        && b[2].is_ascii_digit()
        && (1..=22).contains(&((b[1] - b'0') * 10 + (b[2] - b'0')))
}

/// Resolve `game_path` below `root`, ignoring case.
fn resolve_in(root: &Path, game_path: &str) -> Option<PathBuf> {
    let mut cur = root.to_path_buf();
    for part in game_path.split(['\\', '/']).filter(|s| !s.is_empty()) {
        let exact = cur.join(part);
        cur = if exact.exists() {
            exact
        } else {
            std::fs::read_dir(&cur).ok()?.filter_map(|e| e.ok()).find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))?.path()
        };
    }
    Some(cur)
}

/// How a `Data\Levels\<file>` path relates to Warmonger.
enum Owner {
    /// `M01..M22.*`, the intro `mI` and the ending movie `mO`: the selected edition's own file.
    Mission,
    /// `*.net`: the base game's first, Warmonger's own maps are added.
    Skirmish,
    Shared,
}

fn owner(game_path: &str) -> Owner {
    let parts: Vec<&str> = game_path.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
    let [data, levels, file] = parts[..] else { return Owner::Shared };
    if !data.eq_ignore_ascii_case("data") || !levels.eq_ignore_ascii_case("levels") {
        return Owner::Shared;
    }
    match file.rsplit_once('.') {
        Some((stem, _)) if is_mission_stem(stem) || stem.eq_ignore_ascii_case("mo") || stem.eq_ignore_ascii_case("mi") => Owner::Mission,
        Some((_, ext)) if ext.eq_ignore_ascii_case("net") => Owner::Skirmish,
        _ => Owner::Shared,
    }
}

impl GameDir {
    /// A base install with no Warmonger next to it.
    pub fn new(root: PathBuf) -> GameDir {
        GameDir { root, warmonger: None, edition: Edition::Original, chosen: None }
    }

    /// Find the game via `OPENSOW_GAME_DIR`, falling back to the default Steam paths (macOS, Linux); the Warmonger install is looked up too.
    pub fn locate() -> Option<GameDir> {
        let saved = launcher::load();
        let candidates = std::env::var_os(GAME_DIR_ENV).map(PathBuf::from).into_iter().chain(saved.original.clone()).chain(default_steam_dirs());
        let mut game = candidates.filter(|p| p.join("Data").is_dir()).map(GameDir::new).next()?;
        // the environment wins over the launcher's saved folder
        for w in std::env::var_os(WARMONGER_DIR_ENV).map(PathBuf::from).into_iter().chain(saved.warmonger) {
            if game.set_warmonger(&w) {
                break;
            }
        }
        if game.warmonger.is_none() {
            game.warmonger = game.find_sibling_warmonger();
        }
        let l = game.language();
        game.set_language(l);
        Some(game)
    }

    /// Is `dir` a Warmonger install (a `Data\Levels` folder with `M01` files and an exe)?
    pub fn is_warmonger_dir(dir: &Path) -> bool {
        let levels = resolve_in(dir, "Data\\Levels");
        let has_m01 = levels
            .and_then(|l| std::fs::read_dir(l).ok())
            .is_some_and(|d| d.flatten().any(|e| e.file_name().to_string_lossy().to_ascii_lowercase().starts_with("m01.")));
        let has_exe = std::fs::read_dir(dir).is_ok_and(|d| {
            d.flatten().any(|e| {
                e.file_name().to_string_lossy().to_ascii_lowercase().contains("warmonger")
                    && e.file_name().to_string_lossy().to_ascii_lowercase().ends_with(".exe")
            })
        });
        has_m01 && has_exe
    }

    /// A folder next to the base install whose name says Warmonger (`State of War Warmonger`).
    pub fn find_sibling_warmonger(&self) -> Option<PathBuf> {
        std::fs::read_dir(self.root.parent()?)
            .ok()?
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().to_ascii_lowercase().contains("warmonger"))
            .map(|e| e.path())
            .find(|p| Self::is_warmonger_dir(p))
    }

    /// Use `dir` as the Warmonger install if it is one; returns whether it was accepted.
    pub fn set_warmonger(&mut self, dir: &Path) -> bool {
        let ok = Self::is_warmonger_dir(dir);
        if ok {
            self.warmonger = Some(dir.to_path_buf());
        }
        ok
    }

    /// A file of the Warmonger install itself, without the redirect to the base game (e.g. its loose
    /// `Data\Sprites2\extras\radar2.tsp`); `None` when the install is unknown or lacks the file.
    pub fn warmonger_file(&self, game_path: &str) -> Option<PathBuf> {
        resolve_in(self.warmonger.as_ref()?, game_path).filter(|p| p.exists())
    }

    /// Warmonger's executable (its land texts live there), when the install is known.
    pub fn warmonger_exe(&self) -> Option<PathBuf> {
        std::fs::read_dir(self.warmonger.as_ref()?)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) && p.to_string_lossy().to_ascii_lowercase().contains("warmonger"))
    }

    /// The language to use: the launcher's choice, else the install's `Data\lang.dat`, else English; a language the port does not
    /// offer (`Language::SUPPORTED`) counts as English.
    pub fn language(&self) -> lang::Language {
        self.chosen
            .or_else(|| launcher::load().language)
            .or_else(|| self.resolve("Data\\lang.dat").and_then(|p| std::fs::read(p).ok()).and_then(|d| lang::read_lang_dat(&d)))
            .filter(|l| l.supported())
            .unwrap_or(lang::Language::English)
    }

    /// Use the language pack of `l` for voices and pictures (`resolve` then prefers `Data\DiffData\<pack>\...`).
    pub fn set_language(&mut self, l: lang::Language) {
        self.chosen = Some(l);
    }

    /// The language pack's copy of `game_path` (`Data\Sounds\...`, `Data\Sprites2\...`), when there is one.
    fn localized(&self, game_path: &str) -> Option<PathBuf> {
        let dir = self.chosen.filter(|l| l.supported())?.diff_dir()?;
        let parts: Vec<&str> = game_path.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
        if parts.len() < 3 || !parts[0].eq_ignore_ascii_case("data") || !(parts[1].eq_ignore_ascii_case("sounds") || parts[1].eq_ignore_ascii_case("sprites2"))
        {
            return None;
        }
        resolve_in(&self.root, &format!("Data\\DiffData\\{dir}\\{}", parts[1..].join("\\"))).filter(|p| p.exists())
    }

    /// The base game's `Data` folder.
    pub fn data(&self) -> PathBuf {
        self.root.join("Data")
    }

    /// Resolve a path as the game writes it (`data\levels\m01.map`), ignoring case.
    /// The original targets Windows, so stored names rarely match on-disk case.
    /// Mission files `M01..M22` come from the selected edition's install; `*.net` maps fall back to Warmonger's.
    pub fn resolve(&self, game_path: &str) -> Option<PathBuf> {
        if let Some(local) = self.localized(game_path) {
            return Some(local);
        }
        if let Some(w) = &self.warmonger {
            match owner(game_path) {
                Owner::Mission if self.edition == Edition::Warmonger => return resolve_in(w, game_path).filter(|p| p.exists()),
                Owner::Skirmish => return resolve_in(&self.root, game_path).filter(|p| p.exists()).or_else(|| resolve_in(w, game_path)),
                _ => {}
            }
        }
        resolve_in(&self.root, game_path)
    }

    /// The ground a `*.net` skirmish / network map is played on. The `.net` file holds only the level script (header, buildings, units);
    /// its terrain (`.map`, `.srf`, `.til`, `.tmi`) is the campaign mission file `M<level id>` of the edition that owns the `.net`
    /// (checked on every shipped map: no building of the script sits on a blocked cell of that mission's map, and on no other).
    /// Returns a copy of `self` with that edition selected and the mission's name (`M07`).
    pub fn net_terrain(&self, net_map: &str, level_id: i32) -> (GameDir, String) {
        let path = format!("Data\\Levels\\{net_map}.net");
        let in_base = resolve_in(&self.root, &path).is_some_and(|p| p.exists());
        let in_warmonger = self.warmonger.as_ref().is_some_and(|w| resolve_in(w, &path).is_some_and(|p| p.exists()));
        let mut g = self.clone();
        if in_warmonger && !in_base {
            g.edition = Edition::Warmonger;
        } else if in_base {
            g.edition = Edition::Original;
        }
        (g, format!("M{level_id:02}"))
    }

    /// File names in `Data\Levels` as the selected edition sees them: the base folder without the missions when playing Warmonger,
    /// plus Warmonger's missions then, plus Warmonger's skirmish maps always.
    pub fn level_files(&self) -> Vec<String> {
        let names = |dir: Option<PathBuf>| -> Vec<String> {
            dir.and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()
        };
        let is_mission = |n: &str| n.rsplit_once('.').is_some_and(|(s, _)| is_mission_stem(s));
        let is_net = |n: &str| n.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case("net"));
        let warm = self.warmonger.as_ref().map(|w| names(resolve_in(w, "Data\\Levels")));
        let wm_edition = self.edition == Edition::Warmonger;
        let mut out: Vec<String> =
            names(resolve_in(&self.root, "Data\\Levels")).into_iter().filter(|n| !(wm_edition && warm.is_some() && is_mission(n))).collect();
        for n in warm.into_iter().flatten() {
            if (is_net(&n) && !out.iter().any(|o| o.eq_ignore_ascii_case(&n))) || (wm_edition && is_mission(&n)) {
                out.push(n);
            }
        }
        out
    }
}

#[cfg(test)]
mod edition_tests {
    use super::*;

    fn touch(root: &Path, rel: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, rel).unwrap();
    }

    fn installs(tag: &str) -> (PathBuf, GameDir) {
        let base = std::env::temp_dir().join(format!("opensow-edition-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (b, w) = (base.join("State of War"), base.join("State of War Warmonger"));
        for f in [
            "Data/Levels/M01.map",
            "Data/Levels/M05.map",
            "Data/Levels/M00.map",
            "Data/Levels/mO.sgv",
            "Data/Levels/mI.sgv",
            "Data/Levels/01-First Spark.net",
            "Data/Sprites2/Menu/x.tsp",
        ] {
            touch(&b, f);
        }
        for f in [
            "Data/Levels/m01.map",
            "Data/Levels/M02.map",
            "Data/Levels/00-the first step.net",
            "Data/Levels/M00.map",
            "Data/Levels/mO.sgv",
            "Data/Levels/MI.sgv",
            "State of War Warmonger.exe",
        ] {
            touch(&w, f);
        }
        (base, GameDir::new(b))
    }

    #[test]
    fn finds_the_sibling_and_overlays_only_mission_and_skirmish_files() {
        let (base, mut g) = installs("overlay");
        assert!(g.warmonger.is_none());
        g.warmonger = g.find_sibling_warmonger();
        assert!(g.warmonger.is_some(), "the folder next to the base install is found");
        let read = |g: &GameDir, p: &str| g.resolve(p).map(|p| std::fs::read_to_string(p).unwrap());
        // original edition: its own missions, Warmonger's skirmish map is added
        assert_eq!(read(&g, "Data\\Levels\\M01.map").as_deref(), Some("Data/Levels/M01.map"));
        assert!(g.resolve("Data\\Levels\\00-The First Step.net").is_some());
        assert!(g.resolve("Data\\Levels\\M02.map").is_none());
        g.edition = Edition::Warmonger;
        assert_eq!(read(&g, "data\\levels\\M01.map").as_deref(), Some("Data/Levels/m01.map"), "case-insensitive, from Warmonger");
        assert!(g.resolve("Data\\Levels\\M05.map").is_none(), "no Warmonger mission 5: never the base one");
        assert!(g.resolve("Data\\Levels\\M02.map").is_some());
        assert_eq!(read(&g, "Data\\Levels\\M00.map").as_deref(), Some("Data/Levels/M00.map"), "the tutorial stays the base game's");
        assert!(g.resolve("Data\\Sprites2\\Menu\\x.tsp").is_some(), "art is always the base game's");
        assert_eq!(read(&g, "Data\\Levels\\mO.sgv").as_deref(), Some("Data/Levels/mO.sgv"));
        assert_eq!(read(&g, "Data\\Levels\\mI.sgv").as_deref(), Some("Data/Levels/MI.sgv"), "Warmonger's own intro");
        g.edition = Edition::Original;
        assert!(g.resolve("Data\\Levels\\mO.sgv").is_some_and(|p| p.starts_with(&g.root)));
        g.edition = Edition::Warmonger;
        let mut files = g.level_files();
        files.sort();
        assert_eq!(files, ["00-the first step.net", "01-First Spark.net", "M00.map", "M02.map", "m01.map", "mI.sgv", "mO.sgv"]);
        g.edition = Edition::Original;
        let mut files = g.level_files();
        files.sort();
        assert_eq!(files, ["00-the first step.net", "01-First Spark.net", "M00.map", "M01.map", "M05.map", "mI.sgv", "mO.sgv"]);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn steam_libraries_are_searched_for_the_game() {
        let base = std::env::temp_dir().join(format!("opensow-steam-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (home, pf, lib) = (base.join("home"), base.join("pf"), base.join("D/SteamLibrary"));
        assert!(vdf_values("\"path\"\t\t\"D:\\\\Games\\\\Steam\"", "path") == [PathBuf::from("D:\\Games\\Steam")], "escaped backslashes");
        let roots = steam_roots(Some(&home), std::slice::from_ref(&pf));
        assert!(steam_game_dirs(Some(&home), &roots).is_empty());
        // Windows: a second library listed in libraryfolders.vdf, its manifest names the folder
        touch(&pf, "Steam/steamapps/libraryfolders.vdf");
        std::fs::write(
            pf.join("Steam/steamapps/libraryfolders.vdf"),
            format!("\"libraryfolders\"\n{{\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n", lib.display()),
        )
        .unwrap();
        touch(&lib, "steamapps/appmanifest_748040.acf");
        std::fs::write(lib.join("steamapps/appmanifest_748040.acf"), "\"AppState\"\n{\n\t\"installdir\"\t\t\"SOW\"\n}\n").unwrap();
        touch(&lib, "steamapps/common/SOW/State of War/Data/Levels/M01.map");
        touch(&lib, "steamapps/common/SOW/State of War Warmonger/Data/Levels/M01.map");
        touch(&lib, "steamapps/common/SOW/State of War Warmonger/State of War Warmonger.exe");
        let found = steam_game_dirs(Some(&home), &roots);
        assert_eq!(found, [lib.join("steamapps/common/SOW/State of War")]);
        assert!(GameDir::new(found[0].clone()).find_sibling_warmonger().is_some());
        // Linux: a Proton install found by name, and the console download wins over it
        touch(&home, ".local/share/Steam/steamapps/common/State of War/Data/x");
        touch(&home, ".local/share/Steam/steamapps/content/app_748040/depot_748041/State of War/Data/x");
        let found = steam_game_dirs(Some(&home), &roots);
        assert_eq!(found[0], home.join(".local/share/Steam/steamapps/content/app_748040/depot_748041/State of War"));
        assert_eq!(found[1], home.join(".local/share/Steam/steamapps/common/State of War"));
        assert_eq!(found.len(), 3);
        // macOS: the app-bundle depot comes first
        touch(&home, &format!("{DEFAULT_MAC_STEAM_DIR}/Data/x"));
        assert_eq!(steam_game_dirs(Some(&home), &roots)[0], home.join(DEFAULT_MAC_STEAM_DIR));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_folder_without_the_expansion_is_rejected() {
        let (base, mut g) = installs("reject");
        assert!(!g.set_warmonger(&g.root.clone()), "the base install is not Warmonger");
        assert!(g.set_warmonger(&base.join("State of War Warmonger")));
        let _ = std::fs::remove_dir_all(base);
    }
}
