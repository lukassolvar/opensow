//! The players' own skirmish maps (docs/map-editor-ingame.md): the `maps` folder in the per-user data folder, its listing for the
//! skirmish screen's Custom tab and the map editor, loading and saving (`formats::custommap`), and where a map's ground comes from.

use opensow_formats::custommap::{self, CustomMap, Problem, Terrain};
use opensow_formats::{Edition, GameDir};
use std::path::{Path, PathBuf};

/// `$OPENSOW_MAPS_DIR`, else `maps` in the port's per-user data folder.
pub fn dir() -> PathBuf {
    std::env::var_os("OPENSOW_MAPS_DIR").map(PathBuf::from).unwrap_or_else(|| opensow_formats::launcher::data_dir().join("maps"))
}

/// One file of the folder.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The file.
    pub path: PathBuf,
    /// The file name without the extension (the stars' key, `custom/<stem>`).
    pub stem: String,
    /// The map's own name (the file stem when it has none).
    pub name: String,
    /// The parsed map, or why the file could not be read.
    pub map: Result<CustomMap, String>,
}

impl Entry {
    /// The map's key in the skirmish list (custom maps earn no stars).
    pub fn star_key(&self) -> String {
        format!("custom/{}", self.stem)
    }

    /// Why the map cannot be played, if it cannot: a broken file or a side without a headquarters.
    pub fn blocker(&self) -> Option<Blocker> {
        match &self.map {
            Err(e) => Some(Blocker::Broken(e.clone())),
            Ok(m) => m.problems().into_iter().find(|p| p.blocks_play()).map(|p| match p {
                Problem::NoHeadquarters(side) => Blocker::NoHeadquarters(side),
                Problem::NoAdvancer(_) => unreachable!(),
                p => Blocker::Mission(p),
            }),
        }
    }
}

/// Why a custom map cannot be played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocker {
    Broken(String),
    NoHeadquarters(usize),
    /// An escort map without pods or a goal (`custommap::Problem::NoPods`, `NoGoal`).
    Mission(Problem),
}

/// The maps in `folder`, by name (case-insensitive); unreadable files are listed too (their error shows in the list).
pub fn list_in(folder: &Path) -> Vec<Entry> {
    let mut out: Vec<Entry> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(custommap::EXTENSION)))
        .map(|path| {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let map = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|d| CustomMap::parse(&d).map_err(|e| e.to_string()));
            if let Err(e) = &map {
                eprintln!("custom map {}: {e}", path.display());
            }
            let name = map.as_ref().ok().map(|m| m.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| stem.clone());
            Entry { path, stem, name, map }
        })
        .collect();
    out.sort_by_key(|e| (e.name.to_lowercase(), e.stem.clone()));
    out
}

/// The path a map of this name is saved to.
pub fn path_for(folder: &Path, name: &str) -> PathBuf {
    folder.join(format!("{}.{}", custommap::file_stem(name), custommap::EXTENSION))
}

/// Writes `map` to its file in `folder` (created when missing).
pub fn save_in(folder: &Path, map: &CustomMap) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let path = path_for(folder, &map.name);
    std::fs::write(&path, map.to_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Reads and parses one map file.
pub fn load(path: &Path) -> Result<CustomMap, String> {
    let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    CustomMap::parse(&d).map_err(|e| e.to_string())
}

/// The install and mission name a terrain's files resolve under (`Scene::load_with`, `scene::thumbnail`).
pub fn terrain_source(game: &GameDir, t: Terrain) -> (GameDir, String) {
    let mut g = game.clone();
    g.edition = if t.warmonger && game.warmonger.is_some() { Edition::Warmonger } else { Edition::Original };
    (g, format!("M{:02}", t.mission))
}

/// Every terrain a map can be made on: the 22 missions of the base game, then Warmonger's when it is installed.
pub fn terrains(game: &GameDir) -> Vec<Terrain> {
    let mut out: Vec<Terrain> = (1..=22).map(|mission| Terrain { warmonger: false, mission }).collect();
    if game.warmonger.is_some() {
        out.extend((1..=22).map(|mission| Terrain { warmonger: true, mission }));
    }
    out.retain(|&t| {
        let (g, m) = terrain_source(game, t);
        g.resolve(&format!("Data\\Levels\\{m}.map")).is_some_and(|p| p.exists())
    });
    out
}

/// Shows the folder in the system's file manager (created first).
pub fn open_folder() {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(cmd).arg(&d).spawn() {
        eprintln!("cannot open {}: {e}", d.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("opensow-maps-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn saves_lists_and_reports_broken_files() {
        let d = scratch("list");
        let mut m = CustomMap::new("Žltá púšť", Terrain { warmonger: false, mission: 3 });
        let p = save_in(&d, &m).unwrap();
        assert_eq!(p.file_name().unwrap().to_string_lossy(), format!("zlta-pust.{}", custommap::EXTENSION));
        m.name = "Another".into();
        save_in(&d, &m).unwrap();
        std::fs::write(d.join(format!("junk.{}", custommap::EXTENSION)), b"nope").unwrap();
        std::fs::write(d.join("readme.txt"), b"not a map").unwrap();
        let l = list_in(&d);
        assert_eq!(l.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["Another", "junk", "Žltá púšť"]);
        assert!(matches!(l[1].blocker(), Some(Blocker::Broken(_))));
        assert_eq!(l[0].blocker(), Some(Blocker::NoHeadquarters(0)), "an empty map cannot be played");
        assert_eq!(l[2].star_key(), "custom/zlta-pust");
        assert_eq!(load(&p).unwrap().level.level_id(), 3);
        let _ = std::fs::remove_dir_all(&d);
    }
}
