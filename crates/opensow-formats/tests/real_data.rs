//! Invariants checked against a real installation. Skipped when the game is not found.

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
fn multiplayer_maps_have_two_hqs_and_start_positions_per_side() {
    let Some(game) = GameDir::locate() else { return };
    let dir = game.resolve("Data\\Levels").unwrap();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().map_or(true, |x| !x.eq_ignore_ascii_case("net")) {
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
    // Campaign: the human starts with no air forces, the enemy's stock sits in side 1, side 2 is unused.
    for n in 1..=22 {
        let l = Level::parse(&read(&game, &format!("Data\\Levels\\M{n:02}.edt")).unwrap()).unwrap();
        for st in l.air_stock() {
            assert!(st.counts.iter().all(|&c| (0..=20).contains(&c)), "M{n:02} {:?}", st);
            assert_eq!((st.counts[0], st.counts[2]), (0, 0), "M{n:02} {}", st.force.name());
        }
    }
    // Spot checks against the shipped data.
    let m10 = Level::parse(&read(&game, "Data\\Levels\\M10.edt").unwrap()).unwrap().air_stock();
    let side1 = |f: AirForce| m10.iter().find(|s| s.force == f).unwrap().counts[1];
    assert_eq!([side1(AirForce::Bomber), side1(AirForce::Asteroid), side1(AirForce::Carryall), side1(AirForce::Tripler), side1(AirForce::Fighter)], [4, 0, 7, 4, 6]);
    // Multiplayer: both players get the same stock.
    let dir = game.resolve("Data\\Levels").unwrap();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().extension().map_or(false, |x| x.eq_ignore_ascii_case("net")) {
            for st in Level::parse(&std::fs::read(e.path()).unwrap()).unwrap().air_stock() {
                assert_eq!(st.counts[1], st.counts[2], "{} {}", e.path().display(), st.force.name());
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
        let bottoms: Vec<i32> = ('A'..='Z').filter(|&c| c != 'Q' && c != 'J').map(|c| { let g = i.glyph(c).unwrap(); -(g.hot_y as i32) + g.height as i32 }).collect();
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
    assert_eq!(load(Language::Czech).get(0), Some("nová hra"));
    assert_eq!(load(Language::ChineseSimplified).get(0), Some("新游戏"));
}

#[test]
fn fire_tables_exist_for_every_top_and_trace_smooth_loops() {
    use opensow_formats::{fire::{FireTables, FACINGS}, megafile::MegaFile, units::TOPS};
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
    use opensow_formats::{anim::{steps_for, walk_params}, megafile::MegaFile, sprite, units::unit_art};
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
    fn top_case(&self, va: u32) -> (std::collections::HashMap<u32, i32>, Vec<(i32, i32, i32, i32)>) {
        let (mut pc, mut pushes) = (va, Vec::<i32>::new());
        let (mut fields, mut steps) = (std::collections::HashMap::new(), Vec::new());
        for _ in 0..400 {
            if pc == 0x47ab5f || pc == 0x47ab66 {
                return (fields, steps);
            }
            let code = self.at(pc, 12).to_vec();
            let imm32 = |o: usize| i32::from_le_bytes(code[o..o + 4].try_into().unwrap());
            match code[0] {
                0x6A => { pushes.push(code[1] as i8 as i32); pc += 2 }
                0x68 => { pushes.push(imm32(1)); pc += 5 }
                0x8B if code[1] == 0xCF => pc += 2, // mov ecx, edi
                0xC7 if code[1] == 0x47 => { fields.insert(code[2] as u32, imm32(3)); pc += 7 }
                0xC7 if code[1] == 0x87 => { fields.insert(u32::from_le_bytes(code[2..6].try_into().unwrap()), imm32(6)); pc += 10 }
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
    use opensow_formats::{fire::FireTables, megafile::MegaFile, recoil::{fire_sequence, FLASH_SETS}, sprite, units::unit_art};
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
                (0xC7, 0x86) => { fields.insert(u32::from_le_bytes(code[2..6].try_into().unwrap()), imm32(6)); pc += 10 }
                (0xC7, 0x46) => { fields.insert(code[2] as u32, imm32(3)); pc += 7 }
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
            // 13/14/32 compute their speed with the random generator, 28/29 also trigger an effect, 31 branches on a comparison.
            assert!(want.partial || [13, 14, 28, 29, 31, 32].contains(&t), "type {t} could not be decoded but is not marked partial");
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
        (0..code.len() - 10).find(|&i| code[i..i + 2] == [0xC7, 0x83] && u32::from_le_bytes(code[i + 2..i + 6].try_into().unwrap()) == disp).map(|i| i32::from_le_bytes(code[i + 6..i + 10].try_into().unwrap()))
    };
    for (kind, from) in [(101, 0x40a596u32), (102, 0x40a636)] {
        let (spawn, walk) = factory_exit(kind).unwrap();
        let got = |d| find(from, 0xa0, d).unwrap();
        assert_eq!((got(0x124), got(0x128)), spawn, "kind {kind} spawn");
        assert_eq!((got(0x12c), got(0x130)), walk, "kind {kind} walk-to");
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
        let imm: Vec<i32> = (0..code.len() - 5).filter(|&i| code[i..i + 2] == [0xC7, 0x00]).map(|i| i32::from_le_bytes(code[i + 2..i + 6].try_into().unwrap())).take(3).collect();
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
    let calls = (0..code.len() - 4).filter(|&i| code[i] == 0xE8 && (0x406dc0 + i as u32 + 5).wrapping_add(i32::from_le_bytes(code[i + 1..i + 5].try_into().unwrap()) as u32) == 0x407470).count();
    assert_eq!(calls, SFX.len(), "one load call per table entry");
}
