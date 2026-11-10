//! Skirmish: a match on one of the multiplayer maps (`*.net`) against the computer (docs/skirmish.md). Not in the original game.
//! The lobby is the multiplayer one (`multiplayer.rs` with its `skirmish` mode); this module turns its choices into a level script
//! the simulation can start: the human always plays player 0, the computer player 1 (the sim's computer opponent).

use crate::multiplayer::Setup;
use opensow_formats::level::Level;

/// Header word pairs that belong to player 0 / player 1 (docs/levels.md, docs/ai.md): credits, the two Advancer rates, the two
/// "no Advancer respawn" flags and research; the air forces' pairs `5 + 3g`, `6 + 3g` follow below.
const PAIRS: [(usize, usize); 4] = [(2, 3), (31, 32), (34, 35), (37, 38)];

/// Turns the side choice into the sim's orientation: with `human == 1` (green) every owner, unit owner, header pair and turret switch
/// of the two sides is exchanged, so the human is player 0 either way.
pub fn swap_sides(level: &mut Level) {
    for o in &mut level.objects {
        if o.raw[14] == 0 || o.raw[14] == 1 {
            o.raw[14] ^= 1;
        }
    }
    for m in &mut level.markers {
        if m.player == 0 || m.player == 1 {
            m.player ^= 1;
        }
    }
    let h = &mut level.header;
    for (a, b) in PAIRS {
        h.swap(a, b);
    }
    for g in 0..5 {
        h.swap(5 + 3 * g, 6 + 3 * g);
    }
    for t in 0..5 {
        let byte = |b: usize, h: &[i32]| (h[b / 4 + 2] >> (8 * (b % 4))) & 0xff;
        let (a, b) = (byte(84 + t, &level.header[..]), byte(94 + t, &level.header[..]));
        for (idx, v) in [(84 + t, b), (94 + t, a)] {
            let (word, shift) = (idx / 4 + 2, 8 * (idx % 4));
            level.header[word] = (level.header[word] & !(0xff << shift)) | (v << shift);
        }
    }
}

/// The level the match starts from: `setup` (blue = side 0, green = side 1 as the lobby shows them) written into the map's script,
/// then the sides exchanged when the human chose green.
pub fn prepare(map: &Level, setup: &Setup, human: usize) -> Level {
    let mut level = map.clone();
    setup.apply(&mut level);
    if human == 1 {
        swap_sides(&mut level);
    }
    level
}

/// Ground units a map gets for each side at the start of a skirmish, to make up for the carryalls skirmishes no longer
/// start with (`Setup::skirmish_default`). Only 01-Goldmine Valley needs them: 12 tier-1 units each, the same for both
/// sides (kinds 1 anti-air, 4 artillery, 7 cannon, 10 flamer, 13 special).
pub fn start_units(map: &str) -> &'static [i32] {
    // pairs: each pair fills one column of the two 3x2 blocks `World::add_start_units` stands them in (cannons, then the rest)
    const GOLDMINE_VALLEY: [i32; 12] = [7, 7, 7, 7, 7, 7, 4, 4, 10, 10, 13, 13];
    if map.to_ascii_lowercase().ends_with("goldmine valley") {
        &GOLDMINE_VALLEY
    } else {
        &[]
    }
}

/// The skirmish map tweaks (`data/skirmish-maps.txt`, docs/map-editor.md: moves, removals, additions and owner changes of a
/// map's buildings and units, written by `opensow-viewer --edit`), compiled in.
pub const TWEAKS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/skirmish-maps.txt"));

/// Applies the [`TWEAKS`] of `map` to its script (edits that no longer name an object of the shipped file are skipped).
pub fn rebalance(map: &str, level: &mut Level) {
    let tweaks = match opensow_formats::maptweak::Tweaks::parse(TWEAKS) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("data/skirmish-maps.txt: {e}");
            return;
        }
    };
    let (tracked, skipped) = opensow_formats::maptweak::apply(level, tweaks.edits(map));
    for line in skipped {
        eprintln!("data/skirmish-maps.txt [{map}]: `{line}` names nothing in the map, skipped");
    }
    *level = tracked.level;
    even_start_health(level);
}

/// Skirmish start health (docs/skirmish.md): every owned building and turret starts at full hit points and every neutral one at
/// half. Object word 18 is the start percentage; the exe uses it for owned objects only (`Object::start_hp`, `0x43a003`), and
/// the shipped maps leave owned ones anywhere from 69 % to 104 % (Ground Zero's turrets, both sides' HQs on Great Crush ...),
/// unevenly between the sides.
pub fn even_start_health(level: &mut Level) {
    for o in &mut level.objects {
        match o.raw[14] {
            0 | 1 => o.raw[18] = 100,
            2 => o.raw[18] = 50,
            _ => {}
        }
    }
}

/// Reads a skirmish map's script (`Data\Levels\<map>.net`) with its balance tweaks ([`rebalance`]).
pub fn load_map(game: &opensow_formats::GameDir, map: &str) -> Option<Level> {
    let mut level = game.resolve(&format!("Data\\Levels\\{map}.net")).and_then(|p| std::fs::read(p).ok()).and_then(|d| Level::parse(&d).ok())?;
    rebalance(map, &mut level);
    Some(level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplayer::SideSetup;

    fn sample() -> Level {
        let mut raw = [0i32; 19];
        raw[0] = 123;
        raw[1] = 100;
        raw[16] = 3;
        raw[17] = 4;
        let mut hq1 = raw;
        hq1[14] = 1;
        let mut neutral = raw;
        neutral[14] = 2;
        let mut level = Level { version: 4, header: [0; opensow_formats::level::HEADER_WORDS], objects: Vec::new(), markers: Vec::new() };
        let mk = |raw: [i32; 19]| opensow_formats::level::Object { kind: raw[1], cell_x: raw[16], cell_y: raw[17], raw };
        level.objects = vec![mk(raw), mk(hq1), mk(neutral)];
        level.markers = vec![opensow_formats::level::Marker { kind: 40, player: 0, x: 1, y: 2 }, opensow_formats::level::Marker { kind: 1, player: 1, x: 3, y: 4 }];
        level
    }

    fn setup() -> Setup {
        let mut s = Setup::default();
        s.sides[0] = SideSetup { credits: 1500, research: 300, air: [1, 2, 3, 4, 5], turrets: [true, false, true, false, true] };
        s.sides[1] = SideSetup { credits: 700, research: 0, air: [0, 0, 0, 0, 9], turrets: [false, true, false, true, false] };
        s
    }

    #[test]
    fn owned_objects_start_full_and_neutral_ones_half() {
        let mut level = sample();
        level.objects[0].raw[18] = 69;
        level.objects[1].raw[18] = 104;
        level.objects[2].raw[18] = 100;
        even_start_health(&mut level);
        let max = 13536;
        assert_eq!(level.objects.iter().map(|o| o.start_hp(max)).collect::<Vec<_>>(), vec![max, max, max / 2]);
    }

    #[test]
    fn every_skirmish_map_starts_owned_objects_full_and_neutral_ones_half() {
        let Some(game) = opensow_formats::GameDir::locate() else { return };
        let mut maps = 0;
        for file in game.level_files().into_iter().filter(|f| f.to_lowercase().ends_with(".net")) {
            let level = load_map(&game, &file[..file.len() - 4]).expect("map");
            for o in &level.objects {
                let want = if o.raw[14] == 2 { 6768 } else { 13536 };
                assert_eq!(o.start_hp(13536), want, "{file}: {} at ({},{})", o.object_kind().name(), o.cell_x, o.cell_y);
            }
            maps += 1;
        }
        assert!(maps >= 8);
    }

    #[test]
    fn the_tweaks_file_parses_and_leaves_maps_without_a_section_alone() {
        let t = opensow_formats::maptweak::Tweaks::parse(TWEAKS).expect("data/skirmish-maps.txt parses");
        assert!(!t.edits("06-Meteor Swarm").is_empty());
        let before = sample();
        let mut other = before.clone();
        rebalance("03-Gold Rush", &mut other);
        let mut expected = before.clone();
        even_start_health(&mut expected);
        assert_eq!(other.objects, expected.objects, "only the start health is evened");
        assert_eq!(other.markers, before.markers);
    }

    #[test]
    fn every_tweak_names_an_object_and_puts_it_on_free_buildable_ground_of_the_real_map() {
        use opensow_formats::{buildings::footprint, map::Map, maptweak::{apply, Tweaks}};
        let Some(game) = opensow_formats::GameDir::locate() else { return };
        let tweaks = Tweaks::parse(TWEAKS).unwrap();
        for (name, edits) in &tweaks.maps {
            let Some(file) = game.level_files().into_iter().find(|f| f.eq_ignore_ascii_case(&format!("{name}.net"))) else { continue };
            let stem = &file[..file.len() - 4];
            let raw = game.resolve(&format!("Data\\Levels\\{file}")).and_then(|p| std::fs::read(p).ok()).and_then(|d| Level::parse(&d).ok()).unwrap();
            let (tracked, skipped) = apply(&raw, edits);
            assert!(skipped.is_empty(), "{name}: {skipped:?} name nothing in the shipped map");
            let level = tracked.level;
            let (tg, mission) = game.net_terrain(stem, level.level_id());
            let map = Map::parse(&std::fs::read(tg.resolve(&format!("Data\\Levels\\{mission}.map")).unwrap()).unwrap()).unwrap();
            for (i, o) in level.objects.iter().enumerate() {
                let moved = tracked.object_origin[i].map_or(true, |j| (raw.objects[j].cell_x, raw.objects[j].cell_y) != (o.cell_x, o.cell_y));
                if !moved {
                    continue;
                }
                let (w, h, _) = footprint(o.kind).unwrap_or((1, 1, 0));
                for y in o.cell_y..o.cell_y + h {
                    for x in o.cell_x..o.cell_x + w {
                        let c = map.cell(x as u32, y as u32).unwrap_or_else(|| panic!("{name}: {} at ({},{}) leaves the map", o.object_kind().name(), o.cell_x, o.cell_y));
                        assert!(!c.blocked && !c.no_build, "{name}: ({x},{y}) under {} is not buildable", o.object_kind().name());
                    }
                }
                for (j, p) in level.objects.iter().enumerate().filter(|&(j, _)| j != i) {
                    let (pw, ph, _) = footprint(p.kind).unwrap_or((1, 1, 0));
                    let overlap = p.cell_x < o.cell_x + w && o.cell_x < p.cell_x + pw && p.cell_y < o.cell_y + h && o.cell_y < p.cell_y + ph;
                    assert!(!overlap, "{name}: {} at ({},{}) overlaps object {j} {} at ({},{})", o.object_kind().name(), o.cell_x, o.cell_y, p.object_kind().name(), p.cell_x, p.cell_y);
                }
            }
            for (side, label) in [(0, "blue"), (1, "green")] {
                assert!(level.objects.iter().any(|o| o.kind == 100 && o.raw[14] == side), "{name}: {label} keeps a headquarters");
            }
            // a building that a tweak made blue or green (added, or neutral before) starts at full hit points, not the neutral half
            for (i, o) in level.objects.iter().enumerate().filter(|(_, o)| o.raw[14] != 2) {
                let was_neutral = tracked.object_origin[i].map_or(true, |j| raw.objects[j].raw[14] == 2);
                if was_neutral {
                    assert_eq!(o.raw[18], 100, "{name}: {} at ({},{}) would start damaged", o.object_kind().name(), o.cell_x, o.cell_y);
                }
            }
        }
    }

    #[test]
    fn only_goldmine_valley_gets_start_units() {
        let u = start_units("01-goldmine valley");
        assert_eq!(u.len(), 12);
        assert!(u.iter().all(|&k| (1..=15).contains(&k) && (k - 1) % 3 == 0), "tier 1 only: {u:?}");
        assert!(!u.contains(&1), "no anti-air units");
        assert!(start_units("02-Ice Blizzard").is_empty() && start_units("03-Gold Rush").is_empty());
    }

    #[test]
    fn a_skirmish_starts_without_carryalls_whatever_the_map_says() {
        let mut level = sample();
        let mut s = setup();
        s.sides[1].air = [1, 2, 3, 4, 5];
        s.apply(&mut level);
        let carry = opensow_formats::air::HEADER_ORDER.iter().position(|&f| f == opensow_formats::air::AirForce::Carryall).unwrap();
        assert_eq!(level.air_stock()[carry].counts[..2], [4, 4], "the map holds 4 carryalls each (lobby row 3 is the carryall)");
        let d = Setup::skirmish_default(&level);
        let mut l2 = level.clone();
        d.apply(&mut l2);
        assert_eq!(l2.air_stock()[carry].counts[..2], [0, 0]);
        for g in (0..5).filter(|&g| g != carry) {
            assert_eq!(l2.air_stock()[g].counts, level.air_stock()[g].counts, "group {g} untouched");
        }
    }

    #[test]
    fn apply_is_the_inverse_of_from_level() {
        let mut level = sample();
        setup().apply(&mut level);
        assert_eq!(Setup::from_level(&level).sides[0].credits, 1500);
        assert_eq!(Setup::from_level(&level).sides[1].air, [0, 0, 0, 0, 9]);
        let mut again = level.clone();
        let mut back = Setup::from_level(&level); // `from_level` does not read turret switches (all on)
        back.sides[0].turrets = setup().sides[0].turrets;
        back.sides[1].turrets = setup().sides[1].turrets;
        back.apply(&mut again);
        assert_eq!(again.header, level.header, "air, credits and research round-trip");
        // the turret switches land where the world reads them (`World::from_level`: byte 84 + t / 94 + t)
        let byte = |b: usize| (level.header[b / 4 + 2] >> (8 * (b % 4))) & 0xff;
        assert_eq!([byte(84), byte(85), byte(86), byte(87), byte(88)], [1, 0, 1, 0, 1]);
        assert_eq!([byte(94), byte(95), byte(96), byte(97), byte(98)], [0, 1, 0, 1, 0]);
    }

    #[test]
    fn blue_keeps_the_map_and_green_swaps_everything() {
        let blue = prepare(&sample(), &setup(), 0);
        assert_eq!(blue.objects[0].raw[14], 0);
        assert_eq!(blue.markers[0].player, 0);
        assert_eq!(blue.header[2], 1500);
        let green = prepare(&sample(), &setup(), 1);
        assert_eq!([green.objects[0].raw[14], green.objects[1].raw[14], green.objects[2].raw[14]], [1, 0, 2], "neutral stays neutral");
        assert_eq!([green.markers[0].player, green.markers[1].player], [1, 0]);
        assert_eq!([green.header[2], green.header[3]], [700, 1500], "player 0 (the human on green) gets green's credits");
        assert_eq!([green.header[37], green.header[38]], [0, 300]);
        let from = Setup::from_level(&green);
        assert_eq!(from.sides[0].air, [0, 0, 0, 0, 9]);
        assert_eq!(from.sides[1].air, [1, 2, 3, 4, 5]);
        let byte = |b: usize| (green.header[b / 4 + 2] >> (8 * (b % 4))) & 0xff;
        assert_eq!([byte(84), byte(85), byte(94), byte(95)], [0, 1, 1, 0]);
        // swapping twice is the identity
        let mut twice = green.clone();
        swap_sides(&mut twice);
        assert_eq!(twice.header, blue.header);
        assert_eq!(twice.objects, blue.objects);
    }
}
