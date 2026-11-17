//! The game's settings as the launcher edits them (docs/launcher.md, "Settings"): the original's `State of War_1.cfg`
//! (`formats::config::SettingsFile`, docs/options.md) and this port's `opensow.cfg` (`name=0|1` lines). The game opens both in
//! its current directory; the launcher starts it in the per-user data folder, so these are the files it reads.

use opensow_formats::config::SettingsFile;
use std::path::Path;

/// The original's settings file (the exe's own name).
pub const GAME_FILE: &str = "State of War_1.cfg";
/// This port's own settings (`options::EXTRA_FILE` in the game).
pub const EXTRA_FILE: &str = "opensow.cfg";

/// The stored size for NATIVE: the game draws at the window's pixel size (`options::NATIVE`).
pub const NATIVE: (u32, u32) = (0, 0);
/// The sizes the game's Options menu steps through (`options::RESOLUTIONS`: the exe's table, then this port's).
pub const RESOLUTIONS: [(u32, u32); 8] = [(640, 480), (800, 600), (1024, 768), (1600, 900), (1920, 1080), (2560, 1440), (3840, 2160), NATIVE];

/// This port's switches in the order of the game's Options page 2 (`options::OWN_ROWS`): file key and default (`Extra::default`).
pub const EXTRAS: [(&str, bool); 9] = [
    ("enhance_textures", true),
    ("soft_shadows", true),
    ("contact_shadows", true),
    ("explosion_glow", true),
    ("turret_shadows", true),
    ("enhance_sound", true),
    ("turret_range", true),
    ("map_zoom", true),
    ("swap_buttons", false),
];

/// One value the settings page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Resolution,
    /// CLOUDS detail 0..=4.
    Clouds,
    Radar,
    Quake,
    /// VBLANK.
    Vsync,
    HwCursor,
    /// KEYS SCROLL 0..=10.
    KeysScroll,
    /// MOUSE SCROLL 0..=10.
    MouseScroll,
    Music,
    Speech,
    Effects,
    MusicVolume,
    SpeechVolume,
    EffectsVolume,
    /// This port's switch `EXTRAS[k]`.
    Extra(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSettings {
    pub file: SettingsFile,
    pub extras: [bool; 9],
    /// Lines of `opensow.cfg` the launcher does not know, written back unchanged.
    pub other: Vec<String>,
}

impl Default for GameSettings {
    /// What the game starts with when neither file exists (`GameOptions::first_run`, `Extra::default`).
    fn default() -> Self {
        let mut file = SettingsFile::first_run();
        (file.options.width, file.options.height) = NATIVE;
        GameSettings { file, extras: EXTRAS.map(|(_, d)| d), other: vec![] }
    }
}

impl GameSettings {
    /// Reads `opensow.cfg`: known switches, everything else kept.
    pub fn parse_extras(&mut self, text: &str) {
        for line in text.lines() {
            let known = line.split_once('=').and_then(|(k, v)| Some((EXTRAS.iter().position(|(n, _)| *n == k.trim())?, v.trim())));
            match known {
                Some((i, v)) => self.extras[i] = v != "0",
                None if !line.trim().is_empty() => self.other.push(line.to_owned()),
                None => {}
            }
        }
    }

    pub fn extras_text(&self) -> String {
        let mut out: String = EXTRAS.iter().zip(self.extras).map(|((k, _), on)| format!("{k}={}\n", on as u8)).collect();
        for l in &self.other {
            out += l;
            out.push('\n');
        }
        out
    }

    /// The two files in `dir` (missing or broken ones read as the game's first-run values).
    pub fn load_from(dir: &Path) -> GameSettings {
        let mut s = GameSettings::default();
        if let Some(f) = std::fs::read(dir.join(GAME_FILE)).ok().and_then(|d| SettingsFile::parse(&d).ok()) {
            s.file = f;
        }
        if let Ok(t) = std::fs::read_to_string(dir.join(EXTRA_FILE)) {
            s.parse_extras(&t);
        }
        s
    }

    pub fn save_to(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(GAME_FILE), self.file.write())?;
        std::fs::write(dir.join(EXTRA_FILE), self.extras_text())
    }

    /// A switch's state; `None` for a value that is stepped.
    pub fn switch(&self, f: Field) -> Option<bool> {
        let (o, a) = (&self.file.options, &self.file.audio);
        Some(match f {
            Field::Radar => o.flag_a,
            Field::Quake => o.flag_b,
            Field::HwCursor => o.flag_c,
            Field::Vsync => o.flag_d,
            Field::Music => a.music_on,
            Field::Speech => a.voice_on,
            Field::Effects => a.fx_on,
            Field::Extra(k) => *self.extras.get(k)?,
            _ => return None,
        })
    }

    pub fn toggle(&mut self, f: Field) {
        let (o, a) = (&mut self.file.options, &mut self.file.audio);
        let b = match f {
            Field::Radar => &mut o.flag_a,
            Field::Quake => &mut o.flag_b,
            Field::HwCursor => &mut o.flag_c,
            Field::Vsync => &mut o.flag_d,
            Field::Music => &mut a.music_on,
            Field::Speech => &mut a.voice_on,
            Field::Effects => &mut a.fx_on,
            Field::Extra(k) if k < self.extras.len() => &mut self.extras[k],
            _ => return,
        };
        *b = !*b;
    }

    /// A stepped value: (value, min, max, step) as the game's arrows move it (docs/options.md).
    fn stepped(&mut self, f: Field) -> Option<(&mut u32, u32, u32, u32)> {
        let (o, a) = (&mut self.file.options, &mut self.file.audio);
        Some(match f {
            Field::Clouds => (&mut o.detail, 0, 4, 1),
            Field::KeysScroll => (&mut o.scroll_b, 0, 10, 1),
            Field::MouseScroll => (&mut o.scroll_a, 0, 10, 1),
            Field::MusicVolume => (&mut a.music_volume, 0, 100, 5),
            Field::SpeechVolume => (&mut a.voice_volume, 0, 100, 5),
            Field::EffectsVolume => (&mut a.fx_volume, 0, 100, 5),
            _ => return None,
        })
    }

    /// One arrow step (`dir` = -1 or +1); the resolution walks [`RESOLUTIONS`] like the game's arrows (a size not in the table
    /// goes to the first one).
    pub fn step(&mut self, f: Field, dir: i32) {
        if f == Field::Resolution {
            let o = &mut self.file.options;
            let i = RESOLUTIONS.iter().position(|&r| r == (o.width, o.height));
            let next = match i {
                Some(i) => (i as i32 + dir).clamp(0, RESOLUTIONS.len() as i32 - 1) as usize,
                None => 0,
            };
            (o.width, o.height) = RESOLUTIONS[next];
            return;
        }
        if let Some((v, lo, hi, s)) = self.stepped(f) {
            *v = (*v as i32 + dir * s as i32).clamp(lo as i32, hi as i32) as u32;
        }
    }

    /// The number a stepped value shows (`None` for the resolution and the switches).
    pub fn number(&self, f: Field) -> Option<u32> {
        self.clone().stepped(f).map(|(v, ..)| *v)
    }

    pub fn resolution(&self) -> (u32, u32) {
        (self.file.options.width, self.file.options.height)
    }

    /// Can the value go one step further that way?
    pub fn can_step(&self, f: Field, dir: i32) -> bool {
        let mut c = self.clone();
        c.step(f, dir);
        c != *self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extras_keep_unknown_lines_and_defaults() {
        let mut s = GameSettings::default();
        s.parse_extras("soft_shadows=0\nfuture_thing=1\nturret_shadows=1\n");
        assert_eq!(s.extras, [true, false, true, true, true, true, true, true, false]);
        assert_eq!(s.extras_text().lines().last(), Some("future_thing=1"));
        let mut back = GameSettings::default();
        back.parse_extras(&s.extras_text());
        assert_eq!(back, s);
    }

    #[test]
    fn steps_follow_the_games_arrows() {
        let mut s = GameSettings::default();
        assert_eq!(s.resolution(), NATIVE, "first run draws at the window's size");
        assert!(!s.can_step(Field::Resolution, 1));
        s.step(Field::Resolution, -1);
        assert_eq!(s.resolution(), (3840, 2160));
        s.file.options.width = 1234;
        s.step(Field::Resolution, 1);
        assert_eq!(s.resolution(), (640, 480), "an unknown size goes to the table's first");
        s.step(Field::MusicVolume, 1);
        assert_eq!(s.number(Field::MusicVolume), Some(100), "volumes stop at 100");
        s.step(Field::MusicVolume, -1);
        assert_eq!(s.number(Field::MusicVolume), Some(95));
        assert_eq!(s.number(Field::Clouds), Some(0));
        s.step(Field::Clouds, -1);
        assert_eq!(s.number(Field::Clouds), Some(0));
        assert_eq!(s.switch(Field::Radar), Some(true));
        s.toggle(Field::Radar);
        assert_eq!(s.switch(Field::Radar), Some(false));
        assert_eq!(s.switch(Field::Extra(4)), Some(true), "turret shadows are on by default");
        s.toggle(Field::Extra(4));
        assert_eq!(s.switch(Field::Extra(4)), Some(false));
    }

    #[test]
    fn files_round_trip_in_a_temp_folder() {
        let dir = std::env::temp_dir().join(format!("opensow-launcher-settings-{}", std::process::id()));
        let mut s = GameSettings::default();
        s.toggle(Field::Quake);
        s.step(Field::KeysScroll, 1);
        s.toggle(Field::Extra(0));
        s.save_to(&dir).unwrap();
        assert_eq!(GameSettings::load_from(&dir), s);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(GameSettings::load_from(&dir), GameSettings::default(), "no files = first run");
    }
}
