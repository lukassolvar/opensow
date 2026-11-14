//! Background music: the plain PCM tracks of `Data\Sounds\Music`, one at a time, looping.
//!
//! Which track plays where (calls of the exe's music player `0x407bb0`): `menu` on the main menu (`0x433c94`), `brief` or `brief2`
//! (a coin flip, `0x441cf9`) on the campaign map, `Level01..Level04` (random) when a mission starts (`0x42d6e0`),
//! `statistic` on the results screen and `uvod` on the credits. Notes: docs/audio.md.

use crate::intro::Sound;
use opensow_formats::{wav, GameDir};

/// The background music: one track at a time, restarted when the output fails.
pub struct Music {
    sound: Option<Sound>,
    /// The track that is playing (its file stem), empty when none.
    pub current: String,
    /// The options menu's MUSIC row: switch and volume 0..=100.
    pub on: bool,
    /// Volume, 0..=100.
    pub volume: i32,
    /// The track asked for, started again when the switch comes back on.
    wanted: String,
    game: Option<GameDir>,
    /// The output died under the track: start the wanted track again once a new output opens.
    lost: bool,
}

impl Music {
    /// No track, music on, full volume.
    pub fn new() -> Music {
        Music { sound: None, current: String::new(), on: true, volume: 100, wanted: String::new(), game: None, lost: false }
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
        // no output yet (the sound server is not up, or none at all): `tick` starts the track once one opens
        self.lost = self.sound.is_none();
        self.apply_volume();
    }

    fn apply_volume(&self) {
        if let Some(s) = &self.sound {
            s.player.set_volume(crate::sfx::gain(true, self.volume));
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

    /// Called every frame (after `audio::tick`): when the output died under the track (the device was switched or unplugged, the
    /// sound server went away, the machine slept) the track is silent, so it starts again on the next output (`audio::mixer`
    /// opens one at once, then retries at most every 3 s).
    pub fn tick(&mut self) {
        if self.sound.as_ref().is_some_and(|s| s.generation != crate::audio::generation()) {
            eprintln!("music: the audio output was replaced, starting {} again", self.wanted);
            self.stop();
            self.lost = true;
        }
        if !self.lost || !self.on || self.sound.is_some() || self.wanted.is_empty() || crate::audio::mixer().is_none() {
            return;
        }
        let Some(game) = self.game.clone() else { return };
        let wanted = self.wanted.clone();
        self.play(&game, &wanted);
        self.lost = self.sound.is_none();
    }

    /// Stops the track.
    pub fn stop(&mut self) {
        if let Some(s) = self.sound.take() {
            s.player.stop();
        }
        self.current.clear();
    }
}

/// A cheap 0..n pick for track choice (the exe uses its visual random generator; the sequence need not match).
pub fn pick(n: u32) -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_micros()) % n.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_starts_again_when_the_output_is_replaced() {
        let Some(game) = GameDir::locate() else { return };
        let mut m = Music::new();
        m.play(&game, "uvod");
        if m.sound.is_none() {
            return; // no audio device here
        }
        // healthy: nothing happens
        m.tick();
        let g = m.sound.as_ref().unwrap().generation;
        assert!(m.current == "uvod" && !m.lost);
        // the output dies: the next tick starts the same track on a new one
        crate::audio::kill();
        m.tick();
        let s = m.sound.as_ref().expect("playing again");
        assert_eq!((s.generation, m.current.as_str(), m.lost), (g + 1, "uvod", false));
        // another track on the same output does not open a new one
        m.play(&game, "menu");
        assert_eq!(m.sound.as_ref().unwrap().generation, g + 1);
        // switched off: no track, and nothing comes back on its own
        m.set(false, 100);
        crate::audio::kill();
        m.tick();
        assert!(m.sound.is_none());
    }
}
