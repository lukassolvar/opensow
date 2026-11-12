//! The game's two settings files, decoded from `State of War.exe` (`COptions` vtable `0x48fdcc`, `Serialize` `0x4536d0`, loader
//! `0x452c30`, saver `0x453310`; `CPassedLevels` vtable `0x48fe98`, `Serialize` `0x456ac0`). Both are MFC `CArchive` streams
//! whose first words are `u16 version = 1, u16 magic = 0x1d6c`. Notes: docs/config.md.
//!
//! * `State of War_1.cfg` (next to the exe; `State of War Warmonger` has its own name) holds one [`Options`] record followed by
//!   the audio object's [`Audio`] record (both written by `0x453310`, read by `0x452c30`): a [`SettingsFile`].
//! * `Savegame\player.cfg` holds the [`PassedLevels`] record, written as a 313-byte obfuscated block.

use crate::{Error, Reader, Result};

/// A record header `(version, magic)`: version 1, magic `0x1d6c`.
pub const HEADER: (u16, u16) = (1, 0x1d6c);

/// The per-player settings object (`0x28` bytes at `0x6bf710 + 0x28 * player`). Field offsets in the comments are the in-memory
/// ones; the stream writes them in this order: `+0xc +0x14 +0x18 +0x1c +0x20`, bools `+8 +9 +0x10 +0x11`, then `+0x24`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// `+8`: toggled by the first option button.
    pub flag_a: bool,
    /// `+9`: second option button.
    pub flag_b: bool,
    /// `+0x10`.
    pub flag_c: bool,
    /// `+0x11`.
    pub flag_d: bool,
    /// `+0xc`, 0..=4: effect detail (`0x453060` caps the cloud list at 0, 13, 32, 45 or 70 objects).
    pub detail: u32,
    /// `+0x14`, 0..=10: a scroll speed (the map scroll step is multiplied by it, `0x416b8f`).
    pub scroll_a: u32,
    /// `+0x18`, 0..=10: the other scroll speed (`0x415ef4`).
    pub scroll_b: u32,
    /// `+0x1c`, `+0x20`: screen size (defaults 1920 x 1080; the options menu offers 640x480 .. 1920x1080).
    pub width: u32,
    /// `+0x20`: screen height.
    pub height: u32,
    /// `+0x24`: language id 0..=7 (`lang.rs`; selects `data\diffdata\<SK|CZ|PL|HU|RU>\`).
    pub language: u32,
}

impl Default for Options {
    /// The constructor `0x452a90`'s defaults for the fields it sets; the rest start at zero. The game's first run uses
    /// [`Options::first_run`] instead.
    fn default() -> Options {
        Options { flag_a: false, flag_b: false, flag_c: false, flag_d: false, detail: 0, scroll_a: 0, scroll_b: 0, width: 0x780, height: 0x438, language: 0 }
    }
}

impl Options {
    /// What the loader `0x452c30` stores when `State of War_1.cfg` is missing or unreadable (`0x452fd1..0x453000`): radar and
    /// quake on, clouds off (detail 0), hardware cursor off, vblank wait on, mouse scroll speed 3, keys scroll speed 5,
    /// size 1024 x 768 (not the constructor's 1920 x 1080), language unchanged (0).
    pub fn first_run() -> Options {
        Options { flag_a: true, flag_b: true, flag_c: false, flag_d: true, detail: 0, scroll_a: 3, scroll_b: 5, width: 0x400, height: 0x300, language: 0 }
    }

    /// Parses an [`Options`] record.
    pub fn parse(data: &[u8]) -> Result<Options> {
        Options::read(&mut Reader::new(data))
    }

    /// Reads the record at the reader's position (the settings file continues with [`Audio`]).
    pub fn read(r: &mut Reader) -> Result<Options> {
        let head = (r.u16("version")?, r.u16("magic")?);
        if head != HEADER {
            return Err(Error::Invalid(format!("options header {head:?}")));
        }
        let detail = r.u32("detail")?;
        let scroll_a = r.u32("scroll a")?;
        let scroll_b = r.u32("scroll b")?;
        let width = r.u32("width")?;
        let height = r.u32("height")?;
        let (flag_a, flag_b, flag_c, flag_d) = (r.u8("flag")? != 0, r.u8("flag")? != 0, r.u8("flag")? != 0, r.u8("flag")? != 0);
        let language = r.u32("language")?;
        Ok(Options { flag_a, flag_b, flag_c, flag_d, detail, scroll_a, scroll_b, width, height, language })
    }

    /// Encodes the record as the exe writes it.
    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(HEADER.0.to_le_bytes());
        out.extend(HEADER.1.to_le_bytes());
        for v in [self.detail, self.scroll_a, self.scroll_b, self.width, self.height] {
            out.extend(v.to_le_bytes());
        }
        out.extend([self.flag_a as u8, self.flag_b as u8, self.flag_c as u8, self.flag_d as u8]);
        out.extend(self.language.to_le_bytes());
        out
    }

    /// Maximum number of smoke/cloud objects for a detail level (`0x453060`: 0 -> none, 1..=4 -> 13, 32, 45, 70).
    pub fn cloud_cap(&self) -> usize {
        [0, 13, 32, 45, 70].get(self.detail as usize).copied().unwrap_or(0)
    }
}

/// The audio object `0x6b3d38`'s part of the settings file (`Serialize` `0x407ff0`, constructor defaults `0x408510`): header
/// `u16 1, u16 0x6121`, then six `u32`: the music, speech and effect volumes (`+0x50/+0x54/+0x58`, clamped to 0..=100 when
/// loaded), and the music, speech and effect switches (`+0x5c/+0x5d/+0x5e`, any non-zero value is on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audio {
    /// Music volume, 0..=100.
    pub music_volume: u32,
    /// Speech volume, 0..=100.
    pub voice_volume: u32,
    /// Sound effect volume, 0..=100.
    pub fx_volume: u32,
    /// Music switch.
    pub music_on: bool,
    /// Speech switch.
    pub voice_on: bool,
    /// Sound effect switch.
    pub fx_on: bool,
}

/// The audio record's header `(version, magic)`.
pub const AUDIO_HEADER: (u16, u16) = (1, 0x6121);

impl Default for Audio {
    /// `0x408510`: all volumes 100, music, speech and effects on (also what the first run uses).
    fn default() -> Audio {
        Audio { music_volume: 100, voice_volume: 100, fx_volume: 100, music_on: true, voice_on: true, fx_on: true }
    }
}

impl Audio {
    /// Reads an [`Audio`] record, clamping the volumes to 0..=100.
    pub fn read(r: &mut Reader) -> Result<Audio> {
        let head = (r.u16("audio version")?, r.u16("audio magic")?);
        if head != AUDIO_HEADER {
            return Err(Error::Invalid(format!("audio header {head:?}")));
        }
        let mut vol = |what| r.i32(what).map(|v| v.clamp(0, 100) as u32);
        let (music_volume, voice_volume, fx_volume) = (vol("music volume")?, vol("voice volume")?, vol("fx volume")?);
        let (music_on, voice_on, fx_on) = (r.u32("music switch")? != 0, r.u32("voice switch")? != 0, r.u32("fx switch")? != 0);
        Ok(Audio { music_volume, voice_volume, fx_volume, music_on, voice_on, fx_on })
    }

    /// Encodes the record as the exe writes it.
    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(AUDIO_HEADER.0.to_le_bytes());
        out.extend(AUDIO_HEADER.1.to_le_bytes());
        for v in [self.music_volume, self.voice_volume, self.fx_volume, self.music_on as u32, self.voice_on as u32, self.fx_on as u32] {
            out.extend(v.to_le_bytes());
        }
        out
    }
}

/// The whole `State of War_1.cfg`: the [`Options`] record, then the [`Audio`] record (`0x453310` / `0x452c30`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFile {
    /// The per-player settings record.
    pub options: Options,
    /// The audio record.
    pub audio: Audio,
}

impl SettingsFile {
    /// Missing or unreadable file: `0x452fd1` stores [`Options::first_run`] and `0x408510` the [`Audio`] defaults.
    pub fn first_run() -> SettingsFile {
        SettingsFile { options: Options::first_run(), audio: Audio::default() }
    }

    /// Parses the whole settings file.
    pub fn parse(data: &[u8]) -> Result<SettingsFile> {
        let mut r = Reader::new(data);
        let options = Options::read(&mut r)?;
        let audio = Audio::read(&mut r)?;
        Ok(SettingsFile { options, audio })
    }

    /// Encodes the whole settings file.
    pub fn write(&self) -> Vec<u8> {
        let mut out = self.options.write();
        out.extend(self.audio.write());
        out
    }
}

/// Number of campaign levels in the progress block.
pub const LEVELS: usize = 50;
/// Length of the `CPassedLevels` block after the header: 79 + 20 random bytes, 1 balancing byte, 50 x (state byte, random
/// byte), 113 random bytes.
pub const BLOCK_LEN: usize = 79 + 20 + 1 + 2 * LEVELS + 113;

/// The progress block of `player.cfg`. The exe hides the 50 level states (`0` locked, `1` passed, `2` available and blinking on the map - the object's bytes
/// `+4..+0x36`) among random bytes: each state byte is `base(state) + 0x86 * index` (mod 256) with base 0x17, 0x2d, 0x4c
/// for states 0, 1, 2, followed by one random byte; the 20 random bytes before it plus the balancing byte add up to `0xac`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassedLevels {
    /// One state per level: 0 locked, 1 passed, 2 available.
    pub states: [u8; LEVELS],
}

const BASES: [u8; 3] = [0x17, 0x2d, 0x4c];

/// What `Pass` decided (`0x4565a0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passed {
    /// Back to the world map (land states updated).
    Map,
    /// Mission 22 was passed: the exe goes to state 2 (the ending) instead of the map.
    Ending,
}

/// The lands (missions) in the order `Pass` unlocks them: the first one still at state 0 becomes available.
pub const UNLOCK_ORDER: [usize; 18] = [6, 4, 7, 3, 10, 11, 12, 13, 14, 15, 5, 16, 17, 18, 19, 8, 20, 21];

/// Warmonger's unlock order (the exe checks them in this sequence).
pub const WARMONGER_UNLOCK_ORDER: [usize; 6] = [4, 6, 7, 11, 12, 20];

impl PassedLevels {
    /// The object after a failed load (`0x456a6f`): lands 1, 2 and 9 are available (state 2), everything else locked.
    pub fn initial() -> PassedLevels {
        let mut states = [0u8; LEVELS];
        states[1] = 2;
        states[2] = 2;
        states[9] = 2;
        PassedLevels { states }
    }

    /// The Warmonger progress at its start (ctor `0x450f3f`): lands 1, 2 and 3 are available.
    pub fn initial_warmonger() -> PassedLevels {
        let mut states = [0u8; LEVELS];
        states[1..=3].fill(2);
        PassedLevels { states }
    }

    /// Warmonger's `Pass` (`0x450b00`): the land becomes passed; if land 15 is passed land 22 opens, else when lands 1, 2, 3, 4, 6,
    /// 7, 11, 12 and 20 are all passed land 15 opens, else the first locked land of 4, 6, 7, 11, 12, 20 does. Land 22 ends the campaign.
    pub fn pass_warmonger(&mut self, level: usize) -> Passed {
        if level == 22 {
            return Passed::Ending;
        }
        if self.states.get(level) == Some(&2) {
            self.states[level] = 1;
            if self.states[15] == 1 {
                self.states[22] = 2;
            } else if [1, 2, 3, 4, 6, 7, 11, 12, 20].iter().all(|&l| self.states[l] == 1) {
                self.states[15] = 2;
            } else if let Some(&l) = WARMONGER_UNLOCK_ORDER.iter().find(|&&l| self.states[l] == 0) {
                self.states[l] = 2;
            }
        }
        Passed::Map
    }

    /// `CPassedLevels::Pass(level)` (`0x4565a0`), `level` = 1..=22: an available land (2) becomes passed (1), then the next
    /// land of [`UNLOCK_ORDER`] opens; when lands 1..=20 are all passed land 22 opens instead. Mission 22 ends the campaign.
    pub fn pass(&mut self, level: usize) -> Passed {
        if level == 22 {
            return Passed::Ending;
        }
        if self.states.get(level) == Some(&2) {
            self.states[level] = 1;
            if (1..=20).all(|l| self.states[l] == 1) {
                self.states[22] = 2;
            } else if let Some(&l) = UNLOCK_ORDER.iter().find(|&&l| self.states[l] == 0) {
                self.states[l] = 2;
            }
        }
        Passed::Map
    }

    /// `0x456720`: merge another progress record into this one (1 wins over 2 wins over 0).
    pub fn merge(&mut self, other: &PassedLevels) {
        for (a, b) in self.states.iter_mut().zip(&other.states) {
            *a = if *a == 1 || *b == 1 {
                1
            } else if *a == 2 || *b == 2 {
                2
            } else {
                0
            };
        }
    }

    /// Parses the progress block of `player.cfg`.
    pub fn parse(data: &[u8]) -> Result<PassedLevels> {
        let mut r = Reader::new(data);
        let head = (r.u16("version")?, r.u16("magic")?);
        if head != HEADER {
            return Err(Error::Invalid(format!("player.cfg header {head:?}")));
        }
        r.bytes(79, "filler")?;
        let sum = r.bytes(20, "check bytes")?.iter().fold(0u8, |a, &b| a.wrapping_add(b));
        let fix = r.u8("balance")?;
        if sum.wrapping_add(fix) != 0xac {
            return Err(Error::Invalid("player.cfg checksum bytes do not add up to 0xac".into()));
        }
        let mut states = [0u8; LEVELS];
        for (i, s) in states.iter_mut().enumerate() {
            let v = r.u8("state")?.wrapping_sub(0x86u8.wrapping_mul(i as u8));
            *s = BASES.iter().position(|&b| b == v).ok_or_else(|| Error::Invalid(format!("level {i} state byte {v:#x}")))? as u8;
            r.u8("filler")?;
        }
        r.bytes(113, "filler")?;
        Ok(PassedLevels { states })
    }

    /// Writes the block with `fill` supplying the random bytes (the exe uses its visual generator `0x437cd0`).
    pub fn write(&self, mut fill: impl FnMut() -> u8) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(HEADER.0.to_le_bytes());
        out.extend(HEADER.1.to_le_bytes());
        out.extend((0..79).map(|_| fill()).collect::<Vec<_>>());
        let check: Vec<u8> = (0..20).map(|_| fill()).collect();
        let sum = check.iter().fold(0u8, |a, &b| a.wrapping_add(b));
        out.extend(&check);
        out.push(0xacu8.wrapping_sub(sum));
        for (i, &s) in self.states.iter().enumerate() {
            out.push(BASES[s.min(2) as usize].wrapping_add(0x86u8.wrapping_mul(i as u8)));
            out.push(fill());
        }
        out.extend((0..113).map(|_| fill()).collect::<Vec<_>>());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_round_trip() {
        let o =
            Options { flag_a: true, flag_b: false, flag_c: true, flag_d: true, detail: 3, scroll_a: 7, scroll_b: 10, width: 1024, height: 768, language: 2 };
        let bytes = o.write();
        assert_eq!(bytes.len(), 4 + 20 + 4 + 4);
        assert_eq!(Options::parse(&bytes).unwrap(), o);
        assert_eq!(o.cloud_cap(), 45);
        assert_eq!(Options::default().width, 1920);
        assert!(Options::parse(&bytes[..10]).is_err());
        let mut bad = bytes.clone();
        bad[2] = 0;
        assert!(Options::parse(&bad).is_err());
    }

    #[test]
    fn warmonger_progress_follows_the_exes_chain() {
        let mut p = PassedLevels::initial_warmonger();
        assert_eq!(&p.states[1..=4], &[2, 2, 2, 0]);
        for l in [1, 2, 3] {
            p.pass_warmonger(l);
        }
        assert_eq!((p.states[3], p.states[4]), (1, 2), "each pass opens the next of 4, 6, 7, 11, 12, 20");
        for l in [4, 6, 7, 11, 12] {
            assert_eq!(p.pass_warmonger(l), Passed::Map);
        }
        assert_eq!((p.states[20], p.states[15]), (2, 0));
        p.pass_warmonger(20);
        assert_eq!(p.states[15], 2, "land 15 opens when the nine before it are passed");
        p.pass_warmonger(15);
        assert_eq!(p.states[22], 2);
        assert_eq!(p.pass_warmonger(22), Passed::Ending);
        assert_eq!(p.states[5], 0, "lands without a Warmonger mission never open");
    }

    #[test]
    fn first_run_values_and_the_audio_record() {
        let f = SettingsFile::first_run();
        let o = &f.options;
        assert_eq!(
            (o.flag_a, o.flag_b, o.flag_c, o.flag_d, o.detail, o.scroll_a, o.scroll_b, o.width, o.height),
            (true, true, false, true, 0, 3, 5, 1024, 768)
        );
        let bytes = f.write();
        assert_eq!(bytes.len(), 32 + 4 + 24);
        assert_eq!(&bytes[32..36], &[1, 0, 0x21, 0x61]);
        assert_eq!(SettingsFile::parse(&bytes).unwrap(), f);
        // volumes are clamped on load, any non-zero switch is on
        let mut raw = bytes.clone();
        raw[36..40].copy_from_slice(&(-7i32).to_le_bytes());
        raw[40..44].copy_from_slice(&250u32.to_le_bytes());
        raw[48..52].copy_from_slice(&0u32.to_le_bytes());
        raw[52..56].copy_from_slice(&0x100u32.to_le_bytes());
        let g = SettingsFile::parse(&raw).unwrap();
        assert_eq!((g.audio.music_volume, g.audio.voice_volume, g.audio.music_on, g.audio.voice_on), (0, 100, false, true));
        // the options part alone (older files / the `cfg` tool) still parses; the audio part is required by the whole file
        assert!(SettingsFile::parse(&bytes[..32]).is_err());
        let mut bad = bytes;
        bad[34] = 0;
        assert!(SettingsFile::parse(&bad).is_err());
    }

    #[test]
    fn passed_levels_round_trip() {
        let mut states = [0u8; LEVELS];
        for (i, s) in states.iter_mut().enumerate() {
            *s = (i % 3) as u8;
        }
        let p = PassedLevels { states };
        let mut n = 0u8;
        let bytes = p.write(|| {
            n = n.wrapping_mul(31).wrapping_add(7);
            n
        });
        assert_eq!(bytes.len(), 4 + BLOCK_LEN);
        assert_eq!(PassedLevels::parse(&bytes).unwrap(), p);
        let mut bad = bytes.clone();
        bad[4 + 79] ^= 1;
        assert!(PassedLevels::parse(&bad).is_err());
        let mut bad = bytes;
        bad[4 + 100] ^= 0x10;
        assert!(PassedLevels::parse(&bad).is_err());
    }
}

#[cfg(test)]
mod pass_tests {
    use super::*;

    #[test]
    fn a_new_campaign_opens_lands_1_2_and_9() {
        let p = PassedLevels::initial();
        assert_eq!((0..LEVELS).filter(|&l| p.states[l] == 2).collect::<Vec<_>>(), vec![1, 2, 9]);
    }

    #[test]
    fn passing_a_land_unlocks_the_next_one_in_order() {
        let mut p = PassedLevels::initial();
        assert_eq!(p.pass(1), Passed::Map);
        assert_eq!((p.states[1], p.states[6]), (1, 2));
        p.pass(1); // already passed: nothing changes
        assert_eq!(p.states[4], 0);
        p.pass(2);
        assert_eq!((p.states[2], p.states[4]), (1, 2));
        // a locked land cannot be passed
        p.pass(20);
        assert_eq!(p.states[20], 0);
        assert_eq!(p.pass(22), Passed::Ending);
    }

    #[test]
    fn passing_every_land_opens_the_finale() {
        let mut p = PassedLevels { states: [0; LEVELS] };
        for l in 1..=20 {
            p.states[l] = 1;
        }
        p.states[9] = 2;
        p.pass(9);
        assert_eq!(p.states[22], 2);
    }

    #[test]
    fn merge_prefers_passed_over_available() {
        let mut a = PassedLevels::initial();
        let mut b = PassedLevels { states: [0; LEVELS] };
        b.states[1] = 1;
        b.states[5] = 2;
        a.merge(&b);
        assert_eq!((a.states[1], a.states[2], a.states[5]), (1, 2, 2));
    }
}
