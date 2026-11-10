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
    /// Earliest time to try opening the audio device again after its stream failed.
    retry_at: Option<std::time::Instant>,
}

impl Music {
    pub fn new() -> Music {
        Music { sound: None, current: String::new(), on: true, volume: 100, wanted: String::new(), game: None, retry_at: None }
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

    /// Called every frame: when the output stream reported an error (the device was switched or unplugged, the machine slept)
    /// the track would play on silently, so a new stream is opened and the wanted track starts again (at most every 3 s).
    pub fn tick(&mut self) {
        let dead = self.sound.as_ref().is_some_and(|s| s.failed.load(std::sync::atomic::Ordering::Relaxed));
        let lost = self.on && self.sound.is_none() && !self.wanted.is_empty() && self.retry_at.is_some();
        if !(dead || lost) || self.retry_at.is_some_and(|t| std::time::Instant::now() < t) {
            return;
        }
        let Some(game) = self.game.clone() else { return };
        eprintln!("music: the audio device failed, reopening it ({})", self.wanted);
        self.stop();
        self.retry_at = Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
        let wanted = self.wanted.clone();
        self.play(&game, &wanted);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_stream_is_reopened_and_the_track_restarts() {
        let Some(game) = GameDir::locate() else { return };
        let mut m = Music::new();
        m.play(&game, "uvod");
        if m.sound.is_none() {
            return; // no audio device here
        }
        // healthy: nothing happens
        m.tick();
        assert!(m.sound.is_some() && m.current == "uvod" && m.retry_at.is_none());
        // the stream reports an error: the next tick opens a new one with the same track
        m.sound.as_ref().unwrap().failed.store(true, std::sync::atomic::Ordering::Relaxed);
        m.tick();
        let s2 = m.sound.as_ref().expect("a new stream");
        assert!(!s2.failed.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(m.current, "uvod");
        // and not again right away if that one fails too
        s2.failed.store(true, std::sync::atomic::Ordering::Relaxed);
        m.tick();
        assert!(m.sound.as_ref().unwrap().failed.load(std::sync::atomic::Ordering::Relaxed), "retry only after the delay");
        // switched off: no track
        m.set(false, 100);
        m.tick();
        assert!(m.sound.is_none());
    }
}
