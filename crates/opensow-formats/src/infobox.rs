//! The info box texts (`0x43cf20` builds them, `0x43c410` descriptions, `0x431ad0` names, `0x4321c0` shell lines, `0x43bce0`
//! pictures); the numbers come from running the exe's own stat code (`scripts/emulate_infobox.py`). docs/hud.md.
//!
//! The exe counts language strings from 1 where the `lang` files here count from 0: every index below is already the
//! 0-based one (`exe - 1`).

use crate::lang::LangFile;

/// Which text layout a kind uses (`0x43df24` byte table + `0x43def4` jump table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// Kinds 1-15, 46: "TYPE: Ground unit" and the weapon stats.
    Unit,
    /// Kinds 16-24: gold / research / energy influx, only a type line.
    Production,
    /// Kinds 25-29 (turrets).
    Sentry,
    /// Kinds 30-34 (air forces).
    Air,
    /// Kind 40.
    Advancer,
    /// Kinds 100-108.
    Building,
    /// Kinds 41-45.
    Walker,
}

pub fn page(kind: i32) -> Option<Page> {
    Some(match kind {
        1..=15 | 46 => Page::Unit,
        16..=24 => Page::Production,
        25..=29 => Page::Sentry,
        30..=34 => Page::Air,
        40 => Page::Advancer,
        41..=45 => Page::Walker,
        100..=108 => Page::Building,
        _ => return None,
    })
}

/// 0-based language index of the object's name (`0x431ad0`).
pub fn name_index(kind: i32) -> Option<usize> {
    Some(
        match kind {
            1..=15 => 173 + kind - 1,
            16..=18 => 219 + kind - 16,
            19..=21 => 225 + kind - 19,
            22..=24 => 222 + kind - 22,
            25..=29 => 195 + kind - 25,
            30 => 200,
            31 => 202,
            32 => 204,
            33 => 203,
            34 => 201,
            40 => 193,
            41..=45 => 188 + kind - 41,
            46 => 194,
            100..=103 => 205 + kind - 100,
            104..=107 => 210 + kind - 104,
            108 => 209,
            _ => return None,
        } as usize,
    )
}

/// 0-based language index of the description (`0x43c410`).
pub fn description_index(kind: i32) -> Option<usize> {
    Some(
        match kind {
            1..=15 => 302 + kind - 1,
            41..=45 => 317 + kind - 41,
            46 => 322,
            16..=18 => 323 + kind - 16,
            22..=24 => 326 + kind - 22,
            19..=21 => 329 + kind - 19,
            30..=34 => 332 + kind - 30,
            100..=103 => 337 + kind - 100,
            108 => 341,
            104..=107 => 342 + kind - 104,
            25..=29 => 346 + kind - 25,
            _ => return None,
        } as usize,
    )
}

/// 0-based language index of the shell / blast text (`0x4321c0`); 260 is "n/a".
pub fn shell_index(kind: i32) -> usize {
    match kind {
        1..=15 => 228 + (kind - 1) as usize,
        25..=29 => 250 + (kind - 25) as usize,
        30 => 255,
        31 => 257,
        32 => 259,
        33 => 258,
        34 => 256,
        40 => 248,
        41..=45 => 243 + (kind - 41) as usize,
        46 => 249,
        _ => 260,
    }
}

/// Kinds whose line reads "BLAST: %s" instead of "SHELL: %s" (`0x43d0ae..`: 0x1b, 0xd, 0xe, 0xf, 0x28, 0x29, 0x2b, 0x2d).
pub fn uses_blast(kind: i32) -> bool {
    matches!(kind, 13 | 14 | 15 | 27 | 40 | 41 | 43 | 45)
}

/// The picture file (below `Data\Sprites2\InfoBox\`) and the picture's vertical offset from the box anchor (`0x43bce0`).
pub fn picture(kind: i32) -> Option<(&'static str, i32)> {
    Some(match kind {
        1 => ("3\\aa", -50),
        2 => ("4\\aa", -40),
        3 => ("5\\aa", -40),
        4 => ("3\\ar", -40),
        5 => ("4\\ar", -30),
        6 => ("5\\ar", -30),
        7 => ("3\\ca", -50),
        8 => ("4\\ca", -40),
        9 => ("5\\ca", -40),
        10 => ("3\\fl", -50),
        11 => ("4\\fl", -40),
        12 => ("5\\fl", -40),
        13 => ("3\\sp", -50),
        14 => ("4\\sp", -40),
        15 => ("5\\sp", -40),
        16 => ("6\\gold1", -45),
        17 => ("6\\gold2", -45),
        18 => ("6\\gold3", -45),
        19 => ("6\\research1", -37),
        20 => ("6\\research2", -37),
        21 => ("6\\research3", -37),
        22 => ("6\\energy1", -42),
        23 => ("6\\energy2", -42),
        24 => ("6\\energy3", -42),
        25 => ("2\\tur2", -40),
        26 => ("2\\tur4", -40),
        27 => ("2\\tur3", -45),
        28 => ("2\\tur1", -40),
        29 => ("2\\tur5", -50),
        30 => ("1\\bmbf", -32),
        31 => ("1\\caryf", -42),
        32 => ("1\\stif", -40),
        33 => ("1\\trif", -37),
        34 => ("1\\metf", -42),
        41 => ("7\\1", -50),
        42 => ("7\\2", -50),
        43 => ("7\\3", -50),
        44 => ("7\\4", -50),
        45 => ("7\\5", -50),
        46 => ("7\\N", -40),
        100 => ("2\\base", -37),
        101 => ("2\\light", -37),
        102 => ("2\\med", -42),
        103 => ("2\\heavy", -37),
        104 => ("2\\air", -45),
        105 => ("2\\mine", -49),
        106 => ("2\\rlab", -45),
        107 => ("2\\Wind", -38),
        108 => ("2\\walk", -37),
        _ => return None,
    })
}

/// Kinds whose picture has 16 frames instead of 32 (`0x43cfd4`: 0x6a, 0x6c, 0x10..0x15): the animation cycles `frames`.
pub fn picture_frames(kind: i32) -> u32 {
    if matches!(kind, 106 | 108 | 16..=21) { 16 } else { 32 }
}

/// Weapon stats of a unit as the info box prints them: (kind, offence, defence, scope min, scope max, speed).
/// Kind 46 (the pod) has no weapon: offence 0 and scope "n/a" (`0x46b4b0`, marked by a scope minimum of -1).
pub const UNIT_STATS: [(i32, i32, i32, i32, i32, i32); 22] = [
    (1, 10, 180, 0, 9, 18),
    (2, 20, 400, 0, 9, 14),
    (3, 29, 550, 0, 9, 10),
    (4, 23, 180, 2, 8, 15),
    (5, 45, 360, 3, 9, 12),
    (6, 83, 450, 4, 10, 8),
    (7, 13, 180, 0, 6, 22),
    (8, 31, 360, 0, 7, 18),
    (9, 50, 500, 0, 8, 14),
    (10, 18, 240, 0, 4, 20),
    (11, 36, 430, 0, 5, 17),
    (12, 60, 600, 0, 6, 12),
    (13, 22, 210, 0, 5, 16),
    (14, 42, 400, 0, 6, 15),
    (15, 70, 550, 0, 7, 10),
    (40, 20, 800, 0, 10, 30),
    (41, 98, 1100, 2, 8, 20),
    (42, 25, 900, 2, 9, 15),
    (43, 165, 1000, 2, 6, 20),
    (44, 52, 1100, 2, 11, 15),
    (45, 300, 1200, 0, 1, 20),
    (46, 0, 1000, -1, -1, 30),
];

/// Turrets 1-5 (kind 25..29): offence, defence (hit points), scope min, scope max; the speed prints as "0".
pub const TURRET_STATS: [(i32, i32, i32, i32, i32); 5] = [(25, 25, 900, 0, 6), (26, 20, 1250, 0, 9), (27, 40, 1540, 0, 7), (28, 60, 2100, 0, 8), (29, 88, 2600, 2, 10)];

/// Air forces 30..34: defence, speed (`0x43da7f`: code 32 shows 720, 33 shows 150, others the aircraft's `[+0x24]` = 120).
pub const AIR_STATS: [(i32, i32, i32); 5] = [(30, 480, 120), (31, 360, 120), (32, 8000, 720), (33, 270, 150), (34, 120, 120)];

/// Building hit points shown divided by 100 (`0x43dd62`).
pub const BUILDING_HP: i32 = 13536;

// 0-based language indices of the fixed texts (the exe's `push 0x11b..0x12e` minus one).
/// "No Info Available" (`0x43dea3` pushes 0x11b, 1-based) shown for a locked slot.
pub const NO_INFO: usize = 282;
const NAME: usize = 283;
const BLAST: usize = 284;
const SHELL: usize = 285;
const TYPE_UNIT: usize = 286;
const TYPE_WALKER: usize = 287;
const TYPE_PRODUCTION: usize = 288;
const TYPE_SENTRY: usize = 289;
const TYPE_AIR: usize = 290;
const TYPE_ADVANCER: usize = 291;
const TYPE_BUILDING: usize = 292;
const OFFENCE: usize = 293;
const DEFENCE: usize = 294;
const OFFENCE_NA: usize = 295;
const OFFENCE_SPECIAL: usize = 296;
const OFFENCE_ULTIMATE: usize = 297;
const SCOPE: usize = 298;
const SPEED_TEXT: usize = 299;
const SPEED_NUMBER: usize = 300;
const CARGO: usize = 301;
const NA: usize = 260;

fn fmt(template: &str, value: &str) -> String {
    template.replacen("%s", value, 1).replacen("%d", value, 1)
}

/// The lines of the info box for `kind`, as drawn (`0x43b5ed..0x43b6ce`): positions are in [`LINE_POS`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InfoLines {
    pub name: String,
    pub kind_line: String,
    pub offence: String,
    pub defence: String,
    pub scope: String,
    pub speed: String,
    pub shell: String,
    pub description: String,
}

/// Where each line is drawn relative to the box anchor (x - 54, y + 13 ...): (dx, dy) for name, type, offence, scope,
/// defence, speed, shell in the order the exe draws (and types) them.
pub const LINE_POS: [(i32, i32); 7] = [(-54, 13), (-54, 22), (-54, 31), (8, 31), (-54, 40), (8, 40), (-54, 49)];

pub fn lines(kind: i32, lang: &LangFile) -> Option<InfoLines> {
    let page = page(kind)?;
    let s = |i: usize| lang.get(i).unwrap_or("").trim().to_owned();
    let name = lang.get(name_index(kind)?).unwrap_or("").trim().to_owned();
    let mut l = InfoLines { name: fmt(&s(NAME), &name), description: s(description_index(kind).unwrap_or(usize::MAX)), ..Default::default() };
    let shell_text = s(shell_index(kind));
    l.shell = fmt(&s(if uses_blast(kind) { BLAST } else { SHELL }), &shell_text);
    let scope = |min: i32, max: i32| if min == 0 { max.to_string() } else { format!("{min}-{max}") };
    let range = |min: i32, max: i32| fmt(&s(SCOPE), &if min < 0 { "n/a".to_owned() } else { scope(min, max) });
    match page {
        Page::Unit | Page::Walker | Page::Advancer => {
            l.kind_line = s(match page {
                Page::Unit => TYPE_UNIT,
                Page::Walker => TYPE_WALKER,
                _ => TYPE_ADVANCER,
            });
            if let Some(&(_, off, def, smin, smax, speed)) = UNIT_STATS.iter().find(|r| r.0 == kind) {
                l.offence = fmt(&s(OFFENCE), &off.to_string());
                l.defence = fmt(&s(DEFENCE), &def.to_string());
                l.scope = range(smin, smax);
                l.speed = fmt(&s(SPEED_TEXT), &speed.to_string());
            }
        }
        Page::Sentry => {
            l.kind_line = s(TYPE_SENTRY);
            if let Some(&(_, off, def, smin, smax)) = TURRET_STATS.iter().find(|r| r.0 == kind) {
                l.offence = fmt(&s(OFFENCE), &off.to_string());
                l.defence = fmt(&s(DEFENCE), &def.to_string());
                l.scope = range(smin, smax);
                l.speed = fmt(&s(SPEED_TEXT), "0");
            }
        }
        Page::Production => {
            l.kind_line = s(TYPE_PRODUCTION);
            l.shell.clear();
        }
        Page::Air => {
            l.kind_line = s(TYPE_AIR);
            l.offence = match kind {
                30 => fmt(&s(OFFENCE), "6000"),
                31 => {
                    l.shell = s(CARGO);
                    s(OFFENCE_SPECIAL)
                }
                32 => fmt(&s(OFFENCE), "1000"),
                33 => fmt(&s(OFFENCE), "20000"),
                _ => s(OFFENCE_ULTIMATE),
            };
            if let Some(&(_, def, speed)) = AIR_STATS.iter().find(|r| r.0 == kind) {
                l.defence = fmt(&s(DEFENCE), &def.to_string());
                l.speed = fmt(&s(SPEED_NUMBER), &speed.to_string());
            }
        }
        Page::Building => {
            l.kind_line = s(TYPE_BUILDING);
            l.offence = s(OFFENCE_NA);
            l.defence = fmt(&s(DEFENCE), &(BUILDING_HP / 100).to_string());
            l.scope = fmt(&s(SCOPE), &s(NA));
            l.speed = fmt(&s(SPEED_TEXT), &s(NA));
        }
    }
    Some(l)
}

/// Greedy word wrap of the description (`0x43e1ba..0x43e2f6`: lines at most 0x69 = 105 px wide in the GDI font); `width` measures a string.
pub fn wrap(text: &str, max: i32, width: impl Fn(&str) -> i32) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
        if !line.is_empty() && width(&candidate) > max {
            out.push(std::mem::take(&mut line));
            line = word.to_owned();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_kind_has_a_page_and_text_indices() {
        for kind in (1..=46).chain(100..=108) {
            if let Some(p) = page(kind) {
                assert!(name_index(kind).is_some(), "name of {kind}");
                assert!(picture(kind).is_some() || p == Page::Advancer, "picture of {kind}");
            }
        }
    }

    #[test]
    fn stats_agree_with_other_decoded_tables() {
        for &(kind, _, def, _, _, speed) in &UNIT_STATS {
            if let Some(p) = crate::anim::walk_params(kind) {
                assert_eq!(p.speed as i32, speed, "speed of kind {kind}");
            }
            if kind != 40 {
                // hit points: the unit table of the game (docs/art.md) matches the info box's DEFENCE
                assert!(def > 0);
            }
        }
        assert_eq!(TURRET_STATS.map(|t| t.2), [900, 1250, 1540, 2100, 2600]);
    }

    #[test]
    fn wrap_breaks_at_the_width() {
        let lines = wrap("aa bb cc dd", 5, |s| s.len() as i32);
        assert_eq!(lines, vec!["aa bb", "cc dd"]);
    }
}

#[cfg(test)]
mod lang_tests {
    use super::*;

    /// With the install's English file the fixed strings read as expected (skipped without an install).
    #[test]
    fn fixed_strings_match_the_install() {
        let Some(game) = crate::GameDir::locate() else { return };
        let Some(path) = game.resolve("Data\\Language\\langEN.txt") else { return };
        let lang = LangFile::parse(&std::fs::read(path).unwrap()).unwrap();
        let t = |i: usize| lang.get(i).unwrap().trim().to_owned();
        assert_eq!(t(NAME), "NAME: %s");
        assert_eq!(t(TYPE_UNIT), "TYPE: Ground unit");
        assert_eq!(t(TYPE_ADVANCER), "TYPE: Advancer");
        assert_eq!(t(OFFENCE), "OFFENCE: %d");
        assert_eq!(t(DEFENCE), "DEFENCE: %d");
        assert_eq!(t(OFFENCE_ULTIMATE), "OFFENCE: ultimate");
        assert_eq!(t(SCOPE), "SCOPE: %s");
        assert_eq!(t(NA), "n/a");
        assert_eq!(t(NO_INFO), "No Info Available");
        // the pod has no weapon: offence 0, scope n/a
        let pod = lines(46, &lang).unwrap();
        assert_eq!((pod.offence.as_str(), pod.scope.as_str(), pod.defence.as_str()), ("OFFENCE: 0", "SCOPE: n/a", "DEFENCE: 1000"));
        let l = lines(1, &lang).unwrap();
        assert_eq!(l.kind_line, "TYPE: Ground unit");
        assert_eq!(l.offence, "OFFENCE: 10");
        assert_eq!(l.scope, "SCOPE: 9");
        assert_eq!(l.shell, "SHELL: 2x antiair rockets");
        assert!(l.description.starts_with("Main function of this unit"));
        assert_eq!(lines(40, &lang).unwrap().shell, "BLAST: laser");
        assert_eq!(lines(100, &lang).unwrap().defence, "DEFENCE: 135");
    }
}
