//! Background music: the plain PCM tracks of `Data\Sounds\Music`, one at a time, looping.
//!
//! Which track plays where (calls of the exe's music player `0x407bb0`): `menu` on the main menu (`0x433c94`), `brief` or `brief2`
//! (a coin flip, `0x441c5c`) on the campaign map, `Level01..Level04` (random) when a mission starts (`0x42d6e0`),
//! `statistic` on the results screen and `uvod` on the credits. Notes: docs/audio.md.

use crate::intro::Sound;
use opensow_formats::{wav, GameDir};

pub struct Music {
    sound: Option<Sound>,
    pub current: String,
}

impl Music {
    pub fn new() -> Music {
        Music { sound: None, current: String::new() }
    }

    /// Start `name` (a file stem such as `Menu`) unless it is already the current track; silent when the file or device is missing.
    pub fn play(&mut self, game: &GameDir, name: &str) {
        if self.sound.is_some() && self.current.eq_ignore_ascii_case(name) {
            return;
        }
        self.stop();
        self.current = name.to_owned();
        let Some(w) = game.resolve(&format!("Data\\Sounds\\Music\\{name}.wav")).and_then(|p| std::fs::read(p).ok()).and_then(|d| wav::parse(&d).ok()) else {
            eprintln!("no music track {name}");
            return;
        };
        self.sound = Sound::start_pcm(w.channels, w.sample_rate, w.pcm.iter().map(|&s| s as f32 / 32768.0).collect(), true);
    }

    pub fn stop(&mut self) {
        if let Some(s) = self.sound.take() {
            s.player.stop();
        }
        self.current.clear();
    }
}

/// A cheap 0..n pick for track choice (the exe uses its visual random generator; the sequence need not match).
pub fn pick(n: u32) -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() / 1000) % n.max(1)
}
