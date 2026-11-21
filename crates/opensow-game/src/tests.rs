//! Tests of the game's screens and input: each drives a `Game` the way the window does (clicks, keys, frames).

use super::*;
use winit::keyboard::SmolStr;

fn game() -> Option<Game> {
    Assets::load(GameDir::locate()?).ok().map(Game::new)
}

/// Saves the running skirmish into slot 1 of a store in `dir`, as the save page does.
fn save_skirmish(g: &mut Game, dir: &std::path::Path) -> saveui::Store {
    let store = saveui::Store { write: dir.to_path_buf(), also: None, edition: g.assets.game.edition };
    let skirmish = g.skirmish_bytes();
    let p = g.play.as_mut().unwrap();
    let (level, snapshot) = (p.level.clone(), p.snapshot());
    store.write(1, "t", &level, &snapshot, &skirmish, &vec![0u16; 160 * 120]).unwrap();
    store
}

/// Level, tick, number of structures, the units, the skirmish flag and the difficulty of a running game.
type Fingerprint = (String, u64, usize, Vec<(i32, i32, i32, i32)>, bool, sim::Difficulty);

/// What a running game is, compared before saving and after loading.
fn fingerprint(g: &Game) -> Fingerprint {
    let p = g.play.as_ref().expect("a running game");
    let mut units: Vec<_> = p.world.units.iter().map(|u| (u.kind, u.x as i32, u.y as i32, u.hp)).collect();
    units.sort();
    (p.level.clone(), p.world.tick, p.world.structures.len(), units, p.world.skirmish, p.world.difficulty)
}

#[test]
fn a_skirmish_save_on_a_shipped_map_loads_back() {
    let Some(mut g) = game() else { return };
    g.headless_skirmish("03-Gold", 0, Some((7777, 5000)), None, sim::Difficulty::Hard, None);
    assert!(g.skirmish.is_some(), "the skirmish started");
    for _ in 0..400 {
        g.play.as_mut().unwrap().world.step();
    }
    let before = fingerprint(&g);
    let dir = std::env::temp_dir().join(format!("opensow-skirmish-save-{}", std::process::id()));
    let store = save_skirmish(&mut g, &dir);
    let saved = store.level_of(1).unwrap();
    assert!(!saved.skirmish.is_empty(), "the run is stored");
    // from the main menu
    g.act(Action::ToMain);
    g.saveui_event(saveui::Event::Load(saved));
    assert!(g.screen.is_none(), "no error message: {:?}", g.screen.as_ref().map(|s| format!("{s:?}")));
    assert_eq!(fingerprint(&g), before);
    let run = g.skirmish.as_ref().expect("the skirmish run is back (RESTART, the star)");
    assert_eq!((run.map.as_str(), run.difficulty, run.setup.sides[0].credits, run.custom.is_none()), ("03-Gold Rush", sim::Difficulty::Hard, 7777, true));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_skirmish_save_on_a_custom_map_loads_back_also_one_without_the_stored_run() {
    let Some(mut g) = game() else { return };
    let maps = std::env::temp_dir().join(format!("opensow-skirmish-save-maps-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&maps);
    let path = mapeditor_ui::tests::demo_map_file(&maps, "Save test").expect("demo map");
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    let script = custom_maps::load(&path).unwrap().level;
    let map = format!("custom/{stem}");
    let run = SkirmishRun {
        map: map.clone(),
        setup: multiplayer::Setup::of_map(&script),
        human: 0,
        difficulty: sim::Difficulty::Easy,
        spectate: None,
        default_setup: true,
        custom: Some(path.clone()),
    };
    g.start_skirmish(run, script);
    for _ in 0..400 {
        g.play.as_mut().unwrap().world.step();
    }
    let before = fingerprint(&g);
    let dir = std::env::temp_dir().join(format!("opensow-skirmish-save-custom-{}", std::process::id()));
    let store = save_skirmish(&mut g, &dir);
    let saved = store.level_of(1).unwrap();
    assert_eq!(saved.level, map);
    g.act(Action::ToMain);
    g.saveui_event(saveui::Event::Load(saved.clone()));
    assert!(g.screen.is_none(), "no error message: {:?}", g.screen.as_ref().map(|s| format!("{s:?}")));
    assert_eq!(fingerprint(&g), before);
    assert_eq!(g.skirmish.as_ref().map(|r| (r.custom.clone(), r.difficulty)), Some((Some(path.clone()), sim::Difficulty::Easy)));
    // a save written before the run was stored: only the level name; the map is found in the maps folder by its key
    let legacy = saveui::Saved { skirmish: Vec::new(), ..saved.clone() };
    let (run, _) = g.saved_skirmish(&legacy, &maps).expect("found by its key");
    assert_eq!((run.map.as_str(), run.custom.as_deref()), (map.as_str(), Some(path.as_path())));
    g.play = None;
    g.start_skirmish(run, custom_maps::load(&path).unwrap().level);
    g.play.as_mut().unwrap().restore(&legacy.snapshot).unwrap();
    assert_eq!(fingerprint(&g), before);
    // a mission is not a skirmish
    assert!(g.saved_skirmish(&saveui::Saved { level: "M03".into(), ..legacy }, &maps).is_none());
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(maps);
}

#[test]
fn a_skirmish_save_on_a_generated_map_loads_back() {
    let Some(mut g) = game() else { return };
    let maps = std::env::temp_dir().join(format!("opensow-skirmish-save-layout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&maps);
    let path = mapeditor_ui::tests::generated_map_file(&maps, "Layout test").expect("generated map");
    let m = custom_maps::load(&path).unwrap();
    assert!(m.layout.is_some());
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    let run = SkirmishRun {
        map: format!("custom/{stem}"),
        setup: multiplayer::Setup::of_map(&m.level),
        human: 0,
        difficulty: sim::Difficulty::Easy,
        spectate: None,
        default_setup: true,
        custom: Some(path.clone()),
    };
    g.start_skirmish(run, m.level.clone());
    let ground = g.play.as_ref().unwrap().scene.bg.clone();
    let built = custom_maps::load_scene(&g.assets.game, &m.ground(), 0, None, None).unwrap();
    assert!(ground == built.bg, "the match plays on the composed ground");
    for _ in 0..300 {
        g.play.as_mut().unwrap().world.step();
    }
    let before = fingerprint(&g);
    let dir = std::env::temp_dir().join(format!("opensow-skirmish-save-layout-store-{}", std::process::id()));
    let store = save_skirmish(&mut g, &dir);
    let saved = store.level_of(1).unwrap();
    g.act(Action::ToMain);
    g.saveui_event(saveui::Event::Load(saved));
    assert!(g.screen.is_none(), "no error message: {:?}", g.screen.as_ref().map(|s| format!("{s:?}")));
    assert_eq!(fingerprint(&g), before);
    assert!(g.play.as_ref().unwrap().scene.bg == ground, "the same ground is rebuilt");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(maps);
}

#[test]
fn the_map_editor_row_opens_the_editor_and_back_returns() {
    let Some(mut g) = game() else { return };
    g.act(Action::MapEditor);
    assert!(g.map_editor.is_some() && g.screen.is_none());
    let (w, h) = (1280, 800);
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert!(g.map_editor.is_none() && matches!(g.screen, Some(Screen::Main)));
}

#[test]
fn the_in_game_wheel_zoom_is_eased_and_keeps_the_point_under_the_pointer() {
    let Some(mut g) = game() else { return };
    g.mute = true;
    g.start("M01");
    let (w, h) = (1280, 800);
    let settings = g.settings.clone();
    let p = g.play.as_mut().expect("M01 runs");
    p.update(1.0 / 60.0, &settings, w, h);
    p.paused = true;
    let start = p.view.zoom;
    p.cursor = (640, 400);
    let under = (p.view.x + 640.0 / p.view.zoom, p.view.y + 400.0 / p.view.zoom);
    p.wheel(1.0, w, h);
    assert_eq!(p.view.zoom, start, "the wheel only aims");
    p.update(1.0 / 60.0, &settings, w, h);
    let one = p.view.zoom;
    assert!(one > start && one < start * 1.08, "{start} -> {one}: part of the way in one frame");
    for _ in 0..120 {
        p.update(1.0 / 60.0, &settings, w, h);
    }
    assert!((p.view.zoom - start * 1.08).abs() < 1e-3, "8 % a notch: {}", p.view.zoom);
    let now = (p.view.x + 640.0 / p.view.zoom, p.view.y + 400.0 / p.view.zoom);
    assert!((now.0 - under.0).abs() < 1.0 && (now.1 - under.1).abs() < 1.0, "{under:?} -> {now:?}");
    // never past the limits
    for _ in 0..40 {
        p.wheel(-1.0, w, h);
    }
    for _ in 0..240 {
        p.update(1.0 / 60.0, &settings, w, h);
    }
    let fill = (w as f32 / p.scene.bg_w as f32).max(h as f32 / p.scene.bg_h as f32).max(0.3);
    assert!((p.view.zoom - fill).abs() < 1e-4);
}

#[test]
fn map_zoom_off_keeps_the_original_scale() {
    let Some(mut g) = game() else { return };
    g.mute = true;
    g.start("M01");
    let (w, h) = (1280, 800);
    let mut settings = g.settings.clone();
    settings.zoom = true;
    let p = g.play.as_mut().expect("M01 runs");
    p.update(1.0 / 60.0, &settings, w, h);
    p.paused = true;
    p.wheel(3.0, w, h);
    for _ in 0..120 {
        p.update(1.0 / 60.0, &settings, w, h);
    }
    assert!(p.view.zoom > 1.2, "zoomed in: {}", p.view.zoom);
    let centre = |p: &crate::play::Play| (p.view.x + w as f32 / p.view.zoom / 2.0, p.view.y + h as f32 / p.view.zoom / 2.0);
    let before = centre(p);
    // switched off: back to scale 1 around the same middle, and the wheel does nothing
    settings.zoom = false;
    p.update(1.0 / 60.0, &settings, w, h);
    assert_eq!(p.view.zoom, 1.0);
    let after = centre(p);
    assert!((after.0 - before.0).abs() < 1.0 && (after.1 - before.1).abs() < 1.0, "{before:?} -> {after:?}");
    p.wheel(3.0, w, h);
    for _ in 0..60 {
        p.update(1.0 / 60.0, &settings, w, h);
    }
    assert_eq!(p.view.zoom, 1.0, "the wheel is ignored");
}

#[test]
fn menu_navigation() {
    let Some(mut g) = game() else { return };
    let (w, h) = (1280, 800);
    let b = g.buttons(w, h);
    assert_eq!(b.len(), 8, "the six original rows, SKIRMISH and MAP EDITOR");
    let first = b[0].rect;
    g.mouse_moved(first.x + 5, first.y + 5, w, h);
    assert_eq!(g.hover, Some(0));
    g.mouse_button(MouseButton::Left, true, first.x + 5, first.y + 5, w, h);
    g.mouse_button(MouseButton::Left, false, first.x + 5, first.y + 5, w, h);
    // New game opens the campaign map; Escape (or BACK) returns to the main menu
    assert!(g.lands.is_some() && g.screen.is_none());
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert!(g.lands.is_none());
    assert_eq!(g.screen, Some(Screen::Main));
    // Load game opens the load page; CANCEL goes back to the main menu
    let load = g.buttons(w, h)[1].rect;
    g.mouse_button(MouseButton::Left, true, load.x + 5, load.y + 5, w, h);
    g.mouse_button(MouseButton::Left, false, load.x + 5, load.y + 5, w, h);
    assert!(g.saveui.is_some() && g.screen.is_none());
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert!(g.saveui.is_none());
    assert_eq!(g.screen, Some(Screen::Main));
    // Skirmish opens the lobby against the computer; Escape returns
    let sk = g.buttons(w, h)[4].rect;
    g.mouse_button(MouseButton::Left, true, sk.x + 5, sk.y + 5, w, h);
    g.mouse_button(MouseButton::Left, false, sk.x + 5, sk.y + 5, w, h);
    assert!(g.skirmish_menu.is_some() && g.screen.is_none());
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert!(g.skirmish_menu.is_none());
    assert_eq!(g.screen, Some(Screen::Main));
    let mut buf = vec![0u32; w * h];
    g.draw(&mut buf, w, h);
    assert!(buf.iter().any(|&p| p != 0));
}

/// The bridge to the original's state stream (docs/savegame.md "The sim bridge"): a running mission is written as the exe's
/// stream, read back, applied to a fresh world of the level and written again; the two streams are identical, so everything
/// the bridge carries survives, and the restored world still runs.
#[test]
fn a_mission_round_trips_through_the_original_state_stream() {
    let Some(mut g) = game() else { return };
    for mission in ["M01", "M05", "M10", "M16", "M22"] {
        mission_trip(&mut g, mission);
    }
}

fn mission_trip(g: &mut Game, mission: &str) {
    use opensow_formats::{config::PassedLevels, save::State};
    g.start(mission);
    let (mut airborne, mut shots) = (0, 0);
    let (bytes, units, structures, credits) = {
        let p = g.play.as_mut().unwrap();
        let adv = p.world.player_units().find(|u| u.kind == kinds::ADVANCER).map(|u| u.id);
        if let Some(id) = adv {
            p.world.order_move(id, (20, 20), None);
        }
        // a bomber and a tripler in flight, and let the armies shoot
        p.world.air_stock[0][0] = 1;
        p.world.air_stock[0][3] = 1;
        let (cx, cy) = (p.world.cells.0 * 16, p.world.cells.1 * 16);
        p.world.launch_strike(0, opensow_formats::air::AirForce::Bomber, cx, cy);
        p.world.launch_strike(0, opensow_formats::air::AirForce::Tripler, cx / 2, cy / 2);
        for _ in 0..900 {
            p.world.step();
        }
        airborne += p.world.aircraft.len();
        shots += p.world.bullets.len() + p.world.beams.len();
        let number: u32 = mission[1..].parse().unwrap();
        let st = p.world.to_state(number, &PassedLevels::initial(), (64, 96));
        let bytes = st.to_bytes().expect("the state can be written");
        let key = |w: &sim::World| {
            let mut v: Vec<_> = w.units.iter().map(|u| (u.kind, u.player, u.x as i32, u.y as i32, u.hp)).collect();
            v.sort();
            v
        };
        let s: Vec<_> = p.world.structures.iter().map(|s| (s.cell, s.kind_id, s.hp.max(0), s.active)).collect();
        (bytes, key(&p.world), s, p.world.credits)
    };
    // the stream is valid and re-writes itself
    if let Some(dir) = std::env::var_os("OPENSOW_DUMP_STATES") {
        // scripts/verify_save_load.py runs the exe's load branch on these
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(std::path::Path::new(&dir).join(format!("{mission}.state")), &bytes);
    }
    let parsed = State::parse(&bytes, 1).expect("the written stream parses");
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
    assert_eq!(parsed.squads.iter().map(|g| g.1.len()).sum::<usize>(), airborne, "{mission}: squads");
    // with the install's sprite sheets every top and chassis carries its size words (the exe's initialiser reads them)
    for (_, o) in parsed.units.iter().flat_map(|g| g.1.iter()) {
        if o.get(0x2c).unwrap_or(0) != 0 {
            continue; // corpses have no parts
        }
        if let Some(t) = o.child(4).filter(|t| t.get(0x54).is_some_and(|i| i != u32::MAX)) {
            assert!(t.get(0x64).unwrap_or(0) > 0 && t.get(0x68).unwrap_or(0) > 0, "{mission}: top sprite size of kind {:?}", o.get(0x40));
        }
        if let Some(c) = o.child(8) {
            assert!(c.get(0x20).unwrap_or(0) > 1 && c.get(0x24).unwrap_or(0) > 1, "{mission}: chassis sprite size of kind {:?}", o.get(0x40));
        }
    }
    assert_eq!(parsed.bullets.iter().map(|g| g.1.len()).sum::<usize>(), shots, "{mission}: bullets");
    assert!(!units.is_empty() && !structures.is_empty(), "{mission}: nothing to save");
    assert!(parsed.buildings.iter().map(|b| b.1.len()).sum::<usize>() > 0, "{mission}: no building in the stream");
    assert_eq!((parsed.map.scroll_x, parsed.map.scroll_y, parsed.head.level), (64, 96, mission[1..].parse().unwrap()));
    // a fresh world of the level takes it
    g.start(mission);
    let p = g.play.as_mut().unwrap();
    p.world.apply_state(&parsed).expect("the state applies");
    let mut got: Vec<_> = p.world.units.iter().map(|u| (u.kind, u.player, u.x as i32, u.y as i32, u.hp)).collect();
    got.sort();
    assert_eq!(got, units, "{mission}: units differ after the trip");
    let s: Vec<_> = p.world.structures.iter().map(|s| (s.cell, s.kind_id, s.hp.max(0), s.active)).collect();
    assert_eq!(s, structures, "{mission}");
    assert_eq!((p.world.aircraft.len(), p.world.bullets.len() + p.world.beams.len()), (airborne, shots), "{mission}: shots in flight");
    assert_eq!(p.world.credits, credits);
    let again = p.world.to_state(mission[1..].parse().unwrap(), &PassedLevels::initial(), (64, 96)).to_bytes().unwrap();
    assert_eq!(again.len(), bytes.len());
    assert!(again == bytes, "{mission}: the second stream differs from the first");
    for _ in 0..300 {
        p.world.step();
    }
    // through the save container as a file of the original (no OpenSOW extension): the level comes from the head, the stream applies
    let dir = std::env::temp_dir().join(format!("opensow-bridge-{}-{}", mission, std::process::id()));
    let store = saveui::Store { write: dir.clone(), also: None, edition: opensow_formats::Edition::Original };
    store.write(2, "t", mission, &saveui::pack_payload(&bytes, &[]), &[], &vec![0u16; 160 * 120]).unwrap();
    let saved = store.level_of(2).unwrap();
    assert_eq!((saved.level.as_str(), saved.edition), (mission, opensow_formats::Edition::Original));
    let payload = saved.snapshot;
    g.start(mission);
    let p = g.play.as_mut().unwrap();
    p.restore(&payload).unwrap();
    let mut got: Vec<_> = p.world.units.iter().map(|u| (u.kind, u.player, u.x as i32, u.y as i32, u.hp)).collect();
    got.sort();
    assert_eq!(got, units, "{mission}: units differ after loading the file");
    let _ = std::fs::remove_dir_all(dir);
}

/// Streams for `scripts/verify_save_load.py` (the exe's load branch): every campaign mission of both editions and a few skirmish
/// maps, each right after the start and mid-game (air strikes, Advancer beams, corpses, a Rogon blast, a set bookmark and Tab / H
/// moved). Run with
/// `OPENSOW_DUMP_STATES=dir cargo test --release -p opensow-game -- --ignored streams_for_the_exe_load_check`.
#[test]
#[ignore]
fn streams_for_the_exe_load_check() {
    use opensow_formats::{save::State, Edition};
    let Some(dir) = std::env::var_os("OPENSOW_DUMP_STATES").map(std::path::PathBuf::from) else { return };
    let Some(mut g) = game() else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let dump = |g: &mut Game, name: &str| {
        for ticks in [0, 1500] {
            let p = g.play.as_mut().unwrap();
            if ticks > 0 {
                live_effects(&mut p.world, ticks);
                // a bookmark of our own and both round robins moved: the input record carries them
                p.look_at(600.0, 500.0);
                p.bookmark(4, true);
                p.select_next_unit();
                p.select_next_unit();
                p.select_next_advancer();
            }
            let (beams, nukes, corpses, air, bullets) =
                (p.world.beams.len(), p.world.nukes.len(), p.world.corpses.len(), p.world.aircraft.len(), p.world.bullets.len());
            let payload = p.snapshot();
            let stream = saveui::unpack_payload(&payload).unwrap().0.to_vec();
            assert_eq!(State::parse(&stream, 1).unwrap().to_bytes().unwrap(), stream, "{name}");
            std::fs::write(dir.join(format!("{name}_t{ticks}.state")), &stream).unwrap();
            println!("{name} t{ticks}: bullets {bullets} beams {beams} blasts {nukes} corpses {corpses} aircraft {air}");
        }
    };
    let started = |g: &mut Game, level: &str| {
        g.start(level);
        g.play.as_ref().is_some_and(|p| p.level == level) // M23 has no level script (.edt) and does not start
    };
    for level in g.levels.clone() {
        if started(&mut g, &level) {
            dump(&mut g, &level);
        }
    }
    for map in ["01-First", "03-Gold", "05-Nuclear", "08-Island"] {
        g.headless_skirmish(map, 0, None, None, sim::Difficulty::Normal, None);
        if g.play.is_some() {
            dump(&mut g, &format!("skirmish_{map}"));
        }
    }
    if g.assets.game.warmonger.is_some() {
        g.set_edition(Edition::Warmonger);
        for level in g.levels.clone() {
            if started(&mut g, &level) {
                dump(&mut g, &format!("WM_{level}"));
            }
        }
        g.set_edition(Edition::Original);
    }
}

/// Runs `ticks` steps with what a save mid-game should carry: two air strikes, an Advancer on its way to destroy a building,
/// and in the last steps a pod's Rogon blast and a few dead units still lying in their slots.
fn live_effects(w: &mut sim::World, ticks: usize) {
    w.air_stock[0][0] = 1;
    w.air_stock[0][3] = 1;
    let (cx, cy) = (w.cells.0 * 16, w.cells.1 * 16);
    w.launch_strike(0, opensow_formats::air::AirForce::Bomber, cx, cy);
    w.launch_strike(0, opensow_formats::air::AirForce::Tripler, cx / 2, cy / 2);
    for _ in 0..ticks - 40 {
        w.step();
    }
    let advancer = w.player_units().find(|u| u.kind == kinds::ADVANCER).map(|u| u.id);
    if let Some(a) = advancer {
        if let Some(t) = (0..w.structures.len()).find(|&t| w.structures[t].owner != opensow_formats::level::Owner::Player) {
            w.order_action(a, t);
        }
    }
    let at = w.player_units().next().map(|u| (u.x as i32, u.y as i32)).unwrap_or((cx, cy));
    if let Some(pod) = w.add_unit_near(kinds::POD, 1, at.0 + 96, at.1 + 96) {
        w.units.iter_mut().find(|u| u.id == pod).unwrap().hp = 0;
    }
    for u in w.units.iter_mut().filter(|u| u.kind != kinds::ADVANCER && u.kind != kinds::POD).take(3) {
        u.hp = 0;
    }
    for _ in 0..40 {
        w.step();
    }
}

/// The numpad bookmarks and the Tab / H round robins are the input record's `+0x2e4..`, `+0x2dc` and `+0x2e0` (`0x419100`): a mission
/// starts with the exe's nine default bookmarks (`0x414e20`), and both a save of ours and a stream-only save (the original's file)
/// bring back the bookmarks and continue Tab and H with the unit they would have picked next.
#[test]
fn bookmarks_and_the_tab_and_h_round_robins_survive_a_save() {
    use opensow_formats::save::State;
    let Some(mut g) = game() else { return };
    g.start("M05");
    let p = g.play.as_mut().unwrap();
    assert_eq!(p.bookmarks[0], (0x1c0, 0x640), "numpad 1 starts at the exe's default");
    assert_eq!(p.bookmarks[8], (0x6c0, 0x240));
    p.look_at(700.0, 650.0);
    p.bookmark(2, true);
    let mark = p.bookmarks[2];
    p.select_next_unit();
    p.select_next_unit();
    p.select_next_advancer();
    let picked = |p: &mut play::Play, advancer: bool| {
        if advancer {
            p.select_next_advancer()
        } else {
            p.select_next_unit()
        }
        p.world.units.iter().find(|u| u.selected).map(|u| (u.kind, u.x as i32, u.y as i32))
    };
    let payload = p.snapshot();
    let (stream, _) = saveui::unpack_payload(&payload).unwrap();
    let stock = &State::parse(stream, 1).unwrap().stocks[0];
    assert_eq!((stock.get(0x2e4 + 16), stock.get(0x2e8 + 16)), (Some(mark.0 as u32), Some(mark.1 as u32)), "bookmark 3 in the stream");
    let stream_only = saveui::pack_payload(stream, &[]);
    let next_tab = picked(p, false);
    let next_h = picked(p, true);
    assert!(next_tab.is_some() && next_h.is_some());
    for save in [payload.clone(), stream_only] {
        g.start("M05");
        let p = g.play.as_mut().unwrap();
        p.restore(&save).unwrap();
        assert_eq!(p.bookmarks[2], mark);
        assert_eq!(picked(p, false), next_tab, "Tab continues");
        assert_eq!(picked(p, true), next_h, "H continues");
    }
}

/// The original's stream has no tutorial script (its step `[0x70f2cc]` is outside the saved ranges), so a stream without our
/// extension loads level 0 as a plain mission; with the extension the script continues.
#[test]
fn the_tutorial_script_is_only_in_our_extension() {
    let Some(mut g) = game() else { return };
    g.start("M00");
    let a = {
        let p = g.play.as_mut().unwrap();
        for _ in 0..30 {
            p.world.step();
        }
        assert!(p.world.tutorial.is_some());
        p.snapshot()
    };
    let (stream, ours) = saveui::unpack_payload(&a).unwrap();
    assert!(!ours.is_empty());
    g.start("M00");
    g.play.as_mut().unwrap().restore(&a).unwrap();
    assert!(g.play.as_ref().unwrap().world.tutorial.is_some(), "our extension keeps the script");
    g.start("M00");
    let bare = saveui::pack_payload(stream, &[]);
    g.play.as_mut().unwrap().restore(&bare).unwrap();
    let w = &g.play.as_ref().unwrap().world;
    assert!((w.tutorial.is_none(), w.tutorial_step, w.ai) == (true, None, true), "the bare stream is a plain mission");
}

#[test]
fn a_saved_game_continues_exactly_where_it_was_saved() {
    let Some(mut g) = game() else { return };
    g.start("M05");
    let (a, b) = {
        let p = g.play.as_mut().unwrap();
        // give the player something to do so the state is not just the start
        let id = p.world.player_units().find(|u| u.kind == kinds::ADVANCER).map(|u| u.id).expect("an Advancer");
        p.world.order_move(id, (20, 20), None);
        for _ in 0..1500 {
            p.world.step();
        }
        (p.snapshot(), p.world.tick)
    };
    let mut orig = g.play.take().unwrap();
    assert_eq!(b, 1500);
    // through the save container: write a slot, read it back
    let dir = std::env::temp_dir().join(format!("opensow-roundtrip-{}", std::process::id()));
    let store = saveui::Store { write: dir.clone(), also: None, edition: opensow_formats::Edition::Original };
    store.write(3, "t", "M05", &a, &[], &vec![0u16; 160 * 120]).unwrap();
    let saved = store.level_of(3).unwrap();
    assert_eq!((saved.level.as_str(), saved.snapshot.len(), saved.skirmish.len()), ("M05", a.len(), 0));
    let snap = saved.snapshot;
    // a fresh play of the level takes the snapshot and both run on identically
    g.start("M05");
    g.play.as_mut().unwrap().restore(&snap).unwrap();
    let mut again = g.play.take().unwrap();
    assert_eq!(again.world.tick, 1500);
    g.start("M05");
    let mut other = g.play.take().unwrap();
    other.restore(&a).unwrap();
    for _ in 0..600 {
        again.world.step();
        other.world.step();
        orig.world.step();
    }
    // (JSON maps are compared as maps: the HashMaps' key order differs from run to run)
    let json = |w: &mut sim::World| serde_json::from_slice::<serde_json::Value>(&w.snapshot()).unwrap();
    assert!(json(&mut again.world) == json(&mut other.world), "the restored games diverged");
    assert!(json(&mut orig.world) == json(&mut other.world), "the restored game differs from the one that was never saved");
    let fresh_after = {
        g.start("M05");
        let mut f = g.play.take().unwrap();
        for _ in 0..2100 {
            f.world.step();
        }
        f.world.snapshot()
    };
    // (a fresh game without the move order differs; the restored one has the order's effect)
    assert!(serde_json::from_slice::<serde_json::Value>(&fresh_after).unwrap() != json(&mut orig.world));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn f5_quick_saves_to_slot_zero_and_shift_f5_loads_it() {
    let Some(mut g) = game() else { return };
    let dir = std::env::temp_dir().join(format!("opensow-quick-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("OPENSOW_SAVE_DIR", &dir);
    let (w, h) = (1024, 768);
    g.start("M05");
    g.update(0.0, w, h);
    for _ in 0..50 {
        g.play.as_mut().unwrap().world.step();
    }
    g.key(&Key::Named(NamedKey::F5), true, w, h);
    let mut buf = vec![0u32; w * h];
    g.draw(&mut buf, w, h); // the grab happens on the next drawn frame
    assert!(dir.join("savegame_048.info").exists() || std::fs::read_dir(&dir).is_ok_and(|d| d.count() >= 2), "slot 0 written");
    for _ in 0..70 {
        g.play.as_mut().unwrap().world.step();
    }
    assert_eq!(g.play.as_ref().unwrap().world.tick, 120);
    g.key(&Key::Named(NamedKey::Shift), true, w, h);
    g.key(&Key::Named(NamedKey::F5), true, w, h);
    assert_eq!(g.play.as_ref().unwrap().world.tick, 50, "back at the quick save");
    // the automatic save goes to the same slot: not right after a load, then when its time comes (no speech, same grab)
    g.autosave = true;
    for f in std::fs::read_dir(&dir).unwrap().flatten() {
        let _ = std::fs::remove_file(f.path());
    }
    g.update(0.0, w, h);
    assert!(!g.quick_save, "a loaded game is not saved over at once");
    let at = g.play.as_ref().unwrap().autosave_at;
    assert_eq!(at, 50 + crate::AUTOSAVE_TICKS);
    g.play.as_mut().unwrap().autosave_at = 50;
    g.update(0.0, w, h);
    assert!(g.quick_save && g.quick_save_auto, "due: the next frame is saved");
    g.draw(&mut buf, w, h);
    assert!(!g.quick_save && !g.quick_save_auto);
    assert!(std::fs::read_dir(&dir).is_ok_and(|d| d.count() >= 2), "slot 0 written again");
    assert_eq!(g.play.as_ref().unwrap().autosave_at, 50 + crate::AUTOSAVE_TICKS);
    std::env::remove_var("OPENSOW_SAVE_DIR");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_advancers_beam_sparks_are_drawn_under_its_body() {
    // exe order (0x420xxx): ground units, then bullets and effects, then the hovering Advancer's body (0x46a310) last
    let Some(mut g) = game() else { return };
    let (w, h) = (1024, 768);
    g.start("M01");
    g.update(0.0, w, h);
    let (id, centre) = {
        let p = g.play.as_mut().unwrap();
        let t = p.world.structure_at(13 * 32 + 16, 38 * 32 + 16).expect("a building to work on");
        let id = p.world.player_units().find(|u| u.kind == kinds::ADVANCER).map(|u| u.id).expect("an Advancer");
        assert!(p.world.order_action(id, t));
        for _ in 0..6000 {
            p.world.step();
            if !p.world.beams.is_empty() {
                break;
            }
        }
        assert!(!p.world.beams.is_empty(), "the Advancer never fired");
        for _ in 0..20 {
            p.world.step();
        }
        p.view.zoom = 2.0;
        let u = p.world.unit(id).unwrap();
        let (ux, uy) = (u.x, u.sprite_y());
        p.view.x = ux - w as f32 / 2.0 / p.view.zoom;
        p.view.y = uy - h as f32 / 2.0 / p.view.zoom;
        (id, p.view.to_screen(ux, uy))
    };
    let mut with = vec![0u32; w * h];
    g.draw(&mut with, w, h);
    {
        let p = g.play.as_mut().unwrap();
        p.world.beams.clear();
        p.world.bullets.clear();
        p.world.effects.clear();
    }
    let mut without = vec![0u32; w * h];
    g.draw(&mut without, w, h);
    let _ = id;
    assert!(with != without, "no sparks were drawn at all");
    for dy in -4i32..=4 {
        for dx in -4i32..=4 {
            let i = ((centre.1 + dy) as usize) * w + (centre.0 + dx) as usize;
            assert_eq!(with[i], without[i], "a spark covers the Advancer at ({dx}, {dy})");
        }
    }
}

#[test]
fn ground_units_behind_a_tower_are_hidden_by_its_foreground_pixels() {
    // the exe draws ground units through the screen mask (`0x461da0`): M04's tall towers are foreground tiles, so a tank north of a
    // tower's foot but under its dome on screen is not drawn at all, while the same tank on open ground is
    let Some(mut g) = game() else { return };
    let (w, h) = (800, 600);
    g.start("M04");
    g.update(0.0, w, h);
    // the tank's screen neighbourhood with and without it, the view centred on map point (x, y)
    let mut look = |x: f32, y: f32| -> (Vec<u32>, Vec<u32>) {
        let mut shots = Vec::new();
        for with in [false, true] {
            {
                let p = g.play.as_mut().unwrap();
                p.world.units.retain(|u| !(u.player == 0 && u.kind == 1 && (u.x - x).abs() < 1.0 && (u.y - y).abs() < 1.0));
                if with {
                    let id = p.world.add_unit_near(1, 0, x as i32, y as i32).expect("room for a tank");
                    let u = p.world.units.iter_mut().find(|u| u.id == id).unwrap();
                    (u.x, u.y) = (x, y);
                }
                p.view.zoom = 1.0;
                p.view.x = x - w as f32 / 2.0;
                p.view.y = y - h as f32 / 2.0;
            }
            let mut buf = vec![0u32; w * h];
            g.draw(&mut buf, w, h);
            shots.push((h / 2 - 40..h / 2 + 30).flat_map(|r| buf[r * w + w / 2 - 35..r * w + w / 2 + 35].to_vec()).collect());
        }
        (shots.remove(0), shots.remove(0))
    };
    let (without, with) = look((44 * 32 + 16) as f32, (20 * 32 + 16) as f32);
    let differ: Vec<usize> = (0..with.len()).filter(|&i| with[i] != without[i]).collect();
    assert!(differ.is_empty(), "the tank behind the tower shows through its dome at {:?}", differ.iter().map(|i| (i % 70, i / 70)).collect::<Vec<_>>());
    let (without, with) = look((44 * 32 + 16) as f32, (40 * 32 + 16) as f32);
    assert_ne!(without, with, "the tank on open ground is not drawn");
}

/// The scripted events of the real missions (docs/ai.md "Win, lose and mission scripts", docs/air.md "Mission-script launches"):
/// start carryalls and their squads, the countdown messages and rewards of the survival missions, the timer defeats.
#[test]
fn mission_scripts_on_the_real_levels() {
    let Some(mut g) = game() else { return };
    // a started mission with the computer switched off (only the scripts are under test) and, when `clock` is given, a
    // shortened countdown of that many seconds (the real ones are 20..50 minutes of ticks)
    let mut start = |m: &str, clock: Option<i32>| {
        g.start(m);
        let mut w = std::mem::take(&mut g.play).unwrap().world;
        w.ai = false;
        if let (Some(sec), Some((_, _, mode))) = (clock, w.timer) {
            w.timer = Some((sec / 60, sec % 60, mode));
        }
        w
    };
    let steps = |w: &mut sim::World, n: u64| (0..n).for_each(|_| w.step());
    let player = |w: &sim::World, kind: i32| w.units.iter().filter(|u| u.player == 0 && u.hp > 0 && u.kind == kind).count();
    let ids = |w: &sim::World, of: &[u8]| w.log.iter().map(|&(_, id)| id).filter(|id| of.contains(id)).collect::<Vec<_>>();

    // the clocks the levels start with (header words 65/66/67: enabled, minutes, end mode)
    for (m, want) in [
        ("M07", (20, 0, 2)),
        ("M12", (20, 0, 2)),
        ("M17", (30, 0, 2)),
        ("M15", (30, 0, 1)),
        ("M16", (40, 0, 1)),
        ("M21", (50, 0, 1)),
        ("M22", (50, 0, 1)),
        ("M01", (0, 0, 9)),
    ] {
        let w = start(m, None);
        assert_eq!(w.timer.unwrap_or((0, 0, 9)), want, "{m} clock");
    }

    // mission 1: two carryalls land six units each (4 x cannon 7 + 2 x flamer 10), mission 4 one carryall with two walkers
    let mut w = start("M01", None);
    steps(&mut w, 1500);
    assert_eq!((player(&w, 7), player(&w, 10)), (8, 4), "mission 1 airlift");
    // each carryall plays `carryall-land` (62) when it arrives and again when it leaves (`0x467592`, flags `[+0x8c]`, `[+0x8d]`)
    assert_eq!(w.sounds.iter().filter(|s| s.0 == 62).count(), 4, "carryall sounds");
    // mission 1: both tech-levels of the medium factory at their limit pay 8 fighters and 3 carryalls once (`0x42e6f5`), message 13
    let stock = w.air_stock[0];
    assert!(ids(&w, &[13]).is_empty());
    for s in w.structures.iter_mut().filter(|s| s.owner == opensow_formats::level::Owner::Player && s.kind_id == kinds::MEDIUM_FACTORY) {
        s.slots.iter_mut().take(2).for_each(|sl| sl.level = sl.max_level);
    }
    steps(&mut w, 125);
    assert_eq!((w.air_stock[0][4] - stock[4], w.air_stock[0][2] - stock[2]), (8, 3), "mission 1 reward");
    assert_eq!(ids(&w, &[13]), vec![13]);
    steps(&mut w, 300);
    assert_eq!(w.air_stock[0][4] - stock[4], 8, "paid only once");
    let mut w = start("M04", None);
    steps(&mut w, 1500);
    assert_eq!(player(&w, 41), 2, "mission 4 airlift");

    // missions 7 and 17: 5000 credits (17: and 5000 research) and two carryalls with squads 11 / 10, one second after the
    // clock shows 0:00 (`0x42ebd0` looks at the clock, then counts a second off); messages 13, 11 (and 12)
    for (m, squad, research, announced) in [("M07", 11, 0, vec![13, 11]), ("M17", 10, 5000, vec![13, 11, 12])] {
        let mut w = start(m, Some(150));
        steps(&mut w, 150 * 60 - 1);
        assert!(w.aircraft.is_empty() && ids(&w, &[13]).is_empty(), "{m} too early");
        let (credits, rp) = (w.credits[0], w.research[0]);
        steps(&mut w, 62);
        assert_eq!(w.aircraft.iter().map(|a| a.squad).collect::<Vec<_>>(), vec![squad, squad], "{m} carryalls");
        assert!(w.credits[0] - credits >= 5000, "{m} credits");
        assert!(w.research[0] - rp >= research, "{m} research");
        assert_eq!(ids(&w, &[11, 12, 13]), announced, "{m} messages");
        // the 2-minutes message sits at the clock value 2:01
        if let Some(&(t, _)) = w.log.iter().find(|&&(_, id)| id == 17) {
            assert!((t as i64 - 29 * 60).abs() <= 61, "{m} 2 minutes message at tick {t}");
        }
        assert_eq!(w.outcome, None, "{m}: mode 2 ends nothing");
    }
    // mission 12: 10000 credits, 2500 research, 6 asteroids, 3 bombers, 3 carryalls (squad 16), message 13 only
    let mut w = start("M12", Some(150));
    steps(&mut w, 150 * 60 - 1);
    let (credits, rp, stock) = (w.credits[0], w.research[0], w.air_stock[0]);
    steps(&mut w, 62);
    assert_eq!(w.aircraft.iter().map(|a| a.squad).collect::<Vec<_>>(), vec![16, 16, 16]);
    assert!(w.credits[0] - credits >= 10_000 && w.research[0] - rp >= 2500);
    assert_eq!((w.air_stock[0][0] - stock[0], w.air_stock[0][1] - stock[1]), (3, 6));
    assert_eq!(ids(&w, &[11, 12, 13]), vec![13]);

    // the timed defeats (mode 1): M15, M16, M21, M22 are lost when the clock has run out, and not before
    for m in ["M15", "M16", "M21", "M22"] {
        let mut w = start(m, Some(60));
        steps(&mut w, 60 * 60 - 1);
        assert_eq!(w.outcome, None, "{m} must still run");
        steps(&mut w, 125);
        assert_eq!(w.outcome, Some(sim::Outcome::Defeat), "{m} timer defeat");
    }
}

/// The dev tools overlay: F12 shows it, its buttons set the cheats and the speed, a unit button plus a map click places a unit,
/// UNLOCK ALL opens every land of the campaign map without touching the real progress.
#[test]
fn dev_tools_panel_flow() {
    use devtools::Item;
    let Some(mut g) = game() else { return };
    let (w, h) = (1024, 768);
    g.start("M01");
    g.update(0.0, w, h);
    let mut buf = vec![0u32; w * h];
    g.draw(&mut buf, w, h);
    assert!(!g.dev.visible);
    g.key(&Key::Named(NamedKey::F12), true, w, h);
    assert!(g.dev.visible);
    let fonts = g.assets.font("small");
    let click = |g: &mut Game, item: Item| {
        let r = g.dev.rect_of(&devtools::Fonts::clone(&fonts), item, w, h).expect("item");
        let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
        g.mouse_moved(x, y, w, h);
        g.mouse_button(MouseButton::Left, true, x, y, w, h);
        g.mouse_button(MouseButton::Left, false, x, y, w, h);
    };
    click(&mut g, Item::Rich);
    click(&mut g, Item::Invulnerable);
    click(&mut g, Item::Freeze);
    click(&mut g, Item::Speed(10));
    g.update(1.0 / 60.0, w, h);
    let p = g.play.as_ref().unwrap();
    assert_eq!(p.world.cheats, sim::Cheats { invulnerable: true, rich: true, freeze_enemy: true });
    assert_eq!(p.world.credits[0], opensow_formats::production::CREDITS_CAP);
    assert!(p.world.tick >= 8, "10x runs several ticks in one frame (ran {})", p.world.tick);
    // an air force button adds one to the stock of the chosen side
    let stock = g.play.as_ref().unwrap().world.air_stock;
    click(&mut g, Item::Air(4));
    assert_eq!(g.play.as_ref().unwrap().world.air_stock[0][4], stock[0][4] + 1);
    // a unit button arms the placement, a click on open ground puts the unit there (and not on a panel click)
    let before = g.play.as_ref().unwrap().world.units.len();
    click(&mut g, Item::Side(1));
    click(&mut g, Item::Kind(7));
    assert_eq!(g.play.as_ref().unwrap().dev_spawn, Some((7, 1)));
    assert_eq!(g.play.as_ref().unwrap().world.units.len(), before, "pressing the button places nothing");
    let panel_right = g.dev.rect_of(&fonts, Item::Title, w, h).unwrap().x + 400;
    let (x, y) = (panel_right, h as i32 / 2);
    g.mouse_moved(x, y, w, h);
    g.mouse_button(MouseButton::Left, true, x, y, w, h);
    g.mouse_button(MouseButton::Left, false, x, y, w, h);
    let p = g.play.as_ref().unwrap();
    assert!(p.world.units.len() > before || p.world.is_blocked(p.to_map(x, y).0 / 32, p.to_map(x, y).1 / 32), "a unit was placed");
    // Esc stops the placement before it opens the pause menu
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert_eq!(g.play.as_ref().unwrap().dev_spawn, None);
    assert!(g.screen.is_none(), "Esc did not pause");
    // the campaign map: locked lands open with the switch and close again without it
    g.dev.visible = false;
    g.play = None;
    g.open_lands(false);
    assert_eq!(g.lands.as_ref().unwrap().progress.states[3], 0);
    g.dev.unlock_all = true;
    g.dev_event(Item::UnlockAll);
    assert_eq!(g.lands.as_ref().unwrap().progress.states[3], 2);
    assert_eq!(g.progress.states[3], 0, "the saved progress is untouched");
    g.dev.unlock_all = false;
    g.dev_event(Item::UnlockAll);
    assert_eq!(g.lands.as_ref().unwrap().progress.states[3], 0);
}

#[test]
fn playing_a_mission() {
    let Some(mut g) = game() else { return };
    let (w, h) = (1024, 768);
    g.start("M01");
    assert!(g.play.is_some() && g.screen.is_none());
    // the mission starts with the Advancer only (the squad arrives by carryall): give the test four units beside it
    {
        let world = &mut g.play.as_mut().unwrap().world;
        let (x, y) = world.player_units().find(|u| u.kind == kinds::ADVANCER).map(|u| u.px()).unwrap();
        for _ in 0..4 {
            world.add_unit_near(7, 0, x, y + 48);
        }
    }
    g.update(0.0, w, h);
    let mut buf = vec![0u32; w * h];
    g.draw(&mut buf, w, h);
    // box-select everything on screen, then order a move
    g.mouse_button(MouseButton::Left, true, 80, 40, w, h);
    g.mouse_button(MouseButton::Left, false, w as i32 - 2, h as i32 - 30, w, h);
    let selected = g.play.as_ref().unwrap().world.units.iter().filter(|u| u.selected).count();
    assert!(selected >= 4, "box selection picked {selected}");
    g.mouse_moved(w as i32 / 2, h as i32 / 2, w, h);
    // the left button commands: with a selection the pointer over open ground is the move pointer (water is "blocked")
    let (mut gx, mut gy) = (0, 0);
    'scan: for y in (100..h as i32 - 100).step_by(20) {
        for x in (100..w as i32 - 100).step_by(20) {
            g.mouse_moved(x, y, w, h);
            if g.play.as_ref().unwrap().pointer() == opensow_formats::cursor::Pointer::Move {
                (gx, gy) = (x, y);
                break 'scan;
            }
        }
    }
    assert!(gx > 0, "no open ground on screen");
    g.mouse_button(MouseButton::Left, true, gx, gy, w, h);
    g.mouse_button(MouseButton::Left, false, gx, gy, w, h);
    assert!(g.play.as_ref().unwrap().world.units.iter().any(|u| u.selected && !u.path.is_empty()));
    // a right click deselects, it gives no order
    g.mouse_button(MouseButton::Right, true, w as i32 / 2, h as i32 / 2 - 100, w, h);
    g.mouse_button(MouseButton::Right, false, w as i32 / 2, h as i32 / 2 - 100, w, h);
    assert!(!g.play.as_ref().unwrap().world.units.iter().any(|u| u.selected));
    for _ in 0..30 {
        g.update(0.05, w, h);
    }
    g.draw(&mut buf, w, h);
    // pause menu
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert_eq!(g.screen, Some(Screen::Pause));
    g.draw(&mut buf, w, h);
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert!(g.screen.is_none());
    g.key(&Key::Character(SmolStr::new("q")), true, w, h);
    g.act(Action::ToMain);
    assert_eq!(g.screen, Some(Screen::Main));
    assert!(g.play.is_none());
}

#[test]
fn swapped_mouse_buttons_trade_places_in_a_mission() {
    let Some(mut g) = game() else { return };
    let (w, h) = (1024, 768);
    g.start("M01");
    {
        let world = &mut g.play.as_mut().unwrap().world;
        let (x, y) = world.player_units().find(|u| u.kind == kinds::ADVANCER).map(|u| u.px()).unwrap();
        for _ in 0..4 {
            world.add_unit_near(7, 0, x, y + 48);
        }
    }
    g.settings.swap_buttons = true;
    g.update(0.0, w, h);
    let selected = |g: &Game| g.play.as_ref().unwrap().world.units.iter().filter(|u| u.selected).count();
    let moving = |g: &Game| g.play.as_ref().unwrap().world.units.iter().any(|u| u.selected && !u.path.is_empty());
    // selecting stays on the left button
    g.mouse_button(MouseButton::Left, true, 80, 40, w, h);
    g.mouse_button(MouseButton::Left, false, w as i32 - 2, h as i32 - 30, w, h);
    assert!(selected(&g) >= 4, "the left button box-selects");
    let (mut gx, mut gy) = (0, 0);
    'scan: for y in (100..h as i32 - 100).step_by(20) {
        for x in (100..w as i32 - 100).step_by(20) {
            g.mouse_moved(x, y, w, h);
            if g.play.as_ref().unwrap().pointer() == opensow_formats::cursor::Pointer::Move {
                (gx, gy) = (x, y);
                break 'scan;
            }
        }
    }
    assert!(gx > 0, "no open ground on screen");
    // the right click gives the move order and keeps the selection
    g.mouse_button(MouseButton::Right, true, gx, gy, w, h);
    g.mouse_button(MouseButton::Right, false, gx, gy, w, h);
    assert!(moving(&g), "the right click orders the move");
    assert!(selected(&g) >= 4);
    // the left click where an order would go does the original right click: it deselects
    g.mouse_moved(gx, gy, w, h);
    g.mouse_button(MouseButton::Left, true, gx, gy, w, h);
    g.mouse_button(MouseButton::Left, false, gx, gy, w, h);
    assert_eq!(selected(&g), 0, "the left click deselects");
}

#[test]
fn multiplayer_button_opens_the_network_screens() {
    let Some(mut g) = game() else { return };
    let (w, h) = (1280, 800);
    let b = g.buttons(w, h)[3].rect;
    g.mouse_button(MouseButton::Left, true, b.x + 5, b.y + 5, w, h);
    g.mouse_button(MouseButton::Left, false, b.x + 5, b.y + 5, w, h);
    assert!(g.net_menu.is_some() && g.screen.is_none());
    assert_eq!(g.net_menu.as_ref().unwrap().page, multiplayer::Page::Sessions);
    let mut buf = vec![0u32; w * h];
    g.draw(&mut buf, w, h);
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert!(g.net_menu.is_none());
    assert_eq!(g.screen, Some(Screen::Main));
}

#[test]
fn options_menu_flow() {
    let Some(mut g) = game() else { return };
    let (w, h) = (1280, 800);
    g.screen = Some(Screen::Pause);
    g.act(Action::Options);
    assert!(g.options_menu.is_some() && g.screen.is_none());
    let click = |g: &mut Game, hit: options::Hit| {
        let c = g.options_menu.as_ref().unwrap().centre(hit);
        let (s, ox, oy) = (h as f32 / 480.0, (w as f32 - 640.0 * h as f32 / 480.0) / 2.0, 0.0);
        let (x, y) = ((c.0 as f32 * s + ox) as i32, (c.1 as f32 * s + oy) as i32);
        g.mouse_moved(x, y, w, h);
        g.mouse_button(MouseButton::Left, true, x, y, w, h);
    };
    g.sfx.played.clear();
    click(&mut g, options::Hit::Arrow(3));
    click(&mut g, options::Hit::Arrow(3));
    click(&mut g, options::Hit::Arrow(2));
    assert_eq!(g.options_menu.as_ref().unwrap().opts.fx_volume, 95);
    assert_eq!(g.sfx.fx_volume, 95, "the volume reaches the player at once");
    click(&mut g, options::Hit::Label(3));
    assert!(!g.sfx.fx_on);
    assert!(g.sfx.played.contains(&33) && g.sfx.played.contains(&34) && g.sfx.played.contains(&30));
    click(&mut g, options::Hit::Ok);
    assert!(g.options_menu.is_none());
    assert_eq!(g.screen, Some(Screen::Pause), "back to the pause menu");
    assert_eq!(g.options.fx_volume, 95);
    assert!(g.sfx.played.contains(&38));
    // Escape cancels
    g.act(Action::Options);
    click(&mut g, options::Hit::Label(0));
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert!(g.options_menu.is_none() && g.options.cfg.flag_a);
    let mut buf = vec![0u32; w * h];
    g.draw(&mut buf, w, h);
}

#[test]
fn options_page_two_toggles_the_enhancement() {
    let Some(mut g) = game() else { return };
    let (w, h) = (1280, 800);
    g.screen = Some(Screen::Pause);
    g.act(Action::Options);
    let click = |g: &mut Game, hit: options::Hit| {
        let c = g.options_menu.as_ref().unwrap().centre(hit);
        let (s, ox) = (h as f32 / 480.0, (w as f32 - 640.0 * h as f32 / 480.0) / 2.0);
        let (x, y) = ((c.0 as f32 * s + ox) as i32, (c.1 as f32 * s) as i32);
        g.mouse_moved(x, y, w, h);
        g.mouse_button(MouseButton::Left, true, x, y, w, h);
    };
    click(&mut g, options::Hit::Page(1));
    assert_eq!(g.options_menu.as_ref().unwrap().page, 1);
    let mut buf = vec![0u32; w * h];
    g.draw(&mut buf, w, h);
    click(&mut g, options::Hit::Extra(0));
    assert!(!g.options_menu.as_ref().unwrap().opts.extra.enhance);
    click(&mut g, options::Hit::Ok);
    assert!(!g.options.extra.enhance);
    assert!(!opensow_viewer::upscale::enabled(), "OK applies the switch");
    // back on, so the other tests are not affected
    g.act(Action::Options);
    click(&mut g, options::Hit::Page(1));
    click(&mut g, options::Hit::Extra(0));
    click(&mut g, options::Hit::Ok);
    assert!(g.options.extra.enhance && opensow_viewer::upscale::enabled());
    // the sound row (the last one) switches the sound enhancement the same way
    g.act(Action::Options);
    click(&mut g, options::Hit::Page(1));
    click(&mut g, options::Hit::Extra(5));
    click(&mut g, options::Hit::Ok);
    assert!(!g.options.extra.sound && !hifi::enabled(), "OK applies the sound switch");
    g.act(Action::Options);
    click(&mut g, options::Hit::Page(1));
    click(&mut g, options::Hit::Extra(5));
    click(&mut g, options::Hit::Ok);
    assert!(g.options.extra.sound && hifi::enabled());
    // the turret range row (on by default) reaches the play settings on O.K.
    assert!(g.settings.turret_range);
    g.act(Action::Options);
    click(&mut g, options::Hit::Page(1));
    click(&mut g, options::Hit::Extra(6));
    click(&mut g, options::Hit::Ok);
    assert!(!g.options.extra.turret_range && !g.settings.turret_range, "OK applies the turret range switch");
    // the map zoom row (the last one, on by default) too
    assert!(g.settings.zoom);
    g.act(Action::Options);
    click(&mut g, options::Hit::Page(1));
    click(&mut g, options::Hit::ExtraArrow(7, true));
    click(&mut g, options::Hit::Ok);
    assert!(!g.options.extra.zoom && !g.settings.zoom, "OK applies the map zoom switch");
    // the mouse button swap (the last row, off by default)
    assert!(!g.settings.swap_buttons);
    g.act(Action::Options);
    click(&mut g, options::Hit::Page(1));
    click(&mut g, options::Hit::Extra(8));
    click(&mut g, options::Hit::Ok);
    assert!(g.options.extra.swap_buttons && g.settings.swap_buttons, "OK applies the mouse button swap");
}

#[test]
fn pause_menu_flow() {
    let Some(mut g) = game() else { return };
    let (w, h) = (1280, 800);
    g.start("M01");
    g.sfx.played.clear();
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert_eq!(g.screen, Some(Screen::Pause));
    assert_eq!(g.sfx.played, vec![35], "opening plays ingame-come");
    let click = |g: &mut Game, hit: ingame::Hit| {
        let (x, y) = g.pause_menu().centre(hit, w, h);
        g.mouse_moved(x, y, w, h);
        g.mouse_button(MouseButton::Left, true, x, y, w, h);
    };
    g.sfx.played.clear();
    click(&mut g, ingame::Hit::Item(3));
    assert!(g.options_menu.is_some());
    assert_eq!(g.sfx.played, vec![36, 37], "hover then select");
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert!(g.options_menu.is_none());
    assert_eq!(g.screen, Some(Screen::Pause), "options return to the in-game menu");
    // restart asks first; the X goes back
    click(&mut g, ingame::Hit::Item(4));
    assert_eq!(g.pause_menu().page, ingame::Page::ConfirmRestart);
    click(&mut g, ingame::Hit::No);
    assert_eq!(g.pause_menu().page, ingame::Page::Items);
    // load and save open their pages; CANCEL resumes the mission (state 7), not the oval
    for item in [1, 2] {
        click(&mut g, ingame::Hit::Item(item));
        assert!(g.saveui.is_some() && g.screen.is_none(), "item {item}");
        let c = g.saveui.as_ref().unwrap().centre(saveui::Hit::Cancel);
        g.mouse_button(MouseButton::Left, true, c.0, c.1, 640, 480);
        assert!(g.saveui.is_none() && g.screen.is_none(), "item {item} resumes");
        g.open_pause();
    }
    // main menu with the check mark
    click(&mut g, ingame::Hit::Item(5));
    g.sfx.played.clear();
    click(&mut g, ingame::Hit::Yes);
    assert_eq!(g.screen, Some(Screen::Main));
    assert!(g.play.is_none() && g.ingame.is_none());
    assert!(g.sfx.played.contains(&38));
}

#[test]
fn enter_opens_the_chat_line_and_cheat_phrases_take_effect() {
    let Some(mut g) = game() else { return };
    let (w, h) = (1024, 768);
    g.start("M01");
    g.update(0.0, w, h);
    let chr = |c: &str| Key::Character(SmolStr::new(c));
    let say = |g: &mut Game, text: &str| {
        g.key(&Key::Named(NamedKey::Enter), true, w, h);
        assert!(g.play.as_ref().unwrap().chat.is_some(), "Enter opens the line");
        for c in text.chars() {
            if c == ' ' {
                g.key(&Key::Named(NamedKey::Space), true, w, h);
            } else {
                g.key(&chr(&c.to_string()), true, w, h);
            }
        }
        g.key(&Key::Named(NamedKey::Enter), true, w, h);
        assert!(g.play.as_ref().unwrap().chat.is_none(), "Enter sends and closes");
    };
    let before = {
        let wd = &g.play.as_ref().unwrap().world;
        (wd.credits[0], wd.research[0], wd.air_stock[0])
    };
    // keys typed into the line do not reach the game: "2" would arm a turret, "p" would pause
    g.play.as_mut().unwrap().world.turret_flags[0] = [true; 5];
    g.play.as_mut().unwrap().world.credits[0] = 5000;
    g.key(&Key::Named(NamedKey::Enter), true, w, h);
    g.key(&chr("2"), true, w, h);
    g.key(&chr("p"), true, w, h);
    assert_eq!(g.play.as_ref().unwrap().chat.as_deref(), Some("2p"));
    assert!(g.play.as_ref().unwrap().placing.is_none() && !g.play.as_ref().unwrap().paused);
    g.key(&Key::Named(NamedKey::Backspace), true, w, h);
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert!(g.play.as_ref().unwrap().chat.is_none() && g.screen.is_none(), "Esc closes the line, not the game");
    g.play.as_mut().unwrap().world.credits[0] = before.0;
    say(&mut g, "give me money");
    say(&mut g, "speed up my research");
    say(&mut g, "give me fighter");
    say(&mut g, "give me bomber");
    say(&mut g, "give me meteor storm");
    say(&mut g, "give me carryall");
    say(&mut g, "give me tripler");
    let wd = &g.play.as_ref().unwrap().world;
    assert_eq!((wd.credits[0] - before.0, wd.research[0] - before.1), (1000, 200));
    assert!((0..5).all(|i| wd.air_stock[0][i] == before.2[i] + 1), "one of every force");
    assert_eq!(wd.chat.len(), 7, "every cheat is shown as a chat line too");
    assert_eq!(wd.chat.last().map(|c| c.1.as_str()), Some("give me tripler"));
    // a wrong case is only a chat line; Tab brings the last line back
    say(&mut g, "Give me money");
    let wd = &g.play.as_ref().unwrap().world;
    assert_eq!(wd.credits[0] - before.0, 1000);
    assert_eq!(wd.chat.last().map(|c| c.1.as_str()), Some("Give me money"));
    g.key(&Key::Named(NamedKey::Enter), true, w, h);
    g.key(&Key::Named(NamedKey::Tab), true, w, h);
    assert_eq!(g.play.as_ref().unwrap().chat.as_deref(), Some("Give me money"));
    g.key(&Key::Named(NamedKey::Enter), true, w, h);
    // the mission 1 clock does not run in a mission without a countdown; "one minute" adds to a running one only
    g.play.as_mut().unwrap().world.timer = Some((3, 10, 0));
    say(&mut g, "give me one minute");
    assert_eq!(g.play.as_ref().unwrap().world.timer, Some((4, 10, 0)));
    g.play.as_mut().unwrap().world.timer_expired = true;
    say(&mut g, "give me one minute");
    assert_eq!(g.play.as_ref().unwrap().world.timer, Some((4, 10, 0)), "not after the countdown ended");
    say(&mut g, "draw fps");
    assert!(g.play.as_ref().unwrap().show_fps);
}

/// The Warmonger campaign takes its exe's phrases (docs/chat.md): its own cheat and "bring me ..." work, the base game's do not.
#[test]
fn the_warmonger_campaign_takes_its_own_cheat_phrases() {
    use opensow_formats::Edition;
    let Some(mut g) = game() else { return };
    if g.assets.game.warmonger.is_none() {
        return;
    }
    let (w, h) = (1024, 768);
    g.set_edition(Edition::Warmonger);
    g.start("M01");
    g.update(0.0, w, h);
    let say = |g: &mut Game, text: &str| {
        g.key(&Key::Named(NamedKey::Enter), true, w, h);
        for c in text.chars() {
            match c {
                ' ' => g.key(&Key::Named(NamedKey::Space), true, w, h),
                _ => g.key(&Key::Character(SmolStr::new(c.to_string())), true, w, h),
            };
        }
        g.key(&Key::Named(NamedKey::Enter), true, w, h);
    };
    let (credits, stock) = {
        let wd = &g.play.as_ref().unwrap().world;
        (wd.credits[0], wd.air_stock[0])
    };
    say(&mut g, "give me money");
    let wd = &g.play.as_ref().unwrap().world;
    assert_eq!(wd.credits[0], credits, "the base game's phrase is only a chat line here");
    assert_eq!(wd.chat.last().map(|c| c.1.as_str()), Some("give me money"));
    say(&mut g, "bring me money");
    say(&mut g, "rahman is son of a bitch");
    let wd = &g.play.as_ref().unwrap().world;
    assert_eq!(wd.credits[0] - credits, 1000 + 2222);
    assert!((0..5).all(|i| wd.air_stock[0][i] == stock[i] + 1));
    assert_eq!(wd.chat.last().map(|c| c.1.as_str()), Some("Yeah! Let's Finish him!"), "the reply is shown, not the phrase");
    // Tab (not in Warmonger's exe) brings back what was typed
    g.key(&Key::Named(NamedKey::Enter), true, w, h);
    g.key(&Key::Named(NamedKey::Tab), true, w, h);
    assert_eq!(g.play.as_ref().unwrap().chat.as_deref(), Some("rahman is son of a bitch"));
}

#[test]
fn mission_hotkeys_follow_the_exes_key_handler() {
    let Some(mut g) = game() else { return };
    let (w, h) = (1024, 768);
    g.start("M01");
    g.update(0.0, w, h);
    let chr = |c: &str| Key::Character(SmolStr::new(c));
    // 1-5 (`0x415d7c`): drop the selection and arm the turret, only with the turret available and 200 * tier credits
    {
        let p = g.play.as_mut().unwrap();
        p.world.turret_flags[0] = [true; 5];
        p.world.credits[0] = 399;
        p.world.select_all();
    }
    g.key(&chr("2"), true, w, h);
    assert_eq!(g.play.as_ref().unwrap().placing, None, "tier 2 costs 400");
    g.play.as_mut().unwrap().world.credits[0] = 400;
    g.key(&chr("2"), true, w, h);
    let p = g.play.as_ref().unwrap();
    assert_eq!(p.placing, Some(2));
    assert!(!p.world.units.iter().any(|u| u.selected), "the key deselects everything");
    // the physical key counts, not the character: Czech layouts make the number row "+ ě š č ř" without shift
    g.play.as_mut().unwrap().placing = None;
    g.digit = Some(3);
    g.key(&chr("š"), true, w, h);
    g.digit = None;
    assert_eq!(g.play.as_ref().unwrap().placing, None, "tier 3 costs 600");
    g.play.as_mut().unwrap().world.credits[0] = 600;
    g.digit = Some(3);
    g.key(&chr("š"), true, w, h);
    g.digit = None;
    assert_eq!(g.play.as_ref().unwrap().placing, Some(3));
    // building one plays sound 72 (turret-place), the player's own: a priority sound, not a positional one
    {
        let wd = &mut g.play.as_mut().unwrap().world;
        let own = wd.structures.iter().find(|s| s.owner == opensow_formats::level::Owner::Player).map(|s| s.cell).unwrap();
        wd.sounds.clear();
        let placed = (-8..8).flat_map(|dy| (-8..8).map(move |dx| (own.0 + dx, own.1 + dy))).any(|c| wd.place_turret(0, 1, c));
        assert!(placed && wd.player_sounds.contains(&72) && !wd.sounds.iter().any(|s| s.0 == 72), "turret-place sound");
    }
    // placement tiles: one verdict per footprint cell (tier 5 is a 1x2 turret), all refused far from the own buildings
    assert_eq!(g.play.as_ref().unwrap().world.turret_cells(0, 5, (3, 3)).len(), 2);
    assert!(g.play.as_ref().unwrap().world.turret_cells(0, 3, (0, 0)).iter().all(|&ok| !ok));
    g.play.as_mut().unwrap().world.credits[0] = 400;
    g.play.as_mut().unwrap().placing = Some(2);
    // the gun's reach is tinted under the pointer while placing, unless Options page 2 "Turret range" is off
    g.mouse_moved(w as i32 / 2, h as i32 / 3, w, h);
    let shot = |g: &mut Game| {
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        buf
    };
    let ring = shot(&mut g);
    g.play.as_mut().unwrap().turret_range = false;
    let plain = shot(&mut g);
    assert_ne!(ring, plain, "the range ring shows while placing");
    g.settings.turret_range = false;
    g.play.as_mut().unwrap().update(0.0, &g.settings.clone(), w, h);
    assert!(!g.play.as_ref().unwrap().turret_range, "the play screen follows the setting");
    g.settings.turret_range = true;
    g.play.as_mut().unwrap().turret_range = true;
    // P pauses (scan code 0x19) and only P resumes; Space does neither
    g.key(&chr("p"), true, w, h);
    assert!(g.play.as_ref().unwrap().paused);
    g.key(&Key::Named(NamedKey::Space), true, w, h);
    assert!(g.play.as_ref().unwrap().paused);
    g.key(&chr("p"), true, w, h);
    assert!(!g.play.as_ref().unwrap().paused);
    // rebound keys (Options, Controls pages): pause on K, P is free then; the window's physical key wins over the logical one
    let keys = g.options.keys;
    g.options.keys.bind(keybinds::Act::Pause, 0, winit::keyboard::KeyCode::KeyK);
    g.key(&chr("p"), true, w, h);
    assert!(!g.play.as_ref().unwrap().paused, "P no longer pauses");
    g.key(&chr("k"), true, w, h);
    assert!(g.play.as_ref().unwrap().paused, "K pauses");
    g.code = Some(winit::keyboard::KeyCode::KeyK);
    g.key(&chr("x"), true, w, h);
    g.code = None;
    assert!(!g.play.as_ref().unwrap().paused, "the physical K resumes whatever the layout types");
    // a bound middle mouse button pauses and resumes too; the left / right buttons stay the original's
    g.options.keys.bind(keybinds::Act::Pause, 1, MouseButton::Middle);
    g.mouse_button(MouseButton::Middle, true, w as i32 / 2, h as i32 / 2, w, h);
    g.mouse_button(MouseButton::Middle, false, w as i32 / 2, h as i32 / 2, w, h);
    assert!(g.play.as_ref().unwrap().paused, "the middle button pauses");
    g.mouse_button(MouseButton::Middle, true, w as i32 / 2, h as i32 / 2, w, h);
    assert!(!g.play.as_ref().unwrap().paused, "and resumes");
    g.options.keys = keys;
    // Tab selects one live own unit at a time and walks the list; H the Advancers
    g.key(&Key::Named(NamedKey::Tab), true, w, h);
    let sel = |g: &Game| g.play.as_ref().unwrap().world.units.iter().enumerate().filter(|(_, u)| u.selected).map(|(i, _)| i).collect::<Vec<_>>();
    let first = sel(&g);
    assert_eq!(first.len(), 1);
    let own = g.play.as_ref().unwrap().world.units.iter().filter(|u| u.player == 0 && u.hp > 0).count();
    g.key(&Key::Named(NamedKey::Tab), true, w, h);
    assert_eq!(sel(&g) != first, own > 1, "{own} own units");
    // Space: nothing without a shown message; with one, the view goes to the last event position
    g.play.as_mut().unwrap().look_at(100.0, 100.0);
    let before = g.play.as_ref().unwrap().view;
    g.key(&Key::Named(NamedKey::Space), true, w, h);
    assert_eq!((g.play.as_ref().unwrap().view.x, g.play.as_ref().unwrap().view.y), (before.x, before.y));
    {
        let wd = &mut g.play.as_mut().unwrap().world;
        wd.last_event = Some((1200, 1000));
        wd.log.push((wd.tick, 3));
    }
    g.key(&Key::Named(NamedKey::Space), true, w, h);
    let v = g.play.as_ref().unwrap().view;
    assert!((v.x + w as f32 / v.zoom / 2.0 - 1200.0).abs() < 2.0 && (v.y + h as f32 / v.zoom / 2.0 - 1000.0).abs() < 2.0, "{:?}", (v.x, v.y));
    // numpad bookmarks: shift sets, plain jumps
    g.numpad = true;
    g.shift = true;
    g.play.as_mut().unwrap().look_at(500.0, 400.0);
    g.key(&chr("3"), true, w, h);
    g.shift = false;
    g.play.as_mut().unwrap().look_at(900.0, 900.0);
    g.key(&chr("3"), true, w, h);
    let v = g.play.as_ref().unwrap().view;
    assert!((v.x + w as f32 / v.zoom / 2.0 - 500.0).abs() < 2.0 || v.x == 0.0 || v.y == 0.0, "the bookmark brings the view back");
    assert_eq!(g.play.as_ref().unwrap().placing, Some(2), "numpad digits are not turret keys");
    // numpad * selects what is on screen, F3 / F4 move the game speed in halves between 0.5 and 3
    g.key(&chr("*"), true, w, h);
    g.numpad = false;
    for _ in 0..6 {
        g.key(&Key::Named(NamedKey::F4), true, w, h);
    }
    assert_eq!(g.game_speed, 3.0);
    for _ in 0..9 {
        g.key(&Key::Named(NamedKey::F3), true, w, h);
    }
    assert_eq!(g.game_speed, 0.5);
    // every press shows the "speed  N.Nx" line (string 144 + "  %.1fx", `0x4031d9`), even at the limit
    let notes: Vec<String> = g.play.as_ref().unwrap().notes.iter().map(|n| n.1.clone()).collect();
    assert_eq!(notes.len(), 15);
    assert!(notes[0].ends_with("  1.5x") && notes[5].ends_with("  3.0x") && notes[14].ends_with("  0.5x"), "{notes:?}");
    // F1 (`0x415bda`) toggles the key help overlay (`[0x5baa9b]`), not the "draw fps" counter
    let (keys, fps) = (g.play.as_ref().unwrap().show_keys, g.play.as_ref().unwrap().show_fps);
    g.key(&Key::Named(NamedKey::F1), true, w, h);
    assert_eq!((g.play.as_ref().unwrap().show_keys, g.play.as_ref().unwrap().show_fps), (!keys, fps));
    // X scatters the selection: every selected unit gets a go-to
    {
        let p = g.play.as_mut().unwrap();
        p.world.clear_selection();
        let (x, y) = (p.view.x as i32 + 300, p.view.y as i32 + 300);
        let ids: Vec<u32> = (0..3).filter_map(|k| p.world.add_unit_near(7, 0, x + 40 * k, y)).collect();
        assert!(ids.len() >= 2, "units to scatter");
        for u in p.world.units.iter_mut().filter(|u| ids.contains(&u.id)) {
            u.selected = true;
        }
    }
    g.key(&chr("x"), true, w, h);
    assert!(g.play.as_ref().unwrap().world.units.iter().filter(|u| u.selected).all(|u| !u.is_idle()), "all of them move");
}

#[test]
fn shift_h_finds_idle_advancers_and_a_double_click_selects_the_kind_on_screen() {
    let Some(mut g) = game() else { return };
    let chr = |c: &str| Key::Character(SmolStr::new(c));
    let (w, h) = (1024, 768);
    g.start("M01");
    g.update(0.0, w, h);
    let busy = {
        let p = g.play.as_mut().unwrap();
        let a = p.world.player_units().find(|u| u.kind == kinds::ADVANCER).map(|u| (u.id, u.x as i32, u.y as i32)).expect("an Advancer");
        // a second Advancer that is on its way somewhere is not idle
        let b = p.world.add_unit(kinds::ADVANCER, 0, a.1 + 64, a.2);
        p.world.order_move(b, (a.1 / 32 + 6, a.2 / 32), None);
        p.look_at(0.0, 0.0);
        (a.0, b)
    };
    g.shift = true;
    for _ in 0..3 {
        g.key(&chr("H"), true, w, h);
        let p = g.play.as_ref().unwrap();
        let sel: Vec<u32> = p.world.units.iter().filter(|u| u.selected).map(|u| u.id).collect();
        assert_eq!(sel, vec![busy.0], "only the idle one");
        let u = p.world.unit(busy.0).unwrap();
        let v = p.view;
        assert!(u.x > v.x && u.x < v.x + w as f32 / v.zoom && u.y > v.y && u.y < v.y + h as f32 / v.zoom, "the view goes to it");
    }
    g.shift = false;
    // a double click on a unit selects the units of its kind inside the view, the rest of the army is not
    let p = g.play.as_mut().unwrap();
    let a = p.world.unit(busy.0).map(|u| (u.x as i32, u.y as i32)).unwrap();
    let kind = 7;
    let first = p.world.add_unit_near(kind, 0, a.0 + 100, a.1 + 100).expect("room for a unit");
    let (x, y) = p.world.unit(first).map(|u| (u.x, u.y)).unwrap();
    let other = p.world.add_unit_near(kind, 0, x as i32 + 64, y as i32).expect("room for a second");
    let _different = p.world.add_unit_near(1, 0, x as i32, y as i32 + 64).expect("room for another kind");
    let far = p.world.add_unit_near(kind, 0, a.0 - 1000, a.1 - 1000).expect("room far away");
    p.view.zoom = 1.0;
    p.look_at(x, y);
    let (sx, sy) = p.view.to_screen(x, p.world.units.iter().find(|u| u.x == x && u.y == y).unwrap().sprite_y());
    let click = |g: &mut Game| {
        g.mouse_moved(sx, sy, w, h);
        g.mouse_button(MouseButton::Left, true, sx, sy, w, h);
        g.mouse_button(MouseButton::Left, false, sx, sy, w, h);
    };
    click(&mut g);
    let p = g.play.as_ref().unwrap();
    assert_eq!(p.world.units.iter().filter(|u| u.selected).map(|u| u.id).collect::<Vec<_>>(), vec![first], "one click selects one");
    click(&mut g);
    let p = g.play.as_ref().unwrap();
    let sel: Vec<u32> = p.world.units.iter().filter(|u| u.selected).map(|u| u.id).collect();
    assert!(sel.contains(&other) && !sel.contains(&far) && p.world.units.iter().filter(|u| u.selected).all(|u| u.kind == kind), "{sel:?}");
}

#[test]
fn hud_clicks_reach_the_panel_not_the_map() {
    let Some(mut g) = game() else { return };
    let (w, h) = (640, 480);
    g.start("M12");
    g.update(0.0, w, h);
    let mut buf = vec![0u32; w * h];
    g.draw(&mut buf, w, h);
    let l = g.play.as_ref().unwrap().hud_layout().unwrap().clone();
    // the radar moves the camera
    let before = (g.play.as_ref().unwrap().view.x, g.play.as_ref().unwrap().view.y);
    g.mouse_button(MouseButton::Left, true, l.radar.x + 2, l.radar.y + 2, w, h);
    g.mouse_button(MouseButton::Left, false, l.radar.x + 2, l.radar.y + 2, w, h);
    let after = (g.play.as_ref().unwrap().view.x, g.play.as_ref().unwrap().view.y);
    assert_ne!(before, after, "radar click did not scroll");
    // a right click on the radar toggles the black radar
    assert!(!g.play.as_ref().unwrap().radar_black());
    g.mouse_button(MouseButton::Right, true, l.radar.x + 2, l.radar.y + 2, w, h);
    g.mouse_button(MouseButton::Right, false, l.radar.x + 2, l.radar.y + 2, w, h);
    assert!(g.play.as_ref().unwrap().radar_black());
    // a turret icon arms the placement only with enough credits; a right click cancels
    g.play.as_mut().unwrap().world.credits[0] = 1000;
    let t = l.tabs[1].1; // tier 2 (tabs run from the right)
    g.mouse_button(MouseButton::Left, true, t.x + 3, t.y + 3, w, h);
    g.mouse_button(MouseButton::Left, false, t.x + 3, t.y + 3, w, h);
    assert_eq!(g.play.as_ref().unwrap().placing, Some(2));
    g.mouse_button(MouseButton::Right, true, t.x + 3, t.y + 3, w, h);
    g.mouse_button(MouseButton::Right, false, t.x + 3, t.y + 3, w, h);
    assert_eq!(g.play.as_ref().unwrap().placing, None);
    g.play.as_mut().unwrap().world.credits[0] = 0;
    g.mouse_button(MouseButton::Left, true, t.x + 3, t.y + 3, w, h);
    assert_eq!(g.play.as_ref().unwrap().placing, None);
}

#[test]
fn the_rally_button_arms_the_mode_and_a_map_click_sets_the_point() {
    let Some(mut g) = game() else { return };
    for (w, h) in [(640usize, 480usize), (1024, 768), (1920, 1080)] {
        g.start("M01");
        g.update(0.0, w, h);
        let own = g
            .play
            .as_ref()
            .unwrap()
            .world
            .structures
            .iter()
            .position(|s| s.owner == opensow_formats::level::Owner::Player && s.kind_id == kinds::MEDIUM_FACTORY)
            .unwrap();
        let p = g.play.as_mut().unwrap();
        let c = p.world.structures[own].cell;
        p.look_at(c.0 as f32 * 32.0 + 16.0, c.1 as f32 * 32.0 + 16.0);
        // no button before the factory is selected
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        assert!(g.play.as_ref().unwrap().hud_layout().unwrap().rally.is_none());
        g.play.as_mut().unwrap().sel_building = Some(own);
        g.update(0.0, w, h);
        g.draw(&mut buf, w, h);
        let l = g.play.as_ref().unwrap().hud_layout().unwrap().clone();
        let (r, ..) = l.rally.expect("the selected own factory shows the button");
        // it sits above the top slot icon
        assert!(r.y + r.h <= l.slots[0].0.y, "button above the icon at {w}x{h}");
        let (bx, by) = (r.x + r.w / 2, r.y + r.h / 2);
        assert_eq!(l.hit(bx, by), Some(crate::hud::Hit::Rally));
        g.mouse_button(MouseButton::Left, true, bx, by, w, h);
        g.mouse_button(MouseButton::Left, false, bx, by, w, h);
        assert_eq!(g.play.as_ref().unwrap().rallying, Some(own));
        // a click on free ground sets the point (the cell centre) and ends the mode
        let p = g.play.as_mut().unwrap();
        let target = (c.0 + 4, c.1 + 6);
        p.look_at(target.0 as f32 * 32.0 + 16.0, target.1 as f32 * 32.0 + 16.0);
        let (sx, sy) = (w as i32 / 2 - 100, h as i32 / 2);
        let (mx, my) = p.to_map(sx, sy);
        let free = !p.world.is_blocked(mx.div_euclid(32), my.div_euclid(32));
        g.mouse_moved(sx, sy, w, h);
        g.mouse_button(MouseButton::Left, true, sx, sy, w, h);
        g.mouse_button(MouseButton::Left, false, sx, sy, w, h);
        let p = g.play.as_ref().unwrap();
        if free {
            assert_eq!(p.rallying, None);
            assert_eq!(p.world.structures[own].rally, Some(opensow_formats::rally::snap(mx, my)));
        } else {
            assert_eq!(p.rallying, Some(own), "a blocked cell keeps the mode on");
        }
        g.play.as_mut().unwrap().world.structures[own].rally = None;
    }
}

#[test]
fn any_building_shows_its_slots_when_hovered_and_only_own_ones_open_the_info_box() {
    let Some(mut g) = game() else { return };
    let (w, h) = (1024, 768);
    g.start("M01");
    g.update(0.0, w, h);
    let p = g.play.as_mut().unwrap();
    // an enemy production building and the neutral research lab (slots 19, 20)
    let enemy = p.world.structures.iter().position(|s| s.owner == opensow_formats::level::Owner::Enemy && !s.slots.is_empty()).unwrap();
    let lab = p.world.structures.iter().position(|s| s.owner == opensow_formats::level::Owner::Neutral && !s.slots.is_empty()).unwrap();
    let own = p.world.structures.iter().position(|s| s.owner == opensow_formats::level::Owner::Player && s.kind_id == kinds::MEDIUM_FACTORY).unwrap();
    for target in [own, enemy, lab] {
        let p = g.play.as_mut().unwrap();
        let c = p.world.structures[target].cell;
        p.look_at(c.0 as f32 * 32.0 + 16.0, c.1 as f32 * 32.0 + 16.0);
        let mut at = None;
        'scan: for y in (80..h as i32 - 80).step_by(6) {
            for x in (120..w as i32 - 120).step_by(6) {
                if p.world.structure_at(p.to_map(x, y).0, p.to_map(x, y).1) == Some(target) {
                    at = Some((x, y));
                    break 'scan;
                }
            }
        }
        let (x, y) = at.expect("building on screen");
        // hovering shows the production slots of any building, but no info box
        g.mouse_moved(x, y, w, h);
        g.update(0.0, w, h);
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        let n = g.play.as_ref().unwrap().hud_layout().unwrap().slots.len();
        assert!(n > 0 && (target != own || n == 2), "M01's blue medium factory has two slots, got {n}");
        assert!(g.play.as_ref().unwrap().infobox.kind.is_none(), "hover alone opens no info box");
        // a click selects only an own building (info box); an enemy's or the neutral one stays hover-only
        g.mouse_button(MouseButton::Left, true, x, y, w, h);
        g.mouse_button(MouseButton::Left, false, x, y, w, h);
        assert_eq!(g.play.as_ref().unwrap().sel_building, (target == own).then_some(target));
        g.update(0.0, w, h);
        g.draw(&mut buf, w, h);
        assert!(!g.play.as_ref().unwrap().hud_layout().unwrap().slots.is_empty());
        if target == own {
            assert!(g.play.as_ref().unwrap().infobox.visible());
        } else {
            assert!(g.play.as_ref().unwrap().infobox.kind.is_none(), "only the selected own building gets the page");
        }
        g.mouse_button(MouseButton::Right, true, 5, 5, w, h);
        g.mouse_button(MouseButton::Right, false, 5, 5, w, h);
        g.play.as_mut().unwrap().sel_building = None;
    }
}

#[test]
fn info_box_slot_click_buys_a_tech_level() {
    let Some(mut g) = game() else { return };
    let (w, h) = (1280, 800);
    g.start("M12");
    g.update(0.0, w, h);
    let p = g.play.as_mut().unwrap();
    let Some(b) = p
        .world
        .structures
        .iter()
        .position(|s| s.owner == opensow_formats::level::Owner::Player && s.slots.first().is_some_and(|sl| sl.level < sl.max_level && s.active > 0))
    else {
        return;
    };
    p.sel_building = Some(b);
    p.world.research[0] = 5000;
    let mut buf = vec![0u32; w * h];
    g.draw(&mut buf, w, h);
    let l = g.play.as_ref().unwrap().hud_layout().unwrap().clone();
    let r = l.arrows[0].unwrap().0;
    let before = g.play.as_ref().unwrap().world.structures[b].slots[0].kind;
    g.mouse_button(MouseButton::Left, true, r.x + 3, r.y + 3, w, h);
    g.mouse_button(MouseButton::Left, false, r.x + 3, r.y + 3, w, h);
    assert_ne!(g.play.as_ref().unwrap().world.structures[b].slots[0].kind, before);
    assert!(g.play.as_ref().unwrap().world.research[0] < 5000);
}

#[test]
fn a_win_with_the_default_setup_earns_the_levels_star_and_a_changed_setup_does_not() {
    let Some(mut g) = game() else { return };
    g.mute = true;
    let (w, h) = (640, 480);
    let win = |g: &mut Game| {
        g.results = None; // the banner of the previous match
        g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
        g.update(0.0, w, h);
    };
    assert_eq!(g.stars.get("01-First Spark"), None);
    // a loss earns nothing
    g.headless_skirmish("01-First", 0, None, None, sim::Difficulty::Normal, None);
    g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Defeat);
    g.update(0.0, w, h);
    assert_eq!(g.stars.get("01-First Spark"), None);
    // a win with the map's own setup: silver for Normal
    g.headless_skirmish("01-First", 0, None, None, sim::Difficulty::Normal, None);
    win(&mut g);
    assert_eq!(g.stars.get("01-First Spark"), Some(sim::Difficulty::Normal));
    // changed credits (or research, air, turrets): no star, even on Brutal
    g.headless_skirmish("01-First", 0, Some((801, 800)), None, sim::Difficulty::Brutal, None);
    win(&mut g);
    assert_eq!(g.stars.get("01-First Spark"), Some(sim::Difficulty::Normal));
    // an easier win with the default setup never lowers it; the side is not part of the setup
    g.headless_skirmish("01-First", 1, None, None, sim::Difficulty::Easy, None);
    win(&mut g);
    assert_eq!(g.stars.get("01-First Spark"), Some(sim::Difficulty::Normal));
    g.headless_skirmish("01-First", 1, None, None, sim::Difficulty::Hard, None);
    win(&mut g);
    assert_eq!(g.stars.get("01-First Spark"), Some(sim::Difficulty::Hard));
    // the lobby shows it
    g.open_skirmish();
    assert_eq!(g.skirmish_menu.as_ref().unwrap().names[0], "01-First Spark");
}

#[test]
fn the_main_menu_has_a_skirmish_row_that_opens_the_lobby_against_the_computer() {
    let Some(mut g) = game() else { return };
    let buttons = menu::layout(&Screen::Main, &mut g.assets, &g.levels, &g.settings, 640, 480);
    assert_eq!(buttons.len(), 8);
    assert!(buttons.iter().any(|b| b.action == Action::MapEditor), "a MAP EDITOR row");
    let row = buttons.iter().find(|b| b.action == Action::Skirmish).expect("a SKIRMISH row");
    assert!(buttons.iter().all(|b| b.rect.y + b.rect.h <= 480), "all rows fit the lowest resolution");
    g.act(row.action.clone());
    let m = g.skirmish_menu.as_ref().expect("the skirmish setup screen");
    assert!(m.map_level().is_some(), "the first map's picture and script are loaded");
    assert_eq!((m.local, m.map), (0, 0));
}

#[test]
fn a_skirmish_starts_with_the_lobbys_choices_for_either_side_and_restarts() {
    let Some(mut g) = game() else { return };
    g.mute = true;
    // blue: the human keeps the map's player 0 side
    g.headless_skirmish("01-First", 0, Some((1234, 77)), Some((300, 0)), sim::Difficulty::Hard, None);
    let p = g.play.as_ref().expect("the match started");
    assert_eq!((p.world.credits[0], p.world.credits[1]), (1234, 77));
    assert_eq!(p.world.research[0], 300);
    assert_eq!(p.world.difficulty, sim::Difficulty::Hard, "the lobby's difficulty reaches the world");
    assert_eq!(g.skirmish.as_ref().map(|s| s.map.as_str()), Some("01-First Spark"));
    let blue_hq =
        p.world.structures.iter().find(|s| s.kind_id == kinds::HEADQUARTERS && s.owner == opensow_formats::level::Owner::Player).map(|s| s.cell).unwrap();
    assert_eq!(blue_hq, (16, 3), "First Spark's blue HQ");
    // green: the sides are exchanged, so the human (player 0) sits at the green HQ and has green's resources
    g.headless_skirmish("01-First", 1, Some((1234, 77)), None, sim::Difficulty::Normal, None);
    let p = g.play.as_ref().unwrap();
    assert_eq!((p.world.credits[0], p.world.credits[1]), (77, 1234));
    let hq = p.world.structures.iter().find(|s| s.kind_id == kinds::HEADQUARTERS && s.owner == opensow_formats::level::Owner::Player).map(|s| s.cell).unwrap();
    assert_eq!(hq, (9, 55), "First Spark's green HQ");
    assert!(p.world.units.iter().filter(|u| u.player == 0).all(|u| u.px().1 > 32 * 30), "the human's units start on the green side");
    assert!(p.world.ai, "the computer plays the other side");
    // RESTART replays the same skirmish, ESC menu -> main menu forgets it
    g.act(Action::Restart);
    assert_eq!(g.play.as_ref().unwrap().world.credits[0], 77);
    assert_eq!(g.play.as_ref().unwrap().world.difficulty, sim::Difficulty::Normal, "RESTART keeps the level");
    g.act(Action::ToMain);
    assert!(g.skirmish.is_none());
}

#[test]
fn briefing_movie_before_the_mission_and_outro_after_the_last_one() {
    let Some(mut g) = game() else { return };
    let (w, h) = (640, 480);
    g.movies = true;
    g.mute = true;
    // choosing a mission shows its briefing first; a click skips it and the mission starts
    g.act(Action::Level("M01".into()));
    assert_eq!(g.intro.as_ref().map(|i| i.name.as_str()), Some("M01"));
    assert!(g.play.is_none());
    let mut buf = vec![0u32; w * h];
    g.update(0.1, w, h);
    g.draw(&mut buf, w, h);
    assert!(buf.iter().any(|&p| p != 0));
    g.mouse_button(MouseButton::Left, true, 5, 5, w, h);
    assert!(g.intro.is_none() && g.play.is_some() && g.screen.is_none());
    // restarting does not replay the briefing
    g.act(Action::Restart);
    assert!(g.intro.is_none() && g.play.is_some());
    // winning the final mission shows the banner and the statistics, then plays the outro and shows the message
    g.start(FINAL_LEVEL);
    g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
    g.update(0.0, w, h);
    assert!(g.results.is_some() && g.intro.is_none());
    pass_results(&mut g, w, h);
    assert_eq!(g.intro.as_ref().map(|i| i.name.as_str()), Some("mO"));
    g.key(&Key::Named(NamedKey::Space), true, w, h);
    assert!(g.intro.is_none() && matches!(g.screen, Some(Screen::Message(_))));
    // winning any other mission (not from the campaign map) returns to the main menu after the statistics
    g.start("M02");
    g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
    g.update(0.0, w, h);
    pass_results(&mut g, w, h);
    assert!(g.intro.is_none() && g.results.is_none() && g.play.is_none() && matches!(g.screen, Some(Screen::Main)));
}

#[test]
fn p_pauses_and_only_p_resumes() {
    let Some(mut g) = game() else { return };
    let (w, h) = (640, 480);
    g.mute = true;
    g.start("M01");
    g.update(0.5, w, h);
    let tick = g.play.as_ref().unwrap().world.tick;
    g.key(&Key::Character(SmolStr::new("p")), true, w, h);
    assert!(g.play.as_ref().unwrap().paused);
    g.update(1.0, w, h);
    assert_eq!(g.play.as_ref().unwrap().world.tick, tick, "the game stands still");
    // Esc and the mouse do nothing while paused
    g.key(&Key::Named(NamedKey::Escape), true, w, h);
    assert!(g.screen.is_none());
    g.mouse_button(MouseButton::Left, true, 100, 100, w, h);
    assert!(g.play.as_ref().unwrap().paused && g.screen.is_none());
    let mut buf = vec![0xff8040u32; w * h];
    g.draw(&mut buf, w, h);
    assert!(buf[100 * w..101 * w].iter().all(|&p| p == 0), "even rows are black");
    assert!(buf[101 * w..102 * w].iter().any(|&p| p != 0));
    g.key(&Key::Character(SmolStr::new("p")), true, w, h);
    assert!(!g.play.as_ref().unwrap().paused);
    g.update(0.5, w, h);
    assert!(g.play.as_ref().unwrap().world.tick > tick);
}

/// Click through the banner (after its two seconds) and the statistics screen.
fn pass_results(g: &mut Game, w: usize, h: usize) {
    assert!(g.results.is_some());
    g.mouse_button(MouseButton::Left, true, 5, 5, w, h);
    assert!(!g.results.as_ref().unwrap().in_statistics(), "the banner waits two seconds");
    g.update(2.5, w, h);
    g.mouse_button(MouseButton::Left, true, 5, 5, w, h);
    assert!(g.results.as_ref().unwrap().in_statistics());
    g.update(0.1, w, h);
    assert!(!g.results.as_ref().unwrap().finished(), "the bars are still counting");
    g.mouse_button(MouseButton::Left, true, 5, 5, w, h); // completes the bars
    assert!(g.results.as_ref().unwrap().finished() && g.results.is_some());
    g.mouse_button(MouseButton::Left, true, 5, 5, w, h); // leaves
    assert!(g.results.is_none());
}

#[test]
fn the_tutorial_button_starts_m00_without_a_movie_and_its_end_returns_to_the_menu() {
    let Some(mut g) = game() else { return };
    let (w, h) = (640, 480);
    g.movies = true;
    g.mute = true;
    g.act(Action::Tutorial);
    assert!(g.intro.is_none() && g.screen.is_none() && !g.campaign);
    let p = g.play.as_ref().unwrap();
    assert_eq!(p.level, "M00");
    assert_eq!(p.world.tutorial.as_ref().map(|t| t.step), Some(0));
    assert!(!p.world.ai);
    // the script's last step goes straight to the main menu (state 0x1c), no result screen
    g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
    g.update(0.0, w, h);
    assert!(g.play.is_none() && matches!(g.screen, Some(Screen::Main)));
}

#[test]
fn the_warmonger_edition_has_its_own_missions_progress_and_saves() {
    use opensow_formats::{config::PassedLevels, Edition};
    let Some(mut g) = game() else { return };
    if g.assets.game.warmonger.is_none() {
        return;
    }
    assert!(g.levels.iter().any(|l| l == "M05"));
    let base_m01 = g.assets.game.resolve("Data\\Levels\\M01.map").unwrap();
    g.set_edition(Edition::Warmonger);
    assert_eq!(g.progress, PassedLevels::initial_warmonger());
    assert!(!g.levels.iter().any(|l| l == "M05"), "{:?}", g.levels);
    assert!(g.levels.iter().any(|l| l == "M07"));
    assert_ne!(g.assets.game.resolve("Data\\Levels\\M01.map").unwrap(), base_m01);
    assert_eq!(saveui::Store::locate(&g.assets.game).edition, Edition::Warmonger, "new saves are flagged");
    // a mission loads from Warmonger's data and passing it follows the Warmonger unlock chain
    g.campaign = true;
    g.start("M03");
    assert!(g.play.as_ref().is_some_and(|p| p.level == "M03" && p.world.units.iter().any(|u| u.kind == kinds::POD)), "the convoy mission's Rogon pods");
    g.progress.pass_warmonger(3);
    assert_eq!((g.progress.states[3], g.progress.states[4]), (1, 2));
    g.open_lands(false);
    assert!(g.lands.as_ref().unwrap().headings(3).0.starts_with("TERRITORY : 03"));
    g.set_edition(Edition::Original);
    assert_eq!(g.progress, PassedLevels::initial());
    assert!(g.levels.iter().any(|l| l == "M05"));
}

#[test]
fn a_save_slot_remembers_its_campaign_and_loading_it_switches_the_edition() {
    use opensow_formats::Edition;
    let Some(mut g) = game() else { return };
    if g.assets.game.warmonger.is_none() {
        return;
    }
    g.set_edition(Edition::Warmonger);
    g.start("M03");
    let snapshot = g.play.as_mut().unwrap().snapshot();
    let dir = std::env::temp_dir().join(format!("opensow-edition-save-{}", std::process::id()));
    let mut store = saveui::Store { write: dir.clone(), also: None, edition: Edition::Warmonger };
    let thumb = vec![0u16; 160 * 120];
    store.write(1, "wm", "M03", &snapshot, &[], &thumb).unwrap();
    store.edition = Edition::Original;
    store.write(2, "base", "M03", &snapshot, &[], &thumb).unwrap();
    let saved = store.level_of(1).unwrap();
    assert_eq!((saved.level.as_str(), saved.edition), ("M03", Edition::Warmonger));
    assert_eq!(store.level_of(2).unwrap().edition, Edition::Original);
    // loading it from the base game's campaign selects Warmonger and its mission
    g.set_edition(Edition::Original);
    g.saveui_event(saveui::Event::Load(saved));
    assert_eq!(g.assets.game.edition, Edition::Warmonger);
    assert!(g.play.as_ref().is_some_and(|p| p.level == "M03" && p.world.units.iter().any(|u| u.kind == kinds::POD)), "Warmonger's convoy mission");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn campaign_map_to_mission_and_back_with_progress() {
    let Some(mut g) = game() else { return };
    let (w, h) = (640, 480);
    g.act(Action::NewGame);
    assert!(g.lands.is_some());
    // pick land 1 (Canada) on its pixels, accept at the bottom left
    let (px, py) = (170, 190);
    g.mouse_moved(px, py, w, h);
    assert_eq!(g.lands.as_ref().unwrap().hovered(), Some(1));
    g.mouse_button(MouseButton::Left, true, px, py, w, h);
    assert_eq!(g.lands.as_ref().unwrap().selected(), Some(1));
    g.update(3.0, w, h); // the reticle plays first; ACCEPT and BACK appear after it
    g.mouse_moved(40, 450, w, h);
    g.mouse_button(MouseButton::Left, true, 40, 450, w, h);
    assert!(g.lands.is_none() && g.play.is_some(), "ACCEPT starts mission 1");
    assert_eq!(g.play.as_ref().unwrap().level, "M01");
    // winning shows the banner and the statistics; leaving them passes the land, opens the next one in the exe's order and
    // brings the map back
    g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
    g.update(0.0, w, h);
    assert_eq!(g.progress.states[1], 2, "not passed before the statistics are left");
    pass_results(&mut g, w, h);
    assert_eq!((g.progress.states[1], g.progress.states[6]), (1, 2));
    assert!(g.lands.is_some() && g.play.is_none());
    assert_eq!(g.lands.as_ref().unwrap().progress.states[6], 2);
}

#[test]
fn a_campaign_mission_won_after_loading_a_save_passes_its_land() {
    // the statistics screen passes the mission number whoever started it (0x44d7d6): a quick load does not leave the campaign
    let Some(mut g) = game() else { return };
    let (w, h) = (640, 480);
    g.progress.pass(1); // opens land 6 (Russia)
    assert_eq!(g.progress.states[6], 2);
    g.campaign = true;
    g.start("M06");
    let snapshot = g.play.as_mut().unwrap().snapshot();
    let dir = std::env::temp_dir().join(format!("opensow-campaign-load-{}", std::process::id()));
    let store = saveui::Store { write: dir.clone(), also: None, edition: g.assets.game.edition };
    store.write(0, "Quick Save", "M06", &snapshot, &[], &vec![0u16; 160 * 120]).unwrap();
    let saved = store.level_of(0).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    // from the main menu, as after a restart of the game
    g.act(Action::ToMain);
    assert!(!g.campaign);
    g.saveui_event(saveui::Event::Load(saved));
    assert!(g.campaign && g.play.as_ref().is_some_and(|p| p.level == "M06"));
    g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
    g.update(0.0, w, h);
    pass_results(&mut g, w, h);
    assert_eq!(g.progress.states[6], 1, "Russia is passed");
    assert!(g.lands.is_some() && g.play.is_none(), "the campaign map comes back");
    assert_eq!(game_screens::campaign_mission("M00"), None);
    assert_eq!(game_screens::campaign_mission("custom/foo"), None);
    assert_eq!(game_screens::campaign_mission("m22"), Some(22));
}

#[test]
fn a_warmonger_mission_won_after_loading_a_save_passes_its_land() {
    // Warmonger's statistics screen is the same code (0x447dcc..0x447deb: Pass([0x491360]) on its own record)
    use opensow_formats::Edition;
    let Some(mut g) = game() else { return };
    if g.assets.game.warmonger.is_none() {
        return;
    }
    let (w, h) = (640, 480);
    g.set_edition(Edition::Warmonger);
    g.progress.states[3] = 2;
    g.start("M03");
    let snapshot = g.play.as_mut().unwrap().snapshot();
    let dir = std::env::temp_dir().join(format!("opensow-wm-campaign-load-{}", std::process::id()));
    let store = saveui::Store { write: dir.clone(), also: None, edition: Edition::Warmonger };
    store.write(0, "Quick Save", "M03", &snapshot, &[], &vec![0u16; 160 * 120]).unwrap();
    let saved = store.level_of(0).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    g.act(Action::ToMain);
    g.saveui_event(saveui::Event::Load(saved));
    assert!(g.campaign && g.assets.game.edition == Edition::Warmonger);
    g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
    g.update(0.0, w, h);
    pass_results(&mut g, w, h);
    assert_eq!((g.progress.states[3], g.progress.states[4]), (1, 2), "the Warmonger chain moves on");
    assert!(g.lands.is_some());
}

#[test]
fn menus_and_the_map_raise_the_exes_interface_sounds() {
    let Some(mut g) = game() else { return };
    let (w, h) = (640, 480);
    // hovering a main-menu button plays mainmenu-move (30), clicking mainmenu-ok (29)
    let first = g.buttons(w, h)[0].rect;
    g.mouse_moved(first.x + 5, first.y + 5, w, h);
    assert_eq!(g.sfx.played, vec![30]);
    g.mouse_button(MouseButton::Left, true, first.x + 5, first.y + 5, w, h);
    g.mouse_button(MouseButton::Left, false, first.x + 5, first.y + 5, w, h);
    assert_eq!(g.sfx.played, vec![30, 29]);
    // the map: hovering a land is silent (the headings type with briefing-type02 later); selecting it flashes (46) and says its
    // location (41) on the first frame, and the description box opens (44) once the reticle has played
    g.sfx.played.clear();
    g.mouse_moved(170, 190, w, h);
    g.mouse_button(MouseButton::Left, true, 170, 190, w, h);
    assert!(g.sfx.played.is_empty());
    g.update(0.0, w, h);
    assert_eq!(g.sfx.played, vec![46, 41]);
    g.update(1.5, w, h);
    assert_eq!(g.sfx.played, vec![46, 41, 44]);
    // a shot from a turret or unit raises its muzzle sound
    g.sfx.played.clear();
    g.lands = None;
    g.start("M01");
    g.update(0.0, w, h);
    g.sfx.played.clear();
    let p = g.play.as_mut().unwrap();
    let (vx, vy) = (p.view.x as i32 + 50, p.view.y as i32 + 50);
    p.world.sounds.push((4, vx, vy, true));
    p.world.sounds.push((5, vx + 100_000, vy, true)); // far off screen: not heard
    g.update(0.0, w, h);
    assert!(!g.sfx.played.contains(&5));
    assert!(g.sfx.played.contains(&4), "{:?}", g.sfx.played);
    // the player's own actions raised by the world (air strike far off screen, turret placed) are always heard
    g.sfx.played.clear();
    g.play.as_mut().unwrap().world.player_sounds.extend([68, 72]);
    g.update(0.0, w, h);
    assert_eq!(g.sfx.played, vec![68, 72]);
    assert!(g.play.as_ref().unwrap().world.player_sounds.is_empty());
}

#[test]
fn clicking_any_building_plays_a_click() {
    let Some(mut g) = game() else { return };
    let (w, h) = (1024, 768);
    g.start("M01");
    g.update(0.0, w, h);
    for owner in [opensow_formats::level::Owner::Player, opensow_formats::level::Owner::Enemy] {
        let p = g.play.as_mut().unwrap();
        p.world.clear_selection();
        p.sel_building = None;
        let b =
            p.world.structures.iter().position(|s| s.alive() && s.owner == owner && !matches!(s.kind, opensow_formats::level::ObjectKind::Turret(_))).unwrap();
        let (cx, cy) = (p.world.structures[b].cell.0 * sim::CELL + 8, p.world.structures[b].cell.1 * sim::CELL + 8);
        assert_eq!(p.world.structure_at(cx, cy), Some(b));
        p.view.x = cx as f32 - 300.0 / p.view.zoom;
        p.view.y = cy as f32 - 300.0 / p.view.zoom;
        g.sfx.played.clear();
        g.mouse_button(MouseButton::Left, true, 300, 300, w, h);
        g.mouse_button(MouseButton::Left, false, 300, 300, w, h);
        assert!(g.sfx.played.contains(&play::BUILDING_CLICK_SOUND), "{owner:?}: {:?}", g.sfx.played);
        assert_eq!(g.play.as_ref().unwrap().sel_building.is_some(), owner == opensow_formats::level::Owner::Player);
    }
}

#[test]
fn message_events_are_spoken_and_the_mission_end_speaks() {
    let Some(mut g) = game() else { return };
    let (w, h) = (640, 480);
    g.start("M01");
    g.update(0.0, w, h);
    g.sfx.played.clear();
    let p = g.play.as_mut().unwrap();
    let t = p.world.tick;
    p.world.log.push((t, opensow_formats::messages::BUILDING_CAPTURED));
    p.world.log.push((t, 11)); // 5000 credits: speech plus the info-box blip
    p.world.log.push((t, 6)); // "new airstrike ready" has no speech file
    g.update(0.0, w, h);
    assert_eq!(g.sfx.spoken, vec!["06-building-captured.wav", "02-5000-credits-arrived.wav"]);
    assert_eq!(g.sfx.played, vec![51]);
    // spoken once
    g.update(0.0, w, h);
    assert_eq!(g.sfx.spoken.len(), 2);
    // losing says so, shows the banner and the statistics and returns to the main menu
    g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Defeat);
    g.update(0.0, w, h);
    assert_eq!(g.sfx.spoken.last().map(String::as_str), Some("01-mis-failed.wav"));
    pass_results(&mut g, w, h);
    assert!(g.play.is_none() && matches!(g.screen, Some(Screen::Main)));
}

#[test]
fn warmongers_radar_pictures_fit_its_small_radar_list() {
    let Some(mut g) = game() else { return };
    if g.assets.game.warmonger.is_none() {
        return;
    }
    let set = g.assets.warmonger_radar().expect("Warmonger's radar2.tsp");
    assert_eq!(set.len(), 11);
    for m in [1, 2, 3, 4, 6, 7, 11, 12, 15, 20, 22] {
        let f = &set[hud::radar2_frame(m)];
        let small = hud::SMALL_RADAR_WARMONGER.contains(&m);
        assert_eq!((f.width, f.height) == (64, 64), small, "M{m:02}: {}x{}", f.width, f.height);
    }
}

/// The lockstep draws up to the computer's first strategy roll follow the exe (docs/ai.md "The first roll"): the curves (600)
/// and the 19 units' initialisers (2 each) during the load, then in the first tick blue's turn (its 3 turrets create their gun
/// units, 2 draws each) and green's computer, whose Advancer scores the 42 buildings (`rand(3)` each) before the roll at
/// index 686: strategy 1 (every idle unit to the HQ), timer 900 + 324.
#[test]
fn m06_rolls_the_exes_first_strategy_at_the_exes_table_position() {
    let Some(mut g) = game() else { return };
    g.start("M06");
    let p = g.play.as_mut().unwrap();
    assert_eq!(p.world.ai_state().2, Some(638), "curves + 19 units x 2 when the level is loaded");
    p.world.step();
    let (strategy, timer, _) = p.world.ai_state();
    assert_eq!((strategy, timer), (1, 1224));
}

#[test]
fn movie_and_briefing_animation_sound_follow_the_options() {
    let Some(mut g) = game() else { return };
    g.sfx.enabled = true;
    g.options.music_on = true;
    g.options.music_volume = 100;
    g.options.fx_on = true;
    g.options.fx_volume = 50;
    g.apply_audio();
    assert_eq!(g.movie_gain(), 1.0, "music at 100: movies at full volume");
    assert!((g.anim_gain() - sfx::gain(true, 50)).abs() < 1e-6, "the BRIEFING animation follows FX");
    g.options.music_volume = 50;
    g.apply_audio();
    assert!((g.movie_gain() - 10f32.powf(-1500.0 / 2000.0)).abs() < 1e-6, "music 50 = -15 dB");
    g.options.music_on = false;
    g.options.fx_volume = 0;
    g.apply_audio();
    assert_eq!((g.movie_gain(), g.anim_gain()), (0.0, 0.0), "music off silences movies, FX 0 the animation");
    g.options.music_on = true;
    g.mute = true;
    g.apply_audio();
    assert_eq!(g.movie_gain(), 0.0, "--mute opens nothing");
}

#[test]
fn dying_structures_show_their_destroyed_frame_and_a_turret_loses_only_its_gun() {
    use opensow_viewer::art::Source;
    let Some(mut g) = game() else { return };
    g.mute = true;
    g.start("M01");
    let (w, h) = (640, 480);
    let mut buf = vec![0u32; w * h];
    let p = g.play.as_mut().expect("M01 runs");
    // M01: an enemy light factory (kind 101) at (24, 14), an enemy turret 3 (kind 27) at (27, 20)
    let at = |p: &play::Play, cell: (i32, i32)| p.world.structures.iter().position(|s| s.cell == cell).unwrap();
    let (factory, turret) = (at(p, (24, 14)), at(p, (27, 20)));
    p.world.debug_destroy_at(25, 15);
    p.world.debug_destroy_at(27, 20);
    let mut layers = |g: &mut Game, i: usize| {
        g.draw(&mut buf, w, h);
        let p = g.play.as_ref().unwrap();
        let object = p.world.structures[i].object;
        p.scene.placed.iter().filter(|s| matches!(s.source, Source::Object(o) if o == object)).map(|s| (s.layer, s.frame)).collect::<Vec<_>>()
    };
    let destroyed = |kind| opensow_formats::buildings::anim(kind).unwrap().range(opensow_formats::buildings::STATE_DESTROYED).0;
    assert_eq!(layers(&mut g, factory), vec![(0, destroyed(101))]);
    assert_eq!(layers(&mut g, turret), vec![(0, 5)], "the base stays (its wrecked frame 5), the gun top goes");
    // the turret's animation ends after about 90 passes (2 per tick); then it is removed
    let p = g.play.as_mut().unwrap();
    for _ in 0..60 {
        p.world.step();
    }
    assert!(p.world.structures[turret].dying.is_none());
    assert!(layers(&mut g, turret).is_empty());
}

#[test]
fn new_game_plays_the_intro_once_before_the_campaign_map() {
    let Some(mut g) = game() else { return };
    (g.movies, g.mute) = (true, true);
    g.act(Action::NewGame);
    assert_eq!(g.intro.as_ref().map(|i| i.name.as_str()), Some("mI"));
    assert!(g.lands.is_none(), "the map waits for the intro");
    g.end_movie();
    assert!(g.intro.is_none() && g.lands.is_some(), "the map follows the intro");
    g.lands_event(lands::Event::Back);
    g.act(Action::NewGame);
    assert!(g.intro.is_none(), "the intro plays only once");
    assert!(g.lands.is_some());
}

#[test]
fn the_first_switch_to_warmonger_plays_its_intro_once() {
    use opensow_formats::Edition;
    let Some(mut g) = game() else { return };
    if g.assets.game.warmonger.is_none() {
        return;
    }
    (g.movies, g.mute) = (true, true);
    g.seen_intros = [true, false];
    g.act(Action::NewGame);
    assert!(g.intro.is_none() && g.lands.is_some(), "the base intro was seen already");
    g.lands_event(lands::Event::Edition(Edition::Warmonger));
    assert_eq!(g.intro.as_ref().map(|i| i.name.as_str()), Some("mI"));
    assert!(g.lands.is_none());
    g.end_movie();
    assert!(g.lands.is_some() && g.assets.game.edition == Edition::Warmonger, "Warmonger's map follows its intro");
    g.lands_event(lands::Event::Edition(Edition::Original));
    g.lands_event(lands::Event::Edition(Edition::Warmonger));
    assert!(g.intro.is_none() && g.lands.is_some(), "only the first switch plays it");
}

/// Ctrl or Alt (`0x4179bf`) turn the attack pointer into type 5 (`0x417d21`) and make a building attack a lasting chase
/// (`0x417d81`: the flag goes to `0x469fe0`); without them a building click is one route. Catches the port showing type 4
/// with the key held, or always (never) keeping the building as a chase target.
#[test]
fn ctrl_or_alt_turns_the_building_attack_into_a_lasting_chase() {
    use opensow_formats::cursor::Pointer;
    let Some(mut g) = game() else { return };
    let (w, h) = (1024, 768);
    g.start("M01");
    g.update(0.0, w, h);
    let (id, b) = {
        let p = g.play.as_mut().unwrap();
        let b = (0..p.world.structures.len())
            .find(|&k| p.world.structures[k].alive() && p.world.structures[k].owner != opensow_formats::level::Owner::Player)
            .expect("an enemy building");
        let (bx, by) = p.world.structures[b].center();
        // the building at a third of the screen's width, clear of the HUD
        p.view.x = bx as f32 - w as f32 / p.view.zoom / 3.0;
        p.view.y = by as f32 - h as f32 / p.view.zoom / 2.0;
        p.world.clear_selection();
        let id = p.world.add_unit_near(7, 0, bx, by + 200).unwrap();
        p.world.units.iter_mut().find(|u| u.id == id).unwrap().selected = true;
        (id, b)
    };
    let (bx, by) = g.play.as_ref().unwrap().world.structures[b].center();
    let (sx, sy) = g.play.as_ref().unwrap().view.to_screen(bx as f32, by as f32);
    for (held, pointer, lasting) in [(false, Pointer::Attack, false), (true, Pointer::AttackModifier, true)] {
        g.set_attack_key(held);
        g.mouse_moved(sx, sy, w, h);
        assert_eq!(g.play.as_ref().unwrap().pointer(), pointer, "key held: {held}");
        g.mouse_button(MouseButton::Left, true, sx, sy, w, h);
        g.mouse_button(MouseButton::Left, false, sx, sy, w, h);
        assert_eq!(g.play.as_ref().unwrap().world.unit(id).unwrap().chase.is_some(), lasting, "key held: {held}");
    }
}

/// With Ctrl or Alt held the exe takes the attack path over own objects too (`0x417a1d`, `0x417ba6`): type 5 and a lasting
/// attack over an own building or unit with a selection, destroy (7) over an own building with an Advancer, and no selection
/// of an own building without one. Catches the port still selecting or showing the select pointer there.
#[test]
fn ctrl_or_alt_attack_own_units_and_buildings() {
    use opensow_formats::cursor::Pointer;
    let Some(mut g) = game() else { return };
    let (w, h) = (1024, 768);
    g.start("M01");
    g.update(0.0, w, h);
    let (b, shooter, mate, adv) = {
        let p = g.play.as_mut().unwrap();
        let b = (0..p.world.structures.len())
            .find(|&k| p.world.structures[k].alive() && p.world.structures[k].owner == opensow_formats::level::Owner::Player)
            .expect("an own building");
        let (bx, by) = p.world.structures[b].center();
        p.view.x = bx as f32 - w as f32 / p.view.zoom / 3.0;
        p.view.y = by as f32 - h as f32 / p.view.zoom / 2.0;
        p.world.clear_selection();
        let shooter = p.world.add_unit_near(7, 0, bx, by + 150).unwrap();
        let mate = p.world.add_unit_near(7, 0, bx + 150, by).unwrap();
        let adv = p.world.player_units().find(|u| u.kind == kinds::ADVANCER).unwrap().id;
        (b, shooter, mate, adv)
    };
    let screen = |g: &Game, x: f32, y: f32| g.play.as_ref().unwrap().view.to_screen(x, y);
    let (bx, by) = g.play.as_ref().unwrap().world.structures[b].center();
    let (sx, sy) = screen(&g, bx as f32, by as f32);
    let click = |g: &mut Game, x: i32, y: i32| {
        g.mouse_moved(x, y, w, h);
        g.mouse_button(MouseButton::Left, true, x, y, w, h);
        g.mouse_button(MouseButton::Left, false, x, y, w, h);
    };
    let pointer = |g: &Game| g.play.as_ref().unwrap().pointer();
    g.set_attack_key(true);
    // nothing selected: the arrow, and the click selects nothing
    g.mouse_moved(sx, sy, w, h);
    assert_eq!(pointer(&g), Pointer::Arrow);
    click(&mut g, sx, sy);
    assert_eq!(g.play.as_ref().unwrap().sel_building, None);
    // a unit selected: type 5 over the own building, the click is a lasting attack on it
    g.play.as_mut().unwrap().world.units.iter_mut().find(|u| u.id == shooter).unwrap().selected = true;
    g.mouse_moved(sx, sy, w, h);
    assert_eq!(pointer(&g), Pointer::AttackModifier);
    click(&mut g, sx, sy);
    assert_eq!(g.play.as_ref().unwrap().world.unit(shooter).unwrap().chase, Some(sim::Target::Structure(b)));
    // and over an own unit
    let (mx, my) = g.play.as_ref().unwrap().world.unit(mate).map(|u| (u.x, u.sprite_y())).unwrap();
    let (ux, uy) = screen(&g, mx, my);
    g.mouse_moved(ux, uy, w, h);
    assert_eq!(pointer(&g), Pointer::AttackModifier);
    click(&mut g, ux, uy);
    assert_eq!(g.play.as_ref().unwrap().world.unit(shooter).unwrap().chase, Some(sim::Target::Unit(mate)));
    assert!(!g.play.as_ref().unwrap().world.unit(mate).unwrap().selected, "the click does not select");
    // the Advancer selected: destroy over the own building
    {
        let p = g.play.as_mut().unwrap();
        p.world.clear_selection();
        p.world.units.iter_mut().find(|u| u.id == adv).unwrap().selected = true;
    }
    g.mouse_moved(sx, sy, w, h);
    assert_eq!(pointer(&g), Pointer::Destroy);
    click(&mut g, sx, sy);
    let a = g.play.as_ref().unwrap().world.unit(adv).unwrap().action;
    assert_eq!(a.map(|a| a.1), Some(b));
    // released: the own building is the select pointer again
    g.set_attack_key(false);
    g.mouse_moved(sx, sy, w, h);
    assert_ne!(pointer(&g), Pointer::Destroy);
}
