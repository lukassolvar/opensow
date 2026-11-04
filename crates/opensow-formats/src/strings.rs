//! What the 364 strings of `langXX.txt` are for (docs/text.md has the full table).
//!
//! **Indices here are file indices** (0-based, what [`crate::lang::LangFile::get`] takes). The exe numbers them one higher: its
//! `strtok` makes the UTF-8 BOM in front of the first `@` its own token 0, so "New Game" is `GetString(1)` there and the trailing
//! `\r\n` is token 365. The groups come from the xrefs of the string getter `0x466ae0` (377 call sites, all on the global
//! `0x6bed04`); `tests/real_data.rs` re-reads the exe's two kind -> string switches.

use std::ops::RangeInclusive;

/// Main menu items New Game, Load Game, Tutorial, Multiplayer, Options, Exit Game (`0x445620` draws `GetString(1 + i)`, i < 6).
pub const MAIN_MENU: RangeInclusive<usize> = 0..=5;
/// Options screen (`0x44b320`): title, then GFX / ACCESSORY / SFX / Radar / Quake / Clouds / HWMOUSE / VBLANK / FX / MUSIC / VOICE /
/// RESOLUTION / Keys scroll / Mouse scroll.
pub const OPTIONS_TITLE: usize = 4;
pub const OPTIONS_LABELS: RangeInclusive<usize> = 6..=19;
/// "Language" and the eight language names ENGLISH..CHINESSE_T: no code path of the exe asks for them.
pub const LANGUAGE_LABEL: usize = 20;
pub const LANGUAGE_NAMES: RangeInclusive<usize> = 23..=30;
pub const OK: usize = 21;
pub const CANCEL: usize = 22;
pub const ON: usize = 267;
pub const OFF: usize = 268;
/// Campaign map (`0x43f8b0`), land `1..=22`: heading, place, briefing, objectives; a locked land shows [`NO_DATA`].
pub const NO_DATA: usize = 53;
pub fn land_heading(land: usize) -> usize {
    30 + land
}
pub fn land_place(land: usize) -> usize {
    53 + land
}
pub fn land_briefing(land: usize) -> usize {
    75 + land
}
pub fn land_objectives(land: usize) -> usize {
    97 + land
}
/// Speech / message events 1..=24 (`0x402ec0`, see [`crate::messages`]) use 120..=143; 144 is the game-speed readout label "speed".
pub const MESSAGES: RangeInclusive<usize> = 120..=143;
pub const SPEED: usize = 144;
/// Building hover line "SATELLITE PROTECTION" (`0x409400`).
pub const SATELLITE_PROTECTION: usize = 145;
/// Tutorial step texts (`0x4325b0`); each starts with `i ` (lesson heading) or `! ` (task).
pub const TUTORIAL: RangeInclusive<usize> = 146..=171;
/// Names of units, turrets, air forces and buildings, picked by [`object_name`] (`0x431ad0`): 172..=227.
pub const OBJECT_NAMES: RangeInclusive<usize> = 172..=227;
/// Weapon names (`0x4321c0`), picked by [`weapon_name`]: 228..=259; 260 = "n/a" (no weapon), 261 = "increase tech-level for 500"
/// (the object name of kinds >= 1000).
pub const WEAPON_NAMES: RangeInclusive<usize> = 228..=259;
pub const NO_WEAPON: usize = 260;
pub const TECH_LEVEL_HINT: usize = 261;
/// Multiplayer level picker (`0x447100`): "Selected level:", then BLUE SIDE, GREEN SIDE, CREDITS, RESEARCH (273..=276); ACCEPT/BACK 352/353.
pub const SELECTED_LEVEL: usize = 262;
pub const SIDE_AND_START: RangeInclusive<usize> = 273..=276;
pub const ACCEPT: usize = 352;
pub const BACK: usize = 353;
/// Multiplayer lobby (`0x446ca0`, `0x4485e0`, `0x434e80`): CREATE SESSION, SHOW ALL SESSIONS, join/leave and lost-session lines.
pub const SESSION_BUTTONS: RangeInclusive<usize> = 263..=264;
pub const PLAYER_JOINED_LEFT: RangeInclusive<usize> = 265..=266;
pub const SESSION_LOST: RangeInclusive<usize> = 277..=279;
/// Save slots (`0x443ca0`): Empty Savegame..., No Time, No Date, savegame is damaged!!!
pub const SLOT_TEXTS: RangeInclusive<usize> = 269..=272;
/// Mission hint boxes (`0x4303e0`, `0x430770`).
pub const HINTS: RangeInclusive<usize> = 280..=281;
/// Info box (`0x43cf20`): "No Info Available", then the NAME/BLAST/SHELL/TYPE/OFFENCE/DEFENCE/SCOPE/SPEED line formats (`%s`/`%d`).
pub const INFOBOX_LINES: RangeInclusive<usize> = 282..=301;
/// Info box description pages, one per object (`0x43c410`): 302..=350.
pub const DESCRIPTIONS: RangeInclusive<usize> = 302..=350;
pub const QUICK_SAVE: usize = 351;
/// Statistics screen (`0x44d500`): CAPTURED, LOST, PRODUCED, BUILT, SENT.
pub const STATISTICS: RangeInclusive<usize> = 354..=358;
/// Load / save screen (`0x443ad0`): LOAD, SAVE, and the title "Save Game" (363).
pub const LOAD: usize = 359;
pub const SAVE: usize = 360;
pub const SAVE_TITLE: usize = 363;
/// Pause banner "P A U S E" (`0x435330`) and the mission clock label "Remaining Time" (`0x430d50`).
pub const PAUSE: usize = 361;
pub const REMAINING_TIME: usize = 362;

/// (object kind, string): the exe's switch `0x431ad0` (byte table `0x43214c`, jump table `0x432064`). Kinds 1-15 ground chassis
/// (light/medium/heavy x AntiAir, Artillery, Panzer, Flamer, Special), 16-24 gold/energy/research influx levels, 25-29 turrets,
/// 30-34 air forces (30 bomber, 31 carryall, 32 fighter, 33 tripler, 34 meteors), 40 Advancer, 41-45 walkers, 46 Rogon bomb, 100-108
/// buildings, 109-113 top sentries, 115 radar. Any other kind < 1000 has no name.
pub const OBJECT_KIND_NAMES: [(u16, u16); 56] = [
    (1, 173), (2, 174), (3, 175), (4, 176), (5, 177), (6, 178), (7, 179), (8, 180),
    (9, 181), (10, 182), (11, 183), (12, 184), (13, 185), (14, 186), (15, 187), (16, 219),
    (17, 220), (18, 221), (19, 225), (20, 226), (21, 227), (22, 222), (23, 223), (24, 224),
    (25, 195), (26, 196), (27, 197), (28, 198), (29, 199), (30, 200), (31, 202), (32, 204),
    (33, 203), (34, 201), (40, 193), (41, 188), (42, 189), (43, 190), (44, 191), (45, 192),
    (46, 194), (100, 205), (101, 206), (102, 207), (103, 208), (104, 210), (105, 211), (106, 212),
    (107, 213), (108, 209), (109, 214), (110, 215), (111, 216), (112, 217), (113, 218), (115, 172),
];

/// (object kind, string): the exe's switch `0x4321c0` (byte table `0x4323d0`, jump table `0x432348`); the other kinds say "n/a".
pub const WEAPON_KIND_NAMES: [(u16, u16); 32] = [
    (1, 228), (2, 229), (3, 230), (4, 231), (5, 232), (6, 233), (7, 234), (8, 235),
    (9, 236), (10, 237), (11, 238), (12, 239), (13, 240), (14, 241), (15, 242), (25, 250),
    (26, 251), (27, 252), (28, 253), (29, 254), (30, 255), (31, 257), (32, 259), (33, 258),
    (34, 256), (40, 248), (41, 243), (42, 244), (43, 245), (44, 246), (45, 247), (46, 249),
];

fn find(table: &[(u16, u16)], kind: u32) -> Option<usize> {
    table.iter().find(|e| e.0 as u32 == kind).map(|e| e.1 as usize)
}

/// File index of the display name of object `kind` (`0x431ad0`); kinds >= 1000 give [`TECH_LEVEL_HINT`], unknown ones `None`.
pub fn object_name(kind: u32) -> Option<usize> {
    if kind >= 1000 {
        return Some(TECH_LEVEL_HINT);
    }
    find(&OBJECT_KIND_NAMES, kind)
}

/// File index of the weapon name shown in the info box for `kind` (`0x4321c0`); [`NO_WEAPON`] ("n/a") for kinds without one.
pub fn weapon_name(kind: u32) -> usize {
    find(&WEAPON_KIND_NAMES, kind).unwrap_or(NO_WEAPON)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups_follow_the_exes_switches() {
        assert_eq!(object_name(40), Some(193)); // Advancer
        assert_eq!(object_name(108), Some(209)); // Walker Factory sits between Heavy Factory and Air Navigator in the string list
        assert_eq!(object_name(1000), Some(TECH_LEVEL_HINT));
        assert_eq!(object_name(35), None);
        assert_eq!(weapon_name(1), 228);
        assert_eq!(weapon_name(16), NO_WEAPON);
    }

    #[test]
    fn ranges_do_not_overlap_and_land_helpers_cover_their_groups() {
        assert_eq!(land_heading(1), 31);
        assert_eq!(land_heading(22), 52);
        assert_eq!(land_place(1), 54);
        assert_eq!(land_briefing(22), 97);
        assert_eq!(land_objectives(22), 119);
        assert_eq!(*MESSAGES.start(), land_objectives(22) + 1);
        assert_eq!(*MESSAGES.end() + 1, SPEED);
        assert_eq!(*DESCRIPTIONS.end() + 1, QUICK_SAVE);
        assert_eq!(*OBJECT_NAMES.end() + 1, *WEAPON_NAMES.start());
        assert_eq!(*WEAPON_NAMES.end() + 1, NO_WEAPON);
    }
}
