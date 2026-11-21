//! Invariants checked against a real installation. Skipped when the game is not found.

use opensow_formats::kinds;
use opensow_formats::{level::*, map::Map, GameDir};

fn read(game: &GameDir, rel: &str) -> Option<Vec<u8>> {
    std::fs::read(game.resolve(rel)?).ok()
}

#[test]
fn campaign_objects_are_on_open_ground_inside_the_map() {
    let Some(game) = GameDir::locate() else { return eprintln!("game not found, skipping") };
    let mut checked = 0;
    for n in 1..=22 {
        let name = format!("M{n:02}");
        let map = Map::parse(&read(&game, &format!("Data\\Levels\\{name}.map")).unwrap()).unwrap();
        let level = Level::parse(&read(&game, &format!("Data\\Levels\\{name}.edt")).unwrap()).unwrap();
        assert_eq!(level.level_id(), n, "{name} header level id");
        for o in &level.objects {
            let c = map.cell(o.cell_x as u32, o.cell_y as u32).unwrap_or_else(|| panic!("{name}: object outside map"));
            assert!(!c.blocked, "{name}: {} at ({},{}) on a blocked cell", o.object_kind().name(), o.cell_x, o.cell_y);
            assert!(!matches!(o.object_kind(), ObjectKind::Unknown(_)), "{name}: unknown kind {}", o.kind);
            checked += 1;
        }
        for m in &level.markers {
            assert!(m.x >= 0 && (m.x as u32) < map.width * 32 && m.y >= 0 && (m.y as u32) < map.height * 32);
            assert!(UnitKind::max_hp(m.kind).is_some(), "{name}: marker kind {} has no unit", m.kind);
            assert!(!matches!(m.unit_kind(), UnitKind::RuntimeOnly(_) | UnitKind::Unknown(_)), "{name}: kind {}", m.kind);
            assert!(m.player == 0 || m.player == 1, "{name}: player {}", m.player);
        }
    }
    assert!(checked > 1000);
}

#[test]
fn every_level_file_is_written_back_byte_identical() {
    let Some(game) = GameDir::locate() else { return };
    let mut dirs = vec![game.resolve("Data\\Levels").unwrap()];
    dirs.extend(game.warmonger.as_ref().and_then(|w| GameDir::new(w.clone()).resolve("Data\\Levels")));
    let mut n = 0;
    for dir in dirs {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("net") || x.eq_ignore_ascii_case("edt")) {
                continue;
            }
            let data = std::fs::read(&p).unwrap();
            let Ok(level) = Level::parse(&data) else { continue }; // the empty template M00 has version 3 and is checked elsewhere
            assert!(level.to_bytes() == data, "{} is not written back as it was", p.display());
            n += 1;
        }
    }
    assert!(n >= 31, "{n} level files");
}

#[test]
fn multiplayer_maps_have_two_hqs_and_start_positions_per_side() {
    let Some(game) = GameDir::locate() else { return };
    let dir = game.resolve("Data\\Levels").unwrap();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| !x.eq_ignore_ascii_case("net")) {
            continue;
        }
        let l = Level::parse(&std::fs::read(&p).unwrap()).unwrap();
        let hqs = l.objects.iter().filter(|o| o.object_kind() == ObjectKind::Headquarters).count();
        let starts = l.markers.iter().filter(|m| m.is_advancer()).count();
        let two_hq_each = p.to_string_lossy().contains("Cross of Fire");
        assert_eq!((hqs, starts), if two_hq_each { (4, 4) } else { (2, 2) }, "{}", p.display());
    }
}

#[test]
fn building_and_turret_art_exists_in_the_archive() {
    use opensow_formats::{megafile::MegaFile, sprite};
    let Some(game) = GameDir::locate() else { return };
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    let frames = |name: &str, kind| {
        let e = mega.find(name).unwrap_or_else(|| panic!("{name} missing"));
        sprite::parse(&mega.read(e).unwrap(), kind).unwrap().len()
    };
    for stem in ["_base", "_light", "_med", "_heavy", "_air", "_mine", "_rlab", "_wind", "_walker"] {
        for team in ["blue", "green"] {
            assert!(frames(&format!("builds\\{team}\\{stem}.ps6"), sprite::Kind::Color) >= 6, "{team} {stem}");
        }
        assert!(frames(&format!("builds\\shadow\\{stem}.sha"), sprite::Kind::Shadow) >= 6, "shadow {stem}");
    }
    // The last frame of every blue set except the HQ is the grey "neutral" look; blue has one more frame than green.
    assert_eq!(frames("builds\\blue\\_rlab.ps6", sprite::Kind::Color), 18);
    assert_eq!(frames("builds\\green\\_rlab.ps6", sprite::Kind::Color), 17);
    // The exe loads only turrets 1, 2, 3 and 5: turret 4 (satellite laser) has no ground sprite.
    for n in [1, 2, 3, 5] {
        assert!(mega.find(&format!("units\\tur_0{n}_blue.ps6")).is_some());
    }
    assert!(mega.find("units\\tur_04_blue.ps6").is_none());
}

#[test]
fn every_placed_unit_kind_has_art_in_the_archive() {
    use opensow_formats::{megafile::MegaFile, sprite, units::unit_art};
    let Some(game) = GameDir::locate() else { return };
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for n in 1..=22 {
        let level = Level::parse(&read(&game, &format!("Data\\Levels\\M{n:02}.edt")).unwrap()).unwrap();
        seen.extend(level.markers.iter().map(|m| m.kind));
    }
    assert!(seen.len() >= 12, "expected a variety of unit kinds, got {seen:?}");
    for kind in seen {
        let art = unit_art(kind).unwrap_or_else(|| panic!("kind {kind} has no art mapping"));
        for name in [Some(art.chassis), art.top].into_iter().flatten() {
            for team in ["blue", "gren"] {
                let file = format!("units\\{name}_{team}.ps6");
                let e = mega.find(&file).unwrap_or_else(|| panic!("{file} missing for kind {kind}"));
                let frames = sprite::parse(&mega.read(e).unwrap(), sprite::Kind::Color).unwrap();
                assert!(frames.len() >= 2 || name == "1I", "{file} has {} frames", frames.len());
            }
        }
    }
}

#[test]
fn air_force_stock_matches_the_exes_loader_assumptions() {
    use opensow_formats::air::AirForce;
    let Some(game) = GameDir::locate() else { return };
    // Campaign: the stock sits in the player's words (5+3g, loader 0x41d88e -> block 0x70ba50), the enemy's words (6+3g) and the unread words (4+3g) are 0.
    for n in 1..=22 {
        let l = Level::parse(&read(&game, &format!("Data\\Levels\\M{n:02}.edt")).unwrap()).unwrap();
        for st in l.air_stock() {
            assert!(st.counts.iter().all(|&c| (0..=20).contains(&c)), "M{n:02} {:?}", st);
            assert_eq!((st.counts[1], st.counts[2]), (0, 0), "M{n:02} {}", st.force.name());
        }
    }
    // the loader reads word 5 (0x51797c) for block 0x70ba50 and word 6 (0x517980) for block 0x70c6b8, never word 4 (0x517978)
    let code = Exe::open(&game).map(|e| e.at(0x41d88c, 0x40).to_vec());
    if let Some(code) = code {
        let has = |pat: &[u8]| code.windows(pat.len()).any(|w| w == pat);
        assert!(has(&[0x39, 0x35, 0x7C, 0x79, 0x51, 0x00]) || has(&[0x83, 0x3D, 0x7C, 0x79, 0x51, 0x00]), "cmp [0x51797c]");
        assert!(has(&[0xB9, 0x50, 0xBA, 0x70, 0x00]), "mov ecx, 0x70ba50");
    }
    // Spot checks against the shipped data.
    let m10 = Level::parse(&read(&game, "Data\\Levels\\M10.edt").unwrap()).unwrap().air_stock();
    let side1 = |f: AirForce| m10.iter().find(|s| s.force == f).unwrap().counts[0];
    assert_eq!(
        [side1(AirForce::Bomber), side1(AirForce::Asteroid), side1(AirForce::Carryall), side1(AirForce::Tripler), side1(AirForce::Fighter)],
        [4, 0, 7, 4, 6]
    );
    // Multiplayer: both players get the same stock; the unread words stay 0.
    let dir = game.resolve("Data\\Levels").unwrap();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("net")) {
            for st in Level::parse(&std::fs::read(e.path()).unwrap()).unwrap().air_stock() {
                assert_eq!((st.counts[0], st.counts[2]), (st.counts[1], 0), "{} {}", e.path().display(), st.force.name());
            }
        }
    }
}

#[test]
fn fonts_follow_the_ascii_minus_32_layout_and_masks_nest() {
    use opensow_formats::{font::Font, megafile::MegaFile, sprite};
    let Some(game) = GameDir::locate() else { return };
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    let load = |name: &str, kind| {
        let e = mega.find(&format!("extras\\fonts\\{name}")).unwrap_or_else(|| panic!("{name} missing"));
        Font::new(sprite::parse(&mega.read(e).unwrap(), kind).unwrap())
    };
    for f in ["small", "medium", "big", "msg"] {
        let (i, o) = (load(&format!("{f}_i.msk"), sprite::Kind::Shadow), load(&format!("{f}_o.msk"), sprite::Kind::Shadow));
        assert_eq!((i.glyph_count(), o.glyph_count()), (94, 94), "{f}");
        // Digits come right after ASCII '/', i.e. frame 16 is '0'.
        assert!(i.glyph('0').is_some() && i.glyph('}').is_some() && i.glyph('~').is_none());
        // The outline mask must enclose the inner mask when both are placed at pen - hot.
        for c in ('A'..='Z').chain('a'..='z').chain('0'..='9') {
            let (gi, go) = (i.glyph(c).unwrap(), o.glyph(c).unwrap());
            let (li, lo) = (-(gi.hot_x as i32), -(go.hot_x as i32));
            let (ti, to) = (-(gi.hot_y as i32), -(go.hot_y as i32));
            assert!(lo <= li && lo + go.width as i32 >= li + gi.width as i32, "{f} {c}: horizontal nesting");
            assert!(to <= ti && to + go.height as i32 >= ti + gi.height as i32, "{f} {c}: vertical nesting");
        }
        // Capitals share a baseline (round letters may overshoot it by one pixel).
        let bottoms: Vec<i32> = ('A'..='Z')
            .filter(|&c| c != 'Q' && c != 'J')
            .map(|c| {
                let g = i.glyph(c).unwrap();
                -(g.hot_y as i32) + g.height as i32
            })
            .collect();
        let (lo, hi) = (*bottoms.iter().min().unwrap(), *bottoms.iter().max().unwrap());
        assert!(hi - lo <= 1, "{f}: capitals should share a baseline, got {lo}..{hi}");
    }
    for name in ["font02_dark", "font02_light", "font_msg2"] {
        assert_eq!(load(&format!("{name}.ps6"), sprite::Kind::Color).glyph_count(), 94, "{name}");
    }
    for name in ["font_briefing_big", "font_briefing_small"] {
        assert_eq!(load(&format!("{name}.ps6"), sprite::Kind::Color).glyph_count(), 95, "{name}");
    }
}

#[test]
fn language_files_parse_and_hungarian_is_the_damaged_one() {
    use opensow_formats::lang::{read_lang_dat, LangFile, Language};
    let Some(game) = GameDir::locate() else { return };
    assert_eq!(read_lang_dat(&read(&game, "Data\\lang.dat").unwrap()), Some(Language::English));
    let load = |l: Language| LangFile::parse(&read(&game, &format!("Data\\Language\\{}", l.file_name())).unwrap()).unwrap();
    let en = load(Language::English);
    assert_eq!(en.strings.len(), 364);
    assert_eq!((en.get(0), en.get(1)), (Some("New Game"), Some("Load Game")));
    for l in Language::ALL {
        let f = load(l);
        assert_eq!(f.strings.len(), 364, "{}", l.code());
        let differing = f.strings.iter().zip(&en.strings).filter(|(a, b)| a != b).count();
        match l {
            // The Steam package ships English text under the Polish and Russian names (3 labels differ).
            Language::Polish | Language::Russian => assert_eq!(differing, 3, "{}", l.code()),
            Language::English => assert_eq!(differing, 0),
            _ => assert!(differing > 100, "{} has {differing} translated strings", l.code()),
        }
        let damaged = f.damaged();
        if l == Language::Hungarian {
            assert!(!damaged.is_empty(), "the shipped Hungarian file is expected to contain damaged text");
        } else {
            assert!(damaged.is_empty(), "{} unexpectedly damaged: {damaged:?}", l.code());
        }
    }
    // every typo correction matches the shipped text once, and none is left after `with_supplement`
    for l in [Language::Slovak, Language::Czech] {
        let shipped = load(l);
        let fixed = shipped.clone().with_supplement(l);
        for &(i, wrong, right) in opensow_formats::lang::corrections(l) {
            let s = shipped.get(i).unwrap();
            assert_eq!(s.matches(wrong).count(), 1, "{} {i}: {wrong:?} not found once in {s:?}", l.code());
            // (lower-cased: labels are capitalised after the correction; case-only corrections are checked by the first test)
            let (f, wl, rl) = (fixed.get(i).unwrap().to_lowercase(), wrong.to_lowercase(), right.to_lowercase());
            assert!(f.contains(&rl) && (wl == rl || rl.contains(&wl) || !f.contains(&wl)), "{} {i}: {f:?}", l.code());
        }
    }
    assert_eq!(load(Language::Slovak).with_supplement(Language::Slovak).get(145), Some("SATELITNÁ OCHRANA"));
    assert_eq!(load(Language::Czech).get(0), Some("nová hra"));
    assert_eq!(load(Language::ChineseSimplified).get(0), Some("新游戏"));
}

#[test]
fn fire_tables_exist_for_every_top_and_trace_smooth_loops() {
    use opensow_formats::{
        fire::{FireTables, FACINGS},
        megafile::MegaFile,
        units::TOPS,
    };
    let Some(game) = GameDir::locate() else { return };
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    let load = |name: &str, ext: &str| {
        let e = mega.find(&format!("units\\{name}.{ext}")).unwrap_or_else(|| panic!("{name}.{ext} missing"));
        FireTables::parse(&mega.read(e).unwrap()).unwrap()
    };
    for top in TOPS {
        let (p1, p2) = (load(top, "po1"), load(top, "po2"));
        assert_eq!(p2.variants(), 1, "{top}.po2");
        assert!((1..=6).contains(&p1.variants()), "{top}.po1 has {} tables", p1.variants());
        if top == "1I" {
            continue; // the Advancer has no weapon: constant (0, 2)
        }
        for (name, t) in [("po1", &p1), ("po2", &p2)] {
            for v in 0..t.variants() {
                let pts: Vec<(i32, i32)> = (0..FACINGS).map(|f| t.muzzle(v, f).unwrap()).collect();
                if pts.iter().all(|&p| p == (0, 0)) {
                    continue; // unused barrel slot (walkers)
                }
                for f in 0..FACINGS {
                    let (a, b) = (pts[f], pts[(f + 1) % FACINGS]);
                    let step = (a.0 - b.0).abs().max((a.1 - b.1).abs());
                    assert!(step <= 70, "{top}.{name}[{v}] jumps {step} px between facings {f} and {}", (f + 1) % FACINGS);
                }
            }
        }
    }
    // The 1C cannon points right at frame 8 and left at frame 24.
    let c = load("1C", "po1");
    assert_eq!((c.muzzle(0, 8), c.muzzle(0, 24)), (Some((20, 1)), Some((-21, 1))));
    // Twin AA guns have two tables; single-barrel tanks use identical po1 and po2.
    assert_eq!(load("1AA", "po1").variants(), 2);
    for t in ["1A", "1C", "1F", "1S"] {
        assert_eq!(load(t, "po1"), load(t, "po2"), "{t}");
    }
}

/// What [`Exe::top_case`] reads from a weapon-top constructor case: the fields stored (offset -> value) and the fire
/// steps added (phase, flash group, barrel, shots).
type TopCase = (std::collections::HashMap<u32, i32>, Vec<(i32, i32, i32, i32)>);

/// Minimal PE reader: bytes at a virtual address of `State of War.exe`.
struct Exe {
    data: Vec<u8>,
    image_base: u32,
    sections: Vec<(u32, u32, u32)>, // (virtual address, raw size, raw offset)
}

impl Exe {
    fn open(game: &GameDir) -> Option<Exe> {
        let data = std::fs::read(game.root.join("State of War.exe")).ok()?;
        let u32at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
        let u16at = |o: usize| u16::from_le_bytes(data[o..o + 2].try_into().unwrap()) as usize;
        let pe = u32at(0x3c) as usize;
        let (n, opt_size) = (u16at(pe + 6), u16at(pe + 20));
        let image_base = u32at(pe + 24 + 28);
        let sections = (0..n)
            .map(|i| {
                let o = pe + 24 + opt_size + i * 40;
                (u32at(o + 12), u32at(o + 16), u32at(o + 20))
            })
            .collect();
        Some(Exe { data, image_base, sections })
    }

    fn at(&self, va: u32, len: usize) -> &[u8] {
        let rva = va - self.image_base;
        let (sva, _, raw) = *self.sections.iter().find(|(s, size, _)| rva >= *s && rva < s + size).expect("address in a section");
        let o = (rva - sva + raw) as usize;
        &self.data[o..o + len]
    }

    /// What a table target returns in eax: `mov eax, imm32` (B8) or `xor eax, eax` (31 C0 / 33 C0).
    fn returns(&self, va: u32) -> u32 {
        let b = self.at(va, 5);
        match b[0] {
            0xB8 => u32::from_le_bytes(b[1..5].try_into().unwrap()),
            0x31 | 0x33 if b[1] == 0xC0 => 0,
            _ => panic!("unexpected code at {va:#x}: {b:02x?}"),
        }
    }

    fn table(&self, va: u32, n: usize) -> Vec<u32> {
        self.at(va, n * 4).chunks_exact(4).map(|c| self.returns(u32::from_le_bytes(c.try_into().unwrap()))).collect()
    }
}

#[test]
fn facing_port_matches_the_exes_jump_tables() {
    use opensow_formats::facing::facing;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // Heading with a counter-clockwise angle `theta` from east, as a point 1000 px away (y down on screen).
    let heading = |theta: f64| {
        let r = theta.to_radians();
        (1000 + (1000.0 * r.cos()).round() as i32, 1000 - (1000.0 * r.sin()).round() as i32)
    };
    let t32 = exe.table(0x46b350, 32);
    for (s, &want) in t32.iter().enumerate() {
        let (bx, by) = heading(s as f64 * 11.25);
        assert_eq!(facing(1000, 1000, bx, by, 32), want, "32-step sector {s}");
    }
    let t16 = exe.table(0x46b3d0, 17);
    for (s, &want) in t16.iter().enumerate().take(16) {
        let (bx, by) = heading(s as f64 * 22.5);
        assert_eq!(facing(1000, 1000, bx, by, 16), want, "16-step sector {s}");
    }
    // The wrap-around sector (angle just under 360 degrees) uses table entry 16, which repeats entry 15's value.
    assert_eq!(t16[16], t16[15]);
    let (bx, by) = heading(355.0);
    assert_eq!(facing(1000, 1000, bx, by, 16), t16[16]);
}

impl Exe {
    /// The value stored by `mov dword ptr [esi+field], imm32` in the straight-line code at `va`, which ends at the
    /// first jump. The per-kind cases only contain such stores (`C7 46 disp8 imm32` / `C7 86 disp32 imm32`).
    ///
    /// The Advancer's case (kind 40) mixes in a `lea` and a `call`, so for it the whole case `[va, end)` is searched byte by byte.
    fn store_imm(&self, va: u32, end: u32, field: u8) -> Option<u32> {
        if va == 0x47001d {
            let code = self.at(va, (end - va) as usize);
            return (0..code.len() - 6).find(|&i| code[i..i + 3] == [0xC7, 0x46, field]).map(|i| u32::from_le_bytes(code[i + 3..i + 7].try_into().unwrap()));
        }
        let code = self.at(va, 200);
        let mut i = 0;
        while i + 7 <= code.len() {
            match (code[i], code[i + 1]) {
                (0xC7, 0x46) => {
                    if code[i + 2] == field {
                        return Some(u32::from_le_bytes(code[i + 3..i + 7].try_into().unwrap()));
                    }
                    i += 7;
                }
                (0xC7, 0x86) => i += 10,
                _ => return None, // a jump (E9/EB) or code this scanner does not know ends the case
            }
        }
        None
    }
}

#[test]
fn walk_parameters_match_the_exes_chassis_initialiser() {
    use opensow_formats::anim::walk_params;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let byte_table = exe.at(0x470408, 46).to_vec();
    let cases = *byte_table.iter().max().unwrap() as usize + 1;
    let jt: Vec<u32> = exe.at(0x4703b4, cases * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in (1..=15).chain(40..=46) {
        let target = jt[byte_table[kind - 1] as usize];
        // The cases are laid out one after another: a case ends where the next higher case starts.
        let end = jt.iter().copied().filter(|&t| t > target).min().unwrap_or(target + 160);
        let want = walk_params(kind as i32).unwrap_or_else(|| panic!("no params for kind {kind}"));
        let phases = exe.store_imm(target, end, 0x48).unwrap_or(0); // the default before the switch is 0
        let period = exe.store_imm(target, end, 0x50).unwrap_or(0x3e8); // default 0x3e8
        assert_eq!((phases, period), (want.phases, want.period), "kind {kind}");
        let delay = exe.store_imm(target, end, 0x3c).unwrap_or(4);
        assert_eq!(delay, want.turn_delay, "kind {kind} turn delay");
        let steps = exe.store_imm(target, end, 0x08).unwrap_or(0x20);
        assert_eq!(steps, want.steps, "kind {kind} facings per phase");
        let speed = exe.store_imm(target, end, 0x18).expect("every kind stores a speed");
        assert_eq!(speed, want.speed, "kind {kind} walking speed (px per second)");
    }
}

#[test]
fn chassis_frame_counts_are_phases_times_steps() {
    use opensow_formats::{
        anim::{steps_for, walk_params},
        megafile::MegaFile,
        sprite,
        units::unit_art,
    };
    let Some(game) = GameDir::locate() else { return };
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    for kind in (1..=15).chain(40..=46) {
        let (art, p) = (unit_art(kind).unwrap(), walk_params(kind).unwrap());
        let e = mega.find(&format!("units\\{}_blue.ps6", art.chassis)).unwrap();
        let frames = sprite::parse(&mega.read(e).unwrap(), sprite::Kind::Color).unwrap().len();
        let steps = steps_for(frames, p.phases);
        assert_eq!(steps, p.steps, "kind {kind} ({}) has {frames} frames", art.chassis);
        assert_eq!(frames as u32, p.phases.max(1) * steps, "kind {kind}");
    }
}

#[test]
fn unit_art_chassis_index_matches_the_exes_initialiser() {
    use opensow_formats::units::{unit_art, CHASSIS};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let byte_table = exe.at(0x470408, 46).to_vec();
    let cases = *byte_table.iter().max().unwrap() as usize + 1;
    let jt: Vec<u32> = exe.at(0x4703b4, cases * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in (1..=15).chain(40..=46) {
        let target = jt[byte_table[kind - 1] as usize];
        let end = jt.iter().copied().filter(|&t| t > target).min().unwrap_or(target + 160);
        let chassis = exe.store_imm(target, end, 0x1c).unwrap_or_else(|| panic!("kind {kind}: no chassis store")) as usize;
        assert_eq!(unit_art(kind as i32).unwrap().chassis, CHASSIS[chassis], "kind {kind}");
    }
}

impl Exe {
    /// Decodes a weapon-top constructor case (`0x47a000..`) by following its unconditional jumps (cases share tail code)
    /// until the common tail `0x47ab5f`/`0x47ab66`. Understands `push imm`, `mov ecx, edi`, `mov [edi+disp], imm32` and
    /// `call 0x479760`. Returns the fields stored (later stores win) and the steps added (phase, flash group, barrel, shots).
    fn top_case(&self, va: u32) -> TopCase {
        let (mut pc, mut pushes) = (va, Vec::<i32>::new());
        let (mut fields, mut steps) = (std::collections::HashMap::new(), Vec::new());
        for _ in 0..400 {
            if pc == 0x47ab5f || pc == 0x47ab66 {
                return (fields, steps);
            }
            let code = self.at(pc, 12).to_vec();
            let imm32 = |o: usize| i32::from_le_bytes(code[o..o + 4].try_into().unwrap());
            match code[0] {
                0x6A => {
                    pushes.push(code[1] as i8 as i32);
                    pc += 2
                }
                0x68 => {
                    pushes.push(imm32(1));
                    pc += 5
                }
                0x8B if code[1] == 0xCF => pc += 2, // mov ecx, edi
                0xC7 if code[1] == 0x47 => {
                    fields.insert(code[2] as u32, imm32(3));
                    pc += 7
                }
                0xC7 if code[1] == 0x87 => {
                    fields.insert(u32::from_le_bytes(code[2..6].try_into().unwrap()), imm32(6));
                    pc += 10
                }
                0xE8 => {
                    let target = (pc + 5).wrapping_add(imm32(1) as u32);
                    assert_eq!(target, 0x479760, "unexpected call at {pc:#x}");
                    let p = &pushes[pushes.len() - 4..]; // pushed as shots, barrel, flash group, phase
                    steps.push((p[3], p[2], p[1], p[0]));
                    pushes.clear();
                    pc += 5
                }
                0xE9 => pc = (pc + 5).wrapping_add(imm32(1) as u32),
                0xEB => pc = (pc + 2).wrapping_add(code[1] as i8 as i32 as u32),
                op => panic!("unknown opcode {op:#x} at {pc:#x}"),
            }
        }
        panic!("case at {va:#x} did not reach the common tail");
    }
}

#[test]
fn fire_sequences_match_the_exes_top_constructor() {
    use opensow_formats::recoil::{fire_sequence, FLASH_SETS};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let byte_table = exe.at(0x47ac70, 0x71).to_vec();
    let cases = *byte_table.iter().max().unwrap() as usize + 1;
    let jt: Vec<u32> = exe.at(0x47ac04, cases * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in (1..=15).chain(41..=45).chain(109..=113) {
        let (fields, steps) = exe.top_case(jt[byte_table[kind - 1] as usize]);
        let want = fire_sequence(kind as i32).unwrap_or_else(|| panic!("no sequence for kind {kind}"));
        let got: Vec<_> = want.steps.iter().map(|s| (s.phase as i32, s.flash_group as i32, s.barrel as i32, s.shots as i32)).collect();
        assert_eq!(steps, got, "kind {kind} steps");
        assert_eq!(fields.get(&0x210).copied().unwrap_or(10) as u32, want.reload, "kind {kind} reload");
        assert_eq!(fields.get(&0x58).copied(), want.flash_set.map(|v| v as i32), "kind {kind} flash set");
    }
    assert_eq!(FLASH_SETS.len(), 9);
}

#[test]
fn recoil_sequences_are_consistent_with_the_sprites_and_fire_tables() {
    use opensow_formats::{
        fire::FireTables,
        megafile::MegaFile,
        recoil::{fire_sequence, FLASH_SETS},
        sprite,
        units::unit_art,
    };
    let Some(game) = GameDir::locate() else { return };
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    let frames = |name: &str| {
        let e = mega.find(name).unwrap_or_else(|| panic!("{name} missing"));
        sprite::parse(&mega.read(e).unwrap(), sprite::Kind::Color).unwrap().len() as i32
    };
    for kind in (1..=15).chain(41..=45) {
        let seq = fire_sequence(kind).unwrap();
        let top = unit_art(kind).unwrap().top.unwrap();
        let steps_per_phase = if frames(&format!("units\\{top}_blue.ps6")) % 32 == 0 { 32 } else { 16 };
        let top_frames = frames(&format!("units\\{top}_blue.ps6"));
        let max_phase = seq.steps.iter().map(|s| s.phase as i32).max().unwrap();
        assert!((max_phase + 1) * steps_per_phase <= top_frames, "{top}: phase {max_phase} needs more than {top_frames} frames");
        let e = mega.find(&format!("units\\{top}.po1")).unwrap();
        let tables = FireTables::parse(&mega.read(e).unwrap()).unwrap().variants() as i32;
        for st in seq.steps.iter().filter(|s| s.barrel >= 0) {
            assert!(st.barrel as i32 + st.shots as i32 <= tables, "{top}: barrel {} + {} shots exceeds {tables} tables", st.barrel, st.shots);
        }
        if let Some(set) = seq.flash_set {
            let name = FLASH_SETS[set as usize];
            assert_eq!(name, top, "flash set of kind {kind}");
            let groups = seq.steps.iter().map(|s| s.flash_group as i32).max().unwrap() + 1;
            assert!(groups * steps_per_phase <= frames(&format!("units\\{name}_fire.ps6")), "{name}_fire has too few frames for {groups} groups");
        }
    }
}

#[test]
fn aim_parameters_match_the_exes_top_constructor() {
    use opensow_formats::aim::{aim_params, SEARCH_POLL_TICKS, TURN_DELAY};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let byte_table = exe.at(0x47ac70, 0x71).to_vec();
    let cases = *byte_table.iter().max().unwrap() as usize + 1;
    let jt: Vec<u32> = exe.at(0x47ac04, cases * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in (1..=15).chain(41..=45).chain(109..=113) {
        let (f, _) = exe.top_case(jt[byte_table[kind - 1] as usize]);
        let want = aim_params(kind as i32).unwrap_or_else(|| panic!("no aim params for kind {kind}"));
        let get = |k: u32, default: i32| f.get(&k).copied().unwrap_or(default) as u32;
        assert_eq!(
            (get(0x18, -1), get(0x1c, -1), get(0x28, -1), get(0x2c, 0), get(0x50, 32)),
            (want.min_range, want.max_range, want.reload, want.jitter, want.steps),
            "kind {kind}"
        );
    }
    // Constructor prologue: turn counter and reload (`[obj+0x74]`, `[obj+0x78]`) start at 4; default facings 32 (`[obj+0x50]`).
    let ctor = exe.at(0x479ee0, 0x120);
    let has = |pat: &[u8]| ctor.windows(pat.len()).any(|w| w == pat);
    assert!(has(&[0xC7, 0x47, 0x78, 4, 0, 0, 0]) && has(&[0xC7, 0x47, 0x74, 4, 0, 0, 0]), "turn counter constants");
    assert_eq!(TURN_DELAY, 4);
    assert!(has(&[0xC7, 0x47, 0x50, 0x20, 0, 0, 0]), "default facings");
    // State 0 reloads its search counter `[obj+0x80]` with 30.
    let state0 = exe.at(0x479ca4, 0x80);
    assert!(state0.windows(10).any(|w| w == [0xC7, 0x87, 0x80, 0, 0, 0, 30, 0, 0, 0]), "search poll constant");
    assert_eq!(SEARCH_POLL_TICKS, 30);
}

#[test]
fn weapon_shots_match_the_exes_top_constructor() {
    use opensow_formats::projectile::weapon_shot;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let byte_table = exe.at(0x47ac70, 0x71).to_vec();
    let cases = *byte_table.iter().max().unwrap() as usize + 1;
    let jt: Vec<u32> = exe.at(0x47ac04, cases * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in (1..=15).chain(41..=45).chain(109..=113) {
        let (f, _) = exe.top_case(jt[byte_table[kind - 1] as usize]);
        let want = weapon_shot(kind as i32).unwrap_or_else(|| panic!("no shot for kind {kind}"));
        assert_eq!((f[&0x34] as u8, f[&0x38] as u32), want, "kind {kind}");
        // `[top+0x30]` = projectiles per table and shot (the spawner `0x47cf40` repeats its whole table loop that often): 1, only turret 5 has 6
        assert_eq!(f.get(&0x30).copied(), Some(if kind == 113 { 6 } else { 1 }), "kind {kind} burst");
    }
}

impl Exe {
    /// Fields stored by a bullet-type case of the constructor `0x411dc0` (`mov [esi+disp], imm32`), following jumps to the
    /// common end `0x412468`. Returns `None` for cases that contain conditional jumps (loops), which are not decoded.
    fn bullet_case(&self, va: u32) -> Option<std::collections::HashMap<u32, i32>> {
        let (mut pc, mut fields) = (va, std::collections::HashMap::new());
        for _ in 0..200 {
            if pc == 0x412468 {
                return Some(fields);
            }
            let code = self.at(pc, 12).to_vec();
            let imm32 = |o: usize| i32::from_le_bytes(code[o..o + 4].try_into().unwrap());
            match (code[0], code[1]) {
                (0xC7, 0x86) => {
                    fields.insert(u32::from_le_bytes(code[2..6].try_into().unwrap()), imm32(6));
                    pc += 10
                }
                (0xC7, 0x46) => {
                    fields.insert(code[2] as u32, imm32(3));
                    pc += 7
                }
                (0xC6, 0x86) => pc += 7, // mov byte ptr [esi+disp32], imm8
                // random initial value: push n; call 0x437b10; movzx eax, ax; add esp, 4; mov [esi+disp32], eax
                (0x6A, _) => pc += 2,
                (0xE8, _) if (pc + 5).wrapping_add(imm32(1) as u32) == 0x437b10 => pc += 5,
                (0x0F, 0xB7) => pc += 3,
                (0x83, 0xC4) => pc += 3,
                (0x89, 0x80..=0xBF) => pc += 6, // mov [esi+disp32], reg
                (0xE9, _) => pc = (pc + 5).wrapping_add(imm32(1) as u32),
                (0xEB, _) => pc = (pc + 2).wrapping_add(code[1] as i8 as i32 as u32),
                (0x7C..=0x7F, _) | (0x74, _) | (0x75, _) => return None,
                _ => return None, // anything else (loops, calls) is outside what this decoder understands
            }
        }
        None
    }
}

#[test]
fn bullet_types_match_the_exes_constructor() {
    use opensow_formats::projectile::{bullet_type, BULLET_TYPES};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let jt: Vec<u32> = exe.at(0x412490, 35 * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let mut verified = 0;
    for t in 1..=35u8 {
        let want = bullet_type(t).unwrap();
        let Some(f) = exe.bullet_case(jt[t as usize - 1]) else {
            // 13/14/32 compute their speed with the random generator, 19/20/21 fill spark arrays in loops (sparks.rs), 23 branches on its variant, 28/29 also trigger an effect, 31 branches on a comparison.
            assert!(want.partial || [13, 14, 19, 20, 21, 23, 28, 29, 31, 32].contains(&t), "type {t} could not be decoded but is not marked partial");
            continue;
        };
        let get = |k: u32| f.get(&k).map(|&v| v as u32);
        assert_eq!(get(0x2664), want.speed, "type {t} speed");
        assert_eq!(get(0x264c), want.interval, "type {t} interval");
        let anim = [0x2670, 0x2674, 0x2678, 0x267c].map(|k| f.get(&k).copied());
        let anim = anim.iter().all(Option::is_some).then(|| anim.map(Option::unwrap));
        assert_eq!(anim, want.anim, "type {t} anim");
        verified += 1;
    }
    assert!(verified >= 24, "only {verified} bullet types could be checked against the exe");
    assert_eq!(BULLET_TYPES.len(), 35);
}

#[test]
fn production_slot_values_match_the_exes_slot_initialiser() {
    use opensow_formats::production::{slot_target, slot_unlock_cost};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let byte_table = exe.at(0x40e9b4, 46).to_vec();
    let jt: Vec<u32> = exe.at(0x40e9a4, 4 * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    // `mov dword ptr [esi+field], imm32` (C7 46 field imm32) inside the 40 bytes of a case
    let store = |va: u32, field: u8| {
        let code = exe.at(va, 40);
        (0..code.len() - 6).find(|&i| code[i..i + 3] == [0xC7, 0x46, field]).map(|i| u32::from_le_bytes(code[i + 3..i + 7].try_into().unwrap()))
    };
    for kind in 1..=46i32 {
        let case = jt[byte_table[kind as usize - 1] as usize];
        // cases 0..2 store `[+0xc]` then double it; the "Unknown unit" case stores nothing
        let target = store(case, 0x0c).map(|v| v * 2);
        assert_eq!(target, slot_target(kind), "kind {kind} target");
        let cost = store(case, 0x1c);
        assert_eq!(cost, slot_unlock_cost(kind), "kind {kind} unlock cost");
    }
}

#[test]
fn building_hit_points_match_the_exes_constructor() {
    use opensow_formats::advancer::building_max_hp;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // constructor 0x40a330: `idx = kind - 0x19`; byte table 0x40afd4 -> jump table 0x40afb8
    let byte_table = exe.at(0x40afd4, 0x54).to_vec();
    let jt: Vec<u32> = exe.at(0x40afb8, 8 * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in 25..=108i32 {
        let case = jt[byte_table[kind as usize - 0x19] as usize];
        let code = exe.at(case, 24);
        // `mov dword ptr [ebx+0x70], imm32` = C7 43 70 imm32
        let hp = (0..code.len() - 6).find(|&i| code[i..i + 3] == [0xC7, 0x43, 0x70]).map(|i| u32::from_le_bytes(code[i + 3..i + 7].try_into().unwrap()) as i32);
        assert_eq!(hp, Some(building_max_hp(kind)), "kind {kind}");
    }
}

#[test]
fn advancer_beam_period_comes_from_the_exes_cooldown_member() {
    use opensow_formats::advancer::{BeamTimer, BEAM_PERIOD};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let imm = |b: &[u8], pat: &[u8]| -> Option<u32> {
        (0..b.len() - pat.len() - 3).find(|&i| b[i..i + pat.len()] == *pat).map(|i| u32::from_le_bytes(b[i + pat.len()..i + pat.len() + 4].try_into().unwrap()))
    };
    // 0x47c630 (cooldown member constructor): `mov [ecx+0x20], 10` (reload), `mov [ecx+0x24], 0` (spread)
    let ctor = exe.at(0x47c630, 0x70);
    assert_eq!(imm(ctor, &[0xC7, 0x41, 0x20]), Some(BEAM_PERIOD));
    assert_eq!(imm(ctor, &[0xC7, 0x41, 0x24]), Some(0));
    // the Advancer top's constructor (0x470048..) stores 10 in [top+0x124] = member +0x20 and nothing else changes it
    let top = exe.at(0x470048, 0x40);
    assert_eq!(imm(top, &[0xC7, 0x86, 0x24, 0x01, 0x00, 0x00]), Some(BEAM_PERIOD));
    // 0x47c6b0 tick: `mov esi,[ecx+0x20]`, state 5 / 6 compare chain (`sub eax,5` ... `sub eax,1`)
    let tick = exe.at(0x47c6b0, 0x50);
    assert_eq!(&tick[10..13], &[0x8B, 0x71, 0x20]);
    assert_eq!(&tick[0x1d..0x20], &[0x83, 0xE8, 0x05]);
    assert_eq!(&tick[0x22..0x25], &[0x83, 0xE8, 0x01]);
    assert_eq!(BeamTimer::default().reload as u32, BEAM_PERIOD);
}

#[test]
fn tech_level_tiers_match_the_exes_slot_upgrade() {
    use opensow_formats::production::next_tier;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // 0x40e730: jump table 0x40e854 indexed by kind-1 (kinds 1..=23); a target is either `mov eax, imm` (B8) or the exit 0x40e84b
    let jt: Vec<u32> = exe.at(0x40e854, 23 * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in 1..=23i32 {
        let t = jt[kind as usize - 1];
        let next = if t == 0x40e84b { None } else { Some(exe.returns(t) as i32) };
        assert_eq!(next, next_tier(kind), "kind {kind}");
    }
    assert_eq!(next_tier(24), None);
}

#[test]
fn income_tables_match_the_exes_economy_loop() {
    use opensow_formats::production::income_per_tick;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // 0x42d6b0: `lea eax,[edx+N]` (8D 42 N) per active level; tables 0x42e558 (research) and 0x42e56c (credits)
    for table in [0x42e558u32, 0x42e56c] {
        let jt: Vec<u32> = exe.at(table, 5 * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        for (i, &t) in jt.iter().enumerate() {
            let b = exe.at(t, 3);
            assert_eq!(&b[..2], &[0x8D, 0x42]);
            assert_eq!(b[2] as i32, income_per_tick(i + 1), "table {table:#x} level {}", i + 1);
        }
    }
}

#[test]
fn factory_exit_cells_match_the_exes_constructor() {
    use opensow_formats::production::factory_exit;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // `mov dword ptr [ebx+disp32], imm32` = C7 83 disp32 imm32, inside the constructor cases of kinds 101 (0x40a596..) and 102 (0x40a636..)
    let find = |from: u32, len: usize, disp: u32| {
        let code = exe.at(from, len);
        (0..code.len() - 10)
            .find(|&i| code[i..i + 2] == [0xC7, 0x83] && u32::from_le_bytes(code[i + 2..i + 6].try_into().unwrap()) == disp)
            .map(|i| i32::from_le_bytes(code[i + 6..i + 10].try_into().unwrap()))
    };
    for (kind, from) in [(101, 0x40a596u32), (102, 0x40a636)] {
        let e = factory_exit(kind).unwrap();
        let got = |d| find(from, 0xa0, d).unwrap();
        assert_eq!((got(0x124), got(0x128)), e.spawn, "kind {kind} spawn");
        assert_eq!((got(0x12c), got(0x130)), e.primary, "kind {kind} walk-to");
        assert_eq!((got(0x134), got(0x138)), e.alternative, "kind {kind} alternative walk-to");
    }
}

#[test]
fn building_footprints_match_the_exes_tables() {
    use opensow_formats::buildings::{footprint, LAYOUTS};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    for (i, l) in LAYOUTS.iter().enumerate() {
        let raw = exe.at(0x486b20 + 100 * i as u32, 100);
        let got: Vec<i32> = raw.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();
        assert_eq!(&got[..], &l[..], "layout {i}");
    }
    // 0x409760: byte table 0x409944 -> jump table 0x409910; the cases store width, height and layout through eax: `C7 00 imm32`
    let byte_table = exe.at(0x409944, 0x54).to_vec();
    let jt: Vec<u32> = exe.at(0x409910, 4 * 12).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in [25, 26, 27, 28, 29, 100, 101, 102, 103, 104, 105, 106, 107, 108] {
        let code = exe.at(jt[byte_table[kind as usize - 0x19] as usize], 40);
        let imm: Vec<i32> = (0..code.len() - 5)
            .filter(|&i| code[i..i + 2] == [0xC7, 0x00])
            .map(|i| i32::from_le_bytes(code[i + 2..i + 6].try_into().unwrap()))
            .take(3)
            .collect();
        // stores in the order width ([ebp+0x10]), height ([ebp+0x14]), layout ([ebp+0xc])
        assert_eq!(imm, vec![footprint(kind).unwrap().0, footprint(kind).unwrap().1, footprint(kind).unwrap().2 as i32], "kind {kind}");
    }
}

#[test]
fn tripler_blast_ring_matches_the_exes_static_initialiser() {
    use opensow_formats::projectile::TRIPLER_RING;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // 0x412520: `movaps xmm0,[src]` (0F 28 05 imm32) / `movups [dst],xmm0` (0F 11 05 imm32) pairs fill 0x513328..
    let code = exe.at(0x412520, 0x120).to_vec();
    let (mut src, mut table) = (0u32, std::collections::BTreeMap::new());
    let mut i = 0;
    while i + 7 <= code.len() {
        let imm = |o: usize| u32::from_le_bytes(code[i + o..i + o + 4].try_into().unwrap());
        if code[i..i + 3] == [0x0F, 0x28, 0x05] {
            src = imm(3);
            i += 7;
        } else if code[i..i + 3] == [0x0F, 0x11, 0x05] {
            table.insert(imm(3), exe.at(src, 16).to_vec());
            i += 7;
        } else if code[i] == 0xC3 {
            break;
        } else {
            i += 1;
        }
    }
    let blob: Vec<i32> = table.values().flatten().copied().collect::<Vec<u8>>().chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();
    let want: Vec<i32> = TRIPLER_RING.iter().flat_map(|&(a, b)| [a, b]).collect();
    assert_eq!(&blob[..want.len().min(blob.len())], &want[..want.len().min(blob.len())]);
    assert!(blob.len() >= 64, "{} words", blob.len());
}

#[test]
fn generic_file_is_the_exes_random_table() {
    use opensow_formats::rng::{GameRng, TABLE_LEN};
    let Some(game) = GameDir::locate() else { return };
    let data = std::fs::read(game.data().join("generic.file")).expect("generic.file");
    assert_eq!(data.len(), 111_111);
    let mut r = GameRng::from_file(&data).expect("table");
    let first = u16::from_le_bytes([data[0], data[1]]);
    assert_eq!(r.next(1000), first % 1000);
    // the exe's wrap test `cmp ecx, 0xc346; cmovg` and the table size check
    let exe = Exe::open(&game).expect("exe");
    let code = exe.at(0x437b10, 0x40).to_vec();
    assert!(code.windows(6).any(|w| w == [0x81, 0xF9, 0x46, 0xC3, 0x00, 0x00]), "cmp ecx, 0xc346");
    assert_eq!(TABLE_LEN, 0xc346 + 1);
}

#[test]
fn message_table_matches_the_exes_event_switch_and_speech_files() {
    use opensow_formats::messages::MESSAGES;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // 0x402ec0: jump table 0x4034ac indexed by id-1; each case starts with `push imm` (6A imm8 / 68 imm32) of the string index
    let jt: Vec<u32> = exe.at(0x4034ac, 26 * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for (id, string, wav) in MESSAGES {
        let code = exe.at(jt[id as usize - 1], 5);
        let idx = match code[0] {
            0x6A => code[1] as usize,
            0x68 => u32::from_le_bytes(code[1..5].try_into().unwrap()) as usize,
            b => panic!("event {id}: opcode {b:#x}"),
        };
        assert_eq!(idx, string, "event {id}");
        if let Some(w) = wav {
            assert!(game.data().join("Sounds").join("Speech").join(w).exists(), "{w}");
        }
    }
}

#[test]
fn sfx_table_files_exist_and_match_the_exes_loader() {
    use opensow_formats::sfx::SFX;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // two sounds the exe loads are absent from the shipped data (it tolerates that): mainmenu-fly (31) and airicon-come (49)
    let missing: Vec<u8> = SFX.iter().filter(|(_, path)| game.resolve(&format!("Data\\Sounds\\{}", path.replace('/', "\\"))).is_none()).map(|s| s.0).collect();
    assert_eq!(missing, vec![31, 49]);
    // the loader passes the id as `push imm8` before `call 0x407470`: ids 0..=0x48
    let code = exe.at(0x406dc0, 0x6a0);
    let calls = (0..code.len() - 4)
        .filter(|&i| code[i] == 0xE8 && (0x406dc0 + i as u32 + 5).wrapping_add(i32::from_le_bytes(code[i + 1..i + 5].try_into().unwrap()) as u32) == 0x407470)
        .count();
    assert_eq!(calls, SFX.len(), "one load call per table entry");
}

impl Exe {
    /// Virtual address of the NUL-terminated string `s` in the image.
    fn string_va(&self, s: &str) -> u32 {
        let mut needle = s.as_bytes().to_vec();
        needle.push(0);
        for &(va, size, raw) in &self.sections {
            let hay = &self.data[raw as usize..(raw + size) as usize];
            if let Some(i) = hay.windows(needle.len()).position(|w| w == &needle[..]) {
                return self.image_base + va + i as u32;
            }
        }
        panic!("string {s:?} not in the image");
    }

    /// Sprite arrays and blit calls of a bullet-draw case: `push [abs]` (FF 35) and `push [reg*4+abs]` (FF 34 xx) give the
    /// arrays, `call rel32` the routines; unconditional jumps are followed; stops at `ret 8`.
    fn draw_case(&self, mut va: u32) -> (Vec<u32>, Vec<u32>) {
        let (mut arrays, mut calls) = (Vec::new(), Vec::new());
        let mut jumps = 0;
        let mut i = 0usize;
        loop {
            let code = self.at(va + i as u32, 8);
            match code[0] {
                0xC2 => return (arrays, calls),
                0xFF if code[1] == 0x35 => {
                    arrays.push(u32::from_le_bytes(code[2..6].try_into().unwrap()));
                    i += 6;
                }
                0xFF if code[1] == 0x34 && code[2] == 0x85 => {
                    arrays.push(u32::from_le_bytes(code[3..7].try_into().unwrap()));
                    i += 7;
                }
                0xE8 => {
                    let rel = i32::from_le_bytes(code[1..5].try_into().unwrap());
                    calls.push((va + i as u32 + 5).wrapping_add(rel as u32));
                    i += 5;
                }
                0xE9 | 0xEB => {
                    let (rel, len) = if code[0] == 0xE9 { (i32::from_le_bytes(code[1..5].try_into().unwrap()), 5) } else { (code[1] as i8 as i32, 2) };
                    va = (va + i as u32 + len).wrapping_add(rel as u32);
                    i = 0;
                    jumps += 1;
                    assert!(jumps < 4, "too many jumps");
                }
                _ => i += 1, // bytes of instructions this scan does not need (they never contain the three patterns above in this code)
            }
            assert!(i < 400, "no ret found");
        }
    }
}

#[test]
fn flamer_trail_branch_matches_the_exes_flight_step() {
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // 0x4118e2: cmp edx,8 / je / cmp edx,9 / jne; counter [+0x264c] dec, reload [+0x2650]; puff at site 0x411952
    assert_eq!(exe.at(0x4118e2, 3), [0x83, 0xFA, 0x08]);
    assert_eq!(exe.at(0x4118e7, 3), [0x83, 0xFA, 0x09]);
    assert_eq!(exe.at(0x4118f0, 6), [0xFF, 0x8E, 0x4C, 0x26, 0x00, 0x00]);
    assert_eq!(exe.at(0x411903, 6), [0x8B, 0x86, 0x50, 0x26, 0x00, 0x00]);
    assert!(opensow_formats::effects::flame_trail(9).is_some() && opensow_formats::effects::flame_trail(6).is_none());
    assert_eq!(opensow_formats::projectile::bullet_type(9).unwrap().interval, Some(6));
}

#[test]
fn extras_table_matches_the_exes_loader() {
    use opensow_formats::effects::EXTRAS;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // every load is `push cap; push dest; push ecx; mov ecx, esp; push name` (cap as 6A imm8 or 68 imm32)
    let code = exe.at(0x424700, 0xf00);
    for e in EXTRAS {
        let name = exe.string_va(e.name);
        let mut pat = if e.slots < 0x80 { vec![0x6A, e.slots as u8] } else { [&[0x68][..], &(e.slots as u32).to_le_bytes()].concat() };
        pat.push(0x68);
        pat.extend(e.addr.to_le_bytes());
        pat.extend([0x51, 0x8B, 0xCC]);
        assert!(code.windows(pat.len()).any(|w| w == &pat[..]), "{} not loaded as {} slots at {:#x}", e.name, e.slots, e.addr);
        // the name follows (a branch on the low-detail flag may sit between for the two explosion sprites)
        let mut push_name = vec![0x68];
        push_name.extend(name.to_le_bytes());
        assert!(code.windows(5).any(|w| w == &push_name[..]), "{} name push", e.name);
        let file = game.resolve("Data\\sprites.data");
        assert!(file.is_some());
    }
}

#[test]
fn bullet_draw_table_matches_the_exes_draw_function() {
    use opensow_formats::effects::bullet_draw;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let jt: Vec<u32> = exe.at(0x410e7c, 34 * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for ty in 1..=34u8 {
        let (arrays, calls) = exe.draw_case(jt[ty as usize - 1]);
        let layers = bullet_draw(ty);
        // the particle types loop over their fragments but have one array and one call in the code
        assert_eq!(arrays.len(), layers.len(), "type {ty} arrays");
        assert_eq!(calls.len(), layers.len(), "type {ty} calls");
        for (i, l) in layers.iter().enumerate() {
            assert_eq!((arrays[i], calls[i]), (l.array, l.routine), "type {ty} layer {i}");
        }
    }
}

#[test]
fn impact_sounds_match_the_exes_impact_function() {
    use opensow_formats::effects::impact_sound;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let map = exe.at(0x410a14, 26).to_vec();
    let jt: Vec<u32> = exe.at(0x4109f0, 9 * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for ty in 1..=26u8 {
        let target = jt[map[ty as usize - 1] as usize];
        let want = if target == 0x40f66a {
            None
        } else {
            // the case pushes the sound id (6A imm8) last before it jumps to the shared call at 0x40f660
            let code = exe.at(target, 48);
            let (mut i, mut id) = (0, None);
            loop {
                match code[i] {
                    0x6A => {
                        id = Some(code[i + 1]);
                        i += 2;
                    }
                    0xE8 => i += 5,
                    0x0F => i += 6,
                    0xFF => i += 3,
                    0xEB | 0xE9 | 0xB9 => break, // the jump to the shared call, or (type 24 and 25) the call setup itself
                    _ => i += 1,
                }
            }
            id
        };
        if ty == 22 {
            continue; // type 22 is excluded before the table (cmp edx, 0x16; je)
        }
        assert_eq!(impact_sound(ty), want, "type {ty}");
    }
}

#[test]
fn animation_spawn_sites_match_the_exes_calls() {
    use opensow_formats::effects::SPAWNS;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    for s in SPAWNS {
        // push flag; push repeat; push to; push from; push array; push period; (push y; push x ...) call
        let window = exe.at(s.site - 64, 64);
        let imm = |v: u32| if v < 0x80 { vec![0x6A, v as u8] } else { [&[0x68][..], &v.to_le_bytes()].concat() };
        let tokens = [
            imm(s.spec.repeat as u32),
            imm(s.spec.to as u32),
            imm(s.spec.from as u32),
            [&[0x68][..], &s.spec.array.to_le_bytes()].concat(),
            imm(s.spec.period as u32),
        ];
        // the argument pushes appear in this order, possibly with other instructions between them
        let mut at = 0;
        for t in &tokens {
            let p =
                window[at..].windows(t.len()).position(|w| w == &t[..]).unwrap_or_else(|| panic!("site {:#x} ({}): token {t:02x?} missing", s.site, s.what));
            at += p + t.len();
        }
        assert_eq!(exe.at(s.site, 1)[0], 0xE8, "site {:#x} is a call", s.site);
    }
}

#[test]
fn the_advancer_gun_beam_matches_the_exes_flight_step_and_constructor() {
    use opensow_formats::effects::{gun_trail, BODKY_ARRAYS, BODKY_STRIDE, GUN_TRAIL_SITE};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let has = |va: u32, pat: &[u8]| exe.at(va, pat.len()) == pat;
    // flight step: `cmp eax, 0x1f`, then (after the counter) push flag 1, repeat 15, to 1; array = 0x5e6b80 + 80 * [+0x268c]; from 0
    assert!(has(0x411b12, &[0x83, 0xF8, 0x1F]));
    assert!(has(0x411b67, &[0x6A, 0x01, 0x6A, 0x0F, 0x6A, 0x01, 0x8D, 0x04, 0x80]), "flag, repeat 15, to 1, 5 * colour");
    assert!(has(0x411b79, &[0xC1, 0xE0, 0x04, 0x05, 0x80, 0x6B, 0x5E, 0x00]), "<< 4, + bodky_r");
    assert!(has(0x411b77, &[0x6A, 0x00]), "from 0");
    assert!(has(0x411b82, &[0x6A, 0x01]), "period 1");
    assert_eq!(exe.at(GUN_TRAIL_SITE, 1)[0], 0xE8, "call");
    assert!(has(0x411b8f, &[0xC7, 0x47, 0x34, 6, 0, 0, 0]), "additive draw mode 6");
    // constructor case: `setne al` on the player, `[+0x268c] = 2 * al + 1`, interval 2, speed 600
    assert!(has(0x412258, &[0x0F, 0x95, 0xC0]));
    assert!(has(0x412269, &[0x8D, 0x04, 0x45, 0x01, 0, 0, 0]) && has(0x412270, &[0x89, 0x86, 0x8C, 0x26, 0, 0]));
    // the four colour arrays are 80 bytes apart: red, blue, yellow, green
    let names: Vec<_> = (0..4).map(|c| opensow_formats::effects::extra_at(BODKY_ARRAYS + BODKY_STRIDE * c).map(|(e, o)| (e.name, o))).collect();
    assert_eq!(names[1], Some(("extras\\bodky_b", 0)));
    assert_eq!(names[3], Some(("extras\\bodky_g", 0)));
    assert_eq!(gun_trail(31, 0).unwrap().array, BODKY_ARRAYS + BODKY_STRIDE);
    assert_eq!(gun_trail(31, 1).unwrap().array, BODKY_ARRAYS + 3 * BODKY_STRIDE);
}

#[test]
fn effect_frame_ranges_fit_the_sprite_files() {
    use opensow_formats::effects::{blast, extra_at, AnimSpec, SPAWNS};
    use opensow_formats::{megafile::MegaFile, sprite};
    let Some(game) = GameDir::locate() else { return };
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    let frames = |spec: &AnimSpec| -> Option<usize> {
        let (e, offset) = extra_at(spec.array)?;
        let entry = mega.find(&format!("{}.ps6", e.name)).unwrap_or_else(|| panic!("{} missing", e.name));
        Some(sprite::parse(&mega.read(entry).unwrap(), sprite::Kind::Color).unwrap().len() - offset)
    };
    for s in SPAWNS {
        if let Some(n) = frames(&s.spec) {
            assert!(s.spec.to as usize <= n, "site {:#x} ({}): frames {}..{} of {n}", s.site, s.what, s.spec.from, s.spec.to);
        }
    }
    for size in 0..7u8 {
        let spec = blast(size);
        // the file holds exactly the frames played, except blast2_defrag (size 4): 28 frames, the exe plays 23
        let n = frames(&spec).unwrap();
        assert!(if size == 4 { n == 28 } else { n == spec.to as usize }, "blast size {size}: {n} frames");
    }
}

#[test]
fn smoke_puff_constants_match_the_exes_constructor() {
    use opensow_formats::effects::{smoke_puff, DECAL_ARRAY};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let code = exe.at(0x405340, 0x160).to_vec();
    let has = |pat: &[u8]| code.windows(pat.len()).any(|w| w == pat);
    let spec = smoke_puff(0, 0);
    assert!(has(&[0xC7, 0x47, 0x0C, spec.period, 0, 0, 0]), "period [+0xc]");
    assert!(has(&[0xC7, 0x47, 0x10, spec.period, 0, 0, 0]), "period [+0x10]");
    assert!(has(&[0xC7, 0x47, 0x34, 8, 0, 0, 0]), "draw mode 8");
    assert!(has(&[0xC6, 0x47, 0x1C, 0]), "flag 0");
    assert!(has(&[0x6A, 0x02]) && has(&[0x6A, 0x0D]) && has(&[0x83, 0xC0, 0x04]), "random(2), random(13) + 4");
    assert!(has(&[0x83, 0xFA, spec.to as u8]), "9 frames per pass");
    assert!(has(&spec.array.to_le_bytes()), "fire_sm array");
    // the decal draw (0x41a160) reads the 0x5e6960 array through the darkening blit
    let draw = exe.at(0x41a160, 0x60);
    assert!(draw.windows(4).any(|w| w == DECAL_ARRAY.to_le_bytes()));
    assert!(draw.windows(5).any(|w| w == [0xE8, 0x7B, 0xE7, 0x03, 0x00]) || draw.contains(&0xE8), "calls the blit");
    // fire_sm1/2 and pustkrat exist with the frames the puff and decal use
    use opensow_formats::{megafile::MegaFile, sprite};
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    let frames = |name: &str| sprite::parse(&mega.read(mega.find(name).unwrap_or_else(|| panic!("{name}"))).unwrap(), sprite::Kind::Color).unwrap().len();
    assert!(frames("extras\\fire_sm1.ps6") >= 9 && frames("extras\\fire_sm2.ps6") >= 9);
    assert!(frames("__extras\\pustkrat.ps6") >= 5);
}

#[test]
fn savegame_constants_match_the_exes_save_code() {
    use opensow_formats::save::{INFO_VERSION, STATE_VERSION, TAGS, THUMB_H, THUMB_W};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // section tags and their order in the string table
    for tag in TAGS {
        exe.string_va(tag);
    }
    let has = |va: u32, len: usize, pat: &[u8]| exe.at(va, len).windows(pat.len()).any(|w| w == pat);
    // state serializer 0x42a400: [ebp-0x4c] = 1, push 0x5a10, call the header writer 0x403e10
    assert!(has(0x42a400, 0x60, &[0xC7, 0x45, 0xB4, STATE_VERSION.0 as u8, 0, 0, 0]));
    assert!(has(0x42a400, 0x60, &[0x68, STATE_VERSION.1 as u8, (STATE_VERSION.1 >> 8) as u8, 0, 0]));
    // info writer 0x42be20: 2 and 0x1e5
    assert!(has(0x42be20, 0x60, &[0xC7, 0x85, 0xA0, 0xFE, 0xFF, 0xFF, INFO_VERSION.0 as u8, 0, 0, 0]));
    assert!(has(0x42be20, 0x60, &[0x68, INFO_VERSION.1 as u8, (INFO_VERSION.1 >> 8) as u8, 0, 0]));
    // the thumbnail is the u16 range 0x71065c..0x719c5c of the exe's memory
    assert_eq!((0x719c5cu32 - 0x71065c) as usize, THUMB_W * THUMB_H * 2);
    assert!(has(0x42be20, 0x400, &0x71065cu32.to_le_bytes()) && has(0x42be20, 0x400, &0x719c5cu32.to_le_bytes()));
}

#[test]
fn constructor_sounds_match_the_exes_bullet_constructor() {
    // every case of the bullet constructor's switch (`0x411dc0`, jump table `0x412490`, 35 types) runs to the shared tail `0x412468`;
    // a case that plays a sound calls `0x4075b0` after `push edi; push edx; push <id>` (57 52 6A id)
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    const TAIL: u32 = 0x412468;
    let rel = |va: u32, b: &[u8]| va.wrapping_add(5).wrapping_add(u32::from_le_bytes(b[1..5].try_into().unwrap()));
    let mut sounding = 0;
    let cases: Vec<u32> = exe.at(0x412490, 35 * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for (k, case) in cases.into_iter().enumerate() {
        let ty = k as u8 + 1;
        let mut found = None;
        let mut va = case;
        while va < TAIL {
            let b = exe.at(va, 5);
            if b[0] == 0xE9 && rel(va, b) == TAIL {
                break;
            }
            if b[0] == 0xE8 && rel(va, b) == 0x4075b0 {
                let before = exe.at(case, (va - case) as usize);
                let at = before
                    .windows(4)
                    .rposition(|w| w[0] == 0x57 && w[1] == 0x52 && w[2] == 0x6A)
                    .unwrap_or_else(|| panic!("type {ty}: no id push before the call at {va:#x}"));
                found = Some(before[at + 3]);
                sounding += 1;
                break;
            }
            va += 1;
        }
        assert_eq!(opensow_formats::effects::constructor_sound(ty), found, "bullet type {ty} (case {case:#x})");
    }
    assert_eq!(sounding, 3, "types 28, 29 and 31 sound");
}

#[test]
fn muzzle_sounds_match_the_exes_volley_spawner() {
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let index = exe.at(0x47d28c, 0x71).to_vec();
    let cases = *index.iter().max().unwrap() as usize + 1;
    let targets: Vec<u32> = exe.at(0x47d244, cases * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let mut checked = 0;
    for kind in 1..=0x71u32 {
        let target = targets[index[kind as usize - 1] as usize];
        let expected = if target == 0x47d226 {
            None
        } else {
            // push 0; push <jitter>; push ecx; push ebx; push <id>  (6A 00 6A j 51 53 6A id)
            let c = exe.at(target, 9);
            assert_eq!((c[0], c[2], c[4], c[5], c[6]), (0x6A, 0x6A, 0x51, 0x53, 0x6A), "case of kind {kind}: {c:02x?}");
            Some((c[7], c[3] == 1))
        };
        assert_eq!(opensow_formats::sfx::muzzle(kind as u8), expected, "kind {kind}");
        checked += 1;
    }
    assert_eq!(checked, 0x71);
    // the exe leaves everything above the table's range silent, like `muzzle`
    assert_eq!(opensow_formats::sfx::muzzle(0x72), None);
}

#[test]
fn unit_death_matches_the_exes_destruction_code() {
    use opensow_formats::effects::{debris, debris_variant, DEBRIS_STRENGTH, UNIT_DEATH_CLOUDS, UNIT_DEATH_SOUND};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // the clouds: 8 initialisers of the e_unit2 array (0x5e9888, frames 0..0x1d, period 3) and one wreck (0x5e92e8)
    let code = exe.at(0x46a560, 0x700).to_vec();
    let count = |pat: &[u8]| code.windows(pat.len()).filter(|w| *w == pat).count();
    let mut cloud = vec![0x6A, 0x01, 0x6A, 0x01, 0x6A, 0x1D, 0x6A, 0x00, 0x68];
    cloud.extend(0x5e9888u32.to_le_bytes());
    assert_eq!(count(&cloud), 2 * UNIT_DEATH_CLOUDS as usize, "four clouds in each of the two branches");
    let mut wreck = vec![0x6A, 0x00, 0x6A, 0x01, 0x6A, 0x14, 0x6A, 0x00, 0x68];
    wreck.extend(0x5e92e8u32.to_le_bytes());
    assert_eq!(count(&wreck), 1);
    // sound 27 (0x1b) through 0x4077e0, once, for every kind
    assert!(exe.at(0x46aba1, 8).starts_with(&[0xFF, 0x77, 0x18, 0xFF, 0x77, 0x14, 0x6A, UNIT_DEATH_SOUND]));
    // debris classes: byte table 0x46b110 over kinds 1..=45, cases at 0x46b100 push the spreads 0x1e, 0x28, 0x3c
    let index = exe.at(0x46b110, 0x2d).to_vec();
    let cases: Vec<u32> = exe.at(0x46b100, 16).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in 1..=45i32 {
        let case = cases[index[kind as usize - 1] as usize];
        match debris(kind) {
            None => assert_eq!(case, 0x46acf2, "kind {kind}"),
            Some(d) => {
                let c = exe.at(case, 0x20);
                // push 2; call rand; ...; push spread
                assert!(c.windows(2).any(|w| w == [0x6A, d.spread as u8]), "kind {kind} spread {}", d.spread);
                assert!(exe.at(case, 0x80).windows(7).any(|w| w == [0xC7, 0x45, 0xE8, d.bullet_param as u8, 0, 0, 0]), "kind {kind} bullet param");
                assert!(exe.at(case, 0x80).windows(7).any(|w| w == [0xC7, 0x45, 0xE4, d.wreck_param as u8, 0, 0, 0]), "kind {kind} wreck param");
            }
        }
    }
    // the bullet: 0x411dc0(type 15 + (team != 0), 0x28, ...)
    // `push 0x28` right after the owner/target pushes is argument 2 of 0x411dc0, which the constructor stores at [+0x2660]
    assert_eq!(DEBRIS_STRENGTH, 0x28);
    assert!(exe.at(0x46ae02, 12).windows(2).any(|w| w == [0x6A, 0x28]));
    let ctor = exe.at(0x411dc0, 0x40);
    assert!(ctor.windows(6).any(|w| w == [0x8B, 0x45, 0x0C, 0x89, 0x86, 0x60]), "argument 2 (ebp+0xc) goes to [+0x2660]");
    // the top sprite index [top+0x54] is the piece variant; the jump table 0x46b140 is the identity on 0..=14 and larger indices skip it
    let table: Vec<u32> = exe.at(0x46b140, 15 * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for (i, &t) in table.iter().enumerate() {
        let c = exe.at(t, 5);
        assert!(i == 0 && c.starts_with(&[0x33, 0xF6]) || c.starts_with(&[0xBE, i as u8, 0, 0, 0]), "case {i}");
    }
    let byte_table = exe.at(0x47ac70, 0x71).to_vec();
    let cases = *byte_table.iter().max().unwrap() as usize + 1;
    let jt: Vec<u32> = exe.at(0x47ac04, cases * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in (1..=15).chain(40..=45) {
        let (f, _) = exe.top_case(jt[byte_table[kind - 1] as usize]);
        let idx = f.get(&0x54).copied();
        match debris_variant(kind as i32) {
            Some(v) => assert_eq!(idx, Some(v as i32), "kind {kind} top index"),
            None => assert!(idx.is_some_and(|i| i > 14), "kind {kind} top index {idx:?} must skip the piece"),
        }
    }
}

#[test]
fn intro_videos_are_plain_media_files() {
    use opensow_formats::video::{parse, Video};
    let Some(game) = GameDir::locate() else { return };
    let open = |n: &str| std::fs::read(game.resolve(&format!("data\\levels\\{n}.sgv")).unwrap()).unwrap();
    let Video::Mpeg(m) = parse(&open("M01")).unwrap() else { panic!("M01 is MPEG-1") };
    assert_eq!((m.width, m.height, m.frame_rate()), (320, 208, 25.0));
    // every picture is one frame at 25 fps, so the pack clock must agree with the picture count
    let frames: u32 = m.pictures.iter().sum();
    assert!((frames as f64 / 25.0 - m.duration).abs() < 0.5, "{frames} frames vs {}s", m.duration);
    let Video::Avi(a) = parse(&open("mA")).unwrap() else { panic!("mA is AVI") };
    assert_eq!((a.width, a.height, a.frames), (640, 480, 104));
    assert_eq!(&a.streams[0].handler, b"IV50");
    let Video::Avi(b) = parse(&open("mB")).unwrap() else { panic!("mB is AVI") };
    assert_eq!(&b.streams[0].handler, b"CRAM");
}

#[test]
fn mpeg1_decoder_matches_an_independent_decoder() {
    use opensow_formats::{mpeg1, video};
    let Some(game) = GameDir::locate() else { return };
    let data = std::fs::read(game.resolve("data\\levels\\M01.sgv").unwrap()).unwrap();
    let frames = mpeg1::decode(&video::video_stream(&data).unwrap()).unwrap();
    assert_eq!(frames.len(), 323);
    // mean luma and one pixel of frames 0, 150 and the last (black) one, as decoded by FFmpeg;
    // the integer-vs-float IDCT differs by at most a couple of levels
    for (i, mean, px) in [(0, 84.495, 90i32), (150, 74.535, 100), (322, 16.0, 16)] {
        let f = &frames[i];
        let m = f.y[..208 * 320].iter().map(|&v| v as f64).sum::<f64>() / (208.0 * 320.0);
        assert!((m - mean).abs() < 0.3, "frame {i}: mean {m} vs {mean}");
        assert!((f.y[100 * 320 + 160] as i32 - px).abs() <= 3, "frame {i} pixel");
    }
}

#[test]
fn mp2_decoder_matches_an_independent_decoder() {
    use opensow_formats::{mp2, video};
    let Some(game) = GameDir::locate() else { return };
    let data = std::fs::read(game.resolve("data\\levels\\M01.sgv").unwrap()).unwrap();
    let audio = mp2::decode(&video::audio_stream(&data).unwrap()).unwrap();
    assert_eq!((audio.sample_rate, audio.channels, audio.pcm.len() / 2), (44100, 2, 569088));
    // samples 380..390 of the left channel as decoded by FFmpeg (we may differ by one from rounding)
    let want = [203, 211, 193, 146, 47, -42, -80, -133, -206, -245];
    for (i, w) in want.iter().enumerate() {
        let got = audio.pcm[(380 + i) * 2] as i32;
        assert!((got - w).abs() <= 1, "sample {}: {got} vs {w}", 380 + i);
    }
    // the 160 kbit/s intro uses the other allocation table
    let intro = std::fs::read(game.resolve("data\\levels\\mI.sgv").unwrap()).unwrap();
    let a = mp2::decode(&video::audio_stream(&intro).unwrap()).unwrap();
    assert_eq!(a.pcm.len() / 2, 8210304);
}

#[test]
fn ms_video_1_matches_an_independent_decoder_exactly() {
    use opensow_formats::{msvideo1, video};
    let Some(game) = GameDir::locate() else { return };
    let data = std::fs::read(game.resolve("data\\levels\\mB.sgv").unwrap()).unwrap();
    let video::Video::Avi(avi) = video::parse(&data).unwrap() else { panic!("mB is an AVI") };
    let s = &avi.streams[0];
    let mut dec = msvideo1::Decoder::new(s.format[0] as usize, s.format[1] as usize, msvideo1::Decoder::palette_from_strf(&s.strf));
    let chunks = video::avi_chunks(&data, 0).unwrap();
    assert_eq!(chunks.len(), 100);
    // RGB byte sums and the centre pixel of frames 0, 50 and 99 as decoded by FFmpeg (bit exact)
    let want = [(0, 990095u64, [11u8, 67, 3]), (50, 1023822, [11, 75, 3]), (99, 993693, [11, 75, 3])];
    for (i, c) in chunks.iter().enumerate() {
        dec.decode(c).unwrap();
        if let Some((_, sum, px)) = want.iter().find(|w| w.0 == i) {
            let rgb = dec.to_rgb();
            assert_eq!(rgb.iter().map(|&v| v as u64).sum::<u64>(), *sum, "frame {i}");
            assert_eq!(&rgb[(56 * 128 + 64) * 3..][..3], px);
        }
    }
}

#[test]
fn indeo5_decodes_the_whole_ma_video_like_the_reference_decoder() {
    use opensow_formats::{indeo5, video};
    let Some(game) = GameDir::locate() else { return };
    let data = std::fs::read(game.resolve("data\\levels\\mA.sgv").unwrap()).unwrap();
    let chunks = video::avi_chunks(&data, 0).unwrap();
    assert_eq!(chunks.len(), 104);
    // plane sums and centre pixels from FFmpeg's output (bit exact): (frame, sum Y, sum U, sum V, Y, U, V)
    let want: [(usize, u64, u64, u64, u8, u8, u8); 4] = [
        (0, 4935393, 2456992, 2456905, 80, 108, 101),
        (12, 6745909, 2403372, 2395270, 120, 92, 85),
        (50, 6924667, 2396019, 2385593, 68, 110, 106),
        (103, 16137607, 2120171, 2067167, 113, 97, 93),
    ];
    for (n, chunk) in chunks.iter().enumerate() {
        let f = indeo5::decode(chunk).unwrap_or_else(|e| panic!("frame {n}: {e}"));
        assert_eq!((f.width, f.height, f.chroma_width, f.chroma_height), (640, 480, 160, 120));
        if let Some(&(_, y, u, v, py, pu, pv)) = want.iter().find(|w| w.0 == n) {
            let sum = |p: &[u8]| p.iter().map(|&b| b as u64).sum::<u64>();
            assert_eq!((sum(&f.y), sum(&f.u), sum(&f.v)), (y, u, v), "frame {n}");
            assert_eq!((f.y[240 * 640 + 320], f.u[60 * 160 + 80], f.v[60 * 160 + 80]), (py, pu, pv), "frame {n}");
        }
    }
}

#[test]
fn campaign_progress_rules_match_the_exes_pass_function() {
    use opensow_formats::config::{PassedLevels, UNLOCK_ORDER};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // `0x4565a0` CPassedLevels::Pass: every `cmp byte ptr [ecx+disp8], 0` (80 79 disp 00) names the next land to open, in order
    let code = exe.at(0x4565a0, 0x170);
    let order: Vec<usize> = code.windows(4).filter(|w| w[0] == 0x80 && w[1] == 0x79 && w[3] == 0).map(|w| w[2] as usize - 4).collect();
    assert_eq!(order, UNLOCK_ORDER);
    // the finale opens land 22 (`mov byte ptr [ecx+0x1a], 2`) and level 22 itself goes to state 2 (`push 2` at 0x4565ab)
    assert!(code.windows(4).any(|w| w == [0xc6, 0x41, 0x1a, 0x02]));
    assert_eq!(exe.at(0x4565a6, 7), [0x83, 0xf8, 0x16, 0x75, 0x10, 0x6a, 0x02]);
    // the state of a new campaign (`0x456a86`: word [esi+5] = 0x202, byte [esi+0xd] = 2)
    let init = exe.at(0x456a86, 11);
    assert_eq!(init, [0x66, 0xc7, 0x46, 0x05, 0x02, 0x02, 0xc6, 0x46, 0x0d, 0x02, 0xe8]);
    let p = PassedLevels::initial();
    assert_eq!([p.states[1], p.states[2], p.states[9]], [2, 2, 2]);
}

#[test]
fn every_music_track_is_plain_pcm() {
    let Some(game) = GameDir::locate() else { return };
    let mut total = 0.0;
    for name in [
        "Menu",
        "Brief",
        "Brief2",
        "Statistic",
        "uvod",
        "Level01",
        "Level02",
        "Level03",
        "Level04",
        "Level05",
        "Level06",
        "Level07",
        "Level08",
        "Level09",
        "Level10",
        "Level11",
        "Level12",
        "Level13",
    ] {
        let path = game.resolve(&format!("Data\\Sounds\\Music\\{name}.wav")).unwrap_or_else(|| panic!("{name}.wav missing"));
        let w = opensow_formats::wav::parse(&std::fs::read(path).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(w.sample_rate == 22050 && (1..=2).contains(&w.channels), "{name}: {} Hz {} ch", w.sample_rate, w.channels);
        total += w.seconds();
    }
    assert!(total > 1000.0, "{total} s of music");
}

#[test]
fn pointer_types_match_the_exes_cursor_code() {
    use opensow_formats::{
        cursor::{Pointer, FRAMES_PER_TYPE, SPRITE},
        megafile::MegaFile,
        sprite,
    };
    let Some(game) = GameDir::locate() else { return };
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    let e = mega.find(SPRITE).expect("cursor.ps6");
    // 19 pointer types of 8 frames each
    assert_eq!(sprite::parse(&mega.read(e).unwrap(), sprite::Kind::Color).unwrap().len(), 19 * FRAMES_PER_TYPE);
    let Some(exe) = Exe::open(&game) else { return };
    // `mov dword ptr [esi+0x50], type` (C7 46 50 imm32) at the places the Advancer cases set the type
    for (va, p) in [(0x417c4a, Pointer::Repair), (0x417cdd, Pointer::Destroy), (0x417c8d, Pointer::Upgrade)] {
        let code = exe.at(va, 7);
        assert_eq!(&code[..3], &[0xC7, 0x46, 0x50], "{va:#x}");
        assert_eq!(code[3] as usize, p as usize, "{va:#x}");
    }
    // the scroll arrows: (va, type, x offset, y offset) in the three consecutive moves of 0x416aa5.. (`[+0x50]`, `[+0x54]`, `[+0x58]`)
    for (va, p) in [
        (0x416afc, Pointer::ScrollUpLeft),
        (0x416ae5, Pointer::ScrollDownLeft),
        (0x416aa5, Pointer::ScrollLeft),
        (0x416acb, Pointer::ScrollUp),
        (0x416b48, Pointer::ScrollUpRight),
        (0x416b31, Pointer::ScrollDownRight),
        (0x416b21, Pointer::ScrollRight),
        (0x416b68, Pointer::ScrollDown),
    ] {
        let code = exe.at(va, 7);
        assert_eq!(code[3] as usize, p as usize, "scroll type at {va:#x}");
    }
    // the modifier flag (`0x4179bf..0x4179e1`): four `cmp byte ptr [esi+d], 0` (80 BE d32 00) on the key-state bytes that start at
    // `+0x64`, i.e. DirectInput codes 0x9d / 0x1d / 0xb8 / 0x38: right and left Ctrl, right and left Alt
    let mut keys: Vec<u32> = [0x4179bf, 0x4179c8, 0x4179d1, 0x4179da]
        .iter()
        .map(|&va| {
            let c = exe.at(va, 7);
            assert_eq!((&c[..2], c[6]), (&[0x80, 0xBE][..], 0), "{va:#x}");
            u32::from_le_bytes([c[2], c[3], c[4], c[5]]) - 0x64
        })
        .collect();
    keys.sort();
    assert_eq!(keys, [0x1d, 0x38, 0x9d, 0xb8]);
    // over an enemy unit and over a building the type is `4 + flag` (`add eax, 4; mov [esi+0x50], eax`)
    for va in [0x417abe, 0x417d1e] {
        assert_eq!(exe.at(va, 6), [0x83, 0xC0, 0x04, 0x89, 0x46, 0x50], "{va:#x}");
    }
    assert_eq!(Pointer::Attack as u8 + 1, Pointer::AttackModifier as u8);
    // the order's flag: always 1 on a unit (`push 1`, 0x417b2e), the modifier flag `[ebp-0xcc]` on a building (0x417d81)
    assert_eq!(exe.at(0x417b2e, 2), [0x6A, 0x01]);
    assert_eq!(exe.at(0x417d81, 6), [0xFF, 0xB5, 0x34, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn building_art_offsets_match_the_exes_constructor() {
    use opensow_formats::buildings::art_offset;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // `mov [ebx+0x1c], x` (C7 43 1C imm32) then `mov [ebx+0x20], y` (C7 43 20 imm32), the cases of the constructor
    for (va, kind) in [
        (0x40a547, 100),
        (0x40a59d, 101),
        (0x40a63d, 102),
        (0x40a6dd, 103),
        (0x40a757, 104),
        (0x40a7bb, 105),
        (0x40a81f, 106),
        (0x40a883, 107),
        (0x40a6f7, 108),
    ] {
        let c = exe.at(va, 14);
        assert_eq!(&c[..3], &[0xC7, 0x43, 0x1C], "{va:#x}");
        assert_eq!(&c[7..10], &[0xC7, 0x43, 0x20], "{va:#x}");
        assert_eq!((c[3] as i32, c[10] as i32), art_offset(kind), "kind {kind}");
    }
    // turrets: the two writes come in the other order (y first)
    for (va, kind) in [(0x40a8e7, 25), (0x40a952, 26), (0x40a962, 28)] {
        let c = exe.at(va, 7);
        assert_eq!(&c[..3], &[0xC7, 0x43, 0x20], "{va:#x}");
        assert_eq!(c[3] as i32, art_offset(kind).1, "kind {kind}");
    }
}

#[test]
fn building_animation_tables_match_the_exes_constructor() {
    use opensow_formats::buildings::anim;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // Follow each case through its jumps to the common tail 0x40a981, collecting `mov [ebx+d], imm` stores.
    for (start, kind) in [
        (0x40a540, 100),
        (0x40a596, 101),
        (0x40a636, 102),
        (0x40a6d6, 103),
        (0x40a750, 104),
        (0x40a7b4, 105),
        (0x40a818, 106),
        (0x40a87c, 107),
        (0x40a6f0, 108),
    ] {
        let mut stores = std::collections::HashMap::new();
        let mut va = start;
        loop {
            let c = exe.at(va, 10);
            match (c[0], c[1]) {
                (0xC7, 0x43) => {
                    stores.insert(c[2] as u32, u32::from_le_bytes(c[3..7].try_into().unwrap()));
                    va += 7
                }
                (0xC7, 0x83) => {
                    stores.insert(u32::from_le_bytes(c[2..6].try_into().unwrap()), u32::from_le_bytes(c[6..10].try_into().unwrap()));
                    va += 10
                }
                (0xC6, 0x43) => {
                    stores.insert(c[2] as u32, c[3] as u32);
                    va += 4
                }
                (0xEB, _) => va = (va as i64 + 2 + c[1] as i8 as i64) as u32,
                (0xE9, _) => va = (va as i64 + 5 + i32::from_le_bytes(c[1..5].try_into().unwrap()) as i64) as u32,
                _ => panic!("unexpected code at {va:#x}: {c:02x?}"),
            }
            if va == 0x40a981 {
                break;
            }
        }
        let a = anim(kind).unwrap();
        let bounds: Vec<u32> = (0..8).map(|i| stores[&(0x28 + 4 * i)]).collect();
        assert_eq!(a.bounds.iter().map(|&b| b as u32).collect::<Vec<_>>(), bounds, "kind {kind} bounds");
        if kind != kinds::HEADQUARTERS {
            assert_eq!(a.period, stores[&0x58], "kind {kind} period");
            assert_eq!(stores[&0x54], stores[&0x58], "kind {kind} initial timer");
        }
    }
}

#[test]
fn building_sprite_sets_have_the_exes_frame_counts() {
    use opensow_formats::{buildings::anim, megafile::MegaFile, sprite};
    let Some(game) = GameDir::locate() else { return };
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    for (kind, stem) in
        [(100, "_base"), (101, "_light"), (102, "_med"), (103, "_heavy"), (104, "_air"), (105, "_mine"), (106, "_rlab"), (107, "_wind"), (108, "_walker")]
    {
        let a = anim(kind).unwrap();
        let name = format!("builds\\blue\\{stem}.ps6");
        let e = mega.find(&name).unwrap_or_else(|| panic!("{name}"));
        let frames = sprite::parse(&mega.read(e).unwrap(), sprite::Kind::Color).unwrap().len();
        assert_eq!(frames, a.bounds[7] as usize, "{stem}");
    }
}

#[test]
fn rogon_bomb_constants_match_the_exe() {
    use opensow_formats::rogon::*;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let u32le = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
    // 0x46df40: `cmp dword ptr [esi+0x40], 0x2e` (kind 46), then nine `cmp [4*ecx + disp32], eax` (39 04 8D / 3B 04 8D) on the
    // 3 x 3 block around the cell of the static grid 0x5eac38 (index cx*128+cy)
    let code = exe.at(0x46df40, 0x90).to_vec();
    assert_eq!(&code[0x0b..0x0f], &[0x83, 0x7E, 0x40, KIND as u8]);
    let mut disps: Vec<u32> = (0..code.len() - 7).filter(|&i| code[i..i + 3] == [0x39, 0x04, 0x8D]).map(|i| u32le(&code[i + 3..i + 7])).collect();
    assert_eq!(disps.len(), 9);
    disps.sort();
    let mut want: Vec<u32> = (-1i32..=1).flat_map(|dx| (-1i32..=1).map(move |dy| (0x5eac38i32 + 4 * (dx * 128 + dy)) as u32)).collect();
    want.sort();
    assert_eq!(disps, want);
    // 0x46a4f0 kind 46 case: event type 0x23 with the bomb's position and +20 (the last arguments pushed in reverse order)
    let death = exe.at(0x46a51d, 0x56).to_vec();
    assert_eq!(&death[0..3], &[0x83, 0xFE, KIND as u8]);
    assert!(death.windows(2).any(|w| w == [0x6A, BULLET_TYPE]), "push 0x23");
    // 0x411e7c: the event starts with counter 20 and k = 0 (type 35 case of the jump table at 0x412490)
    let case = exe.at(0x411e7c, 0x24).to_vec();
    assert_eq!(&case[0..6], &[0xC7, 0x86, 0x64, 0x26, 0x00, 0x00]);
    assert_eq!(u32le(&case[6..10]), 100, "[+0x2664] = 0x64");
    assert_eq!(u32le(&case[16..20]) as i32, START_COUNTER);
    // 0x411220: `cmp eax, [0x6b3a34]` = entry 7 of the index table at 0x6b3a18; `idiv 0x11`; sounds 0x43 / 0x13
    let ev = exe.at(0x411323, 6).to_vec();
    assert_eq!((ev[0], ev[1], u32le(&ev[2..6])), (0x3B, 0x05, 0x6b3a18 + 4 * LAST_RING as u32));
    let snd = exe.at(0x411337, 5).to_vec();
    assert_eq!(u32le(&snd[1..5]) as usize, SOUND_PERIOD);
    let tail = exe.at(0x411380, 6).to_vec();
    assert_eq!(&tail[..2], &[0x6A, 0x43]);
    assert_eq!(&tail[4..6], &[0x6A, 0x13]);
}

#[test]
fn random_curve_bullets_draw_what_the_exes_code_draws() {
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // type 14 (0x411f9b): push 0x1e; call rand; movzx; add esp,4; add eax,0x3c  => speed 60 + rand(30)
    let c14 = exe.at(0x411f9b, 20).to_vec();
    assert_eq!((c14[0], c14[1], c14[2]), (0x6A, 0x1E, 0xE8));
    assert_eq!(&c14[13..16], &[0x83, 0xC0, 0x3C]);
    // type 13 (0x411fc0): push 2 first (flag), later push 0x3c and add eax,0x1e => speed 30 + rand(60)
    let c13 = exe.at(0x411fc0, 0x60).to_vec();
    assert_eq!(&c13[0..2], &[0x6A, 0x02]);
    assert!(c13.windows(2).any(|w| w == [0x6A, 0x3C]) && c13.windows(3).any(|w| w == [0x83, 0xC0, 0x1E]));
    // table builder 0x40f2c0: push 0x12c twice (dx, dy), mov edi,0x96 (the -150 bias), push 3 (lift shape), 200 curves
    let b = exe.at(0x40f2c0, 0x240).to_vec();
    assert_eq!(b.windows(5).filter(|w| *w == [0x68, 0x2C, 0x01, 0x00, 0x00]).count(), 2);
    assert!(b.windows(5).any(|w| w == [0xBF, 0x96, 0x00, 0x00, 0x00]));
    assert!(b.windows(2).any(|w| w == [0x6A, 0x03]));
    assert_eq!(exe.at(0x40f51e, 5)[0..2], [0x3D, 0x28]); // cmp eax, 0x4ec228 (= 0x4ec098 + 200 * 2)
}

/// The second-step search `0x475c30` / `0x4732f0` (docs/walking.md): grids, counters and the only call sites.
#[test]
fn second_step_search_uses_the_second_step_grids_and_is_only_called_by_the_networked_planner() {
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let count = |code: &[u8], v: u32| code.windows(4).filter(|w| *w == v.to_le_bytes()).count();
    let ground = exe.at(0x475c30, 0x476868 - 0x475c30).to_vec();
    let air = exe.at(0x4732f0, 0x473f10 - 0x4732f0).to_vec();
    // ground: unit grid = second step grid 0x63ac38 / [0x49a370]; the first step grid 0x61ac38 / [0x49a36c] is asked once (the
    // look-ahead cell next to the unit); the static grid 0x5eac38 / [0x49a360] is only read, never asked about its counter
    assert_eq!((count(&ground, 0x49a370), count(&ground, 0x63ac38), count(&ground, 0x5eac38)), (28, 14, 1));
    assert_eq!(count(&ground, 0x49a36c), 1);
    assert_eq!(count(&ground, 0x61ac38), 1);
    assert_eq!(count(&ground, 0x49a360), 0);
    // air: 0x64ac38 / [0x49a378]; first step grid 0x62ac38 / [0x49a374] once; static grid 0x60ac38
    assert_eq!((count(&air, 0x49a378), count(&air, 0x64ac38), count(&air, 0x60ac38)), (28, 14, 1));
    assert_eq!(count(&air, 0x49a374), 1);
    assert_eq!(count(&air, 0x62ac38), 1);
    assert_eq!(count(&air, 0x49a368), 0);
    // the ground routine hands mode != 0 to the air routine (call at 0x475c5c)
    assert_eq!(ground[0x475c5c - 0x475c30], 0xE8);
    assert_eq!(&ground[0x475c5c - 0x475c30 + 1..][..4], &(0x4732f0u32.wrapping_sub(0x475c61)).to_le_bytes());
    // call sites in the whole code section: 0x475c30 from 0x473057, 0x4732f0 from 0x475c5c, 0x472f50 from 0x46fdee (the networked
    // movement 0x46fa40); 0x476870 and 0x473190 (single player) are called from the other movement code
    let text = exe.at(0x401000, 0x7f000).to_vec();
    let calls = |target: u32| -> Vec<u32> {
        (0..text.len() - 5)
            .filter(|&i| text[i] == 0xE8 && (0x401000 + i as u32 + 5).wrapping_add(u32::from_le_bytes(text[i + 1..i + 5].try_into().unwrap())) == target)
            .map(|i| 0x401000 + i as u32)
            .collect()
    };
    assert_eq!(calls(0x475c30), [0x473057]);
    assert_eq!(calls(0x4732f0), [0x475c5c]);
    assert_eq!(calls(0x472f50), [0x46fdee]);
    assert_eq!(calls(0x46fa40), [0x46f1b5]);
    assert_eq!(calls(0x473190), [0x46f614]);
}

/// `0x4774c0` (free cell near the destination), its flood `0x4726f0` and the networked step `0x46fa40` (docs/pathfinding.md).
#[test]
fn free_cell_fallback_and_networked_step_match_the_decoded_structure() {
    use opensow_formats::{freecell, netwalk, rings::Rings};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let text = exe.at(0x401000, 0x7f000).to_vec();
    let calls_in = |from: u32, to: u32| -> Vec<u32> {
        (0..text.len() - 5)
            .filter(|&i| text[i] == 0xE8 && (0x401000 + i as u32) >= from && (0x401000 + i as u32) < to)
            .map(|i| (0x401000 + i as u32 + 5).wrapping_add(u32::from_le_bytes(text[i + 1..i + 5].try_into().unwrap())))
            .collect()
    };
    let callers = |target: u32| -> Vec<u32> {
        (0..text.len() - 5)
            .filter(|&i| text[i] == 0xE8 && (0x401000 + i as u32 + 5).wrapping_add(u32::from_le_bytes(text[i + 1..i + 5].try_into().unwrap())) == target)
            .map(|i| 0x401000 + i as u32)
            .collect()
    };
    let count = |code: &[u8], v: u32| code.windows(4).filter(|w| *w == v.to_le_bytes()).count();
    // 0x4774c0: the three planner call sites; the flood is called once, from its ground branch
    assert_eq!(callers(0x4774c0), [0x472ffe, 0x473068, 0x4731e4]);
    assert_eq!(callers(0x4726f0), [0x47758e]);
    let f = exe.at(0x4774c0, 0x47762a - 0x4774c0).to_vec();
    // air branch: unit grid 0x62ac38 twice (first cell + loop), free below 0x11170; ground branch: 0x61ac38 twice
    assert_eq!((count(&f, 0x62ac38), count(&f, 0x61ac38), count(&f, 0x11170)), (2, 2, 1));
    // the ring list: offsets 0x65bbd8 (x) / 0x65bbda (y), limit [0x6b3a98] = entry 32 of the index table at 0x6b3a18
    assert_eq!((count(&f, 0x65bbd8), count(&f, 0x65bbda), count(&f, 0x6b3a98)), (4, 4, 2));
    assert_eq!(0x6b3a98, 0x6b3a18 + 4 * freecell::SEARCH_RINGS as u32);
    assert!(Rings::generate().ends[freecell::SEARCH_RINGS] > 0);
    // the map-size test and the destination write: [0x71cec0] / [0x71cec4], [edi+0x5c] / [edi+0x60]
    assert_eq!((count(&f, 0x71cec0), count(&f, 0x71cec4)), (1, 1));
    // the flood: unit grid 0x61ac38 with its counter [0x49a36c], stamp base + 1, counter base + 4 (lea ecx,[esi+3])
    let flood = exe.at(0x4726f0, 0x472a85 - 0x4726f0).to_vec();
    assert_eq!(count(&flood, 0x49a36c), 3);
    assert!(flood.windows(3).any(|w| w == [0x8D, 0x4E, 0x03]));
    assert!(flood.windows(5).any(|w| w == [0x3D, 0x00, 0x40, 0x00, 0x00]), "generation limit 0x4000 dwords = 8192 cells");
    // 0x46fa40: plans with 0x472f50, searches with 0x4753a0, removes the order with 0x470810, builds lines with 0x46ee80, faces
    // with 0x46b180 and asks 0x41f020; there is no turn step 0x46a190 (the single-player step calls it)
    let targets = calls_in(0x46fa40, 0x46ff49);
    for t in [0x472f50, 0x4753a0, 0x470810, 0x46ee80, 0x46b180, 0x41f020, 0x470480, 0x46eb40] {
        assert!(targets.contains(&t), "0x46fa40 calls {t:#x}");
    }
    assert!(!targets.contains(&0x46a190));
    assert!(calls_in(0x46f550, 0x46f9b3).contains(&0x46a190));
    // 0x41f020(player) = [0x575720] > 0 && player != 0
    assert_eq!(
        exe.at(0x41f020, 0x1a),
        [
            0x55, 0x8B, 0xEC, 0x83, 0x3D, 0x20, 0x57, 0x57, 0x00, 0x00, 0x7E, 0x0A, 0x83, 0x7D, 0x08, 0x00, 0x74, 0x04, 0xB0, 0x01, 0x5D, 0xC3, 0x32, 0xC0,
            0x5D, 0xC3
        ]
    );
    // the speed row table 0x5524a0 (60 bytes per row) and the clock comparison seconds * 64 + tick (shl 6)
    let net = exe.at(0x46fa40, 0x46ff49 - 0x46fa40).to_vec();
    assert_eq!(count(&net, 0x5524a0), 1);
    assert!(net.windows(3).any(|w| w == [0xC1, 0xE0, 0x06]));
    assert_eq!(netwalk::ROW, 60);
}

/// The string index a switch case pushes: skips the `mov esi, [ebp+8]` / `mov ecx, 0x6bed04` set-up some cases have, then reads `push imm`.
fn case_string(exe: &Exe, mut va: u32) -> Option<u32> {
    for _ in 0..4 {
        let b = exe.at(va, 5);
        match b[0] {
            0x68 => return Some(u32::from_le_bytes(b[1..5].try_into().unwrap())),
            0x6A => return Some(b[1] as u32),
            0x8B if b[1] == 0x75 => va += 3,
            0xB9 => va += 5,
            _ => return None,
        }
    }
    None
}

#[test]
fn string_groups_match_the_exes_name_switches() {
    use opensow_formats::lang::{LangFile, Language};
    use opensow_formats::strings::*;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // object names 0x431ad0 / weapon names 0x4321c0: `dec kind; movzx byte table; jmp [table*4 + jump table]`; the exe's index is the file index + 1
    let cases = |bytes: u32, jt: u32, n: usize| -> Vec<Option<u32>> {
        let sel = exe.at(bytes, n).to_vec();
        let jmp: Vec<u32> =
            exe.at(jt, 4 * (*sel.iter().max().unwrap() as usize + 1)).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        sel.iter().map(|&s| case_string(&exe, jmp[s as usize])).collect()
    };
    let names = cases(0x43214c, 0x432064, 0x73);
    for (i, got) in names.iter().enumerate() {
        let kind = i as u32 + 1;
        assert_eq!(got.map(|v| v as usize - 1), object_name(kind).filter(|_| kind < 1000), "object kind {kind}");
    }
    let weapons = cases(0x4323d0, 0x432348, 0x6c);
    for (i, got) in weapons.iter().enumerate() {
        let kind = i as u32 + 1;
        assert_eq!(got.map(|v| v as usize - 1), Some(weapon_name(kind)), "weapon kind {kind}");
    }
    // the English strings the groups name
    let en = LangFile::parse(&read(&game, &format!("Data\\Language\\{}", Language::English.file_name())).unwrap()).unwrap();
    let s = |i: usize| en.get(i).unwrap_or("");
    assert_eq!((s(*MAIN_MENU.start()), s(*MAIN_MENU.end())), ("New Game", "Exit Game"));
    assert_eq!((s(OPTIONS_TITLE), s(OK), s(CANCEL), s(ON), s(OFF)), ("Options", "OK", "Cancel", "On", "Off"));
    assert_eq!((s(*LANGUAGE_NAMES.start()), s(*LANGUAGE_NAMES.end())), ("ENGLISH", "CHINESSE_T"));
    assert_eq!(s(NO_DATA), "No data available");
    assert!(s(land_heading(1)).starts_with("TERRITORY") && s(land_place(22)).starts_with("Coordinate"));
    assert!(s(land_objectives(1)).starts_with("Objectives") && s(land_objectives(22)).starts_with("Objectives"));
    assert_eq!((s(*MESSAGES.start()), s(SPEED), s(SATELLITE_PROTECTION)), ("Building captured", "speed", "SATELLITE PROTECTION"));
    assert!(s(*TUTORIAL.start()).starts_with("i Welcome") && s(*TUTORIAL.end()).starts_with("! Excellent"));
    assert_eq!((s(*OBJECT_NAMES.start()), s(*OBJECT_NAMES.end())), ("Radar", "Research Influx 3"));
    assert_eq!(
        (s(*WEAPON_NAMES.start()), s(*WEAPON_NAMES.end()), s(NO_WEAPON), s(TECH_LEVEL_HINT)),
        ("2x antiair rockets", "4x AG bombs", "n/a", "increase tech-level for 500")
    );
    assert_eq!(
        (s(SELECTED_LEVEL), s(*SESSION_BUTTONS.start()), s(*SLOT_TEXTS.start()), s(*SIDE_AND_START.end()), s(*HINTS.start())),
        ("Selected level:", "CREATE SESSION", "Empty Savegame...", "RESEARCH", "Now increase tech-level in your factory.")
    );
    assert_eq!(
        (s(*INFOBOX_LINES.start()), s(*INFOBOX_LINES.end()), s(QUICK_SAVE), s(ACCEPT), s(BACK)),
        ("No Info Available", "cargo: reinforcements", "Quick Save", "ACCEPT", "BACK")
    );
    assert_eq!(
        (s(*STATISTICS.start()), s(*STATISTICS.end()), s(LOAD), s(SAVE), s(PAUSE), s(REMAINING_TIME), s(SAVE_TITLE)),
        ("CAPTURED", "SENT", "LOAD", "SAVE", "P A U S E", "Remaining Time", "Save Game")
    );
}

#[test]
fn rally_point_constants_match_the_exes_code() {
    use opensow_formats::rally::*;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // the widget jump table: id 0x7b -> case 12 -> 0x416cf9 (the click that arms the placement mode)
    assert_eq!(exe.at(0x417eec + (WIDGET_ID - 0x19), 1)[0], 12);
    assert_eq!(u32::from_le_bytes(exe.at(0x417eb4 + 12 * 4, 4).try_into().unwrap()), 0x416cf9);
    // `0x456490(-0x19, -0xb, 0x19, 0xc)` then the position `[+0x2a0] + 4`, `[+0x2a4] - 0x29`
    assert_eq!(exe.at(0x4554d1, 8), &[0x6a, 0x0c, 0x6a, 0x19, 0x6a, 0xf5, 0x6a, 0xe7]);
    assert_eq!((BUTTON_RECT, BUTTON_OFFSET), ((-0x19, -0xb, 0x19, 0xc), (4, -0x29)));
    assert_eq!(exe.at(0x455ddb, 3), &[0x83, 0xe8, 0x29]);
    assert_eq!(exe.at(0x455de5, 3), &[0x83, 0xc0, 0x04]);
    // the button's frames: `[0x5e719c]` and `[0x5e71a4]` = ikonyrpoints (array 0x5e7198) frames 1 and 3
    assert_eq!(exe.at(0x455500, 12), &[0xff, 0x35, 0xa4, 0x71, 0x5e, 0x00, 0xff, 0x35, 0x9c, 0x71, 0x5e, 0x00]);
    assert_eq!((0x5e719cu32 - 0x5e7198) / 4, BUTTON_FRAME as u32);
    assert_eq!((0x5e71a4u32 - 0x5e7198) / 4, BUTTON_HOVER_FRAME as u32);
    // the factory kinds tested by the click case (`cmp ecx, 0x65 / 0x66 / 0x67 / 0x6c`)
    assert_eq!(exe.at(0x416d36, 3), &[0x83, 0xf9, 0x65]);
    assert_eq!(exe.at(0x416d3b, 3), &[0x83, 0xf9, 0x66]);
    assert_eq!(exe.at(0x416d40, 3), &[0x83, 0xf9, 0x67]);
    assert_eq!(exe.at(0x416d45, 3), &[0x83, 0xf9, 0x6c]);
    assert_eq!(FACTORY_KINDS, [0x65, 0x66, 0x67, 0x6c]);
    // the hotkey: `cmp byte ptr [esi+0x77], 1`; the key array starts at +0x64 (numpad 7 = scan 0x47 is `[esi+0xab]`)
    assert_eq!(exe.at(0x416769, 4), &[0x80, 0x7e, 0x77, 0x01]);
    assert_eq!(0x77 - 0x64, HOTKEY_SCAN);
    assert_eq!(exe.at(0x4166eb, 4), &[0x80, 0xbe, 0xaf, 0x00]);
    // the click: `and eax/edi, -0x20` then `add 0x10`, the point stored in `[bld+0x2c0]`, `[bld+0x2c4]`
    assert_eq!(exe.at(0x41748f, 3), &[0x83, 0xe0, 0xe0]);
    assert_eq!(exe.at(0x417498, 3), &[0x83, 0xc0, 0x10]);
    assert_eq!(exe.at(0x417586, 6), &[0x89, 0xb9, 0xc0, 0x02, 0x00, 0x00]);
    // the marker anim `0x4054a0(x, y, 3, 0x5e6028, 0, 0x14, 1, 1)` and the sound `0x4075b0(0x44, ..)`
    assert_eq!(exe.at(0x4175c4, 15), &[0x6a, 0x01, 0x6a, 0x01, 0x6a, 0x14, 0x6a, 0x00, 0x68, 0x28, 0x60, 0x5e, 0x00, 0x6a, 0x03]);
    assert_eq!((CLICK_ARRAY, CLICK_LAST_FRAME, CLICK_PERIOD), (0x5e6028, 0x14, 3));
    assert_eq!(exe.at(0x417634, 2), &[0x6a, CLICK_SOUND]);
    // the marker frame counter: `cmp [esi+0x60], 0x10` and the reload `mov ecx, 3`
    assert_eq!(exe.at(0x40dedb, 4), &[0x83, 0x7e, 0x60, 0x10]);
    assert_eq!(exe.at(0x40ded6, 5), &[0xb9, 0x03, 0x00, 0x00, 0x00]);
    assert_eq!((MARKER_FRAMES, MARKER_PERIOD), (16, 3));
    // the pointer: type 9 for a free cell (`lea eax, [8*eax + 1]` with eax = 1) at offset -1, else type 1
    assert_eq!(exe.at(0x417671, 7)[..3], [0x8d, 0x04, 0xc5]);
    assert_eq!(exe.at(0x417647, 5), &[0xbf, 0xf4, 0xff, 0xff, 0xff]);
    assert_eq!(POINTER_TYPE, 9);
}

#[test]
fn turret_art_matches_the_exes_tables() {
    use opensow_formats::{
        buildings::{anim, turret_art, turret_top_pos},
        units::TOPS,
    };
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // tops: the weapon constructor's cases for the units 109..=113 that turrets 25..=29 create (`0x40e004`)
    let byte_table = exe.at(0x47ac70, 0x71).to_vec();
    let cases = *byte_table.iter().max().unwrap() as usize + 1;
    let jt: Vec<u32> = exe.at(0x47ac04, cases * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    // bases: the building constructor's art cases (byte table 0x40af64, jump table 0x40af30) until the tail 0x40a981
    let art_bytes = exe.at(0x40af64, 0x54).to_vec();
    let art_cases = *art_bytes.iter().max().unwrap() as usize + 1;
    let art_jt: Vec<u32> = exe.at(0x40af30, art_cases * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in 25..=29i32 {
        let (fields, _) = exe.top_case(jt[byte_table[kind as usize + 83] as usize]);
        let art = turret_art(kind).unwrap();
        assert_eq!(TOPS[fields[&0x54] as usize], art.top, "kind {kind} top");
        assert_eq!(fields[&0x60], art.top_dy, "kind {kind} top offset");
        let mut stores = std::collections::HashMap::new();
        let mut va = art_jt[art_bytes[kind as usize - 0x19] as usize];
        while va != 0x40a981 {
            let c = exe.at(va, 10);
            match (c[0], c[1]) {
                (0xC7, 0x43) => {
                    stores.insert(c[2] as u32, u32::from_le_bytes(c[3..7].try_into().unwrap()));
                    va += 7
                }
                (0xC7, 0x83) => {
                    stores.insert(u32::from_le_bytes(c[2..6].try_into().unwrap()), u32::from_le_bytes(c[6..10].try_into().unwrap()));
                    va += 10
                }
                (0xC6, 0x43) => {
                    stores.insert(c[2] as u32, c[3] as u32);
                    va += 4
                }
                (0xEB, _) => va = (va as i64 + 2 + c[1] as i8 as i64) as u32,
                (0xE9, _) => va = (va as i64 + 5 + i32::from_le_bytes(c[1..5].try_into().unwrap()) as i64) as u32,
                _ => panic!("unexpected code at {va:#x}: {c:02x?}"),
            }
        }
        assert_eq!(format!("sentry{:02}", stores[&0x24] - 8), art.base, "kind {kind} base");
        let a = anim(kind).unwrap();
        assert_eq!(a.bounds.iter().map(|&b| b as u32).collect::<Vec<_>>(), (0..8).map(|i| stores[&(0x28 + 4 * i)]).collect::<Vec<_>>(), "kind {kind} bounds");
        assert_eq!(a.period, stores[&0x58], "kind {kind} period");
        let (ox, oy) = opensow_formats::buildings::art_offset(kind);
        assert_eq!((ox as u32, oy as u32), (stores[&0x1c], stores[&0x20]), "kind {kind} offset");
    }
    // the unit of turrets 4 and 5 stands in the second row (`0x40e03e`, `0x40e029`: y + 1)
    assert_eq!(turret_top_pos(26, (3, 4)), (112, 144));
    assert_eq!(turret_top_pos(28, (3, 4)), (112, 176));
}

#[test]
fn satellite_laser_column_matches_the_exes_draw() {
    use opensow_formats::effects::{satlaser_column, SATLASER_END_DY, SATLASER_START_DY, SATLASER_STRENGTH};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // 0x411100: only for type 10; end point from the path (count [+0x2648] is twice the number of points); `sub esi, 0x17`
    let draw = exe.at(0x411100, 0xa0).to_vec();
    let has = |pat: &[u8]| draw.windows(pat.len()).any(|w| w == pat);
    assert!(has(&[0x83, 0xBF, 0x58, 0x26, 0, 0, 0x0A]), "type 10 only");
    assert!(has(&[0x83, 0xEE, 0x17]), "base 23 px above the end point");
    assert!(has(&[0xFF, 0x34, 0x85, 0x68, 0x6B, 0x5E, 0x00]), "base piece: array 0x5e6b68 (satlaser frame + 4)");
    assert!(has(&[0xFF, 0x34, 0x85, 0x58, 0x6B, 0x5E, 0x00]), "column pieces: array 0x5e6b58");
    assert!(has(&[0xBB, 0x0D, 0, 0, 0]), "13 pieces");
    assert!(has(&[0x83, 0xC6, 0xD2]), "46 px up");
    assert!(has(&[0x83, 0xEE, 0x2E]), "46 px up per piece");
    // the creation in 0x409f50: type 10, strength 1000, start -15, end +10 around y + lift
    let fire = exe.at(0x40a07b, 0x30).to_vec();
    let has = |pat: &[u8]| fire.windows(pat.len()).any(|w| w == pat);
    assert!(has(&[0x68, 0xE8, 0x03, 0, 0]) && has(&[0x6A, 0x0A]), "strength 1000, type 10");
    assert!(has(&[0x8D, 0x41, 0xF1]), "start y - 15");
    assert!(has(&[0x8D, 0x41, 0x0A]), "end y + 10");
    assert_eq!((SATLASER_STRENGTH, SATLASER_START_DY, SATLASER_END_DY), (1000, -15, 10));
    let col = satlaser_column(2, 100, 50);
    assert_eq!(col.len(), 14);
    assert_eq!(col[0], (6, 100, 27));
    assert_eq!(col[1], (2, 100, -19));
    assert_eq!(col[13], (2, 100, 27 - 46 * 13));
}

#[test]
fn satellite_check_scans_the_footprint_on_the_air_grid() {
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let f = exe.at(0x409f50, 0x1d0).to_vec();
    let has = |pat: &[u8]| f.windows(pat.len()).any(|w| w == pat);
    // gate 0x41f020(arg [0x575730]): skipped for remote players only (`[0x575720] > 0 && arg`), single player always runs
    assert!(has(&[0xFF, 0x35, 0x30, 0x57, 0x57, 0x00, 0xE8]), "network gate");
    assert_eq!(exe.at(0x41f023, 7), &[0x83, 0x3D, 0x20, 0x57, 0x57, 0x00, 0x00][..]);
    // timer [+0x11c] -= ticks of the frame [0x552494] while > 0; flag [+0x118] == 1 and timer <= 0 -> timer = 0xf0
    assert!(has(&[0x2B, 0x05, 0x94, 0x24, 0x55, 0x00]), "timer minus ticks per frame");
    assert!(has(&[0x83, 0xBF, 0x18, 0x01, 0, 0, 0x01]), "flag == 1");
    assert!(has(&[0xC7, 0x87, 0x1C, 0x01, 0, 0, 0xF0, 0, 0, 0]), "timer = 240");
    // footprint box [+4]..[+4]+[+0x14] (x), [+8]..[+8]+[+0x18] (y) on the air grid 0x60ac38 (index x*128 + y, 4 bytes)
    assert!(has(&[0xC1, 0xE0, 0x07]) && has(&[0x8D, 0x0C, 0x85, 0x38, 0xAC, 0x60, 0x00]), "grid 0x60ac38");
    // handle class 4 (air grid), side != owner [+0x8c], unit kind [+0x40] == 0x28 (Advancer), hp [+0x34] = 0
    assert!(has(&[0x83, 0xF8, 0x04]), "class 4");
    assert!(has(&[0x3B, 0x87, 0x8C, 0, 0, 0]), "side differs from the building's owner");
    assert!(has(&[0x83, 0x7E, 0x40, 0x28]), "Advancer");
    assert!(has(&[0xC7, 0x46, 0x34, 0, 0, 0, 0]), "hp = 0");
    // the flag is cleared on every pass that gets past the gate
    assert!(has(&[0xC7, 0x87, 0x18, 0x01, 0, 0, 0, 0, 0, 0]), "flag cleared");
    // the only caller is the building update tail, for buildings with [+0x114] != 0; the flag is set only by the destroy beam
    assert_eq!(exe.at(0x40e383, 7), &[0x83, 0xBE, 0x14, 0x01, 0, 0, 0][..]);
    assert_eq!(exe.at(0x40e38e, 5), &[0xE8, 0xBD, 0xBB, 0xFF, 0xFF][..]);
    assert_eq!(exe.at(0x4102f7, 10), &[0xC7, 0x82, 0x18, 0x01, 0, 0, 1, 0, 0, 0][..]);
}

#[test]
fn building_damage_table_matches_the_exes_area_damage() {
    use opensow_formats::projectile::{building_hit, BuildingHit};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // 0x42f417: eax = type - 0x11; > 0x12 -> 0x42f458; byte table 0x42f474 -> jump table 0x42f468
    let bytes = exe.at(0x42f474, 19).to_vec();
    let jt: Vec<u32> = exe.at(0x42f468, 12).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for ty in 1..=40u8 {
        let target = if (17..=35).contains(&ty) { jt[bytes[ty as usize - 17] as usize] } else { 0x42f458 };
        let want = match target {
            0x42f44f => BuildingHit::FullUnlessHq,
            0x42f430 => BuildingHit::HalfUnlessHq,
            0x42f458 => BuildingHit::Full,
            t => panic!("unexpected case {t:#x}"),
        };
        assert_eq!(building_hit(ty), want, "type {ty}");
    }
    // the shield test is `cmp [ecx+0x94], 0x64` in both shielded cases; half damage is `sar eax, 1` of strength - sign
    assert!(exe.at(0x42f430, 7).starts_with(&[0x83, 0xB9, 0x94, 0, 0, 0, 0x64]));
    assert!(exe.at(0x42f44f, 7).starts_with(&[0x83, 0xB9, 0x94, 0, 0, 0, 0x64]));
    // [bld+0x94] is the building kind: the constructor `0x40a358` stores esi (kind) there
    assert!(exe.at(0x40a358, 6).starts_with(&[0x89, 0xB3, 0x94, 0, 0, 0]));
}

#[test]
fn targeted_bullets_hit_their_handle_in_the_exes_impact_function() {
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // 0x40ff65: types 1, 10 and 31 (0x1f) jump to 0x4108bc, which reads the handle [bullet+0x2688]
    let c = exe.at(0x40ff65, 0x1d).to_vec();
    assert_eq!(&c[..3], [0x83, 0xF8, 0x01]);
    assert_eq!(&c[9..12], [0x83, 0xF8, 0x0A]);
    assert_eq!(&c[18..21], [0x83, 0xF8, 0x1F]);
    assert_eq!(exe.at(0x4108bc, 6), [0xFF, 0xB3, 0x88, 0x26, 0, 0]);
    // the shot spawner 0x47cf70 stores its fifth argument ([ebp+0x18]) there
    assert_eq!(exe.at(0x47d0fe, 6), [0x89, 0x87, 0x88, 0x26, 0, 0]);
    // the handle's fields: player = bits 16..18, class = bits 19..21, subclass = bits 22..24 (sar 0x10/0x13/0x16, and 7)
    assert_eq!(&exe.at(0x43aa8f, 6)[..3], [0xC1, 0xF8, 0x10]);
    assert_eq!(&exe.at(0x43aaa6, 6)[..3], [0xC1, 0xF8, 0x13]);
    assert_eq!(&exe.at(0x43aab6, 6)[..3], [0xC1, 0xF8, 0x16]);
    // subclass 4 = aircraft: the unit loop is skipped and `[aircraft+0x2c] -= strength` for the entry whose `[+0x1c]` is the handle
    assert_eq!(exe.at(0x41094c, 12), [0x8B, 0x83, 0x60, 0x26, 0, 0, 0x29, 0x42, 0x2C, 0x8B, 0x4D, 0xF4]);
    // other targets: `[unit+0x34] -= strength` for the unit whose `[+0x3c]` is the handle, then the hit flash `[+0x5c] = 180`, `[+0x60] = 5`
    assert_eq!(exe.at(0x4109a8, 12), [0x8B, 0x83, 0x60, 0x26, 0, 0, 0x29, 0x42, 0x34, 0xC7, 0x42, 0x5C]);
}

#[test]
fn anim_sprite_draw_modes_match_the_exes_jump_table() {
    use opensow_formats::effects::{ANIM_COLUMN, ANIM_DRAW_MODES};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let jt: Vec<u32> = exe.at(0x405080, 15 * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for &(mode, routine) in ANIM_DRAW_MODES.iter().filter(|m| m.0 != 12) {
        let case = jt[mode as usize - 1];
        let code = exe.at(case, 0x60).to_vec();
        // the first `call rel32` of the case
        let i = (0..code.len() - 5).find(|&i| code[i] == 0xE8).unwrap();
        let target = (case + i as u32 + 5).wrapping_add(i32::from_le_bytes(code[i + 1..i + 5].try_into().unwrap()) as u32);
        let want = if jt[mode as usize - 1] == 0x40504c { 0x45edf0 } else { routine };
        assert_eq!(target, want, "mode {mode}");
    }
    // the creators' modes: the puff initialiser stores 8, the missile smoke 14, the fighter's smoke trail 9, blasts 6
    let imm = |va: u32, reg_op: u8, mode: u8| assert_eq!(exe.at(va, 7), [0xC7, reg_op, 0x34, mode, 0, 0, 0], "mode store at {va:#x}");
    imm(0x40537a, 0x47, 8);
    imm(0x41159e, 0x43, 14);
    imm(0x4117e8, 0x43, 14);
    imm(0x4118af, 0x43, 14);
    imm(0x4676dd, 0x43, 9);
    imm(0x46777b, 0x43, 9);
    imm(0x40527d, 0x46, 6);
    for s in opensow_formats::effects::SPAWNS {
        let want = match s.site {
            0x411599 | 0x4117e3 | 0x4118aa => 14,
            0x4676d8 | 0x467776 => 9,
            _ => 6,
        };
        assert_eq!(s.spec.mode, want, "{:#x}", s.site);
    }
    assert_eq!(opensow_formats::effects::smoke_puff(0, 0).mode, 8);
    assert_eq!(opensow_formats::effects::smoke_puff(1, 3).routine(), 0x45b5e0);
    // mode 12: the call at 0x404e8c is the additive blit
    let call = exe.at(0x404e8c, 5);
    assert_eq!(call[0], 0xE8);
    assert_eq!(0x404e91u32.wrapping_add(i32::from_le_bytes(call[1..5].try_into().unwrap()) as u32), 0x45cde0);
    // the column: `sub eax, 0x17` / `add edi, 0x2e` / `cmp edi, 0x284`
    let c = exe.at(0x404e51, 0x50).to_vec();
    let has = |pat: &[u8]| c.windows(pat.len()).any(|w| w == pat);
    assert!(has(&[0x83, 0xE8, 0x17]) && has(&[0x83, 0xC7, 0x2E]) && has(&[0x81, 0xFF, 0x84, 0x02, 0, 0]));
    assert_eq!(ANIM_COLUMN, (14, 46, 23));
    assert_eq!((0x284 + 45) / 46, 14); // edi = 0, 46, .., 598 are drawn
}

#[test]
fn shell_smoke_trails_match_the_exes_flight_step() {
    use opensow_formats::effects::trail;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let has = |va: u32, len: usize, pat: &[u8]| exe.at(va, len).windows(pat.len()).any(|w| w == pat);
    // each branch: `cmp edx, type`, then `dec [+0x264c]`; the reload and the jitter (`push n; call 0x437cd0` twice)
    assert!(has(0x41150a, 3, &[0x83, 0xFA, 0x01]) && has(0x4115d1, 3, &[0x83, 0xFA, 0x18]) && has(0x411695, 3, &[0x83, 0xFA, 0x19]));
    assert!(has(0x411759, 3, &[0x83, 0xFA, 0x03]) && has(0x41181b, 3, &[0x83, 0xFA, 0x04]));
    let reload_half = |va: u32| has(va, 0x14, &[0x8B, 0x86, 0x50, 0x26, 0, 0]) && has(va, 0x14, &[0x2B, 0xC2, 0xD1, 0xF8]);
    assert!(reload_half(0x411526) && reload_half(0x411837), "types 1 and 4 reload with [+0x2650] / 2");
    assert!(has(0x411775, 0x10, &[0x8B, 0x86, 0x50, 0x26, 0, 0]) && !has(0x411775, 0x10, &[0xD1, 0xF8]), "type 3 reloads with [+0x2650]");
    assert!(has(0x4115ef, 10, &[0xC7, 0x86, 0x4C, 0x26, 0, 0, 0x0A, 0, 0, 0]), "type 24 reloads 10");
    assert!(has(0x4116b3, 10, &[0xC7, 0x86, 0x4C, 0x26, 0, 0, 0x05, 0, 0, 0]), "type 25 reloads 5");
    for (va, n) in [(0x41155c, 4u8), (0x4117a6, 4), (0x41186d, 4), (0x41161c, 10), (0x4116e0, 6)] {
        assert_eq!(exe.at(va, 2), [0x6A, n], "jitter at {va:#x}");
    }
    // offsets: `dec eax` (-1) for types 1, 3, 4; `add eax, -5` for 24; `add eax, -3` for 25
    assert_eq!(exe.at(0x41157d, 1), [0x48]);
    assert_eq!(exe.at(0x41163d, 3), [0x83, 0xC0, 0xFB]);
    assert_eq!(exe.at(0x411701, 3), [0x83, 0xC0, 0xFD]);
    for (ty, reload, jitter, offset, mode) in [(1, 4, 4, -1, 14), (3, 8, 4, -1, 14), (4, 4, 4, -1, 14), (24, 10, 10, -5, 6), (25, 5, 6, -3, 6)] {
        let t = trail(ty).unwrap();
        assert_eq!((t.reload, t.jitter, t.offset, t.spec.mode), (reload, jitter, offset, mode), "type {ty}");
    }
    // the counters start at the type's interval: 8 for 1, 3, 4 and 0 for the meteors
    use opensow_formats::projectile::bullet_type;
    assert_eq!([1, 3, 4, 24, 25].map(|t| bullet_type(t).unwrap().interval), [Some(8), Some(8), Some(8), Some(0), Some(0)]);
    assert!(trail(2).is_none() && trail(8).is_none());
}

#[test]
fn the_frame_draw_order_stamps_the_mask_before_the_smoke_and_the_top_shadows() {
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let call_at = |va: u32| -> u32 {
        let c = exe.at(va, 5);
        assert_eq!(c[0], 0xE8, "call at {va:#x}");
        (va + 5).wrapping_add(i32::from_le_bytes(c[1..5].try_into().unwrap()) as u32)
    };
    // the main loop (0x42d6b0): the map pass 0x459350 (generation increment `0x459457`, foreground tiles `0x459666` and the static
    // buildings `0x45969d` -> `0x4586a0`, whose loop stamps each building's mask `0x45888c`), then the world pass 0x41f690
    assert_eq!(call_at(0x42e497), 0x459350);
    assert_eq!(call_at(0x42e4a2), 0x41f690);
    assert_eq!(call_at(0x459666), 0x460120);
    assert_eq!(call_at(0x45969d), 0x4586a0);
    assert_eq!(call_at(0x45888c), 0x460120);
    assert_eq!(exe.at(0x459457, 6), [0xA0, 0x3B, 0xA3, 0x49, 0x00, 0xFE]);
    // inside the world pass, in this order: the smoke puffs (list 0x49a3a8, mode 8), the live buildings (they stamp too, `0x4093e8`),
    // the units' tops (masked shadow), the explosion list 0x49a394 and the second AnimSprite list, the bullets, the Advancers' bodies
    let order = [
        (0x41faf7, 0x404e30u32),
        (0x41fb43, 0x4092f0),
        (0x41fd43, 0x46a3a0),
        (0x4200c3, 0x46a370),
        (0x420105, 0x404e30),
        (0x420147, 0x404e30),
        (0x420219, 0x410a30),
        (0x420253, 0x46a310),
    ];
    for (site, target) in order {
        assert_eq!(call_at(site), target, "{site:#x}");
    }
    // the puffs come from the list at 0x49a3ac/0x49a3b0 (data, count of the object at 0x49a3a8), the explosions from 0x49a398/0x49a39c
    assert_eq!(exe.at(0x41fae2, 6), [0x8B, 0x0D, 0xAC, 0xA3, 0x49, 0x00]);
    assert_eq!(exe.at(0x4200f0, 6), [0x8B, 0x0D, 0x98, 0xA3, 0x49, 0x00]);
    // the ground units' chassis draw 0x46e9c0 (pass 0x46a3a0 on [unit+8]) uses the masked shade blit 0x462110 for the shadow and the
    // masked plain blit 0x461da0 for the body (both branches); the tops' draw 0x479a80 (pass 0x46a4a0 on [unit+4]) the same 0x461da0.
    // Only the Advancer's shadow 0x46e980 (unmasked 0x460f60) and its body 0x46a310 -> 0x46e8f0 (plain 0x45edf0, flag 0) skip the mask.
    assert_eq!(call_at(0x46a3dd), 0x46e9c0);
    assert_eq!(call_at(0x46a4dd), 0x479a80);
    for site in [0x46ea2d, 0x46eaa2] {
        assert_eq!(call_at(site), 0x462110, "{site:#x}");
    }
    for site in [0x46ea6b, 0x46eae1, 0x479af3] {
        assert_eq!(call_at(site), 0x461da0, "{site:#x}");
    }
    assert_eq!(call_at(0x46e9af), 0x460f60);
    assert_eq!(call_at(0x46a35c), 0x46e8f0);
    assert_eq!(call_at(0x46e930), 0x45edf0);
    // 0x461da0 reads the screen mask: `add eax, [0x6b3cd4]` and `cmp cl, [0x49a33b]` (draw only below the frame's stamp)
    assert_eq!(exe.at(0x461e80, 6), [0x03, 0x05, 0xD4, 0x3C, 0x6B, 0x00]);
    assert_eq!(exe.at(0x461f27, 6), [0x3A, 0x0D, 0x3B, 0xA3, 0x49, 0x00]);
}

/// A shipped `.map` is the exe's own map-object stream (`0x459960`, level form): `save::MapState` reads it, writes the same
/// bytes back, and its cell layers agree with the plain `map::Map` view. Pins the layer layout of docs/savegame.md.
#[test]
fn map_files_are_map_object_streams_that_round_trip() {
    use opensow_formats::{save::MapState, Reader};
    let Some(game) = GameDir::locate() else { return eprintln!("game not found, skipping") };
    let dir = game.resolve("Data\\Levels").unwrap();
    let mut checked = 0;
    let mut no_marks_cells = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| !x.eq_ignore_ascii_case("map")) {
            continue;
        }
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        let data = std::fs::read(&p).unwrap();
        let mut r = Reader::new(&data);
        let st = MapState::parse(&mut r).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(r.remaining(), 0, "{name}: the stream is consumed whole");
        assert_eq!(st.version, "VER7", "{name}");
        assert!(st.cameras.is_empty() && st.items.is_empty(), "{name}");
        assert_eq!(st.to_bytes().as_deref(), Some(&data[..]), "{name}: writer reproduces the file");
        let map = Map::parse(&data).unwrap();
        assert_eq!((map.a, map.b), (st.scroll_x, st.scroll_y), "{name}");
        let w = st.width as usize;
        for (i, c) in st.cells.iter().enumerate() {
            let (x, y) = (i % w, i / w);
            assert_eq!(c.coord, ((x as u16) << 8) | y as u16, "{name}: cell ({x},{y}) coordinate stamp");
            let m = map.cell(x as u32, y as u32).unwrap();
            assert_eq!((c.ground != 0, c.air != 0), (m.blocked, m.border), "{name}: ({x},{y})");
            assert!(c.ground == 0 || c.ground == 0xFF_FFFD, "{name}: terrain marker");
            assert_eq!(c.no_build != 0, m.no_build, "{name}");
            assert_eq!(c.no_marks != 0, m.no_marks, "{name}");
            assert!(c.reserved == 0 && c.building == 0 && c.no_build <= 1 && c.no_marks <= 1, "{name}: ({x},{y})");
        }
        no_marks_cells.push((name, st.cells.iter().filter(|c| c.no_marks != 0).count()));
        checked += 1;
    }
    assert!(checked >= 23, "{checked} maps");
    // only the second level has a no-marks area
    for (name, n) in no_marks_cells {
        assert_eq!(n, if name.eq_ignore_ascii_case("M02.map") { 497 } else { 0 }, "{name}");
    }
}

#[test]
fn unit_corpse_timer_and_final_blasts_match_the_exe() {
    use opensow_formats::effects::{corpse_blasts, CORPSE_BLAST_BELOW, CORPSE_LIFE};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // 0x46a5a9: unit+0x90 = 430, +0x94 = -1, +0x98 = 0
    assert!(exe.at(0x46a5a9, 10).starts_with(&[0xC7, 0x87, 0x90, 0, 0, 0]));
    assert_eq!(u32::from_le_bytes(exe.at(0x46a5af, 4).try_into().unwrap()) as i32, CORPSE_LIFE);
    // 0x46b649: cmp eax, 0x1e; jge (no blasts at or above 30)
    assert_eq!(exe.at(0x46b649, 3), [0x83, 0xF8, CORPSE_BLAST_BELOW as u8]);
    // the variant dispatch: sub 2 / je 0x46b782, sub 1 / je 0x46b6fe, sub 1 / jne exit, then the chain from 0x46b67a
    assert_eq!(exe.at(0x46b65f, 3), [0x83, 0xE8, 0x02]);
    assert_eq!(u32::from_le_bytes(exe.at(0x46b664, 4).try_into().unwrap()), 0x46b782 - 0x46b668);
    assert_eq!(u32::from_le_bytes(exe.at(0x46b66d, 4).try_into().unwrap()), 0x46b6fe - 0x46b671);
    // the four spawns, in chain order: `push size` right before `push y` (0x46b6c5, 0x46b749, 0x46b7cd, 0x46b851)
    let sizes: Vec<u8> = [0x46b6c5u32, 0x46b749, 0x46b7cd, 0x46b851]
        .iter()
        .map(|&a| {
            let b = exe.at(a, 2);
            assert_eq!(b[0], 0x6A);
            b[1]
        })
        .collect();
    for (variant, n) in [(4u32, 4usize), (3, 3), (2, 2)] {
        assert_eq!(corpse_blasts(variant), &sizes[4 - n..], "variant {variant}");
    }
    assert!(corpse_blasts(0).is_empty() && corpse_blasts(5).is_empty());
}

#[test]
fn top_offsets_match_the_exes_constructor() {
    use opensow_formats::aim::top_dy;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let byte_table = exe.at(0x47ac70, 0x71).to_vec();
    let cases = *byte_table.iter().max().unwrap() as usize + 1;
    let jt: Vec<u32> = exe.at(0x47ac04, cases * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let adj_idx = exe.at(0x47acf4, 0x2d).to_vec();
    let adj_jt: Vec<u32> = exe.at(0x47ace4, 16).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in (1..=15).chain(40..=45).chain(109..=113) {
        let (f, _) = exe.top_case(jt[byte_table[kind as usize - 1] as usize]);
        assert_eq!(f.get(&0x5c), None, "kind {kind}: the horizontal offset stays 0");
        let adj = if kind <= 0x2c {
            match adj_jt[adj_idx[kind as usize] as usize] {
                0x47ab89 => -10,
                0x47ab8f => -4,
                0x47ab95 => -16,
                0x47ab99 => 0,
                o => panic!("{o:x}"),
            }
        } else {
            0
        };
        assert_eq!(top_dy(kind), f.get(&0x60).copied().unwrap_or(0) + adj, "kind {kind}");
    }
}

#[test]
fn a_bombers_bomb_takes_its_flight_direction_as_sprite_variant() {
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // 0x4684e3 push 0x12 (type 18); call 0x411dc0; mov eax, [edi+0x34]; ...; mov [esi+0x268c], eax
    assert_eq!(exe.at(0x4684e3, 2), [0x6A, 0x12]);
    assert_eq!(exe.at(0x4684ea, 3), [0x8B, 0x47, 0x34]);
    assert_eq!(exe.at(0x4684f2, 6), [0x89, 0x86, 0x8C, 0x26, 0, 0]);
}

#[test]
fn turret_placement_test_reads_the_layers_documented_in_ai_md() {
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let code = exe.at(0x408be0, 0x170).to_vec();
    let has = |pat: &[u8]| code.windows(pat.len()).any(|w| w == pat);
    // proximity scan over the building-handle grid 0x6f3a4c, rows x-2.. and columns y-2..
    assert!(has(&[0x8D, 0x1C, 0x85, 0x4C, 0x3A, 0x6F, 0x00]), "building grid 0x6f3a4c");
    assert!(has(&[0x83, 0xEF, 0x02]) && has(&[0x8D, 0x5E, 0xFE]), "x - 2 and y - 2");
    assert!(has(&[0x83, 0xC0, 0x02]), "+ 2 beyond the footprint");
    // editor flag [0x517ae9] forces the proximity condition
    assert!(has(&[0x80, 0x3D, 0xE9, 0x7A, 0x51, 0x00, 0x00]));
    // per cell: static grid 0x5eac38 subclass != 1, walker grid 0x6c7a4c == 0, no-build byte 0x6efa4c == 0, near == 1
    assert!(has(&[0xFF, 0xB6, 0x38, 0xAC, 0x5E, 0x00]), "static grid 0x5eac38");
    assert!(has(&[0x83, 0xB8, 0xFE, 0, 0, 0]) || has(&[0x83, 0xF8, 0x01]), "subclass == 1 test");
    assert!(has(&[0x83, 0xBE, 0x4C, 0x7A, 0x6C, 0x00, 0x00]), "walker grid 0x6c7a4c");
    assert!(has(&[0x80, 0x3F, 0x00]) && has(&[0x8D, 0xB8, 0x4C, 0xFA, 0x6E, 0x00]), "no-build layer 0x6efa4c");
}

#[test]
fn building_start_hp_follows_the_exes_percentage_rule() {
    use opensow_formats::advancer::building_max_hp;
    let Some(game) = GameDir::locate() else { return };
    let dir = game.resolve("Data\\Levels").unwrap();
    let (mut n, mut off) = (0, Vec::new());
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let path = e.path();
        if !path.extension().is_some_and(|x| x.eq_ignore_ascii_case("edt") || x.eq_ignore_ascii_case("net")) {
            continue;
        }
        for o in
            Level::parse(&std::fs::read(&path).unwrap()).unwrap().objects.iter().filter(|o| o.object_kind().is_building() || kinds::TURRETS.contains(&o.kind))
        {
            n += 1;
            let want = o.start_hp(building_max_hp(o.kind));
            if (want - o.hp()).abs() > 1 {
                off.push((path.file_name().unwrap().to_string_lossy().to_string(), o.kind, o.hp(), want));
            }
        }
    }
    // word 13 is the editor's own copy and drifts from the percentage (turrets by up to ~3 %); the exe uses the percentage
    assert!(n > 2000 && off.len() < n / 10, "{} of {n} differ", off.len());
    for o in Level::parse(&std::fs::read(game.resolve("Data\\Levels\\M01.edt").unwrap()).unwrap()).unwrap().objects.iter().filter(|o| o.raw[14] == 2) {
        assert_eq!(o.start_hp(13536), 6768, "neutral buildings start at half");
    }
}

#[test]
fn frame_sender_writes_the_command_queues_and_snapshot_in_the_documented_order() {
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let code = exe.at(0x4296a0, 0xc00).to_vec();
    // every `call 0x4528f0` in the command part (before 0x42a057): the width is the `push imm8` that precedes the value push
    let widths = |from: u32, to: u32, target: u32| -> Vec<u8> {
        let mut out = Vec::new();
        for (i, w) in code.windows(5).enumerate() {
            let at = 0x4296a0 + i as u32;
            if at < from || at >= to || w[0] != 0xE8 {
                continue;
            }
            if (at + 5).wrapping_add(i32::from_le_bytes(w[1..5].try_into().unwrap()) as u32) != target {
                continue;
            }
            // the nearest `6A xx` before the call whose next instruction is a value push (FF /6, 50, or a register push)
            let back = &code[i.saturating_sub(24)..i];
            let n = (0..back.len().saturating_sub(2))
                .rev()
                .find(|&j| back[j] == 0x6A && back[j + 1] <= 0x20 && matches!(back[j + 2], 0xFF | 0x50 | 0x51 | 0x52 | 0x53 | 0x56 | 0x57 | 0x6A))
                .map(|j| back[j + 1]);
            // the release point pushes its width, then computes the value (`sar eax, 5`) before the second push: take the nearest push imm8
            let n = n.or_else(|| (0..back.len().saturating_sub(1)).rev().find(|&j| back[j] == 0x6A && back[j + 1] <= 0x20).map(|j| back[j + 1]));
            out.push(n.unwrap_or(255));
        }
        out
    };
    let want: Vec<u8> = vec![
        1, 7, 7, 3, 12, 1, // PlaceSentry
        1, 7, 7, 32, 1, // ReleasePoint
        1, 7, 7, 9, 32, 1, // UnitGoto
        1, 32, 9, 32, 1, // UnitAttack
        1, 9, 32, 1, // UnitScatter
        1, 32, 32, 1, 1, 32, 32, 1, 1, 32, 32, 1, // HDestroy, HRepair, HUpgrade
        1, 14, 14, 8, 1, // UfoStrike
        1, 32, 3, 1, // TechLevelUp
    ];
    let got = widths(0x4296a0, 0x42a057, 0x4528f0);
    assert_eq!(got, want, "bit widths of the ten command queues");
    // the host part: header selectors 7 (player), 4 (buildings), 3 (units), 6 (aircraft), 5 (bullets), in this order
    let selectors: Vec<u8> = widths(0x42a057, 0x42a273, 0x4527c0);
    assert_eq!(selectors.len(), 5);
    let pos = |pat: &[u8]| code.windows(pat.len()).position(|w| w == pat).map(|p| 0x4296a0 + p as u32);
    let (p4, p3, p6, p5) = (pos(&[0x6A, 0x04, 0x8D]), pos(&[0x6A, 0x03, 0x8D]), pos(&[0x6A, 0x06, 0x8D]), pos(&[0x6A, 0x05, 0x8D]));
    assert!(p4 < p3 && p3 < p6 && p6 < p5, "{p4:?} {p3:?} {p6:?} {p5:?}");
    // bullets of types 0x16, 0x20 and 0x21 are not sent: `cmp eax, 0x16 / 0x20 / 0x21` before the writer call
    for t in [0x16u8, 0x20, 0x21] {
        assert!(code.windows(3).any(|w| w == [0x83, 0xF8, t]), "type {t:#x}");
    }
}

#[test]
fn building_destruction_animation_lengths_match_the_exes_constructor() {
    use opensow_formats::effects::destruction_bursts;
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    // 0x40a330 stores [b+0x108] = 5 first (0x40a381); the cases 0..=2 of the byte table 0x40af64 (indexed by kind - 25) end in
    // 0x40a93f, which stores 2: the five turrets. Every building is another case.
    let table = exe.at(0x40af64, 0x54).to_vec();
    for kind in 25..=108 {
        let case = table[(kind - 25) as usize];
        if kinds::TURRETS.contains(&kind) || (100..=108).contains(&kind) {
            let expect = if case <= 2 { 2 } else { 5 };
            assert_eq!(destruction_bursts(kind), expect, "kind {kind}");
        }
    }
    assert_eq!(exe.at(0x40a381, 10)[..6], [0xC7, 0x83, 0x08, 0x01, 0x00, 0x00]);
    assert_eq!(exe.at(0x40a381, 10)[6], 5);
    assert_eq!(exe.at(0x40a93f, 10)[6], 2);
    // 0x409b60: two passes per tick (`add eax, eax`), debris counter reset 0x5a + rand(15)
    assert_eq!(exe.at(0x409b91, 2), [0x03, 0xC0]);
    assert_eq!(exe.at(0x409e7d, 3), [0x83, 0xC0, 0x5A]);
}

#[test]
fn shot_spread_and_rounds_match_the_exes_top_constructor() {
    use opensow_formats::aim::{rounds, spread};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let byte_table = exe.at(0x47ac70, 0x71).to_vec();
    let cases = *byte_table.iter().max().unwrap() as usize + 1;
    let jt: Vec<u32> = exe.at(0x47ac04, cases * 4).chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    for kind in (1..=15).chain(40..=45).chain(109..=113) {
        let (f, _) = exe.top_case(jt[byte_table[kind - 1] as usize]);
        assert_eq!(f.get(&0x20).copied(), Some(spread(kind as i32) as i32), "spread of kind {kind}");
        assert_eq!(f.get(&0x30).copied(), Some(rounds(kind as i32) as i32), "rounds of kind {kind}");
    }
}

/// The sprite encoder and the archive builder reproduce the installed files exactly (so re-imported art is a drop-in).
#[test]
fn sprite_encoder_and_archive_builder_reproduce_the_install() {
    use opensow_formats::{
        megafile::{self, MegaFile},
        sprite,
    };
    let Some(game) = GameDir::locate() else { return };
    let mega = MegaFile::open(&game.data().join("sprites.info"), &game.data().join("sprites.data")).unwrap();
    let mut all = Vec::new();
    let (mut sprites, mut differ) = (0, Vec::new());
    for e in &mega.entries {
        let bytes = mega.read(e).unwrap();
        if let Some(kind) = sprite::Kind::from_extension(std::path::Path::new(&e.name)) {
            let again = sprite::encode(&sprite::parse(&bytes, kind).unwrap(), kind);
            sprites += 1;
            if again != bytes {
                differ.push(e.name.clone());
            }
        }
        all.push((e.name.clone(), bytes));
    }
    assert!(sprites > 200);
    assert!(differ.is_empty(), "{} of {sprites} sprite files re-encode differently: {:?}", differ.len(), &differ[..differ.len().min(5)]);
    let (info, data) = megafile::build(&all);
    assert_eq!(info, std::fs::read(game.data().join("sprites.info")).unwrap(), "sprites.info");
    assert_eq!(data, std::fs::read(game.data().join("sprites.data")).unwrap(), "sprites.data");
}

/// The chat cheats (`0x426c90`): the phrases the exe compares the typed line with and the amounts / air force codes they hand
/// out are the ones in `formats::cheats` (docs/chat.md).
#[test]
fn cheat_phrases_match_the_exes_key_function() {
    use opensow_formats::cheats::{MAX_LINE_WIDTH, MONEY, PHRASES, RESEARCH};
    let Some(game) = GameDir::locate() else { return };
    let Some(exe) = Exe::open(&game) else { return };
    let (start, len) = (0x426c90u32, 0x428620u32 - 0x426c90);
    let code = exe.at(start, len as usize);
    // `push index; lea ecx,[ecx+4]; call [0x4855f8]; cmp al, imm8`: character `index` of the typed line against `imm8`
    let call = [0xFF, 0x15, 0xF8, 0x55, 0x48, 0x00, 0x3C];
    let mut phrases: Vec<String> = Vec::new();
    for i in 5..code.len() - 8 {
        if code[i..i + 7] == call && code[i - 5] == 0x6A && code[i - 3..i] == [0x8D, 0x49, 0x04] {
            let (index, ch) = (code[i - 4] as usize, code[i + 7] as char);
            if index == 0 {
                phrases.push(String::new());
            }
            let p = phrases.last_mut().expect("the first comparison is character 0");
            assert_eq!(p.len(), index, "characters come in order");
            p.push(ch);
        }
    }
    let ours: Vec<&str> = PHRASES.iter().map(|&(p, _)| p).collect();
    assert_eq!(phrases, ours);
    // the effects: `add dword ptr [4*eax + 0x517af8], 0x3e8` and `... + 0x517b04], 0xc8`
    let find = |pat: &[u8]| code.windows(pat.len()).position(|w| w == pat);
    let money = find(&[0x81, 0x04, 0x85, 0xF8, 0x7A, 0x51, 0x00, 0xE8, 0x03, 0x00, 0x00]).expect("money add");
    assert_eq!(MONEY, 0x3e8);
    let research = find(&[0x81, 0x04, 0x85, 0x04, 0x7B, 0x51, 0x00, 0xC8, 0x00, 0x00, 0x00]).expect("research add");
    assert_eq!(RESEARCH, 0xc8);
    assert!(money < research);
    // `push code; add ecx, 0x70ba50; call 0x453e80` per air force, in phrase order
    let mut at = research;
    for &(_, c) in PHRASES.iter().filter(|(_, c)| c.air_code().is_some()) {
        let code_byte = c.air_code().unwrap() as u8;
        let pat = [0x6A, code_byte, 0x81, 0xC1, 0x50, 0xBA, 0x70, 0x00, 0xE8];
        let off = code[at..].windows(pat.len()).position(|w| w == pat).unwrap_or_else(|| panic!("{c:?}")) + at;
        at = off + 1;
    }
    // one minute: `inc dword ptr [0x5baac4]` after `cmp byte ptr [0x5baa99], 0` (the countdown has not ended)
    assert!(find(&[0x80, 0x3D, 0x99, 0xAA, 0x5B, 0x00, 0x00, 0x75, 0x06, 0xFF, 0x05, 0xC4, 0xAA, 0x5B, 0x00]).is_some());
    // the line stops growing once its width reaches 0x18b
    assert!(find(&[0x81, 0x7D, 0xE8, 0x8B, 0x01, 0x00, 0x00]).is_some(), "cmp [ebp-0x18], 0x18b");
    assert_eq!(MAX_LINE_WIDTH, 0x18b);
}

/// Every installed `.srf` / `.srfa` survives to_jpeg -> from_jpeg unchanged.
#[test]
fn srf_wrapper_reproduces_the_install() {
    use opensow_formats::srf;
    let Some(game) = GameDir::locate() else { return };
    let mut n = 0;
    for dir in ["Data\\PICTURES", "Data\\Levels"] {
        for e in std::fs::read_dir(game.resolve(dir).unwrap()).unwrap().flatten() {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
            if ext == "srf" || ext == "srfa" {
                let bytes = std::fs::read(&p).unwrap();
                assert_eq!(srf::from_jpeg(&bytes, &srf::to_jpeg(&bytes).unwrap()).unwrap(), bytes, "{}", p.display());
                n += 1;
            }
        }
    }
    assert!(n > 40);
}

#[test]
fn warmonger_missions_and_texts_load_through_the_edition_overlay() {
    use opensow_formats::{warmonger, Edition};
    let Some(mut game) = GameDir::locate() else { return eprintln!("game not found, skipping") };
    if game.warmonger.is_none() {
        return eprintln!("Warmonger not found, skipping");
    }
    let base_m01 = read(&game, "Data\\Levels\\M01.map").unwrap();
    game.edition = Edition::Warmonger;
    assert_ne!(read(&game, "Data\\Levels\\M01.map").unwrap(), base_m01, "Warmonger's M01 is its own map");
    // unlike the base game, Warmonger's M07 puts two player turrets (kind 26, cells 68/69,45) on blocked cells: allow only those
    let mut blocked = 0;
    for &n in &warmonger::LANDS {
        let name = format!("M{n:02}");
        let map = Map::parse(&read(&game, &format!("Data\\Levels\\{name}.map")).unwrap()).unwrap();
        let level = Level::parse(&read(&game, &format!("Data\\Levels\\{name}.edt")).unwrap()).unwrap();
        assert_eq!(level.level_id(), n as i32, "{name} header level id");
        for o in &level.objects {
            let c = map.cell(o.cell_x as u32, o.cell_y as u32).unwrap_or_else(|| panic!("{name}: object outside map"));
            blocked += usize::from(c.blocked);
        }
        assert!(game.resolve(&format!("Data\\Levels\\{name}.sgv")).is_some(), "{name} briefing movie");
    }
    assert!(blocked <= 2, "{blocked} objects on blocked cells");
    // the missing lands have no data
    assert!(game.resolve("Data\\Levels\\M05.map").is_none());
    let exe = std::fs::read(game.warmonger_exe().expect("exe")).unwrap();
    let texts = warmonger::parse_texts(&exe).expect("eleven land texts in the exe");
    assert!(texts[0].heading.starts_with("TERRITORY : 01") && texts[10].heading.starts_with("TERRITORY : 11"));
    assert!(texts[2].briefing.contains("convoy") && texts[10].objective.contains("Rahman"));
    let hints = warmonger::parse_hints(&exe).expect("hint lines in the exe");
    assert!(hints.upgrade_next.contains("next unit") && hints.fighters.starts_with("Now use fighters"), "{hints:?}");
    assert!(game.level_files().iter().any(|f| f.to_ascii_lowercase().ends_with(".net")));
}

/// A `.net` map has no terrain files of its own: it is played on the mission `M<level id>` of the edition that owns it
/// (`GameDir::net_terrain`, docs/skirmish.md). Every object of the script is inside that map and off its blocked cells.
#[test]
fn net_maps_are_played_on_the_terrain_of_their_level_id_mission() {
    let Some(game) = GameDir::locate() else { return };
    let mut checked = 0;
    for file in game.level_files().into_iter().filter(|n| n.to_lowercase().ends_with(".net")) {
        let name = file.rsplit_once('.').unwrap().0;
        let level = Level::parse(&read(&game, &format!("Data\\Levels\\{file}")).unwrap()).unwrap();
        let (terrain_game, mission) = game.net_terrain(name, level.level_id());
        assert_eq!(mission, format!("M{:02}", level.level_id()));
        let map = Map::parse(&read(&terrain_game, &format!("Data\\Levels\\{mission}.map")).unwrap_or_else(|| panic!("{name}: no {mission}.map"))).unwrap();
        for o in &level.objects {
            let c = map.cell(o.cell_x as u32, o.cell_y as u32).unwrap_or_else(|| panic!("{name}: object outside {mission}"));
            assert!(!c.blocked, "{name}: {} at ({},{}) on a blocked cell of {mission}", o.object_kind().name(), o.cell_x, o.cell_y);
        }
        checked += 1;
    }
    assert!(checked >= 8);
}
