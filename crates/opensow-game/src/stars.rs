//! Skirmish stars (not in the original game): a map earns a star the first time the player beats the computer on it with the map's
//! default setup (credits, research, air forces and turrets untouched); the star's colour is the hardest level beaten that way:
//! bronze Easy, silver Normal, gold Hard, ruby Brutal. Kept in one text file of the per-user data folder (`skirmish-stars.txt`,
//! one `map=level` line each); tests never touch the disk.

use crate::sim::Difficulty;
use std::collections::HashMap;

const FILE: &str = "skirmish-stars.txt";

/// Star colours (RGB): bronze, silver, gold, ruby, in [`Difficulty::ALL`] order.
pub const COLOURS: [(u8, u8, u8); 4] = [(205, 127, 50), (205, 212, 224), (255, 208, 40), (230, 25, 90)];

pub fn colour(d: Difficulty) -> (u8, u8, u8) {
    COLOURS[rank(d)]
}

fn rank(d: Difficulty) -> usize {
    Difficulty::ALL.iter().position(|&x| x == d).unwrap_or(1)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stars {
    best: HashMap<String, Difficulty>,
}

fn key(map: &str) -> String {
    map.trim().to_lowercase()
}

impl Stars {
    /// The saved stars (none in tests or without a file).
    pub fn load() -> Stars {
        if cfg!(test) {
            return Stars::default();
        }
        let text = std::fs::read_to_string(opensow_formats::launcher::data_dir().join(FILE)).unwrap_or_default();
        Stars::parse(&text)
    }

    pub fn parse(text: &str) -> Stars {
        let best = text
            .lines()
            .filter_map(|l| {
                let (map, level) = l.rsplit_once('=')?;
                let d = Difficulty::ALL.into_iter().find(|d| d.name().eq_ignore_ascii_case(level.trim()))?;
                Some((key(map), d))
            })
            .collect();
        Stars { best }
    }

    pub fn to_text(&self) -> String {
        let mut lines: Vec<String> = self.best.iter().map(|(m, d)| format!("{m}={}", d.name())).collect();
        lines.sort();
        lines.join("\n") + "\n"
    }

    /// The hardest level beaten on `map` with default settings.
    pub fn get(&self, map: &str) -> Option<Difficulty> {
        self.best.get(&key(map)).copied()
    }

    /// Notes a win with default settings; true when it earned a new or a better star.
    pub fn record(&mut self, map: &str, d: Difficulty) -> bool {
        if self.get(map).is_some_and(|b| rank(b) >= rank(d)) {
            return false;
        }
        self.best.insert(key(map), d);
        true
    }

    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        let dir = opensow_formats::launcher::data_dir();
        if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(FILE), self.to_text())).is_err() {
            eprintln!("could not save the skirmish stars");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_harder_win_upgrades_the_star_and_an_easier_one_never_downgrades_it() {
        let mut s = Stars::default();
        assert_eq!(s.get("01-First Spark"), None);
        assert!(s.record("01-First Spark", Difficulty::Normal));
        assert!(!s.record("01-first spark", Difficulty::Easy), "keys ignore case; easier does not count");
        assert!(!s.record("01-First Spark", Difficulty::Normal));
        assert!(s.record("01-First Spark", Difficulty::Brutal));
        assert_eq!(s.get("01-First Spark"), Some(Difficulty::Brutal));
        assert_eq!(s.get("02-Ice Blizzard"), None);
    }

    #[test]
    fn the_file_round_trips_and_ignores_junk() {
        let mut s = Stars::default();
        s.record("03-Gold Rush", Difficulty::Hard);
        s.record("09-The First Step", Difficulty::Easy);
        assert_eq!(Stars::parse(&s.to_text()), s);
        assert_eq!(Stars::parse("garbage\nx=Nightmare\n04-great crush = hard\n").get("04-Great Crush"), Some(Difficulty::Hard));
    }

    #[test]
    fn colours_follow_the_levels() {
        assert_ne!(colour(Difficulty::Hard), colour(Difficulty::Normal));
    }
}
