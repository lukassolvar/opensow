//! Background music: the plain PCM tracks of `Data\Sounds\Music`, one at a time, looping.
//!
//! Which track plays where (calls of the exe's music player `0x407bb0`): `menu` on the main menu (`0x433c94`), `brief` or `brief2`
//! (a coin flip, `0x441cf9`) on the campaign map, `Level01..Level04` (random) when a mission starts (`0x42d6e0`),
//! `statistic` on the results screen and `uvod` on the credits. Notes: docs/audio.md.

use crate::intro::Sound;
use opensow_formats::{wav, GameDir};

pub struct Music {
    sound: Option<Sound>,
    pub current: String,
    /// The options menu's MUSIC row: switch and volume 0..=100.
    pub on: bool,
    pub volume: i32,
    /// The track asked for, started again when the switch comes back on.
    wanted: String,
    game: Option<GameDir>,
}

impl Music {
    pub fn new() -> Music {
        Music { sound: None, current: String::new(), on: true, volume: 100, wanted: String::new(), game: None }
    }

    /// Start `name` (a file stem such as `Menu`) unless it is already the current track; silent when the file or device is missing.
    pub fn play(&mut self, game: &GameDir, name: &str) {
        self.wanted = name.to_owned();
        self.game = Some(game.clone());
        if !self.on || (self.sound.is_some() && self.current.eq_ignore_ascii_case(name)) {
            return;
        }
        self.stop();
        self.current = name.to_owned();
        let Some(w) = game.resolve(&format!("Data\\Sounds\\Music\\{name}.wav")).and_then(|p| std::fs::read(p).ok()).and_then(|d| wav::parse(&d).ok()) else {
            eprintln!("no music track {name}");
            return;
        };
        self.sound = Sound::start_pcm(w.channels, w.sample_rate, w.pcm.iter().map(|&s| s as f32 / 32768.0).collect(), true);
        self.apply_volume();
    }

    fn apply_volume(&self) {
        if let Some(s) = &self.sound {
            s.player.set_volume(opensow_formats::sfx::volume(self.volume, None).map_or(0.0, |v| opensow_formats::sfx::gains(v, 0).0));
        }
    }

    /// The options menu changed the MUSIC row: set the volume, stop the track or start the wanted one again.
    pub fn set(&mut self, on: bool, volume: i32) {
        self.on = on;
        self.volume = volume;
        if !on {
            self.stop();
        } else if self.sound.is_none() && !self.wanted.is_empty() {
            if let Some(g) = self.game.clone() {
                let w = self.wanted.clone();
                self.play(&g, &w);
            }
        }
        self.apply_volume();
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
