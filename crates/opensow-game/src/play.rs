//! The in-game screen: camera, selection, orders, and drawing the world on top of the viewer's map renderer.

use crate::assets::Assets;
use crate::hud::{self, Hit, Inputs, Layout};
use crate::infobox::{self, InfoBox};
use crate::menu::Settings;
use crate::sim::{self, Outcome, Target, World, CELL};
use crate::ui;
use opensow_formats::cursor::{scroll_pointer, Pointer};
use opensow_formats::effects::{bullet_draw, extra_at, Blit, Frame as FrameRule};
use opensow_formats::kinds;
use opensow_formats::level::{ObjectKind, Owner};
use opensow_formats::recoil::FLASH_SETS;
use opensow_formats::rings::Rings;
use opensow_formats::sprite::Frame;
use opensow_formats::units::unit_art;
use opensow_viewer::art::Source;
use opensow_viewer::render::{blit, blit_blend, blit_blend_masked, rect, render, Overlays, View};
use opensow_viewer::scene::Scene;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

type Set = Rc<Vec<Frame>>;

struct UnitSprites {
    chassis: Option<Set>,
    chassis_shadow: Option<Set>,
    top: Option<Set>,
    top_shadow: Option<Set>,
    flash: Option<Set>,
}

/// Units handed to the player when a mission starts without an army (cannons, artillery, flamers, special): PLACEHOLDER for the factories.
const STARTER_SQUAD: [i32; 8] = [8, 8, 9, 5, 11, 11, 7, 14];

/// `AnimSprite` draw mode of the smoke puffs: they live in their own list, drawn before the units.
const PUFF_MODE: u8 = 8;
/// A click on a building plays `tt-click` (54). Not in the exe: its building select (`0x417be0` -> `0x43ad60`) only sounds when
/// units of that group join the selection, so a building click is silent there.
pub const BUILDING_CLICK_SOUND: u8 = 54;

/// One wheel notch zooms the camera by this factor, eased in at `ZOOM_EASE` per second (the map editor's camera alike).
const ZOOM_STEP: f32 = 1.08;
const ZOOM_EASE: f32 = 12.0;

/// The mission screen: the world, the camera, the selection and pointer state, and the HUD.
/// The mission pointer as a picture for the system cursor: `(key, hotspot, width, height, RGBA pixels)`; see
/// [`Play::pointer_cursor`].
pub type PointerPicture = ((usize, u32), (i32, i32), u32, u32, Vec<u8>);

pub struct Play {
    /// Headless debugging (`--no-mask`): draw the smoke and the top shadows without the screen mask.
    pub debug_no_mask: bool,
    /// The level's name, e.g. `M01` or a skirmish map's name.
    pub level: String,
    /// The campaign record the savegame embeds (`0x456ac0`); the game sets it before a save.
    pub progress: opensow_formats::config::PassedLevels,
    /// The level as loaded for drawing: ground, terrain flags, sprites.
    pub scene: Scene,
    /// The simulation.
    pub world: World,
    /// The camera.
    pub view: View,
    sprites: HashMap<(i32, bool), UnitSprites>,
    rings: Rings,
    /// Screen position where a left-button drag started.
    drag: Option<(i32, i32)>,
    /// The pointer's position in the window.
    pub cursor: (i32, i32),
    /// Scroll when the pointer touches a window edge (only once a real pointer position is known).
    pub edge_scroll: bool,
    /// The system's cursor shows the pointer (`pointer_cursor`), so the frame does not draw it.
    pub system_pointer: bool,
    keys: HashSet<&'static str>,
    time: f32,
    /// The game is paused (key `P`).
    pub paused: bool,
    /// The chat line while it is open (Enter, docs/chat.md): the text typed so far.
    pub chat: Option<String>,
    /// The line sent last (Tab brings it back).
    last_chat: String,
    /// Pixel width of the open chat line at the last drawn frame; typing stops once it reaches `cheats::MAX_LINE_WIDTH`.
    chat_width: std::cell::Cell<i32>,
    /// The "draw fps" cheat is on, with the frames counted in the running second and the last second's count.
    pub show_fps: bool,
    /// F1's key help overlay (`[0x5baa9b]`, cleared at mission start, `0x422b4b`).
    pub show_keys: bool,
    fps: (std::time::Instant, u32, u32),
    /// Round-robin positions of Tab (`[pointer+0x2dc]`) and H / numpad + (`[pointer+0x2e0]`), docs/hotkeys.md.
    tab_next: usize,
    adv_next: usize,
    /// Round-robin position of Shift+H over the idle Advancers (not in the original).
    idle_next: usize,
    /// The last click that selected one of our units (when, unit kind): a second one on the same kind soon after is a
    /// double click (not in the original).
    last_unit_click: Option<(std::time::Instant, i32)>,
    /// World tick of the next automatic save to the quick save slot (not in the original; `Game::autosave`).
    pub autosave_at: u64,
    /// Options page 2 "Turret range" (from [`Settings::turret_range`] each update): the placement ring is drawn.
    pub turret_range: bool,
    /// Options page 2 "Swap mouse buttons" (from [`Settings::swap_buttons`] each update): orders go on the right button's click,
    /// the left button's click where an order would go does the original right click (not in the original; docs/controls.md).
    pub orders_on_right: bool,
    /// Options page 2 "Map zoom" (from [`Settings::zoom`] each update): the wheel zooms; off keeps the original scale.
    pub zoom_on: bool,
    /// "speed 1.5x" lines of F3 / F4 (`0x402ec0` events 0x19 / 0x1a, string 144 + `"  %.1fx"`): (world tick, text). They
    /// share the message queue but have no speech.
    pub notes: Vec<(u64, String)>,
    /// Camera bookmarks of numpad 1-9 (`[pointer+0x2e4..0x328]`): the map pixel in the middle of the view.
    pub bookmarks: [Option<(f32, f32)>; 9],
    fit: bool,
    /// The wheel's zoom on its way: the zoom aimed at and the window point that stays put (`update` eases towards it, like the
    /// map editor's camera).
    zoom_to: Option<(f32, (i32, i32))>,
    /// Turret tier (1-5) waiting for a click on the map (keys 1-5).
    pub placing: Option<u32>,
    /// Rally point mode (`[pointer+0x1e9]`, `[pointer+0x1ec]`): the structure whose point the next map click sets.
    pub rallying: Option<usize>,
    /// Dev tools: (unit kind, side) the next map click places (`devtools.rs`).
    pub dev_spawn: Option<(i32, usize)>,
    /// Air force waiting for a click on the map (keys B / F / C / Y / M).
    pub strike: Option<opensow_formats::air::AirForce>,
    /// Interface sounds raised by clicks (effect ids), drained by the game.
    pub ui_sounds: Vec<u8>,
    /// Tutorial voice files `Tutorial\NN.wav` to play, and the step whose instruction is shown.
    pub tut_voices: Vec<u32>,
    /// The instruction panel of the shown step (docs/tutorial.md), `None` before the first text.
    pub tut_panel: Option<crate::tutorial_panel::Panel>,
    /// Font 2 (`medium`) whose widths wrap the text, and the GDI font that draws it (the campaign map's).
    tut_font: Rc<(opensow_formats::font::Font, opensow_formats::font::Font)>,
    tut_gdi: Option<crate::gdi::Gdi>,
    /// Fonts 0 and 1 (`small_i`, `small_o`): the glyphs bytes >= 0x80 reach in the exe's width function.
    tut_small: Rc<(opensow_formats::font::Font, opensow_formats::font::Font)>,
    /// The HUD scale of the last drawn frame (for hit tests of the tutorial panel).
    panel_ui: f32,
    /// How many of `world.log`'s messages have been spoken.
    pub heard: usize,
    /// HUD layout of the last drawn frame (clicks are tested against it) and the viewport it was drawn for.
    hud: Option<Layout>,
    hud_hover: Option<Hit>,
    /// Selected own building (index into `world.structures`), shown in the info box.
    pub sel_building: Option<usize>,
    radar_drag: bool,
    /// Right button held: (|dx|, |dy| moved so far, scrolling).
    right: Option<(i32, i32, bool)>,
    /// Black radar (`[0x5baad8]`), toggled by a right click on the radar.
    radar_black: bool,
    /// The info box and its slide animation.
    pub infobox: InfoBox,
    /// Text of the info box for the kind it shows: (kind, lines, wrapped description), rebuilt in `draw`.
    info_text: Option<(i32, opensow_formats::infobox::InfoLines, Vec<String>)>,
    /// Air forces shown as pills on the left, in the order they appeared.
    pills: Vec<opensow_formats::air::AirForce>,
    viewport: (usize, usize),
    /// The language strings (messages are shown for a few seconds in the HUD).
    lang: Option<opensow_formats::lang::LangFile>,
    /// GDI stand-in for the satellite-protection label (11 px Times New Roman bold).
    label_font: Option<crate::gdi::Gdi>,
    /// Unicode text for the HUD messages, countdown, hints and pause banner when the language is not English: the bitmap fonts only
    /// have ASCII (the exe draws all of these through GDI, docs/text.md). `None` for English keeps the bitmap look.
    hud_gdi: Option<crate::gdi::Gdi>,
    /// The info box's Arial for languages other than English (accents; the bitmap font is ASCII only).
    info_gdi: Option<crate::gdi::Gdi>,
}

impl Play {
    /// Loads a level and builds its world; `carry_counter` is the exe's global carryall counter, handed on between missions.
    pub fn new(assets: &mut Assets, level: &str, settings: &Settings, carry_counter: usize) -> Result<Play, String> {
        let scene = Scene::load(&assets.game, level, 0, None)?;
        Play::with_scene(assets, level, scene, settings, carry_counter)
    }

    /// A skirmish on a `.net` map (docs/skirmish.md): the terrain of the mission the map belongs to, the script `level` already
    /// prepared by [`crate::skirmish::prepare`] (the human is player 0, the computer player 1).
    /// `custom`: a custom map's terrain source and object additions (docs/map-editor-ingame.md); `None` for the shipped maps.
    pub fn new_skirmish(
        assets: &mut Assets,
        map: &str,
        level: opensow_formats::level::Level,
        settings: &Settings,
        difficulty: sim::Difficulty,
        custom: Option<(opensow_formats::custommap::Ground, Vec<opensow_formats::custommap::ObjectExt>)>,
    ) -> Result<Play, String> {
        let start = level.clone();
        let (scene, ext) = match custom {
            // a custom map: its base mission, or its generated layout (docs/generated-maps.md)
            Some((ground, ext)) => (crate::custom_maps::load_scene(&assets.game, &ground, 0, None, Some(level))?, ext),
            None => {
                let (game, terrain) = assets.game.net_terrain(map, level.level_id());
                (Scene::load_with(&game, &terrain, 0, None, Some(level))?, Vec::new())
            }
        };
        let mut play = Play::with_scene(assets, map, scene, settings, 0)?;
        play.world.difficulty = difficulty;
        play.world.skirmish = true;
        // neutral buildings start at their own percentage (50 on the shipped maps), custom additions
        play.world.neutral_start_hp_from_level(&start);
        play.world.apply_custom(&ext);
        // maps that relied on starting carryalls get ground units instead, the same for both sides (docs/skirmish.md)
        // (out in the open: not under the trees and rock tops of the map's foreground layer)
        let (fg, w, h) = (play.scene.foreground.clone(), play.scene.bg_w, play.scene.bg_h);
        let hidden = |c: (i32, i32)| {
            let (x0, y0) = (c.0.max(0) as usize * 32, c.1.max(0) as usize * 32);
            (y0..(y0 + 32).min(h)).any(|y| (x0..(x0 + 32).min(w)).any(|x| fg.get(y * w + x).copied().unwrap_or(false)))
        };
        for side in 0..2 {
            play.world.add_start_units(side, crate::skirmish::start_units(map), hidden);
        }
        Ok(play)
    }

    fn with_scene(assets: &mut Assets, level: &str, mut scene: Scene, settings: &Settings, carry_counter: usize) -> Result<Play, String> {
        let lvl = scene.level.as_ref().ok_or("level has no script (.edt)")?;
        let rng = std::fs::read(assets.game.data().join("generic.file")).ok().and_then(|d| opensow_formats::rng::GameRng::from_file(&d));
        let mut world = World::from_level_with_rng(&scene.map, lvl, rng);
        if settings.starter_squad && world.player_units().filter(|u| u.kind != kinds::ADVANCER).count() < 4 {
            let anchor = world.player_units().find(|u| u.kind == kinds::ADVANCER).or_else(|| world.player_units().next()).map(|u| u.px());
            if let Some((x, y)) = anchor {
                for kind in STARTER_SQUAD {
                    world.add_unit_near(kind, 0, x, y + 48);
                }
            }
        }
        for n in 25..=29 {
            if let Some(t) = opensow_formats::buildings::turret_art(n) {
                if let Some(tables) = assets.art.fire_tables(t.top) {
                    world.turret_muzzles.insert(n + 84, tables);
                }
                if let Some(tables) = assets.art.fire_tables_ref(t.top) {
                    world.turret_muzzles.insert(sim::REFERENCE_KEY + n + 84, tables);
                }
            }
        }
        // the ground units' and walkers' shots start at their tops' `.po1` points (kind -> tables, next to the turrets')
        for kind in (1..=15).chain(41..=45) {
            if let Some(tables) = unit_art(kind).and_then(|a| a.top).and_then(|t| assets.art.fire_tables(t)) {
                world.turret_muzzles.insert(kind, tables);
            }
            if let Some(tables) = unit_art(kind).and_then(|a| a.top).and_then(|t| assets.art.fire_tables_ref(t)) {
                world.turret_muzzles.insert(sim::REFERENCE_KEY + kind, tables);
            }
        }
        // Units are drawn by the game from the simulation; the loader's static unit sprites go away.
        scene.placed.retain(|p| matches!(p.source, Source::Object(_)));
        let mut play = Play {
            progress: opensow_formats::config::PassedLevels::initial(),
            debug_no_mask: false,
            level: level.to_owned(),
            scene,
            world,
            view: View { x: 0.0, y: 0.0, zoom: 1.0 },
            sprites: HashMap::new(),
            rings: Rings::generate(),
            drag: None,
            cursor: (0, 0),
            edge_scroll: false,
            system_pointer: false,
            keys: HashSet::new(),
            time: 0.0,
            paused: false,
            chat: None,
            last_chat: String::new(),
            chat_width: std::cell::Cell::new(0),
            show_fps: false,
            show_keys: false,
            fps: (std::time::Instant::now(), 0, 0),
            tab_next: 0,
            adv_next: 0,
            idle_next: 0,
            last_unit_click: None,
            autosave_at: 0,
            turret_range: settings.turret_range,
            orders_on_right: settings.swap_buttons,
            zoom_on: settings.zoom,
            notes: Vec::new(),
            bookmarks: [None; 9],
            fit: true,
            zoom_to: None,
            placing: None,
            rallying: None,
            dev_spawn: None,
            ui_sounds: Vec::new(),
            tut_voices: Vec::new(),
            tut_panel: None,
            tut_font: assets.font("medium"),
            tut_gdi: crate::gdi::Gdi::load(),
            tut_small: assets.font("small"),
            panel_ui: 1.0,
            heard: 0,
            strike: None,
            hud: None,
            hud_hover: None,
            sel_building: None,
            radar_drag: false,
            right: None,
            radar_black: false,
            infobox: InfoBox::new(),
            info_text: None,
            pills: Vec::new(),
            viewport: (0, 0),
            label_font: crate::gdi::Gdi::load_sized(11.0, 1.0),
            info_gdi: crate::gdi::Gdi::arial_for_language(assets.game.language(), 9.0),
            hud_gdi: (assets.game.language() != opensow_formats::lang::Language::English).then(|| crate::gdi::Gdi::load_sized(15.0, 1.0)).flatten(),
            lang: assets
                .game
                .resolve(&format!("Data\\Language\\{}", assets.game.language().file_name()))
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|d| opensow_formats::lang::LangFile::parse(&d).ok())
                .map(|f| f.with_supplement(assets.game.language())),
        };
        play.world.sheet_sizes = assets.art.sheet_sizes();
        play.world.set_mission(level.trim_start_matches(['M', 'm']).parse().ok());
        play.world.carry_counter = carry_counter; // `[0x89cf50]` survives from mission to mission
        play.world.start_airlift();
        // the tutorial (M00) runs its own script and has no computer opponent
        if level.eq_ignore_ascii_case("M00") {
            play.world.start_tutorial();
            play.world.ai = false;
        }
        play.world.select_all();
        play.world.clear_selection();
        Ok(play)
    }

    /// What a savegame stores (`saveui::pack_payload`): the original's state stream of the running mission (`World::to_state`,
    /// docs/savegame.md) and OpenSOW's own snapshot (the camera as three `f32` and the world as JSON) for an exact continuation.
    pub fn snapshot(&mut self) -> Vec<u8> {
        let mut ours = Vec::new();
        for v in [self.view.x, self.view.y, self.view.zoom] {
            ours.extend_from_slice(&v.to_le_bytes());
        }
        ours.extend(self.world.snapshot());
        let number = self.level.trim_start_matches(|c: char| !c.is_ascii_digit()).parse::<u32>().unwrap_or(0);
        let state = self.world.to_state(number, &self.progress.clone(), (self.view.x.max(0.0) as i32, self.view.y.max(0.0) as i32));
        crate::saveui::pack_payload(&state.to_bytes().unwrap_or_default(), &ours)
    }

    /// Continues a saved game in this freshly started play of the same level (`Play::snapshot`): our own snapshot when the save
    /// has one, else the original's stream is applied (`World::apply_state`).
    pub fn restore(&mut self, data: &[u8]) -> Result<(), String> {
        let (stream, ours) = crate::saveui::unpack_payload(data).ok_or("snapshot too short")?;
        if ours.is_empty() {
            let state = opensow_formats::save::State::parse(stream, 1).map_err(|e| e.to_string())?;
            self.world.apply_state(&state)?;
            // the tutorial script (`[0x70f2cc]`, its panel and arrow) is not in the original's stream: a save of level 0 loads as a plain
            // mission without the script (`-1` after the load's new-game reset, so the HQ rules and the computer opponent are on)
            if self.world.tutorial.is_some() {
                self.world.tutorial = None;
                self.world.tutorial_step = None;
                self.world.ai = true;
            }
            self.view = View { x: state.map.scroll_x as f32, y: state.map.scroll_y as f32, zoom: self.view.zoom };
        } else {
            let (cam, world) = ours.split_at_checked(12).ok_or("snapshot too short")?;
            let f = |i: usize| f32::from_le_bytes([cam[i], cam[i + 1], cam[i + 2], cam[i + 3]]);
            self.world.restore(world)?;
            self.view = View { x: f(0), y: f(4), zoom: f(8) };
        }
        self.fit = false;
        self.heard = self.world.log.len();
        // a loaded game is not saved over at once: the first automatic save comes one interval later
        self.autosave_at = self.world.tick + crate::AUTOSAVE_TICKS;
        self.sel_building = None;
        self.placing = None;
        self.rallying = None;
        self.strike = None;
        self.paused = false;
        Ok(())
    }

    #[cfg(test)]
    pub fn hud_layout(&self) -> Option<&Layout> {
        self.hud.as_ref()
    }

    /// How the mission ended, once it has.
    pub fn outcome(&self) -> Option<Outcome> {
        self.world.outcome
    }

    /// Centres the camera on the player's units (or the map middle) at zoom 1.
    pub fn centre_on_player(&mut self, w: usize, h: usize) {
        self.limit_zoom(w, h);
        let (n, sx, sy) = self.world.player_units().fold((0.0f32, 0.0f32, 0.0f32), |a, u| (a.0 + 1.0, a.1 + u.x, a.2 + u.y));
        let (cx, cy) = if n > 0.0 { (sx / n, sy / n) } else { (self.scene.bg_w as f32 / 2.0, self.scene.bg_h as f32 / 2.0) };
        self.view.x = cx - w as f32 / self.view.zoom / 2.0;
        self.view.y = cy - h as f32 / self.view.zoom / 2.0;
        self.view.clamp(&self.scene, w, h);
    }

    // ---- input --------------------------------------------------------------------------------------------------

    /// A scroll key went down or up.
    pub fn key(&mut self, name: &'static str, down: bool) {
        if down {
            self.keys.insert(name);
        } else {
            self.keys.remove(name);
        }
    }

    /// The map pixel under a window position.
    pub fn to_map(&self, sx: i32, sy: i32) -> (i32, i32) {
        ((self.view.x + sx as f32 / self.view.zoom) as i32, (self.view.y + sy as f32 / self.view.zoom) as i32)
    }

    /// True when the camera sits in the map's top-right corner: the radar then covers world the view can never scroll off, so
    /// it lets clicks through (an object under it, e.g. a corner HQ with an Advancer over it, stays reachable).
    fn radar_passes_through(&self) -> bool {
        let w = self.viewport.0;
        let vw = w as f32 / self.view.zoom;
        let mw = self.scene.bg_w as f32;
        self.view.y <= 0.5 && (vw >= mw || self.view.x >= mw - vw - 0.5)
    }

    /// The HUD element under a screen position; the radar and its frame are skipped in the top-right corner of the map.
    fn hud_hit(&self, x: i32, y: i32) -> Option<Hit> {
        let hit = self.hud.as_ref().and_then(|l| l.hit(x, y))?;
        let radar_area = matches!(hit, Hit::Radar) || self.hud.as_ref().is_some_and(|l| l.on_radar_frame(x, y));
        (!(radar_area && self.radar_passes_through())).then_some(hit)
    }

    /// The left button went down at a window position.
    pub fn left_down(&mut self, x: i32, y: i32) {
        if self.world.tutorial.as_ref().is_some_and(|t| t.arrow) && self.tutorial_arrow_at(x, y) {
            self.world.tutorial_arrow_clicked();
            self.ui_sounds.push(crate::tutorial_panel::CLICK_SOUND); // `0x456410` plays `[+0x58]`
            return;
        }
        // a disabled air pill is not a click target (`0x454d02`): the click goes on to the map
        let locked = self.hud.as_ref().is_some_and(|l| l.pills_locked);
        if let Some(hit) = self.hud_hit(x, y).filter(|h| !(locked && matches!(h, Hit::Air(_)))) {
            self.hud_click(hit, x, y);
            return;
        }
        self.drag = Some((x, y));
    }

    /// The tutorial panel's events (`TutEvent`): the instruction text, its voice, the arrow button's blip; and the script's
    /// view jumps (`0x4588c0`).
    pub fn take_tutorial_events(&mut self) {
        if let Some((x, y)) = self.world.camera_request.take() {
            self.look_at(x as f32, y as f32);
        }
        let events: Vec<sim::TutEvent> = self.world.tutorial.as_mut().map(|t| std::mem::take(&mut t.events)).unwrap_or_default();
        for e in events {
            match e {
                sim::TutEvent::Step(n) => {
                    if let Some(text) = self.lang.as_ref().and_then(|l| l.get(sim::tutorial_text_index(n))) {
                        let (small, medium) = (&self.tut_small, &self.tut_font.0);
                        let wrapped = crate::tutorial_panel::wrap(text.trim(), &|s, spacing| crate::tutorial_panel::byte_width(small, medium, s, spacing));
                        self.tut_panel = Some(crate::tutorial_panel::Panel::new(wrapped));
                    }
                    self.tut_voices.push(n);
                }
                sim::TutEvent::Arrow => self.ui_sounds.push(29),
            }
        }
        // one panel tick: the coin spins, the arrow slides in while it waits and spins under the pointer
        let up = self.world.tutorial.as_ref().is_some_and(|t| t.arrow);
        let hover = up && self.tutorial_arrow_at(self.cursor.0, self.cursor.1);
        if let Some(p) = self.tut_panel.as_mut() {
            p.tick(up, hover);
        }
    }

    /// Whether window pixel `(x, y)` is on the tutorial's arrow button (it exists only while it waits for a click).
    fn tutorial_arrow_at(&self, x: i32, y: i32) -> bool {
        let (_, h) = self.viewport;
        let ui = self.panel_ui.max(1.0);
        self.tut_panel.as_ref().is_some_and(|p| p.on_arrow((x as f32 / ui) as i32, ((y - h as i32) as f32 / ui) as i32))
    }

    /// Centres the camera on a map pixel.
    pub fn look_at(&mut self, px: f32, py: f32) {
        let (w, h) = self.viewport;
        self.view.x = px - w as f32 / self.view.zoom / 2.0;
        self.view.y = py - h as f32 / self.view.zoom / 2.0;
        self.view.clamp(&self.scene, w, h);
    }

    #[cfg(test)]
    pub fn radar_black(&self) -> bool {
        self.radar_black
    }

    /// The selected (clicked) own building: the only one that gets the info box page (`0x455b3b`).
    fn selected_building(&self) -> Option<usize> {
        self.sel_building.filter(|i| self.world.structures.get(*i).is_some_and(|b| b.alive()))
    }

    /// The building under the pointer, whoever owns it (`0x418da0` in `0x455ab0`); none while the pointer is on the HUD or outside.
    fn hovered_building(&self) -> Option<usize> {
        if !self.edge_scroll || self.hud_hit(self.cursor.0, self.cursor.1).is_some() {
            return None;
        }
        let (mx, my) = self.to_map(self.cursor.0, self.cursor.1);
        self.world.structure_at(mx, my).filter(|&i| self.world.structures[i].alive())
    }

    /// The building whose production slots are shown (`0x455ab0`): the selected own building, else the one under the pointer
    /// (an enemy's too: hovering shows its production without the info box page, which only the selected building gets).
    fn shown_building(&self) -> Option<usize> {
        self.selected_building().or_else(|| self.hovered_building())
    }

    fn radar_look(&mut self, x: i32, y: i32) {
        if let Some(l) = &self.hud {
            let (px, py) = l.radar_to_map(x, y);
            self.look_at(px, py);
        }
    }

    fn hud_click(&mut self, hit: Hit, x: i32, y: i32) {
        match hit {
            Hit::Panel => {}
            Hit::Radar => {
                self.radar_drag = true;
                self.radar_look(x, y);
            }
            Hit::Turret(tier) => {
                self.strike = None;
                self.rallying = None;
                if !self.world.turret_flags[0][tier as usize - 1] {
                    return;
                }
                let price = opensow_formats::production::turret_price(tier).unwrap_or(i32::MAX);
                self.placing = (self.placing != Some(tier) && self.world.credits[0] >= price).then_some(tier);
            }
            Hit::Air(force) => {
                self.placing = None;
                self.rallying = None;
                let i = opensow_formats::air::HEADER_ORDER.iter().position(|&f| f == force).unwrap_or(0);
                self.strike = (self.strike != Some(force) && self.world.air_stock[0][i] > 0).then_some(force);
            }
            Hit::Slot(_) => {}
            Hit::Rally => {
                self.strike = None;
                self.placing = None;
                let _ = self.arm_rally();
            }
            Hit::SlotBuy(i) => {
                if let Some(b) = self.shown_building() {
                    self.world.tech_up(b, i, 0);
                }
            }
        }
    }

    /// Turret keys 1-5 (`0x415d7c..0x415edd`): with the turret available and `200 * tier` credits the selection is dropped, the
    /// air-strike and rally modes end and the next map click places the turret; otherwise nothing happens.
    pub fn turret_key(&mut self, tier: u32) {
        if !self.world.turret_flags[0][tier as usize - 1] || self.world.credits[0] < 200 * tier as i32 {
            return;
        }
        self.world.clear_selection();
        self.sel_building = None;
        self.strike = None;
        self.rallying = None;
        self.placing = Some(tier);
    }

    /// Selects `i` alone (`0x43ad60`).
    fn select_only(&mut self, i: usize) {
        self.world.clear_selection();
        self.sel_building = None;
        self.world.units[i].selected = true;
    }

    /// Tab (`0x41622d`): steps round the player's units to the next live one, selects it alone and centres the view on it (`0x418150`).
    pub fn select_next_unit(&mut self) {
        let own: Vec<usize> = (0..self.world.units.len()).filter(|&i| self.world.units[i].player == 0).collect();
        for _ in 0..=own.len() {
            if own.is_empty() {
                return;
            }
            self.tab_next = (self.tab_next + 1) % own.len();
            let i = own[self.tab_next];
            if self.world.units[i].hp > 0 {
                self.select_only(i);
                self.centre_on_selection();
                return;
            }
        }
    }

    /// H and numpad + (`0x415f98`): the same round robin over the player's Advancers (kind 40); no camera move.
    pub fn select_next_advancer(&mut self) {
        let own: Vec<usize> = (0..self.world.units.len()).filter(|&i| self.world.units[i].player == 0 && self.world.units[i].kind == kinds::ADVANCER).collect();
        if own.is_empty() {
            return;
        }
        self.adv_next = (self.adv_next + 1) % own.len();
        let i = own[self.adv_next];
        if self.world.units[i].hp > 0 {
            self.select_only(i);
        }
    }

    /// Shift+H (not in the original): the same round robin as H, but only over the Advancers with nothing to do (no building
    /// order, not moving), and the view centres on the one picked, since an idle one is usually somewhere out of sight.
    pub fn select_next_idle_advancer(&mut self) {
        let idle: Vec<usize> = (0..self.world.units.len())
            .filter(|&i| self.world.units[i].player == 0 && self.world.units[i].kind == kinds::ADVANCER && self.world.units[i].is_idle())
            .collect();
        if idle.is_empty() {
            return;
        }
        self.idle_next = (self.idle_next + 1) % idle.len();
        self.select_only(idle[self.idle_next]);
        self.centre_on_selection();
    }

    /// A double click on one of our units (not in the original): every own unit of that kind inside the view is selected, the
    /// rest is not. Advancers and nuclear vehicles stay single-select (`World::is_single_select`), so they are left alone.
    pub fn select_visible_kind(&mut self, kind: i32) {
        if sim::World::is_single_select(kind) {
            return;
        }
        let (w, h) = self.viewport;
        let (x0, y0) = (self.view.x, self.view.y);
        let (x1, y1) = (x0 + w as f32 / self.view.zoom, y0 + h as f32 / self.view.zoom);
        self.sel_building = None;
        for u in self.world.units.iter_mut().filter(|u| u.player == 0) {
            u.selected = u.kind == kind && u.hp > 0 && u.x > x0 && u.x < x1 && u.y > y0 && u.y < y1;
        }
    }

    /// F3 / F4 (`0x415c8e` / `0x415cea` -> `0x402ec0` event 0x1a / 0x19, case `0x4031d9`): the message line "speed  1.5x", lang
    /// string 144 followed by `"  %.1fx"` of the new speed; no speech (the speech switch `0x403366` stops at event 24) and,
    /// like every message, nothing during the tutorial (`[0x70f2cc] != -1` skips the whole function).
    pub fn show_speed(&mut self, speed: f32) {
        if self.world.tutorial.is_some() {
            return;
        }
        let label = self.lang.as_ref().and_then(|l| l.get(SPEED_STRING - 1)).unwrap_or("speed");
        self.notes.push((self.world.tick, format!("{label}  {speed:.1}x")));
    }

    /// Numpad `*` (`all`), `/` (anti-air units only) and `-` (all others) (`0x4162b1`, `0x416369`, `0x416427`): every own unit strictly
    /// inside the view that is not an Advancer gets the selected flag, every other one loses it. `anti_air`: None = both kinds.
    pub fn select_visible(&mut self, anti_air: Option<bool>) {
        let (w, h) = self.viewport;
        let (x0, y0) = (self.view.x, self.view.y);
        let (x1, y1) = (x0 + w as f32 / self.view.zoom, y0 + h as f32 / self.view.zoom);
        self.sel_building = None;
        for u in self.world.units.iter_mut().filter(|u| u.player == 0) {
            let class = anti_air.is_none_or(|a| crate::sim::is_anti_air(u.kind) == a);
            u.selected = class && u.kind != kinds::ADVANCER && u.x > x0 && u.x < x1 && u.y > y0 && u.y < y1;
        }
    }

    /// Numpad Enter (`0x418150`): centres the view on the middle of the box round the selected units.
    pub fn centre_on_selection(&mut self) {
        let mut it = self.world.units.iter().filter(|u| u.selected);
        let Some(f) = it.next() else { return };
        let b = it.fold((f.x, f.y, f.x, f.y), |b, u| (b.0.min(u.x), b.1.min(u.y), b.2.max(u.x), b.3.max(u.y)));
        self.look_at((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
    }

    /// Backspace (`0x4161b9`): the view goes to the player's (last) headquarters (building kind 100); the selection stays.
    pub fn look_at_hq(&mut self) {
        let hq = self.world.structures.iter().rev().find(|s| s.owner == Owner::Player && s.kind_id == kinds::HEADQUARTERS && s.hp > 0).map(|s| s.center());
        if let Some((x, y)) = hq {
            self.look_at(x as f32, y as f32);
        }
    }

    /// Enter (`0x41612d`): opens the chat line.
    pub fn chat_open(&mut self) {
        self.chat.get_or_insert_with(String::new);
    }

    /// A typed character (`0x4284bf`: characters from 0x20 on, while the line is narrower than `cheats::MAX_LINE_WIDTH`).
    pub fn chat_type(&mut self, ch: char) {
        let full = self.chat_width.get() >= opensow_formats::cheats::MAX_LINE_WIDTH;
        if let Some(t) = self.chat.as_mut().filter(|_| !full && (' '..='~').contains(&ch)) {
            t.push(ch);
        }
    }

    /// Removes the last character of the chat line.
    pub fn chat_backspace(&mut self) {
        if let Some(t) = self.chat.as_mut() {
            t.pop();
        }
    }

    /// Tab (`0x426c90`, character 9): the line sent last replaces the text.
    pub fn chat_recall(&mut self) {
        if let Some(t) = self.chat.as_mut() {
            *t = self.last_chat.clone();
        }
    }

    /// Esc (`0x426d0d`, character 0x1b): the text is dropped and the line closes.
    pub fn chat_cancel(&mut self) {
        self.chat = None;
    }

    /// Enter on an open line: a cheat phrase takes effect (single player, `0x426d73`), any other text becomes a chat line of
    /// the local player; the line closes either way.
    pub fn chat_send(&mut self) {
        let Some(text) = self.chat.take().filter(|t| !t.is_empty()) else { return };
        self.last_chat = text.clone();
        match opensow_formats::cheats::parse(&text) {
            Some(opensow_formats::cheats::Cheat::DrawFps) => self.show_fps = true,
            Some(c) => {
                self.world.apply_cheat(c);
            }
            None => self.world.say_chat(&text),
        }
    }

    /// Space / numpad 0 (`0x416177`): jumps to where the last message event happened, while a message line is shown.
    pub fn look_at_event(&mut self) {
        if let Some((x, y)) = self.world.event_position() {
            self.look_at(x as f32, y as f32);
        }
    }

    /// Numpad 1-9 (`0x416564..0x416674`): the view jumps to the bookmark; with a shift key held (`0x4166ac..0x416763`) the bookmark
    /// is set to the middle of the current view instead.
    pub fn bookmark(&mut self, n: usize, set: bool) {
        if set {
            let (w, h) = self.viewport;
            self.bookmarks[n] = Some((self.view.x + w as f32 / self.view.zoom / 2.0, self.view.y + h as f32 / self.view.zoom / 2.0));
        } else if let Some((x, y)) = self.bookmarks[n] {
            self.look_at(x, y);
        }
    }

    /// The rally button or the R key (`0x416cf9`, `0x416769`): the selected own factory becomes the target of the next map click.
    /// Returns whether the mode is on (false: no own factory is selected).
    pub fn arm_rally(&mut self) -> bool {
        self.rallying = self
            .shown_building()
            .filter(|&b| self.world.structures[b].owner == Owner::Player && opensow_formats::rally::has_rally(self.world.structures[b].kind_id));
        if self.rallying.is_some() {
            self.strike = None;
            self.placing = None;
        }
        self.rallying.is_some()
    }

    /// The left button was released: a click or the end of a selection box; `additive` adds to the selection.
    pub fn left_up(&mut self, x: i32, y: i32, additive: bool) {
        self.radar_drag = false;
        let Some((sx, sy)) = self.drag.take() else { return };
        if (x - sx).abs() > 16 || (y - sy).abs() > 16 {
            let (a, b) = (self.to_map(sx, sy), self.to_map(x, y));
            self.sel_building = None;
            self.world.select_rect(a.0, a.1, b.0, b.1, additive);
            if self.world.units.iter().any(|u| u.selected) {
                self.ui_sounds.push(60); // unit-select
            }
        } else {
            let p = self.to_map(x, y);
            if let Some((kind, side)) = self.dev_spawn {
                if crate::devtools::is_structure(kind) {
                    // turrets and buildings: the clicked cell is the footprint's top-left cell (the mission's own placement shows it likewise)
                    self.world.dev_place_structure(kind, side, (p.0.div_euclid(CELL), p.1.div_euclid(CELL)));
                } else if self.world.add_unit_near(kind, side as i32, p.0, p.1).is_some() {
                    self.ui_sounds.push(60);
                }
                return;
            }
            if let Some(force) = self.strike {
                // `0x417028`: a click on an air-blocked cell launches nothing and the mode stays armed
                if !self.world.is_air_blocked(p.0.div_euclid(CELL), p.1.div_euclid(CELL)) {
                    self.strike = None;
                    self.world.launch_strike(0, force, p.0, p.1);
                }
                return;
            }
            if let Some(tier) = self.placing.take() {
                self.world.place_turret(0, tier, (p.0.div_euclid(CELL), p.1.div_euclid(CELL)));
                return;
            }
            if let Some(b) = self.rallying {
                // `0x417414`: only a free ground cell takes the point (the walkers' grid `0x6c7a4c`); the mode stays on otherwise
                if !self.world.is_blocked(p.0.div_euclid(CELL), p.1.div_euclid(CELL)) {
                    self.rallying = None;
                    if self.world.set_rally(b, 0, p.0, p.1) {
                        let (cx, cy) = opensow_formats::rally::snap(p.0, p.1);
                        self.world.effects.push(sim::Effect {
                            x: cx,
                            y: cy,
                            spec: opensow_formats::effects::AnimSpec {
                                array: opensow_formats::rally::CLICK_ARRAY,
                                from: 0,
                                to: opensow_formats::rally::CLICK_LAST_FRAME,
                                period: opensow_formats::rally::CLICK_PERIOD,
                                repeat: 1,
                                mode: 6,
                            },
                            age: 0,
                        });
                        self.ui_sounds.push(opensow_formats::rally::CLICK_SOUND);
                    }
                }
                return;
            }
            // The left button is the command button (exe `0x417686..0x417e47`, docs/controls.md): what a click does is what
            // the pointer shows at the release. With the buttons swapped the orders go on the right button and this click does
            // the original right click instead; selecting stays here.
            if self.orders_on_right && self.order_pointer() {
                self.cancel_click(p.0, p.1);
                return;
            }
            if self.give_order(p) {
                return;
            }
            let selected = self.world.units.iter().any(|u| u.selected && u.player == 0);
            match self.pointer() {
                Pointer::Own => {
                    let unit_there = self.world.units.iter().any(|u| u.hp > 0 && u.player == 0 && (u.x - p.0 as f32).hypot(u.sprite_y() - p.1 as f32) < 22.0);
                    self.sel_building = None;
                    if unit_there {
                        // select it alone; with shift it is added or toggled (`0x417a25..0x417a76`)
                        self.world.select_at(p.0, p.1, 22.0, additive);
                        if self.world.units.iter().any(|u| u.selected) {
                            self.ui_sounds.push(60);
                        }
                        // a second click on the same kind soon after: all of that kind on screen (not in the original)
                        let kind = self.world.player_units().filter(|u| u.hp > 0).min_by(|a, b| {
                            let d = |u: &sim::Unit| (u.x - p.0 as f32).hypot(u.sprite_y() - p.1 as f32);
                            d(a).total_cmp(&d(b))
                        });
                        let kind = kind.map(|u| u.kind);
                        let now = std::time::Instant::now();
                        match (self.last_unit_click.take(), kind) {
                            (Some((t, k)), Some(kind)) if !additive && k == kind && now.duration_since(t) <= DOUBLE_CLICK => self.select_visible_kind(kind),
                            (_, Some(kind)) => self.last_unit_click = Some((now, kind)),
                            _ => {}
                        }
                    } else if let Some(t) = self.world.structure_at(p.0, p.1) {
                        self.world.clear_selection();
                        self.sel_building = Some(t);
                        self.ui_sounds.push(BUILDING_CLICK_SOUND);
                    }
                }
                _ => {
                    // nothing selected: a click on an own building shows its info box and slots; only the own building gets the
                    // selected flag (`0x417b9a`: `[b+0x8c] == player`), an enemy's slots show on hover alone
                    if !selected {
                        let clicked = self.world.structure_by_art(p.0, p.1);
                        if clicked.is_some() {
                            self.ui_sounds.push(BUILDING_CLICK_SOUND); // any building, own or not (not in the exe)
                        }
                        self.sel_building = clicked.filter(|&t| self.world.structures[t].owner == Owner::Player);
                    }
                }
            }
        }
    }

    /// The pointer gives an order on a click: the Advancer's three, attack, or the go-to with a selection.
    fn order_pointer(&self) -> bool {
        let selected = self.world.units.iter().any(|u| u.selected && u.player == 0);
        match self.pointer() {
            Pointer::Destroy | Pointer::Repair | Pointer::Upgrade | Pointer::Attack => true,
            Pointer::Move => selected,
            _ => false,
        }
    }

    /// The order a click at map point `p` gives by the pointer (exe `0x417686..0x417e47`); `false` when the pointer gives none.
    fn give_order(&mut self, p: (i32, i32)) -> bool {
        if !self.order_pointer() {
            return false;
        }
        match self.pointer() {
            Pointer::Destroy | Pointer::Repair | Pointer::Upgrade => {
                // the Advancer's order (`0x46b8b0` / `0x46b9f0` / `0x46bb30`)
                if let Some(t) = self.world.structure_at(p.0, p.1) {
                    let advancers: Vec<u32> = self.world.player_units().filter(|u| u.selected && u.kind == kinds::ADVANCER).map(|u| u.id).collect();
                    let mut ordered = false;
                    for id in advancers {
                        ordered |= self.world.order_action(id, t);
                    }
                    if ordered {
                        self.ui_sounds.push(55); // tt-dest
                        self.world.blink(t); // `0x4086a0`: the target building flashes
                    }
                }
            }
            Pointer::Attack => {
                if let Some(t) = self.world.enemy_at(p.0, p.1) {
                    if let Target::Structure(k) = t {
                        self.world.blink(k);
                    }
                    self.world.order_attack_selected(t);
                    self.ui_sounds.push(59); // unit-attack
                }
            }
            Pointer::Move => {
                // the group go-to (`0x418220`, only with the move pointer)
                self.world.order_move_selected(p.0, p.1);
                self.ui_sounds.push(61); // unit-dest
                                         // the click marker (`0x418457`): `click` frames 0..8, 3 ticks each, at the clicked cell's centre
                let (cx, cy) = (p.0.div_euclid(CELL) * CELL + CELL / 2, p.1.div_euclid(CELL) * CELL + CELL / 2);
                self.world.effects.push(sim::Effect {
                    x: cx,
                    y: cy,
                    spec: opensow_formats::effects::AnimSpec { array: 0x5e6140, from: 0, to: 8, period: 3, repeat: 1, mode: 6 },
                    age: 0,
                });
            }
            _ => {}
        }
        true
    }

    /// The sounds of selecting the whole army (the group select `0x43ad60`): `tt-click` (54) for every Advancer, `unit-select` (60) once if any other unit joined.
    pub fn select_all_sounds(&self) -> Vec<u8> {
        let mut v = Vec::new();
        let mut other = false;
        for u in self.world.units.iter().filter(|u| u.selected && u.player == 0) {
            if u.kind == kinds::ADVANCER {
                v.push(54)
            } else {
                other = true
            }
        }
        if other {
            v.push(60)
        }
        v
    }

    /// Right button pressed (`0x416964..0x4169c3`): a click unless the mouse then moves more than 4 px, which scrolls the map.
    pub fn right_down(&mut self) {
        self.right = Some((0, 0, false));
    }

    /// Mouse movement with the right button held scrolls the map by the movement (`0x416bac`: `0x457f80(dx, dy)`), the
    /// pointer stays where it was.
    pub fn right_drag(&mut self, dx: i32, dy: i32) {
        let Some((mx, my, _)) = self.right else { return };
        let (mx, my) = (mx + dx.abs(), my + dy.abs());
        let panning = mx > 4 || my > 4 || self.right.is_some_and(|r| r.2);
        self.right = Some((mx, my, panning));
        if panning {
            let (w, h) = self.viewport;
            self.view.x += dx as f32 / self.view.zoom;
            self.view.y += dy as f32 / self.view.zoom;
            self.view.clamp(&self.scene, w, h);
        }
    }

    /// Whether the right button is held.
    pub fn right_down_now(&self) -> bool {
        self.right.is_some()
    }

    /// Right button released: without a drag it cancels (`0x416a17`, `0x417934`): placement and air-strike modes end; with an
    /// Advancer selected a click on an own building toggles repair / upgrade (`0x417964`); anywhere else it deselects
    /// everything (`0x43ad60`). It never gives an order.
    pub fn right_up(&mut self, x: i32, y: i32) {
        let Some((_, _, panning)) = self.right.take() else { return };
        self.cursor = (x, y);
        if panning {
            return;
        }
        if self.hud_hit(x, y).is_some() {
            // a right click on the radar toggles the black radar (`0x416e94`: `[0x5baad8] ^= 1` on the release edge)
            if self.hud_hit(x, y) == Some(Hit::Radar) {
                self.radar_black = !self.radar_black;
            }
            self.placing = None;
            self.strike = None;
            self.rallying = None;
            self.dev_spawn = None;
            return;
        }
        let (mx, my) = self.to_map(x, y);
        if self.placing.take().is_some() | self.strike.take().is_some() | self.rallying.take().is_some() | self.dev_spawn.take().is_some() {
            return;
        }
        // swapped buttons: the right click gives the order the left one gives in the original
        if self.orders_on_right && self.give_order((mx, my)) {
            return;
        }
        self.cancel_click(mx, my);
    }

    /// The original right click on the map at map point `(mx, my)` outside the placement modes (`0x417934`): with an Advancer
    /// selected a click on an own building toggles repair mode (`0x417964`), anything else deselects (`0x43ad60`).
    fn cancel_click(&mut self, mx: i32, my: i32) {
        if self.world.player_units().any(|u| u.selected && u.kind == kinds::ADVANCER)
            && self.world.structure_at(mx, my).is_some_and(|t| self.world.structures[t].owner == Owner::Player)
        {
            self.world.repair_mode ^= true;
            return;
        }
        self.world.clear_selection();
        self.sel_building = None;
    }

    /// The pointer type the original shows at the mouse position (docs/cursor.md, exe `0x416940..0x417e84`).
    pub fn pointer(&self) -> Pointer {
        let (x, y) = self.cursor;
        let (w, h) = self.viewport;
        let on_panel = self.hud_hit(x, y).is_some();
        // screen edges: scroll arrows
        let edge = 6;
        let dx = if x < edge {
            -1
        } else if x >= w as i32 - edge && !on_panel {
            1
        } else {
            0
        };
        let dy = if y < edge {
            -1
        } else if y >= h as i32 - edge {
            1
        } else {
            0
        };
        if let Some(p) = scroll_pointer(dx, dy).filter(|_| self.edge_scroll) {
            return p;
        }
        if on_panel {
            return Pointer::Arrow;
        }
        let (mx, my) = self.to_map(x, y);
        // air-strike mode (`0x417257..0x41727b`): the blocked arrow over cells the air layer `0x60ac38` marks 0xfffffd, else type 10
        if self.strike.is_some() {
            return if self.world.is_air_blocked(mx.div_euclid(CELL), my.div_euclid(CELL)) { Pointer::Blocked } else { Pointer::AirStrike };
        }
        // rally point mode (`0x41763b`): the placement pointer over free ground, the blocked arrow elsewhere
        if self.rallying.is_some() {
            return if self.world.is_blocked(mx.div_euclid(CELL), my.div_euclid(CELL)) { Pointer::Blocked } else { Pointer::Rally };
        }
        let advancer = self.world.player_units().any(|u| u.selected && u.kind == kinds::ADVANCER);
        let selected = self.world.units.iter().any(|u| u.selected && u.player == 0);
        // an own unit under the pointer
        if self.world.units.iter().any(|u| u.hp > 0 && u.player == 0 && (u.x - mx as f32).hypot(u.sprite_y() - my as f32) < 22.0) {
            return Pointer::Own;
        }
        if let Some(t @ Target::Unit(_)) = self.world.enemy_at(mx, my).filter(|t| matches!(t, Target::Unit(_))) {
            // `0x417a8c`: the Advancer only gets the attack pointer over flyers (enemy Advancers) and anti-air units
            return if advancer && !self.world.advancer_can_attack(t) {
                Pointer::Move
            } else if selected {
                Pointer::Attack
            } else {
                Pointer::Arrow
            };
        }
        if let Some(t) = self.world.structure_at(mx, my) {
            if advancer {
                return self.world.advancer_pointer(0, t);
            }
            return if self.world.structures[t].owner == Owner::Player {
                Pointer::Own
            } else if selected {
                Pointer::Attack
            } else {
                Pointer::Arrow
            };
        }
        // open ground: blocked cells only matter for walkers (the Advancer flies, `0x6d7a4c` instead of `0x6c7a4c`)
        if !advancer && self.world.is_blocked(mx.div_euclid(CELL), my.div_euclid(CELL)) {
            Pointer::Blocked
        } else if selected {
            Pointer::Move
        } else {
            Pointer::Arrow
        }
    }

    /// Draws the pointer sprite (`cursor.ps6`, frame `8 * type + phase`) at the mouse position, scaled like the HUD.
    pub fn draw_pointer(&self, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize, ui: f32) {
        let Some(set) = assets.art.load(opensow_formats::cursor::SPRITE) else { return };
        let p = self.pointer();
        let Some(f) = set.get(p.frame(opensow_formats::cursor::phase_at(self.world.tick))) else { return };
        let (ox, oy) = p.offset();
        let x = self.cursor.0 - ((ox + f.hot_x as i32) as f32 * ui).round() as i32;
        let y = self.cursor.1 - ((oy + f.hot_y as i32) as f32 * ui).round() as i32;
        ui::sprite(buf, w, h, f, x, y, ui);
    }

    /// The pointer as an image for the system's cursor (so the compositor moves it at the mouse's own rate instead of once per
    /// frame): `(key, hotspot, width, height, RGBA)` of the same `cursor.ps6` frame [`draw_pointer`] draws, scaled by `ui`. `key`
    /// changes whenever the picture does.
    pub fn pointer_cursor(&self, assets: &mut Assets, ui: f32) -> Option<PointerPicture> {
        let set = assets.art.load(opensow_formats::cursor::SPRITE)?;
        let p = self.pointer();
        let index = p.frame(opensow_formats::cursor::phase_at(self.world.tick));
        let f = set.get(index)?;
        let (ox, oy) = p.offset();
        let (dw, dh) = (((f.width as f32 * ui).round() as u32).max(1), ((f.height as f32 * ui).round() as u32).max(1));
        let mut rgba = vec![0u8; (dw * dh * 4) as usize];
        for y in 0..dh {
            let sy = ((y as f32 / ui) as usize).min(f.height as usize - 1);
            for x in 0..dw {
                let sx = ((x as f32 / ui) as usize).min(f.width as usize - 1);
                let i = sy * f.width as usize + sx;
                if f.mask[i] {
                    let [r, g, b] = opensow_formats::sprite::rgb565_to_rgb8(f.pixels[i]);
                    let o = ((y * dw + x) * 4) as usize;
                    rgba[o..o + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
        }
        let hot = (
            (((ox + f.hot_x as i32) as f32 * ui).round() as i32).clamp(0, dw as i32 - 1),
            (((oy + f.hot_y as i32) as f32 * ui).round() as i32).clamp(0, dh as i32 - 1),
        );
        Some(((index, (ui * 100.0) as u32), hot, dw, dh, rgba))
    }

    /// Zoom never goes out far enough to show anything beyond the map (no black bars).
    /// The zoom range: from filling the window with the map (never smaller, at least 0.3) to 3.
    fn zoom_range(&self, w: usize, h: usize) -> (f32, f32) {
        let fill = (w as f32 / self.scene.bg_w as f32).max(h as f32 / self.scene.bg_h as f32);
        (fill.max(0.3), 3.0)
    }

    /// Keeps the zoom inside the range allowed for this window size.
    pub fn limit_zoom(&mut self, w: usize, h: usize) {
        let (lo, hi) = self.zoom_range(w, h);
        self.view.zoom = self.view.zoom.clamp(lo, hi);
    }

    /// The wheel: 8 % a notch (a trackpad's small deltas by their share), eased in by `ease_zoom` around the pointer.
    pub fn wheel(&mut self, lines: f32, w: usize, h: usize) {
        if !self.zoom_on {
            return;
        }
        let (lo, hi) = self.zoom_range(w, h);
        let from = self.zoom_to.map_or(self.view.zoom, |(z, _)| z);
        let target = (from * ZOOM_STEP.powf(lines.clamp(-4.0, 4.0))).clamp(lo, hi);
        self.zoom_to = Some((target, self.cursor));
    }

    /// With "Map zoom" off: the original scale 1 (or the least zoom that still fills the window with the map), keeping the map
    /// point in the middle of the window where it is.
    fn fixed_scale(&mut self, w: usize, h: usize) {
        self.zoom_to = None;
        let (lo, hi) = self.zoom_range(w, h);
        let target = 1.0f32.clamp(lo, hi);
        if self.view.zoom != target {
            let (cx, cy) = (self.view.x + w as f32 / self.view.zoom / 2.0, self.view.y + h as f32 / self.view.zoom / 2.0);
            self.view.zoom = target;
            self.view.x = cx - w as f32 / target / 2.0;
            self.view.y = cy - h as f32 / target / 2.0;
            self.view.clamp(&self.scene, w, h);
        }
    }

    /// A share of the way to the wheel's zoom each frame (about 90 % in a fifth of a second); the map point under the pointer
    /// where the wheel turned stays put.
    fn ease_zoom(&mut self, dt: f32, w: usize, h: usize) {
        let Some((target, (ax, ay))) = self.zoom_to else { return };
        let (mx, my) = (self.view.x + ax as f32 / self.view.zoom, self.view.y + ay as f32 / self.view.zoom);
        let z = self.view.zoom + (target - self.view.zoom) * (1.0 - (-dt * ZOOM_EASE).exp());
        self.view.zoom = if (target - z).abs() < 0.0005 { target } else { z };
        if self.view.zoom == target {
            self.zoom_to = None;
        }
        self.limit_zoom(w, h);
        self.view.x = mx - ax as f32 / self.view.zoom;
        self.view.y = my - ay as f32 / self.view.zoom;
        self.view.clamp(&self.scene, w, h);
    }

    /// Advances real time by `dt` seconds: scrolls the camera and runs as many game ticks as are due.
    pub fn update(&mut self, dt: f32, settings: &Settings, w: usize, h: usize) {
        self.turret_range = settings.turret_range;
        self.orders_on_right = settings.swap_buttons;
        self.zoom_on = settings.zoom;
        if !self.zoom_on {
            self.fixed_scale(w, h);
        }
        self.ease_zoom(dt, w, h);
        self.limit_zoom(w, h);
        if self.fit {
            self.centre_on_player(w, h);
            self.fit = false;
        }
        self.viewport = (w, h);
        if self.radar_drag {
            let (cx, cy) = self.cursor;
            self.radar_look(cx, cy);
        }
        let hover = self.hud_hit(self.cursor.0, self.cursor.1).filter(|h| !matches!(h, Hit::Panel));
        if hover != self.hud_hover {
            if hover.is_some_and(|h| h != Hit::Radar) {
                self.ui_sounds.push(50); // icon-move
            }
            self.hud_hover = hover;
        }
        // the info box shows the hovered icon, else the shown building (`0x454d30` -> `0x43cde0`, `0x455b3b`)
        // only a production slot icon opens it: the hover loops of the turret tabs (`0x454b56`) and the air pills (`0x454cf0`) set just
        // the hovered id `[hud+4]` and leave the slot index `[ebp-0x10]` at -1, which `0x43cde0` answers by hiding the box
        let hovered_kind = match hover {
            Some(Hit::Slot(i) | Hit::SlotBuy(i)) => {
                self.shown_building().and_then(|b| self.world.structures.get(b)).and_then(|b| b.slots.get(i)).map(|sl| sl.kind)
            }
            _ => None,
        };
        let selected_kind = self.selected_building().and_then(|b| self.world.structures.get(b)).filter(|b| b.alive()).map(|b| b.kind_id);
        let locked = matches!(hover, Some(Hit::Slot(i) | Hit::SlotBuy(i)) if self.shown_building().and_then(|b| self.world.structures.get(b)).is_some_and(|b| i >= b.active));
        self.infobox.show(hovered_kind.or(selected_kind), locked);
        // the map moves `2 * ticks * speed` px per step (`0x457f80` doubles the delta; the speed is the options menu's KEYS
        // SCROLL `[0x6bf728]` for the arrow keys and MOUSE SCROLL `[0x6bf724]` at the window edge; 60 ticks/s)
        let (kspeed, mspeed) = (
            crate::options::scroll_px_per_second(settings.scroll_keys) * dt / self.view.zoom,
            crate::options::scroll_px_per_second(settings.scroll_mouse) * dt / self.view.zoom,
        );
        let (mut dx, mut dy) = (0.0, 0.0);
        let edge = 6;
        let at = self.edge_scroll;
        let on_panel = self.hud_hit(self.cursor.0, self.cursor.1).is_some();
        if self.keys.contains("left") {
            dx -= kspeed;
        }
        if self.keys.contains("right") {
            dx += kspeed;
        }
        if self.keys.contains("up") {
            dy -= kspeed;
        }
        if self.keys.contains("down") {
            dy += kspeed;
        }
        if at && self.cursor.0 < edge {
            dx -= mspeed;
        }
        if at && self.cursor.0 >= w as i32 - edge && !on_panel {
            dx += mspeed;
        }
        if at && self.cursor.1 < edge {
            dy -= mspeed;
        }
        if at && self.cursor.1 >= h as i32 - edge {
            dy += mspeed;
        }
        if dx != 0.0 || dy != 0.0 {
            self.view.x += dx;
            self.view.y += dy;
        }
        self.view.clamp(&self.scene, w, h);
        // what the tutorial script reads from the input (`0x6bed20`, `0x6bee34..3c`, `0x6beef0`)
        let (scrolling, strike, keys) = (
            self.right.is_some_and(|r| r.2),
            self.strike.is_some(),
            ["left", "right", "up", "down"].map(|k| self.keys.contains(k) || (self.edge_scroll && false)),
        );
        if let Some(t) = self.world.tutorial.as_mut() {
            t.rmb_scroll = scrolling;
            t.air_icon = strike;
            for (flag, key) in t.keys.iter_mut().zip(keys) {
                *flag |= key;
            }
        }
        if self.paused || self.world.outcome.is_some() {
            return;
        }
        self.time += dt;
        let tick = 1.0 / settings.ticks_per_second as f32;
        let mut n = 0;
        // the dev tools' speed-up needs more steps per frame (8 at the normal 60 ticks/s)
        let cap = (settings.ticks_per_second as usize * 8 / 60).max(8);
        while self.time >= tick && n < cap {
            self.time -= tick;
            self.world.step();
            self.world.sky_step(settings.cloud_detail);
            // the screen shake nudges the camera by twice the step per tick (`0x458930` -> `0x457f80`)
            let (vw, vh) = ((w as f32 / self.view.zoom) as i32, (h as f32 / self.view.zoom) as i32);
            if let Some((dx, dy)) = self.world.quake_step(settings.quake_on, (self.view.x as i32, self.view.y as i32), (vw, vh)) {
                self.view.x += 2.0 * dx as f32;
                self.view.y += 2.0 * dy as f32;
                self.view.clamp(&self.scene, w, h);
            }
            self.take_tutorial_events();
            let (total, desc) = self.info_text.as_ref().map_or((0, Vec::new()), |(_, l, d)| (infobox::total_chars(l), d.clone()));
            self.infobox.tick(self.info_text.as_ref().map(|t| &t.1), total, &desc);
            n += 1;
        }
        if n == cap {
            self.time = 0.0;
        }
    }

    // ---- drawing ------------------------------------------------------------------------------------------------

    fn sprites_for(&mut self, assets: &mut Assets, kind: i32, player: i32) -> Option<&UnitSprites> {
        let key = (kind, player == 0);
        if let std::collections::hash_map::Entry::Vacant(e) = self.sprites.entry(key) {
            let art = unit_art(kind)?;
            let team = if player == 0 { "blue" } else { "gren" };
            let mut load = |name: &str, ext: &str| assets.art.load(&format!("units\\{name}_{team}.{ext}"));
            let placeholder = |s: &Set| s[0].width <= 16 && s[0].height <= 2;
            let top = art.top.and_then(|t| load(t, "ps6")).filter(|s| !placeholder(s));
            let top_shadow = art.top.and_then(|t| load(t, "sha"));
            let flash = opensow_formats::recoil::fire_sequence(kind)
                .and_then(|s| s.flash_set)
                .and_then(|f| assets.art.load(&format!("units\\{}_fire.ps6", FLASH_SETS[f as usize])));
            let mut load = |name: &str, ext: &str| assets.art.load(&format!("units\\{name}_{team}.{ext}"));
            e.insert(UnitSprites { chassis: load(art.chassis, "ps6"), chassis_shadow: load(art.chassis, "sha"), top, top_shadow, flash });
        }
        self.sprites.get(&key)
    }

    /// Draws the mission: the world, effects, the HUD and whatever is open on top.
    pub fn draw(&mut self, assets: &mut Assets, settings: &Settings, buf: &mut [u32], w: usize, h: usize, _ui_scale: f32) {
        self.limit_zoom(w, h);
        self.viewport = (w, h);
        self.fps.1 += 1;
        if self.fps.0.elapsed().as_secs_f32() >= 1.0 {
            self.fps = (std::time::Instant::now(), 0, self.fps.1);
        }
        if self.fit {
            self.centre_on_player(w, h);
            self.fit = false;
        }
        // structures made during the game (placed turrets, `object == usize::MAX`) get a scene object, so they have art like the level's
        let fresh: Vec<usize> =
            (0..self.world.structures.len()).filter(|&i| self.world.structures[i].object == usize::MAX && self.world.structures[i].alive()).collect();
        if !fresh.is_empty() {
            for i in fresh {
                let st = &self.world.structures[i];
                let mut raw = [0i32; 19];
                raw[14] = if st.owner == Owner::Player { 0 } else { 1 };
                raw[13] = st.hp;
                let obj = opensow_formats::level::Object { kind: st.kind_id, cell_x: st.cell.0, cell_y: st.cell.1, raw };
                let Some(level) = self.scene.level.as_mut() else { break };
                let index = level.objects.len();
                level.objects.push(obj.clone());
                self.scene.placed.extend(assets.art.place(index, &obj));
                self.world.structures[i].object = index;
            }
            self.scene.placed.sort_by_key(|p| (p.sort_y, p.layer));
        }
        // destroyed buildings and turrets disappear when their destruction animation ends (they stay, dead, while it runs: `Structure::dying`)
        // a turret's gun top (unit kinds 109-113) is freed the moment it dies, so a turret vanishes with its first explosion
        let dead: HashSet<usize> =
            self.world.structures.iter().filter(|s| !s.present() || (s.dying.is_some() && matches!(s.kind, ObjectKind::Turret(_)))).map(|s| s.object).collect();
        if !dead.is_empty() {
            self.scene.placed.retain(|p| !matches!(p.source, Source::Object(i) if dead.contains(&i)));
        }
        // idle loops of working buildings, damaged look at half hit points (`0x40dfb6`, `0x40e099`)
        let tick = self.world.tick as u32;
        for st in &self.world.structures {
            if let Some(p) = self.scene.placed.iter_mut().find(|p| matches!(p.source, Source::Object(i) if i == st.object) && p.layer == 0) {
                p.set_building_state(st.owner, st.hp, st.max_hp, st.heal, tick);
            }
            // a turret's gun top turns and recoils like a unit's top
            if let (Some(aim), Some(p)) =
                (&st.aim, self.scene.placed.iter_mut().find(|p| matches!(p.source, Source::Object(i) if i == st.object) && p.layer == 1))
            {
                let steps = aim.params.steps;
                let frame = match aim.firing() {
                    Some((seq, t)) => seq.top_frame(t, aim.facing, steps) as usize,
                    None => aim.facing as usize,
                };
                p.frame = frame.min(p.set.len() - 1);
            }
        }
        let overlays = Overlays {
            blocked: false,
            grid: false,
            objects: false,
            markers: false,
            sprites: true,
            muzzles: false,
            walk: None,
            fire: None,
            foreground: false,
            minimap: false,
        };
        // the world (ground, buildings, units, effects) may use the enhanced art above zoom 1; the HUD below keeps the exe's pixels
        opensow_viewer::upscale::set_world(true);
        render(&self.scene, &self.view, overlays, buf, w, h);
        for d in &self.world.decals {
            if let Some((set, i)) = Self::extra_frame(assets, opensow_formats::effects::DECAL_ARRAY, d.frame as usize) {
                blit_blend(buf, w, h, &self.view, &set[i], d.x, d.y, Blit::Darken);
            }
        }

        // turret muzzle flashes (`units\\{tur_01,tur_02}_fire`), drawn at the top's anchor like a unit's
        for st in self.world.structures.iter().filter(|s| s.alive()) {
            let Some(aim) = &st.aim else { continue };
            let Some((seq, t)) = aim.firing() else { continue };
            let Some(fi) = seq.flash_frame(t, aim.facing, aim.params.steps) else { continue };
            let Some(set) = seq.flash_set.and_then(|f| assets.art.load(&format!("units\\{}_fire.ps6", FLASH_SETS[f as usize]))) else { continue };
            let Some(pl) = self.scene.placed.iter().find(|p| matches!(p.source, Source::Object(i) if i == st.object) && p.layer == 1) else { continue };
            if let Some(fr) = set.get(fi as usize) {
                blit_blend(buf, w, h, &self.view, fr, pl.ax, pl.ay, Blit::Additive);
            }
        }

        if settings.show_range {
            self.draw_ranges(buf, w, h);
        }
        // buildings flashing after an order: their own sprite again, half-additive (`0x409467`)
        for (&t, b) in &self.world.blinks {
            if !b.on {
                continue;
            }
            let Some(st) = self.world.structures.get(t) else { continue };
            if let Some(pl) = self.scene.placed.iter().find(|p| matches!(p.source, Source::Object(i) if i == st.object) && p.layer == 0) {
                blit_blend(buf, w, h, &self.view, &pl.set[pl.frame.min(pl.set.len() - 1)], pl.ax, pl.ay, Blit::AddHalf);
            }
        }
        // the selected factory's rally point (`0x409400`, in the building pass): `rallypoint` frame `tick / 3 % 16`, additive
        if let Some((rx, ry)) = self.shown_building().and_then(|b| self.world.structures.get(b)).filter(|b| b.owner == Owner::Player).and_then(|b| b.rally) {
            if let Some((set, i)) = Self::extra_frame(assets, 0x5eab00, opensow_formats::rally::marker_frame(self.world.tick)) {
                blit_blend(buf, w, h, &self.view, &set[i], rx, ry, Blit::Additive);
            }
        }
        // bullets and effects are drawn inside `draw_units`, between the ground units and the hovering Advancers
        self.draw_aircraft_shadows(assets, buf, w, h);
        self.draw_units(assets, settings, buf, w, h);
        self.draw_aircraft(assets, buf, w, h);
        // the clouds themselves: additive (`0x414700`), over everything in the world and under the HUD (after the aircraft and bullets)
        for c in &self.world.sky.clouds {
            if let Some((set, i)) = Self::extra_frame(assets, opensow_formats::clouds::ARRAY, c.picture as usize) {
                let (x, y) = c.body_pos();
                blit_blend(buf, w, h, &self.view, &set[i], x, y, Blit::Additive);
            }
        }
        opensow_viewer::upscale::set_world(false);
        if self.sel_building.is_some_and(|i| !self.world.structures.get(i).is_some_and(|b| b.alive())) {
            self.sel_building = None;
        }
        // the HUD keeps the original's proportions (designed for 1024x768) at any window size
        let hs = (h as f32 / 768.0).max(1.0);
        self.sync_pills();
        let gold = self.world.structures.iter().any(|b| b.alive() && b.kind == opensow_formats::level::ObjectKind::GoldMine);
        let lab = self.world.structures.iter().any(|b| b.alive() && b.kind == opensow_formats::level::ObjectKind::ResearchLab);
        let inputs = Inputs {
            mission: self.world.mission_no,
            warmonger: assets.game.edition == opensow_formats::Edition::Warmonger,
            map_cells: self.world.cells,
            show_credits: gold || self.world.credits.iter().any(|&c| c > 0),
            show_research: lab || self.world.research.iter().any(|&c| c > 0),
            tiers: (1..=5u32).filter(|&t| self.world.turret_flags[0][t as usize - 1]).collect(),
            pills: self.pills.clone(),
            info: self.infobox.rect(w as i32, hs),
            slots: self.shown_building().and_then(|i| self.world.structures.get(i)).map_or(0, |b| b.slots.len().min(5)),
            buy: self
                .shown_building()
                .and_then(|i| self.world.structures.get(i))
                .map_or(Vec::new(), |b| b.slots.iter().take(5).map(|sl| sl.level < sl.max_level).collect()),
            rally_button: self
                .shown_building()
                .and_then(|i| self.world.structures.get(i))
                .is_some_and(|b| b.owner == Owner::Player && opensow_formats::rally::has_rally(b.kind_id)),
        };
        let mut layout = Layout::new(w, h, hs, &inputs);
        layout.radar_on = settings.radar_on;
        layout.pills_locked = self.world.strike_lock > 0;
        self.hud = Some(layout);
        self.prepare_info(assets);
        self.panel_ui = hs;
        self.draw_hud(assets, buf, w, h, hs);

        if let Some((sx, sy)) = self.drag {
            let (x0, y0, x1, y1) = (sx.min(self.cursor.0), sy.min(self.cursor.1), sx.max(self.cursor.0), sy.max(self.cursor.1));
            if x1 - x0 > 16 || y1 - y0 > 16 {
                rect(buf, w, h, x0, y0, x1, y1, (80, 255, 120), 40);
                for (a, b, c, d) in [(x0, y0, x1, y0 + 1), (x0, y1 - 1, x1, y1), (x0, y0, x0 + 1, y1), (x1 - 1, y0, x1, y1)] {
                    rect(buf, w, h, a, b, c, d, (120, 255, 150), 255);
                }
            }
        }
        if self.edge_scroll && !self.system_pointer {
            self.draw_pointer(assets, buf, w, h, hs);
        }
    }

    fn draw_ranges(&self, buf: &mut [u32], w: usize, h: usize) {
        for u in self.world.units.iter().filter(|u| u.selected && u.player == 0) {
            let Some((min, max)) = sim::weapon_range(u.kind) else { continue };
            if sim::is_anti_air(u.kind) {
                continue;
            }
            let (cx, cy) = u.cell();
            for &(dx, dy) in self.rings.target_search(min as usize, max as usize) {
                let (x, y) = (cx + dx as i32, cy + dy as i32);
                let (x0, y0) = self.view.to_screen((x * CELL) as f32, (y * CELL) as f32);
                let (x1, y1) = self.view.to_screen(((x + 1) * CELL) as f32, ((y + 1) * CELL) as f32);
                rect(buf, w, h, x0, y0, x1, y1, (255, 200, 60), 38);
            }
        }
    }

    /// What goes over the ground units but under the hovering Advancers: turrets in front of units standing behind them (not in the
    /// exe). Buildings and the foreground tiles need nothing here: the ground units are drawn through the screen mask they stamped
    /// (`0x461da0`), so their pixels already stay in front.
    fn finish_ground(&self, buf: &mut [u32], w: usize, h: usize) {
        let view = self.view;
        // units behind a turret (above the foot of its footprint) are hidden by it: draw it again over them (turrets stamp no mask)
        for st in self.world.structures.iter().filter(|s| s.alive()) {
            let Some((_, fh, _)) = opensow_formats::buildings::footprint(st.kind_id) else { continue };
            let Some(pl) = self.scene.placed.iter().find(|p| matches!(p.source, Source::Object(i) if i == st.object) && p.layer == 0) else { continue };
            if pl.mask().is_some() {
                continue;
            }
            let f = &pl.set[pl.frame.min(pl.set.len() - 1)];
            let (x0, y0) = (pl.ax - f.hot_x as i32 - 24, pl.ay - f.hot_y as i32 - 24);
            let (x1, y1) = (x0 + f.width as i32 + 48, y0 + f.height as i32 + 48);
            let foot = ((st.cell.1 + fh) * 32) as f32;
            // only over the units themselves
            for u in self
                .world
                .units
                .iter()
                .filter(|u| u.kind != kinds::ADVANCER && u.y < foot && (x0..x1).contains(&(u.x as i32)) && (y0..y1).contains(&(u.y as i32)))
            {
                let (sx0, sy0) = view.to_screen(u.x - 28.0, u.y - 40.0);
                let (sx1, sy1) = view.to_screen(u.x + 28.0, u.y + 24.0);
                opensow_viewer::render::blit_clip(buf, w, h, &view, f, pl.ax, pl.ay, false, (sx0, sy0, sx1, sy1));
            }
        }
    }

    /// What the exe draws between the ground units and the hovering Advancers (`0x420xxx`): the Advancers' shadows on the ground
    /// (`0x46a370`, also over buildings), then the bullets and effect sprites (`0x410a30`, `0x4673a0`), so the beam sparks lie
    /// over the buildings and the ground units but under the Advancer's body (`0x46a310`, drawn last).
    fn draw_air_pass(&self, assets: &mut Assets, order: &[usize], screen_mask: &[bool], buf: &mut [u32], w: usize, h: usize) {
        let view = self.view;
        for &i in order {
            let u = &self.world.units[i];
            if u.kind != kinds::ADVANCER {
                continue;
            }
            let Some(sp) = self.sprites.get(&(u.kind, u.player == 0)) else { continue };
            let (ax, ay) = u.px();
            let cframe = u.chassis.as_ref().map_or(0, |c| c.frame() as usize);
            if let Some(set) = &sp.chassis_shadow {
                blit(buf, w, h, &view, &set[cframe.min(set.len() - 1)], ax, ay, true);
            }
            if let (Some(set), Some(aim)) = (&sp.top_shadow, &u.aim) {
                let frame = match aim.firing() {
                    Some((seq, t)) => seq.top_frame(t, aim.facing, aim.params.steps) as usize,
                    None => aim.facing as usize,
                };
                blit(buf, w, h, &view, &set[frame.min(set.len() - 1)], ax, ay, true);
            }
        }
        self.draw_shots(assets, screen_mask, buf, w, h);
    }

    fn draw_units(&mut self, assets: &mut Assets, settings: &Settings, buf: &mut [u32], w: usize, h: usize) {
        let mut order: Vec<usize> = (0..self.world.units.len()).collect();
        order.sort_by(|&a, &b| {
            (self.world.units[a].kind == kinds::ADVANCER)
                .cmp(&(self.world.units[b].kind == kinds::ADVANCER))
                .then(self.world.units[a].y.total_cmp(&self.world.units[b].y))
        });
        for &i in &order {
            let (kind, player) = (self.world.units[i].kind, self.world.units[i].player);
            self.sprites_for(assets, kind, player);
        }
        let view = self.view;
        // the exe's screen mask for this frame: stamped by the map and the buildings before the effects and the units are drawn
        let screen_mask = self.screen_mask(w, h);
        // the smoke puffs (list `[0x49a3a8]`) are drawn first (`0x41faf7`), before the live buildings and all units, so those cover them
        self.draw_puffs(assets, &screen_mask, buf, w, h);
        // A ground unit's shadow, body and top are drawn through the screen mask (`0x46e9c0` -> `0x462110` / `0x461da0`, `0x479a80` ->
        // `0x461da0`): no pixel lands where a foreground tile (tree, rock or tower top) or a building's mask frame was stamped, so those
        // hide every unit under them. Only the hovering Advancer's body is drawn unmasked (`0x46a310` -> `0x46e8f0` -> `0x45edf0`).
        let mut ground_done = false;
        for &i in &order {
            let u = &self.world.units[i];
            if u.kind == kinds::ADVANCER && !ground_done {
                ground_done = true;
                self.finish_ground(buf, w, h);
                self.draw_air_pass(assets, &order, &screen_mask, buf, w, h);
            }
            let Some(sp) = self.sprites.get(&(u.kind, u.player == 0)) else { continue };
            let (ax, ay) = u.px();
            // the exe's draw offsets (`[chassis+0x2c]`, `[top+0x60]`): most units are drawn 4..16 px above their ground point
            let (cdy, tdy) = sim::draw_offsets(u.kind);
            let cframe = u.chassis.as_ref().map_or(0, |c| c.frame() as usize);
            // the hovering Advancers' shadows were drawn before the bullets (`draw_air_pass`)
            let shadow = u.kind != kinds::ADVANCER;
            if let (Some(set), true, true) = (&sp.chassis, shadow, opensow_viewer::fx::contact()) {
                // contact shadow: a soft dark pool under the unit (docs/fx.md)
                let f = &set[cframe.min(set.len() - 1)];
                let rx = f.width as f32 * 0.55;
                opensow_viewer::fx::blob_masked(buf, w, h, &view, ax as f32, ay as f32, rx, rx * 0.6, 140.0, Some(&screen_mask));
            }
            // the body blit: masked plain copy for ground units, plain for the Advancer
            let body = |buf: &mut [u32], f: &Frame, x: i32, y: i32| {
                if shadow {
                    blit_blend_masked(buf, w, h, &view, f, x, y, Blit::Plain, Some(&screen_mask));
                } else {
                    blit(buf, w, h, &view, f, x, y, false);
                }
            };
            if let (Some(set), true) = (&sp.chassis_shadow, shadow) {
                opensow_viewer::render::blit_shadow_masked(buf, w, h, &view, &set[cframe.min(set.len() - 1)], ax, ay + cdy, &screen_mask);
            }
            // the Advancer flies 64 px up and bobs (decoded hover table); its shadow stays on the ground
            let ay_body = u.sprite_y().round() as i32;
            if let Some(set) = &sp.chassis {
                body(buf, &set[cframe.min(set.len() - 1)], ax, ay_body + cdy);
            }
            if let (Some(top), Some(aim)) = (&sp.top, &u.aim) {
                let steps = aim.params.steps;
                let (frame, flash) = match aim.firing() {
                    Some((seq, t)) => (seq.top_frame(t, aim.facing, steps) as usize, seq.flash_frame(t, aim.facing, steps)),
                    None => (aim.facing as usize, None),
                };
                if let (Some(set), true) = (&sp.top_shadow, shadow) {
                    // like the chassis shadow, with the masked shade blit `0x462110`
                    opensow_viewer::render::blit_shadow_masked(buf, w, h, &view, &set[frame.min(set.len() - 1)], ax, ay + tdy, &screen_mask);
                }
                body(buf, &top[frame.min(top.len() - 1)], ax, ay + tdy);
                if let (Some(f), Some(set)) = (flash, &sp.flash) {
                    if let Some(fr) = set.get(f as usize) {
                        // the muzzle flash is added to the picture (`0x479a20` -> `0x45cde0`), its black stays invisible
                        blit_blend(buf, w, h, &view, fr, ax, ay + tdy, Blit::Additive);
                    }
                }
            } else if let Some(top) = &sp.top {
                body(buf, &top[cframe.min(top.len() - 1)], ax, ay + tdy);
            }
        }
        if !ground_done {
            self.finish_ground(buf, w, h);
            self.draw_air_pass(assets, &order, &screen_mask, buf, w, h);
        }
        // a selected unit's health bar (`0x46a420` -> `0x437140`): every unit with the local selection flag (a box also takes enemy units), centred on it, 25 px above its ground point (23 for
        // the Advancer, which hovers: its sprite height is added), `max_hp / 16` px wide (`max_hp / 20` for the Advancer, `[unit+0x38]`).
        // The original has no selection frame, so none is drawn.
        if settings.hp_bars {
            for u in self.world.units.iter().filter(|u| u.selected && u.hp > 0) {
                let (scale, dy) = if u.kind == kinds::ADVANCER { (20, -23) } else { (16, -25) };
                health_bar(buf, w, h, &view, u.x as i32, u.sprite_y().round() as i32 + dy, u.hp, u.max_hp, scale);
            }
        }
        // a building's health bar (`0x409400` -> `0x437140`): only while it is selected (clicked) or under the pointer
        let (mx, my) = self.to_map(self.cursor.0, self.cursor.1);
        let hovered = if self.edge_scroll && self.hud_hit(self.cursor.0, self.cursor.1).is_none() { self.world.structure_at(mx, my) } else { None };
        for t in [self.selected_building(), hovered].into_iter().flatten().collect::<std::collections::BTreeSet<_>>() {
            let s = &self.world.structures[t];
            // width = max / scale (scale 144 for buildings, 72 for turrets, `0x40a981..0x40aa01`), centred on the footprint, at its top edge
            let scale = if matches!(s.kind, ObjectKind::Turret(_)) { 72 } else { 144 };
            let (fw, _) = s.size();
            let (cx, top) = (s.cell.0 * CELL + fw * CELL / 2, s.cell.1 * CELL);
            let (sx, sy) = view.to_screen(cx as f32, top as f32);
            health_bar(buf, w, h, &view, cx, top, s.hp, s.max_hp, scale);
            // protected buildings (`[b+0x114]`) also get the string 146 (index 145) "SATELLITE PROTECTION" (`0x40952c..0x409650`): GDI text
            // (white, black shadow +2,+2) whose left edge is 46 px left of the footprint's middle, its top 6 px above the bar's first row
            // for the selected building (`0x40958a`) and 10 px for one that is only under the pointer (`0x40962c`)
            if s.protected {
                if let Some(text) = self.lang.as_ref().and_then(|l| l.get(opensow_formats::strings::SATELLITE_PROTECTION)) {
                    let z = view.zoom;
                    let lift = if self.selected_building() == Some(t) { 11.0 } else { 15.0 }; // the exe: 6 / 10; 5 px higher so the GDI stand-in does not cover the bar
                    let (tx, ty) = (sx as f32 - 46.0 * z, sy as f32 - lift * z);
                    let text = text.trim();
                    if let Some(g) = &self.label_font {
                        // the exe's font `[0x5ddf20]`: 11 px bold Times New Roman, natural width, smooth like GDI's
                        g.draw_px(buf, w, h, text, tx, ty, z);
                    } else {
                        let font = assets.font("small");
                        ui::text(buf, w, h, &font, text, tx as i32 + (2.0 * z) as i32, ty as i32 + (2.0 * z) as i32, z, (0, 0, 0), (0, 0, 0));
                        ui::text(buf, w, h, &font, text, tx as i32, ty as i32, z, (255, 255, 255), (0, 0, 0));
                    }
                }
            }
        }
    }

    /// Frame `index` of the effect sprite array at exe address `array` (an element of one of `effects::EXTRAS`).
    /// The turret placement marks (`0x4156b0`, drawn with the additive blit `0x45cde0`): while a turret is armed and the pointer is on the map, every cell of its footprint
    /// (top-left = the cell under the pointer) gets a tile of `extras\\sizer`, frame 0 (`0x5e68c8`) where it may stand, frame 1
    /// (`0x5e68cc`) where not. A click only builds when all cells are good (`[pointer+0x1e0]`, `0x4172cb`).
    fn draw_placement(&self, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize) {
        let Some(tier) = self.placing else { return };
        if self.hud_hit(self.cursor.0, self.cursor.1).is_some() {
            return;
        }
        let (mx, my) = self.to_map(self.cursor.0, self.cursor.1);
        let cell = (mx.div_euclid(CELL), my.div_euclid(CELL));
        let (fw, _) = opensow_formats::buildings::footprint(24 + tier as i32).map_or((1, 1), |(a, b, _)| (a, b));
        // the gun's reach from that cell (not in the original): the turret searches its targets from its top-left cell over the
        // range rings of its top (kind 108 + tier, `World::find_target`); the anti-air turret's reach is tinted blue. Options
        // page 2 "Turret range" turns it off.
        if let Some((min, max)) = sim::weapon_range(108 + tier as i32).filter(|_| self.turret_range) {
            let colour = if sim::is_anti_air(108 + tier as i32) { (90, 170, 255) } else { (255, 200, 60) };
            for &(dx, dy) in self.rings.target_search(min as usize, (max as usize).min(63)) {
                let (x, y) = (cell.0 + dx as i32, cell.1 + dy as i32);
                let (x0, y0) = self.view.to_screen((x * CELL) as f32, (y * CELL) as f32);
                let (x1, y1) = self.view.to_screen(((x + 1) * CELL) as f32, ((y + 1) * CELL) as f32);
                rect(buf, w, h, x0, y0, x1, y1, colour, 38);
            }
        }
        for (i, ok) in self.world.turret_cells(0, tier, cell).into_iter().enumerate() {
            let (x, y) = (cell.0 + i as i32 % fw, cell.1 + i as i32 / fw);
            if let Some((set, k)) = Self::extra_frame(assets, if ok { 0x5e68c8 } else { 0x5e68cc }, 0) {
                blit_blend(buf, w, h, &self.view, &set[k], x * CELL + CELL / 2, y * CELL + CELL / 2, Blit::Additive);
            }
        }
    }

    fn extra_frame(assets: &mut Assets, array: u32, index: usize) -> Option<(Set, usize)> {
        // the exe reads the pointer at `array + 4 * index`, which can lie in the next array (the bomb's `+ 20 * [+0x268c]` is `bomba_r`)
        let (extra, i) = extra_at(array.wrapping_add(4 * index as u32))?;
        let set = assets.art.load(&format!("{}.ps6", extra.name))?;
        (i < set.len()).then_some((set, i))
    }

    /// The sprite (array address, frame index) an aircraft is drawn with (`0x467240` / `0x4673a0`): bomber `bomber_l/r` frame +
    /// 10 * direction, carryall `carryAll`, tripler `tripler`, fighter `fighter`; the asteroid has none.
    fn aircraft_sprite(a: &opensow_formats::aircraft::Aircraft) -> Option<u32> {
        use opensow_formats::air::AirForce as F;
        Some(match a.force {
            F::Bomber => 0x5e9d38 + 4 * (a.frame + 10 * a.right as i32) as u32,
            F::Carryall => 0x5ea288 + 4 * a.frame as u32,
            F::Tripler => 0x5ea328 + 4 * a.frame as u32,
            F::Fighter => 0x5ea3a4,
            F::Asteroid => return None,
        })
    }

    /// The aircraft shadows (`0x4673a0` through `0x460f60`, one pass before the units): the body sprite as a stencil on the
    /// ground point, without the altitude.
    fn draw_aircraft_shadows(&self, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize) {
        for a in &self.world.aircraft {
            let Some(addr) = Self::aircraft_sprite(a) else { continue };
            if let Some((set, i)) = Self::extra_frame(assets, addr, 0) {
                blit_blend(buf, w, h, &self.view, &set[i], a.x + a.off_x, a.y + a.off_y, Blit::Shade);
            }
        }
    }

    /// The aircraft bodies (`0x467240`), from the smallest y down (the exe repeatedly picks the undrawn one with the smallest y,
    /// `0x4202f0`): the plain sprite at the flight altitude; a carryall also has its landing glow (`carryAll_down`, drawn first,
    /// 3 px up, while it is in the last frame and not yet over the target) and its additive exhaust (`carryAll_rear`).
    fn draw_aircraft(&self, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize) {
        use opensow_formats::air::AirForce as F;
        let mut order: Vec<&opensow_formats::aircraft::Aircraft> = self.world.aircraft.iter().collect();
        order.sort_by_key(|a| a.y);
        for a in order {
            let Some(addr) = Self::aircraft_sprite(a) else { continue };
            let (x, y) = (a.x + a.off_x, a.y + a.off_y - a.altitude);
            let tx = a.target.0;
            if a.force == F::Carryall && a.frame == 4 && (a.x < tx - 1 || a.x > tx + 1) {
                if let Some((set, i)) = Self::extra_frame(assets, 0x5ea2d8, a.down_frame as usize) {
                    blit_blend(buf, w, h, &self.view, &set[i], x, y - 3, Blit::Additive);
                }
            }
            if let Some((set, i)) = Self::extra_frame(assets, addr, 0) {
                blit_blend(buf, w, h, &self.view, &set[i], x, y, Blit::Plain);
            }
            if a.force == F::Carryall && (a.x < tx - 2 || a.x > tx + 15) {
                if let Some((set, i)) = Self::extra_frame(assets, 0x5ea300, a.rear_frame as usize) {
                    blit_blend(buf, w, h, &self.view, &set[i], x, y, Blit::Additive);
                }
            }
        }
    }

    /// Animation sprites (not the smoke puffs, which are drawn before the units: `draw_puffs`), then bullets (`0x4200f0..0x420232`):
    /// the exe draws both after the ground units and before the Advancers' bodies, so the Advancer's sparks come out from below it.
    fn draw_shots(&self, assets: &mut Assets, screen_mask: &[bool], buf: &mut [u32], w: usize, h: usize) {
        // the exe's screen mask (docs/effects.md): the smoke puffs (draw mode 8, `0x45b5e0`) and the darkening smoke (`0x45f4d0`: the
        // wrecks' smoke columns, mode 14) are drawn only where no building's mask frame and no foreground tile was stamped
        let masked = |routine: u32| matches!(routine, 0x45b5e0 | 0x45f4d0);
        // the clouds' shadows (`0x414730`, darkening and masked like the missile smoke) come right before the shots (`0x420095`)
        for c in &self.world.sky.clouds {
            if let Some((set, i)) = Self::extra_frame(assets, opensow_formats::clouds::ARRAY, c.picture as usize) {
                let (x, y) = c.shadow_pos();
                blit_blend_masked(buf, w, h, &self.view, &set[i], x, y, Blit::of_routine(0x45f4d0), Some(screen_mask));
            }
        }
        for e in self.world.effects.iter().filter(|e| e.spec.mode != PUFF_MODE) {
            let Some(frame) = e.spec.frame_at(e.age) else { continue };
            if let Some((set, i)) = Self::extra_frame(assets, e.spec.array, frame) {
                let routine = e.spec.routine();
                let m = if masked(routine) { Some(screen_mask) } else { None };
                blit_blend_masked(buf, w, h, &self.view, &set[i], e.x, e.y, Blit::of_routine(routine), m);
            }
        }
        // the Advancers' beam bullets: sparks at their fragment offsets (additive); the upgrade bullet draws nothing itself,
        // it leaves yellow sparks behind (world.effects)
        for b in &self.world.beams {
            let array = match b.action {
                opensow_formats::advancer::Action::Repair => opensow_formats::advancer::BODKY_B,
                opensow_formats::advancer::Action::Destroy => opensow_formats::advancer::BODKY_R,
                opensow_formats::advancer::Action::Upgrade => continue,
            };
            let (x, y) = b.path.points[b.index.min(b.path.points.len() - 1)];
            for &(fx, fy, frame) in &b.frags {
                if let Some((set, i)) = Self::extra_frame(assets, array, frame as usize) {
                    blit_blend(buf, w, h, &self.view, &set[i], x + fx, y + fy, Blit::Additive);
                }
            }
        }
        for b in &self.world.bullets {
            if b.bullet_type == 10 {
                // the satellite laser draws a column of `satlaser` frames at its end point, not at its position (`0x411100`, additive)
                let (x, y) = *b.path.points.last().unwrap();
                let frame = b.anim.map_or(0, |a| a.frame.max(0));
                for (f, px, py) in opensow_formats::effects::satlaser_column(frame, x, y) {
                    if let Some((set, i)) = Self::extra_frame(assets, 0x5e6b58, f as usize) {
                        blit_blend(buf, w, h, &self.view, &set[i], px, py, Blit::Additive);
                    }
                }
                continue;
            }
            let (x, y) = b.position();
            let mut drawn = false;
            if matches!(b.bullet_type, 15 | 16) {
                // a destroyed unit's flying piece: `units\{top}_blueboom` / `_grenboom`, the top's index is the variant `[+0x2680]`, the
                // frame the animation counter (0..16), drawn plain (`0x45edf0`)
                let name = opensow_formats::units::TOPS.get(b.variant as usize).copied().unwrap_or("1AA");
                let team = if b.bullet_type == 15 { "blueboom" } else { "grenboom" };
                if let Some(set) = assets.art.load(&format!("units\\{name}_{team}.ps6")) {
                    let f = b.anim.map_or(0, |a| a.frame.max(0) as usize).min(set.len() - 1);
                    blit_blend(buf, w, h, &self.view, &set[f], x, y, Blit::Plain);
                    drawn = true;
                }
            }
            for layer in bullet_draw(b.bullet_type) {
                let anim = b.anim.map_or(0, |a| a.frame.max(0) as usize);
                let index = match layer.frame {
                    FrameRule::Fixed(n) => n as usize,
                    FrameRule::Anim(o) => anim + o as usize,
                    FrameRule::Facing => b.facing as usize,
                    FrameRule::Variant268c(n) | FrameRule::Variant2680(n) => anim + n as usize * b.variant as usize,
                };
                if matches!(b.bullet_type, 15 | 16) {
                    continue; // drawn above
                }
                if let Some((set, i)) = Self::extra_frame(assets, layer.array, index) {
                    let m = if masked(layer.routine) { Some(screen_mask) } else { None };
                    blit_blend_masked(buf, w, h, &self.view, &set[i], x, y, Blit::of_routine(layer.routine), m);
                    drawn = true;
                }
            }
            // a dot where the type has art that is not loaded here (the unit boom pieces) or no draw case at all (not the types that draw nothing in the exe)
            let draws_nothing = bullet_draw(b.bullet_type).is_empty() && matches!(b.bullet_type, 6..=10 | 12 | 14 | 20 | 27 | 31);
            if !drawn && !draws_nothing {
                let (sx, sy) = self.view.to_screen(x as f32, y as f32);
                let r = ((2.5 * self.view.zoom) as i32).max(1);
                rect(buf, w, h, sx - r, sy - r, sx + r + 1, sy + r + 1, (255, 235, 120), 255);
            }
        }
    }

    /// The smoke puffs (draw mode 8): drawn through the screen mask before the units; see `draw_units`.
    fn draw_puffs(&self, assets: &mut Assets, screen_mask: &[bool], buf: &mut [u32], w: usize, h: usize) {
        for e in self.world.effects.iter().filter(|e| e.spec.mode == PUFF_MODE) {
            let Some(frame) = e.spec.frame_at(e.age) else { continue };
            if let Some((set, i)) = Self::extra_frame(assets, e.spec.array, frame) {
                blit_blend_masked(buf, w, h, &self.view, &set[i], e.x, e.y, Blit::of_routine(e.spec.routine()), Some(screen_mask));
            }
        }
    }

    /// The exe's screen mask for this frame: the pixels of the foreground tiles (`0x459666`) and of every building's `builds\\mask` frame
    /// (`0x4093e8`, `0x45888c`), stamped with `0x460120`.
    fn screen_mask(&self, w: usize, h: usize) -> Vec<bool> {
        let mut mask = vec![false; w * h];
        if self.debug_no_mask {
            return mask;
        }
        opensow_viewer::render::stamp_foreground(&self.scene, &self.view, &mut mask, w, h);
        for st in self.world.structures.iter().filter(|s| s.alive()) {
            let Some(pl) = self.scene.placed.iter().find(|p| matches!(p.source, Source::Object(i) if i == st.object) && p.layer == 0) else { continue };
            if let Some(m) = pl.mask() {
                opensow_viewer::render::stamp_frame(&mut mask, w, h, &self.view, m, pl.ax, pl.ay);
            }
        }
        mask
    }

    /// The pill list (`0x453f00` adds a pill when a force's stock leaves 0, `0x455520` drops it at 0): level start order is
    /// the header's, later ones append.
    fn sync_pills(&mut self) {
        use opensow_formats::air::HEADER_ORDER;
        let stock = |f: opensow_formats::air::AirForce| HEADER_ORDER.iter().position(|&x| x == f).map_or(0, |g| self.world.air_stock[0][g]);
        let have: Vec<_> = self.pills.iter().copied().filter(|&f| stock(f) > 0).collect();
        self.pills = have;
        for f in HEADER_ORDER {
            if stock(f) > 0 && !self.pills.contains(&f) {
                self.pills.push(f);
            }
        }
    }

    /// Rebuilds the info box texts when the shown kind changed.
    fn prepare_info(&mut self, assets: &mut Assets) {
        let font = assets.font("small");
        let kind = self.infobox.kind;
        if self.infobox.no_info.is_empty() {
            self.infobox.no_info = self.lang.as_ref().and_then(|l| l.get(opensow_formats::infobox::NO_INFO)).unwrap_or("No Info Available").trim().to_string();
        }
        if self.info_text.as_ref().map(|t| t.0) != kind {
            self.info_text = kind.and_then(|k| {
                let l = self.lang.as_ref().and_then(|lang| opensow_formats::infobox::lines(k, lang))?;
                let d = infobox::wrap_description(&font, self.info_gdi.as_ref(), &l.description);
                Some((k, l, d))
            });
        }
    }

    /// Headless: shows `kind` in the info box and runs `ticks` HUD ticks (`--info K --info-ticks N`).
    pub fn run_infobox(&mut self, assets: &mut Assets, kind: i32, ticks: u32) {
        self.infobox.show(Some(kind), false);
        self.prepare_info(assets);
        for _ in 0..ticks {
            let (total, desc) = self.info_text.as_ref().map_or((0, Vec::new()), |(_, l, d)| (infobox::total_chars(l), d.clone()));
            self.infobox.tick(self.info_text.as_ref().map(|t| &t.1), total, &desc);
        }
    }

    /// Width of a HUD line in natural pixels (bitmap or GDI, whichever draws it).
    fn hud_width(&self, font: &(opensow_formats::font::Font, opensow_formats::font::Font), text: &str) -> i32 {
        match &self.hud_gdi {
            Some(g) => g.width(text),
            None => ui::text_width(font, text, 1.0),
        }
    }

    /// A HUD line with its shadow `(2, 2)` window pixels away; `shadow` None = no shadow.
    fn hud_put(
        &self,
        font: &(opensow_formats::font::Font, opensow_formats::font::Font),
        buf: &mut [u32],
        w: usize,
        h: usize,
        text: &str,
        x: i32,
        y: i32,
        ui: f32,
        fg: (u8, u8, u8),
        shadow: Option<(u8, u8, u8)>,
    ) {
        if let Some(g) = &self.hud_gdi {
            match shadow {
                Some(sh) => g.draw_shadowed(buf, w, h, text, x as f32, y as f32, ui, fg, sh),
                // the exe's GDI helpers always draw a shadow; black when none is given (`0x467060`)
                None => g.draw_rgb(buf, w, h, text, x as f32, y as f32, ui, fg),
            }
            return;
        }
        if let Some(sh) = shadow {
            ui::text(buf, w, h, font, text, x + (2.0 * ui).round() as i32, y + (2.0 * ui).round() as i32, ui, sh, sh);
        }
        ui::text(buf, w, h, font, text, x, y, ui, fg, (0, 0, 0));
    }

    fn draw_hud(&self, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize, ui: f32) {
        let font = assets.font("small");
        let u = |v: f32| (v * ui).round() as i32;
        // countdown (`0x430d50`, called from `0x4209d3` with x = W/2, y = 13): "<string> mm:ss", left aligned at the screen's middle
        if let (Some((min, sec, _)), false) = (self.world.timer, self.world.timer_expired) {
            let label = self.lang.as_ref().and_then(|l| l.get(REMAINING_TIME_STRING - 1)).unwrap_or("Remaining Time");
            let text = format!("{} {min:02}:{sec:02}", label.trim()).to_uppercase();
            // `0x467060` passes text colour (255,255,255) and shadow (0,0,0) to `0x466f40` (shadow at +2,+2, as for messages)
            self.hud_put(&font, buf, w, h, &text, w as i32 / 2, u(13.0), ui, (255, 255, 255), Some((0, 0, 0)));
        }
        // messages (`MESSAGES` holds the exe's 1-based string numbers, `LangFile::get` is 0-based; `0x4036a0` draw, `0x404820` add, `0x4047a0` update, see `message_pose`): text colour (255,243,148),
        // shadow (74,4,0) at +2,+2
        // lang messages and chat lines share one queue (each newer line pushes the older ones down)
        // age in ticks, text, text colour, shadow colour
        type Line<'a> = (u32, &'a str, (u8, u8, u8), (u8, u8, u8));
        let mut lines: Vec<Line> = Vec::new();
        for &(t, id) in &self.world.log {
            if let Some(text) = opensow_formats::messages::lang_index(id).and_then(|i| self.lang.as_ref().and_then(|l| l.get(i - 1))) {
                lines.push((t as u32, text.trim(), (255, 243, 148), (74, 4, 0)));
            }
        }
        // the F3 / F4 speed lines: message colours
        lines.extend(self.notes.iter().map(|(t, text)| (*t as u32, text.as_str(), (255, 243, 148), (74, 4, 0))));
        // chat lines of the local (blue) player: light blue (`0xff8080` COLORREF, docs/multiplayer-menus.md)
        lines.extend(self.world.chat.iter().map(|(t, text)| (*t as u32, text.as_str(), (128, 128, 255), (0, 0, 0))));
        lines.sort_by_key(|l| l.0);
        let births: Vec<u32> = lines.iter().map(|l| l.0).collect();
        for (k, &(t, text, fg, shadow)) in lines.iter().enumerate() {
            let tw = self.hud_width(&font, text);
            let Some((x, y)) = message_pose(t, tw, &births[k + 1..], self.world.tick as u32) else { continue };
            let y = h as i32 + u(y as f32);
            self.hud_put(&font, buf, w, h, text, u(x as f32), y, ui, fg, Some(shadow));
        }
        // the open chat line, above the message slots; the cursor blinks twice a second
        if let Some(t) = &self.chat {
            self.chat_width.set(ui::text_width(&font, t, 1.0));
            let cursor = if (self.world.tick / 30).is_multiple_of(2) { "_" } else { "" };
            let text = format!("{t}{cursor}");
            let (x, y) = (u(10.0), h as i32 - u(64.0));
            let r = ui::Rect { x: x - u(4.0), y: y - u(3.0), w: u((opensow_formats::cheats::MAX_LINE_WIDTH + 24) as f32), h: u(19.0) };
            ui::dim_sub(buf, w, h, r, 32);
            ui::text(buf, w, h, &font, &text, x + u(2.0), y + u(2.0), ui, (0, 0, 0), (0, 0, 0));
            ui::text(buf, w, h, &font, &text, x, y, ui, (255, 255, 255), (0, 0, 0));
        }
        // "draw fps" (`0x4302e0`, from `0x420987`): "FPS: n" at the screen's middle, ten pixels above the bottom edge
        if self.show_fps {
            let text = format!("FPS: {}", self.fps.2);
            let (x, y) = (w as i32 / 2, h as i32 - u(10.0));
            ui::text(buf, w, h, &font, &text, x, y, ui, (0, 0, 0), (0, 0, 0));
            ui::text(buf, w, h, &font, &text, x + u(2.0), y, ui, (255, 255, 255), (0, 0, 0));
        }
        // F1's key help (`0x420a40`, from the frame draw `0x4209d8`): around (W/2, H/2 - 30) a box from (-300, -220) to (240, 263)
        // darkened through the `0x71ced0` table (`0x460a00`), then `extras\shortcuts` frame 0 with its hotspot on that point
        if self.show_keys {
            let (cx, cy) = (w as i32 / 2, h as i32 / 2 - u(30.0));
            ui::dim_sub(buf, w, h, ui::Rect { x: cx - u(300.0), y: cy - u(220.0), w: u(540.0), h: u(483.0) }, 32);
            if let Some(f) = assets.art.load("extras\\shortcuts.ps6").as_ref().and_then(|g| g.first()) {
                let at = |v: i32| (v as f32 * ui).round() as i32;
                ui::sprite(buf, w, h, f, cx - at(f.hot_x as i32), cy - at(f.hot_y as i32), ui);
            }
        }
        // the tutorial's instruction panel (`0x430fd0`): the dark box, the text lines, the spinning coin and the arrow button
        if let (Some(panel), true) = (&self.tut_panel, self.world.tutorial.is_some()) {
            self.draw_tutorial_panel(panel, assets, buf, w, h, ui);
        }
        // the hint box of missions 1 and 6 (`0x4303e0`): darkened box around the screen middle, then `0x467060`: white text over a
        // black copy 2 px right and down
        if let Some(hint) = self.world.hint {
            if let Some(text) = self.lang.as_ref().and_then(|l| l.get(hint.text)) {
                let (cx, cy) = (w as i32 / 2, h as i32 / 2);
                let r = ui::Rect { x: cx - u(hint.half as f32), y: cy - u(15.0), w: 2 * u(hint.half as f32), h: u(25.0) };
                ui::dim_sub(buf, w, h, r, 32);
                self.hud_put(&font, buf, w, h, text.trim(), cx - u(hint.text_dx as f32), cy - u(7.0), ui, (255, 255, 255), Some((0, 0, 0)));
            }
        }
        self.draw_placement(assets, buf, w, h);
        if let Some(layout) = &self.hud {
            let state = hud::State {
                view: &self.view,
                world: &self.world,
                strike: self.strike,
                hover: self.hud_hover,
                building: self.shown_building(),
                radar_black: self.radar_black,
            };
            hud::draw(layout, &state, assets, buf, w, h);
            hud::draw_slots(layout, &state, assets, buf, w, h);
            let hs = layout.s;
            let (lines, desc) = self.info_text.as_ref().map_or((None, Vec::new()), |(_, l, d)| (Some(l.clone()), d.clone()));
            infobox::draw(&self.infobox, lines.as_ref(), &desc, self.info_gdi.as_ref(), assets, buf, w, h, hs);
        }
        if self.paused {
            self.draw_pause(assets, buf, w, h);
        }
    }

    /// The instruction panel (docs/tutorial.md). Everything hangs from the bottom edge in the original's pixels, times `ui`.
    fn draw_tutorial_panel(&self, panel: &crate::tutorial_panel::Panel, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize, ui: f32) {
        let u = |v: i32| (v as f32 * ui).round() as i32;
        let bottom = h as i32;
        if panel.text.lines.is_empty() || panel.text.width == 0 {
            return;
        }
        let (x0, y0, x1, y1) = panel.box_rect();
        ui::dim_sub(buf, w, h, ui::Rect { x: u(x0), y: bottom + u(y0), w: u(x1) - u(x0), h: u(y1) - u(y0) }, 32);
        let font = assets.font("medium");
        for (k, line) in panel.text.lines.iter().enumerate() {
            let y = bottom + u(panel.line_y(k));
            match &self.tut_gdi {
                Some(g) => g.draw_px(buf, w, h, line, u(crate::tutorial_panel::TEXT_X) as f32, y as f32, ui),
                None => ui::text(buf, w, h, &font, line, u(crate::tutorial_panel::TEXT_X), y, ui, (255, 255, 255), (0, 0, 0)),
            }
        }
        let gfx = assets.art.load("extras\\tutorialgfx.ps6");
        let place = |buf: &mut [u32], frame: usize, x: i32, y: i32| {
            if let Some(f) = gfx.as_ref().and_then(|g| g.get(frame)) {
                ui::sprite(buf, w, h, f, u(x) - (f.hot_x as f32 * ui).round() as i32, bottom + u(y) - (f.hot_y as f32 * ui).round() as i32, ui);
            }
        };
        if let Some(frame) = panel.coin_frame() {
            let (cx, cy) = panel.coin_pos();
            place(buf, frame, cx, cy);
        }
        if self.world.tutorial.as_ref().is_some_and(|t| t.arrow) {
            place(buf, panel.arrow_frame(), panel.arrow_x, panel.arrow_y());
        }
    }

    /// The P pause (`0x435330`, docs/pause.md): the frozen frame with every even pixel row black and the string 362 "P A U S E"
    /// (white, black shadow at +2,+2) with its left edge at `width / 2 - text width / 2` and its top at the screen's middle row. The exe
    /// uses a 28 px bold italic Times New Roman through GDI; in English the big bitmap font stands in, other languages use GDI (`gdi.rs`).
    fn draw_pause(&self, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize) {
        if opensow_viewer::record::active() {
            opensow_viewer::record::stripes(0, 0, w as i32, h as i32);
        } else {
            for y in (0..h).step_by(2) {
                buf[y * w..(y + 1) * w].fill(0);
            }
        }
        let font = assets.font("big");
        let text = self.lang.as_ref().and_then(|l| l.get(PAUSE_STRING - 1)).map_or("P A U S E", str::trim);
        let ui = (h as f32 / 768.0).max(1.0);
        if let Some(g) = crate::gdi::Gdi::load_sized(28.0, 1.0).filter(|_| self.hud_gdi.is_some()) {
            let (x, y) = ((w as f32 - g.width(text) as f32 * ui) / 2.0, h as f32 / 2.0);
            g.draw_rgb(buf, w, h, text, x, y, ui, (255, 255, 255));
            return;
        }
        let tw = ui::text_width(&font, text, ui);
        let (x, y) = ((w as i32 - tw) / 2, h as i32 / 2);
        ui::text(buf, w, h, &font, text, x + 2, y + 2, ui, (0, 0, 0), (0, 0, 0));
        ui::text(buf, w, h, &font, text, x, y, ui, (255, 255, 255), (0, 0, 0));
    }
}

/// The lang string of the pause (1-based id pushed at `0x43543a`).
const PAUSE_STRING: usize = 362;
/// "Remaining Time" (1-based, `0x430d50`).
const REMAINING_TIME_STRING: usize = 363;
/// Longest gap between the two clicks of a double click (the Windows default).
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(500);
/// "speed" of the F3 / F4 line (1-based, `push 0x91` at `0x403205`).
const SPEED_STRING: usize = 145;

/// Position of a message `now - born` ticks after it was added, or `None` once it is gone (`0x4047a0`, `0x404820`).
/// It starts at x = 10 - width, y = -42 (relative to the screen's bottom) and slides in 3 px per tick up to x = 10; only then
/// its life (480 ticks) runs and the push of 20 px that every newer message (born at `newer`) adds is worked off, per tick
/// by the speed-30 row of the step table (`0x552ba8` is row 30, [`steps_at`]); it is removed when y passes 0.
pub fn message_pose(born: u32, width: i32, newer: &[u32], now: u32) -> Option<(i32, i32)> {
    use opensow_formats::projectile::steps_at;
    let (mut x, mut y, mut life, mut pending) = (10 - width, -42, 480, 0);
    for t in born..now {
        pending += 20 * newer.iter().filter(|&&n| n == t).count() as i32;
        if x < 10 {
            x = (x + 3).min(10);
            continue;
        }
        life -= 1;
        if life <= 0 {
            return None;
        }
        if pending > 0 {
            let step = steps_at(30, t % 60) as i32;
            y += step;
            pending -= step;
            if y > 0 {
                return None;
            }
        }
    }
    Some((x, y))
}

/// The original's health bar: a thin pale-green line over a dark one, no frame; the lost part is dark.
/// The exe's health bar (`0x437140`, RGB565 values): a 1 px frame (`0x0001`) around `max / scale` x 3 px, the filled part
/// `hp / scale` px in three shades (top row light, bottom row dark) and the rest `0x0066`; green while `hp > max / 2`, yellow
/// above `max / 4`, else red. Centred on `x`, its top frame row at `y` (map pixels); the game's zoom scales every exe pixel.
fn health_bar(buf: &mut [u32], w: usize, h: usize, view: &View, x: i32, y: i32, hp: i32, max: i32, scale: i32) {
    use opensow_formats::sprite::rgb565_to_rgb8;
    const GREEN: [u16; 3] = [0xd7f3, 0x64a8, 0x3323];
    const YELLOW: [u16; 3] = [0xff94, 0xb583, 0x7b60];
    const RED: [u16; 3] = [0xfd32, 0xc221, 0x8060];
    const FRAME: u16 = 0x0001;
    const EMPTY: u16 = 0x0066;
    let width = max / scale;
    let fill = hp.max(0) / scale;
    let rows = if hp > max / 2 {
        GREEN
    } else if hp > max / 4 {
        YELLOW
    } else {
        RED
    };
    let (sx, sy) = view.to_screen((x - width / 2) as f32, y as f32);
    let z = view.zoom;
    let px = |n: i32| (n as f32 * z).round() as i32;
    let fill_rect = |buf: &mut [u32], x0: i32, x1: i32, row: i32, c: u16| {
        let [r, g, b] = rgb565_to_rgb8(c);
        rect(buf, w, h, sx + px(x0), sy + px(row), sx + px(x1), sy + px(row + 1), (r, g, b), 255);
    };
    fill_rect(buf, -1, width + 1, 0, FRAME);
    for (i, &c) in rows.iter().enumerate() {
        let row = 1 + i as i32;
        fill_rect(buf, -1, 0, row, FRAME);
        fill_rect(buf, width, width + 1, row, FRAME);
        fill_rect(buf, 0, fill.min(width), row, c);
        fill_rect(buf, fill.min(width), width, row, EMPTY);
    }
    fill_rect(buf, -1, width + 1, 4, FRAME);
}

#[cfg(test)]
mod tests {
    use super::message_pose;

    #[test]
    fn a_message_slides_in_stays_and_expires() {
        // 30 px wide: starts at x = -20, 3 px per tick, in place after 10 ticks
        assert_eq!(message_pose(100, 30, &[], 100), Some((-20, -42)));
        assert_eq!(message_pose(100, 30, &[], 104), Some((-8, -42)));
        assert_eq!(message_pose(100, 30, &[], 110), Some((10, -42)));
        // life of 480 ticks starts once it is in place
        assert_eq!(message_pose(100, 30, &[], 100 + 10 + 478), Some((10, -42)));
        assert_eq!(message_pose(100, 30, &[], 100 + 10 + 480), None);
    }

    #[test]
    fn a_newer_message_pushes_20_px_at_half_a_pixel_per_tick() {
        // speed row 30: 30 steps per 60 ticks, so the 20 px take 40 ticks
        let y = |now| message_pose(0, 0, &[100], now).map(|p| p.1);
        assert_eq!(y(100), Some(-42));
        assert_eq!(y(120), Some(-32));
        assert_eq!(y(140), Some(-22));
        assert_eq!(y(400), Some(-22));
        // a third message in the queue pushes the oldest past the bottom edge
        assert_eq!(message_pose(0, 0, &[100, 200, 300], 400), None);
    }
}
