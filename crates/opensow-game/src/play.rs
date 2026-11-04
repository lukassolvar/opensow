//! The in-game screen: camera, selection, orders, and drawing the world on top of the viewer's map renderer.

use crate::assets::Assets;
use crate::hud::{self, Hit, Inputs, Layout};
use crate::infobox::{self, InfoBox};
use crate::menu::Settings;
use crate::sim::{self, Outcome, Target, World, CELL};
use opensow_formats::cursor::{scroll_pointer, Pointer};
use crate::ui;
use opensow_formats::effects::{bullet_draw, extra_at, Blit, Frame as FrameRule};
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

pub struct Play {
    /// Headless debugging (`--no-mask`): draw the smoke and the top shadows without the screen mask.
    pub debug_no_mask: bool,
    pub level: String,
    pub scene: Scene,
    pub world: World,
    pub view: View,
    sprites: HashMap<(i32, bool), UnitSprites>,
    rings: Rings,
    /// Screen position where a left-button drag started.
    drag: Option<(i32, i32)>,
    pub cursor: (i32, i32),
    /// Scroll when the pointer touches a window edge (only once a real pointer position is known).
    pub edge_scroll: bool,
    keys: HashSet<&'static str>,
    time: f32,
    pub paused: bool,
    fit: bool,
    /// Turret tier (1-5) waiting for a click on the map (keys 1-5).
    pub placing: Option<u32>,
    /// Rally point mode (`[pointer+0x1e9]`, `[pointer+0x1ec]`): the structure whose point the next map click sets.
    pub rallying: Option<usize>,
    /// Air force waiting for a click on the map (keys B / F / C / Y / M).
    pub strike: Option<opensow_formats::air::AirForce>,
    /// Interface sounds raised by clicks (effect ids), drained by the game.
    pub ui_sounds: Vec<u8>,
    /// Tutorial voice files `Tutorial\NN.wav` to play, and the step whose instruction is shown.
    pub tut_voices: Vec<u32>,
    pub tut_text: Option<u32>,
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
}

impl Play {
    pub fn new(assets: &mut Assets, level: &str, settings: &Settings) -> Result<Play, String> {
        let mut scene = Scene::load(&assets.game, level, 0, None)?;
        let lvl = scene.level.as_ref().ok_or("level has no script (.edt)")?;
        let mut world = World::from_level(&scene.map, lvl);
        if let Some(rng) = std::fs::read(assets.game.data().join("generic.file")).ok().and_then(|d| opensow_formats::rng::GameRng::from_file(&d)) {
            world.set_game_rng(rng);
        }
        if settings.starter_squad && world.player_units().filter(|u| u.kind != 40).count() < 4 {
            let anchor = world.player_units().find(|u| u.kind == 40).or_else(|| world.player_units().next()).map(|u| u.px());
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
            }
        }
        // Units are drawn by the game from the simulation; the loader's static unit sprites go away.
        scene.placed.retain(|p| matches!(p.source, Source::Object(_)));
        let mut play = Play {
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
            keys: HashSet::new(),
            time: 0.0,
            paused: false,
            fit: true,
            placing: None,
            rallying: None,
            ui_sounds: Vec::new(),
            tut_voices: Vec::new(),
            tut_text: None,
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
            lang: assets.game.resolve("Data\\Language\\langEN.txt").and_then(|p| std::fs::read(p).ok()).and_then(|d| opensow_formats::lang::LangFile::parse(&d).ok()),
        };
        play.world.mission_no = level.trim_start_matches(['M', 'm']).parse().ok();
        // the tutorial (M00) runs its own script and has no computer opponent
        if level.eq_ignore_ascii_case("M00") {
            play.world.start_tutorial();
            play.world.ai = false;
        }
        play.world.select_all();
        play.world.clear_selection();
        Ok(play)
    }

    /// What a savegame stores: the camera (three `f32`) and the world's snapshot.
    pub fn snapshot(&mut self) -> Vec<u8> {
        let mut o = Vec::new();
        for v in [self.view.x, self.view.y, self.view.zoom] {
            o.extend_from_slice(&v.to_le_bytes());
        }
        o.extend(self.world.snapshot());
        o
    }

    /// Continues a saved game in this freshly started play of the same level (`Play::snapshot`).
    pub fn restore(&mut self, data: &[u8]) -> Result<(), String> {
        let (cam, world) = data.split_at_checked(12).ok_or("snapshot too short")?;
        let f = |i: usize| f32::from_le_bytes([cam[i], cam[i + 1], cam[i + 2], cam[i + 3]]);
        self.world.restore(world)?;
        self.view = View { x: f(0), y: f(4), zoom: f(8) };
        self.fit = false;
        self.heard = self.world.log.len();
        self.sel_building = None;
        self.placing = None;
        self.rallying = None;
        self.strike = None;
        self.paused = false;
        Ok(())
    }

    pub fn hud_layout(&self) -> Option<&Layout> {
        self.hud.as_ref()
    }

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

    pub fn key(&mut self, name: &'static str, down: bool) {
        if down {
            self.keys.insert(name);
        } else {
            self.keys.remove(name);
        }
    }

    pub fn to_map(&self, sx: i32, sy: i32) -> (i32, i32) {
        ((self.view.x + sx as f32 / self.view.zoom) as i32, (self.view.y + sy as f32 / self.view.zoom) as i32)
    }

    pub fn left_down(&mut self, x: i32, y: i32) {
        if self.world.tutorial.as_ref().is_some_and(|t| t.arrow) {
            let (w, h) = self.viewport;
            let ui = self.panel_ui;
            let (px, py, pw, ph) = self.tutorial_panel(w, h, ui);
            let a = (28.0 * ui) as i32; // arrow button: square at the panel's bottom right
            if x >= px + pw - a && x < px + pw && y >= py + ph - a && y < py + ph {
                self.world.tutorial_arrow_clicked();
                self.ui_sounds.push(50);
                return;
            }
        }
        if let Some(hit) = self.hud.as_ref().and_then(|l| l.hit(x, y)) {
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
                    self.tut_text = Some(n);
                    self.tut_voices.push(n);
                }
                sim::TutEvent::Arrow => self.ui_sounds.push(29),
            }
        }
    }

    /// Where the tutorial panel's text box and arrow button are on screen (unscaled original pixels from the bottom left).
    fn tutorial_panel(&self, w: usize, h: usize, ui: f32) -> (i32, i32, i32, i32) {
        let u = |v: f32| (v * ui).round() as i32;
        (u(8.0), h as i32 - u(300.0), u(320.0), u(110.0))
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

    /// The building whose info and slots are shown: the clicked one.
    fn shown_building(&self) -> Option<usize> {
        self.sel_building.filter(|i| self.world.structures.get(*i).is_some_and(|b| b.alive()))
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

    /// The rally button or the R key (`0x416cf9`, `0x416769`): the selected own factory becomes the target of the next map click.
    /// Returns whether the mode is on (false: no own factory is selected).
    pub fn arm_rally(&mut self) -> bool {
        self.rallying = self.shown_building().filter(|&b| self.world.structures[b].owner == Owner::Player && opensow_formats::rally::has_rally(self.world.structures[b].kind_id));
        if self.rallying.is_some() {
            self.strike = None;
            self.placing = None;
        }
        self.rallying.is_some()
    }

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
            if let Some(force) = self.strike.take() {
                self.world.launch_strike(0, force, p.0, p.1);
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
                        self.world.effects.push(sim::Effect { x: cx, y: cy, spec: opensow_formats::effects::AnimSpec { array: opensow_formats::rally::CLICK_ARRAY, from: 0, to: opensow_formats::rally::CLICK_LAST_FRAME, period: opensow_formats::rally::CLICK_PERIOD, repeat: 1, mode: 6 }, age: 0 });
                        self.ui_sounds.push(opensow_formats::rally::CLICK_SOUND);
                    }
                }
                return;
            }
            // The left button is the command button (exe `0x417686..0x417e47`, docs/controls.md): what a click does is what
            // the pointer shows at the release.
            let selected = self.world.units.iter().any(|u| u.selected && u.player == 0);
            match self.pointer() {
                Pointer::Destroy | Pointer::Repair | Pointer::Upgrade => {
                    // the Advancer's order (`0x46b8b0` / `0x46b9f0` / `0x46bb30`)
                    if let Some(t) = self.world.structure_at(p.0, p.1) {
                        let advancers: Vec<u32> = self.world.player_units().filter(|u| u.selected && u.kind == 40).map(|u| u.id).collect();
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
                Pointer::Move if selected => {
                    // the group go-to (`0x418220`, only with the move pointer)
                    self.world.order_move_selected(p.0, p.1);
                    self.ui_sounds.push(61); // unit-dest
                    // the click marker (`0x418457`): `click` frames 0..8, 3 ticks each, at the clicked cell's centre
                    let (cx, cy) = (p.0.div_euclid(CELL) * CELL + CELL / 2, p.1.div_euclid(CELL) * CELL + CELL / 2);
                    self.world.effects.push(sim::Effect { x: cx, y: cy, spec: opensow_formats::effects::AnimSpec { array: 0x5e6140, from: 0, to: 8, period: 3, repeat: 1, mode: 6 }, age: 0 });
                }
                Pointer::Own => {
                    let unit_there = self.world.units.iter().any(|u| u.hp > 0 && u.player == 0 && (u.x - p.0 as f32).hypot(u.sprite_y() - p.1 as f32) < 22.0);
                    self.sel_building = None;
                    if unit_there {
                        // select it alone; with shift it is added or toggled (`0x417a25..0x417a76`)
                        self.world.select_at(p.0, p.1, 22.0, additive);
                        if self.world.units.iter().any(|u| u.selected) {
                            self.ui_sounds.push(60);
                        }
                    } else if let Some(t) = self.world.structure_at(p.0, p.1) {
                        self.world.clear_selection();
                        self.sel_building = Some(t);
                    }
                }
                _ => {
                    // nothing selected: a click on a building shows its info box and slots (any owner)
                    if !selected {
                        if let Some(t) = self.world.structure_by_art(p.0, p.1) {
                            self.sel_building = Some(t);
                        } else {
                            self.sel_building = None;
                        }
                    }
                }
            }
        }
    }

    /// The sounds of selecting the whole army (the group select `0x43ad60`): `tt-click` (54) for every Advancer, `unit-select` (60) once if any other unit joined.
    pub fn select_all_sounds(&self) -> Vec<u8> {
        let mut v = Vec::new();
        let mut other = false;
        for u in self.world.units.iter().filter(|u| u.selected && u.player == 0) {
            if u.kind == 40 { v.push(54) } else { other = true }
        }
        if other { v.push(60) }
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
        if self.hud.as_ref().is_some_and(|l| l.hit(x, y).is_some()) {
            // a right click on the radar toggles the black radar (`0x416e94`: `[0x5baad8] ^= 1` on the release edge)
            if self.hud.as_ref().and_then(|l| l.hit(x, y)) == Some(Hit::Radar) {
                self.radar_black = !self.radar_black;
            }
            self.placing = None;
            self.strike = None;
            self.rallying = None;
            return;
        }
        let (mx, my) = self.to_map(x, y);
        if self.placing.take().is_some() | self.strike.take().is_some() | self.rallying.take().is_some() {
            return;
        }
        if self.world.player_units().any(|u| u.selected && u.kind == 40) && self.world.structure_at(mx, my).is_some_and(|t| self.world.structures[t].owner == Owner::Player) {
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
        let on_panel = self.hud.as_ref().is_some_and(|l| l.hit(x, y).is_some());
        // screen edges: scroll arrows
        let edge = 6;
        let dx = if x < edge { -1 } else if x >= w as i32 - edge && !on_panel { 1 } else { 0 };
        let dy = if y < edge { -1 } else if y >= h as i32 - edge { 1 } else { 0 };
        if let Some(p) = scroll_pointer(dx, dy).filter(|_| self.edge_scroll) {
            return p;
        }
        if on_panel {
            return Pointer::Arrow;
        }
        let (mx, my) = self.to_map(x, y);
        // rally point mode (`0x41763b`): the placement pointer over free ground, the blocked arrow elsewhere
        if self.rallying.is_some() {
            return if self.world.is_blocked(mx.div_euclid(CELL), my.div_euclid(CELL)) { Pointer::Blocked } else { Pointer::Rally };
        }
        let advancer = self.world.player_units().any(|u| u.selected && u.kind == 40);
        let selected = self.world.units.iter().any(|u| u.selected && u.player == 0);
        // an own unit under the pointer
        if self.world.units.iter().any(|u| u.hp > 0 && u.player == 0 && (u.x - mx as f32).hypot(u.sprite_y() - my as f32) < 22.0) {
            return Pointer::Own;
        }
        if let Some(Target::Unit(_)) = self.world.enemy_at(mx, my).filter(|t| matches!(t, Target::Unit(_))) {
            // the Advancer cannot attack units: the plain move pointer
            return if advancer { Pointer::Move } else if selected { Pointer::Attack } else { Pointer::Arrow };
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

    /// Zoom never goes out far enough to show anything beyond the map (no black bars).
    pub fn limit_zoom(&mut self, w: usize, h: usize) {
        let fill = (w as f32 / self.scene.bg_w as f32).max(h as f32 / self.scene.bg_h as f32);
        self.view.zoom = self.view.zoom.clamp(fill.max(0.3), 3.0);
    }

    pub fn wheel(&mut self, lines: f32, w: usize, h: usize) {
        let (mx, my) = self.to_map(self.cursor.0, self.cursor.1);
        self.view.zoom = self.view.zoom * 1.15f32.powf(lines);
        self.limit_zoom(w, h);
        self.view.x = mx as f32 - self.cursor.0 as f32 / self.view.zoom;
        self.view.y = my as f32 - self.cursor.1 as f32 / self.view.zoom;
        self.view.clamp(&self.scene, w, h);
    }

    /// Advances real time by `dt` seconds: scrolls the camera and runs as many game ticks as are due.
    pub fn update(&mut self, dt: f32, settings: &Settings, w: usize, h: usize) {
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
        let hover = self.hud.as_ref().and_then(|l| l.hit(self.cursor.0, self.cursor.1)).filter(|h| !matches!(h, Hit::Panel));
        if hover != self.hud_hover {
            if hover.is_some_and(|h| h != Hit::Radar) {
                self.ui_sounds.push(50); // icon-move
            }
            self.hud_hover = hover;
        }
        // the info box shows the hovered icon, else the shown building (`0x454d30` -> `0x43cde0`, `0x455b3b`)
        let hovered_kind = match hover {
            Some(Hit::Turret(t)) => Some(24 + t as i32),
            Some(Hit::Air(f)) => Some(f.code()),
            Some(Hit::Slot(i) | Hit::SlotBuy(i)) => self.shown_building().and_then(|b| self.world.structures.get(b)).and_then(|b| b.slots.get(i)).map(|sl| sl.kind),
            _ => None,
        };
        let selected_kind = self.shown_building().and_then(|b| self.world.structures.get(b)).filter(|b| b.alive()).map(|b| b.kind_id);
        let locked = matches!(hover, Some(Hit::Slot(i) | Hit::SlotBuy(i)) if self.shown_building().and_then(|b| self.world.structures.get(b)).is_some_and(|b| i >= b.active));
        self.infobox.show(hovered_kind.or(selected_kind), locked);
        // the map moves `2 * ticks * speed` px per step (`0x457f80` doubles the delta; the speed is the options menu's KEYS
        // SCROLL `[0x6bf728]` for the arrow keys and MOUSE SCROLL `[0x6bf724]` at the window edge; 60 ticks/s)
        let (kspeed, mspeed) = (crate::options::scroll_px_per_second(settings.scroll_keys) * dt / self.view.zoom, crate::options::scroll_px_per_second(settings.scroll_mouse) * dt / self.view.zoom);
        let (mut dx, mut dy) = (0.0, 0.0);
        let edge = 6;
        let at = self.edge_scroll;
        let on_panel = self.hud.as_ref().is_some_and(|l| l.hit(self.cursor.0, self.cursor.1).is_some());
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
        let (scrolling, strike, keys) = (self.right.is_some_and(|r| r.2), self.strike.is_some(), ["left", "right", "up", "down"].map(|k| self.keys.contains(k) || (self.edge_scroll && false)));
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
        while self.time >= tick && n < 8 {
            self.time -= tick;
            self.world.step();
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
        if n == 8 {
            self.time = 0.0;
        }
    }

    // ---- drawing ------------------------------------------------------------------------------------------------

    fn sprites_for(&mut self, assets: &mut Assets, kind: i32, player: i32) -> Option<&UnitSprites> {
        let key = (kind, player == 0);
        if !self.sprites.contains_key(&key) {
            let art = unit_art(kind)?;
            let team = if player == 0 { "blue" } else { "gren" };
            let mut load = |name: &str, ext: &str| assets.art.load(&format!("units\\{name}_{team}.{ext}"));
            let placeholder = |s: &Set| s[0].width <= 16 && s[0].height <= 2;
            let top = art.top.and_then(|t| load(t, "ps6")).filter(|s| !placeholder(s));
            let top_shadow = art.top.and_then(|t| load(t, "sha"));
            let flash = opensow_formats::recoil::fire_sequence(kind).and_then(|s| s.flash_set).and_then(|f| assets.art.load(&format!("units\\{}_fire.ps6", FLASH_SETS[f as usize])));
            let mut load = |name: &str, ext: &str| assets.art.load(&format!("units\\{name}_{team}.{ext}"));
            self.sprites.insert(key, UnitSprites { chassis: load(art.chassis, "ps6"), chassis_shadow: load(art.chassis, "sha"), top, top_shadow, flash });
        }
        self.sprites.get(&key)
    }

    pub fn draw(&mut self, assets: &mut Assets, settings: &Settings, buf: &mut [u32], w: usize, h: usize, _ui_scale: f32) {
        self.limit_zoom(w, h);
        self.viewport = (w, h);
        if self.fit {
            self.centre_on_player(w, h);
            self.fit = false;
        }
        // destroyed buildings and turrets disappear
        let dead: HashSet<usize> = self.world.structures.iter().filter(|s| !s.alive()).map(|s| s.object).collect();
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
            if let (Some(aim), Some(p)) = (&st.aim, self.scene.placed.iter_mut().find(|p| matches!(p.source, Source::Object(i) if i == st.object) && p.layer == 1)) {
                let steps = aim.params.steps;
                let frame = match aim.firing() {
                    Some((seq, t)) => seq.top_frame(t, aim.facing, steps) as usize,
                    None => aim.facing as usize,
                };
                p.frame = frame.min(p.set.len() - 1);
            }
        }
        let overlays = Overlays { blocked: false, grid: false, objects: false, markers: false, sprites: true, muzzles: false, walk: None, fire: None, foreground: false };
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
                blit(buf, w, h, &self.view, fr, pl.ax, pl.ay, false);
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
        self.draw_units(assets, settings, buf, w, h);
        self.draw_aircraft(buf, w, h);
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
            map_cells: self.world.cells,
            show_credits: gold || self.world.credits.iter().any(|&c| c > 0),
            show_research: lab || self.world.research.iter().any(|&c| c > 0),
            tiers: (1..=5u32).filter(|&t| self.world.turret_flags[0][t as usize - 1]).collect(),
            pills: self.pills.clone(),
            info: self.infobox.rect(w as i32, hs),
            slots: self.shown_building().and_then(|i| self.world.structures.get(i)).map_or(0, |b| b.slots.len().min(5)),
            buy: self.shown_building().and_then(|i| self.world.structures.get(i)).map_or(Vec::new(), |b| b.slots.iter().take(5).map(|sl| sl.level < sl.max_level).collect()),
            rally_button: self.shown_building().and_then(|i| self.world.structures.get(i)).is_some_and(|b| b.owner == Owner::Player && opensow_formats::rally::has_rally(b.kind_id)),
        };
        let mut layout = Layout::new(w, h, hs, &inputs);
        layout.radar_on = settings.radar_on;
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
        if self.edge_scroll {
            self.draw_pointer(assets, buf, w, h, hs);
        }
    }

    fn draw_ranges(&self, buf: &mut [u32], w: usize, h: usize) {
        for u in self.world.units.iter().filter(|u| u.selected) {
            let Some((min, max)) = sim::weapon_range(u.kind) else { continue };
            if sim::is_anti_air(u.kind) {
                continue;
            }
            let (cx, cy) = u.cell();
            for &(dx, dy) in self.rings.search(min as usize, max as usize) {
                let (x, y) = (cx + dx as i32, cy + dy as i32);
                let (x0, y0) = self.view.to_screen((x * CELL) as f32, (y * CELL) as f32);
                let (x1, y1) = self.view.to_screen(((x + 1) * CELL) as f32, ((y + 1) * CELL) as f32);
                rect(buf, w, h, x0, y0, x1, y1, (255, 200, 60), 38);
            }
        }
    }

    /// Everything that goes over the ground units but under the hovering Advancers: buildings in front of units standing behind
    /// them, and the rest of the foreground.
    fn finish_ground(&self, fg_row: &mut usize, buf: &mut [u32], w: usize, h: usize) {
        let view = self.view;
        // units behind a building or turret (above the foot of its footprint) are hidden by it: draw it again over them
        for st in self.world.structures.iter().filter(|s| s.alive()) {
            let Some((_, fh, _)) = opensow_formats::buildings::footprint(st.kind_id) else { continue };
            let Some(pl) = self.scene.placed.iter().find(|p| matches!(p.source, Source::Object(i) if i == st.object) && p.layer == 0) else { continue };
            let f = &pl.set[pl.frame.min(pl.set.len() - 1)];
            let (x0, y0) = (pl.ax - f.hot_x as i32 - 24, pl.ay - f.hot_y as i32 - 24);
            let (x1, y1) = (x0 + f.width as i32 + 48, y0 + f.height as i32 + 48);
            let foot = ((st.cell.1 + fh) * 32) as f32;
            let mask = pl.mask();
            // only over the units themselves
            for u in self.world.units.iter().filter(|u| u.kind != 40 && (mask.is_some() || u.y < foot) && (x0..x1).contains(&(u.x as i32)) && (y0..y1).contains(&(u.y as i32))) {
                let (sx0, sy0) = view.to_screen(u.x - 28.0, u.y - 40.0);
                let (sx1, sy1) = view.to_screen(u.x + 28.0, u.y + 24.0);
                match mask {
                    // buildings: the exe's mask frame says which pixels stand in front of units (turrets: above the foot)
                    Some(m) => opensow_viewer::render::blit_masked(buf, w, h, &view, f, m, pl.ax, pl.ay, (sx0, sy0, sx1, sy1)),
                    None => opensow_viewer::render::blit_clip(buf, w, h, &view, f, pl.ax, pl.ay, false, (sx0, sy0, sx1, sy1)),
                }
            }
        }
        let rows = self.scene.map.height as usize;
        while *fg_row < rows {
            opensow_viewer::render::draw_foreground_row(&self.scene, &view, buf, w, h, *fg_row);
            *fg_row += 1;
        }
    }

    /// What the exe draws between the ground units and the hovering Advancers (`0x420xxx`): the Advancers' shadows on the ground
    /// (`0x46a370`, also over buildings), then the bullets and effect sprites (`0x410a30`, `0x4673a0`), so the beam sparks lie
    /// over the buildings and the ground units but under the Advancer's body (`0x46a310`, drawn last).
    fn draw_air_pass(&self, assets: &mut Assets, order: &[usize], screen_mask: &[bool], buf: &mut [u32], w: usize, h: usize) {
        let view = self.view;
        for &i in order {
            let u = &self.world.units[i];
            if u.kind != 40 {
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
        order.sort_by(|&a, &b| (self.world.units[a].kind == 40).cmp(&(self.world.units[b].kind == 40)).then(self.world.units[a].y.total_cmp(&self.world.units[b].y)));
        for &i in &order {
            let (kind, player) = (self.world.units[i].kind, self.world.units[i].player);
            self.sprites_for(assets, kind, player);
        }
        let view = self.view;
        // the exe's screen mask for this frame: stamped by the map and the buildings before the effects and the units are drawn
        let screen_mask = self.screen_mask(w, h);
        // the smoke puffs (list `[0x49a3a8]`) are drawn first (`0x41faf7`), before the live buildings and all units, so those cover them
        self.draw_puffs(assets, &screen_mask, buf, w, h);
        // Foreground tiles (trees, rocks) are drawn row by row between the units: a tile in cell row k hides the units
        // standing in rows up to k (behind it) and not the ones below it.
        let mut fg_row = 0usize;
        let mut ground_done = false;
        for &i in &order {
            let u = &self.world.units[i];
            if u.kind == 40 && !ground_done {
                ground_done = true;
                self.finish_ground(&mut fg_row, buf, w, h);
                self.draw_air_pass(assets, &order, &screen_mask, buf, w, h);
            } else if u.kind != 40 {
                let row = (u.y.max(0.0) / 32.0) as usize;
                while fg_row < row {
                    opensow_viewer::render::draw_foreground_row(&self.scene, &view, buf, w, h, fg_row);
                    fg_row += 1;
                }
            }
            let Some(sp) = self.sprites.get(&(u.kind, u.player == 0)) else { continue };
            let (ax, ay) = u.px();
            let cframe = u.chassis.as_ref().map_or(0, |c| c.frame() as usize);
            // the hovering Advancers' shadows were drawn before the bullets (`draw_air_pass`)
            let shadow = u.kind != 40;
            if let (Some(set), true) = (&sp.chassis_shadow, shadow) {
                blit(buf, w, h, &view, &set[cframe.min(set.len() - 1)], ax, ay, true);
            }
            // the Advancer flies 64 px up and bobs (decoded hover table); its shadow stays on the ground
            let ay_body = u.sprite_y().round() as i32;
            if let Some(set) = &sp.chassis {
                blit(buf, w, h, &view, &set[cframe.min(set.len() - 1)], ax, ay_body, false);
            }
            if let (Some(top), Some(aim)) = (&sp.top, &u.aim) {
                let steps = aim.params.steps;
                let (frame, flash) = match aim.firing() {
                    Some((seq, t)) => (seq.top_frame(t, aim.facing, steps) as usize, seq.flash_frame(t, aim.facing, steps)),
                    None => (aim.facing as usize, None),
                };
                if let (Some(set), true) = (&sp.top_shadow, shadow) {
                    // a top's shadow is drawn with the masked shade blit `0x462110` (the chassis shadow is not masked)
                    opensow_viewer::render::blit_shadow_masked(buf, w, h, &view, &set[frame.min(set.len() - 1)], ax, ay, &screen_mask);
                }
                blit(buf, w, h, &view, &top[frame.min(top.len() - 1)], ax, ay, false);
                if let (Some(f), Some(set)) = (flash, &sp.flash) {
                    if let Some(fr) = set.get(f as usize) {
                        blit(buf, w, h, &view, fr, ax, ay, false);
                    }
                }
            } else if let Some(top) = &sp.top {
                blit(buf, w, h, &view, &top[cframe.min(top.len() - 1)], ax, ay, false);
            }
        }
        if !ground_done {
            self.finish_ground(&mut fg_row, buf, w, h);
            self.draw_air_pass(assets, &order, &screen_mask, buf, w, h);
        }
        // overlays above all sprites: selection and health (the health bar only for one selected unit)
        let single = self.world.units.iter().filter(|u| u.selected && u.player == 0).count() == 1;
        for u in &self.world.units {
            let (sx, sy) = view.to_screen(u.x, u.sprite_y());
            let r = (16.0 * view.zoom) as i32;
            if u.selected {
                for (a, b, c, d) in [(sx - r, sy - r, sx - r + 5, sy - r + 1), (sx - r, sy - r, sx - r + 1, sy - r + 5), (sx + r - 5, sy - r, sx + r, sy - r + 1), (sx + r - 1, sy - r, sx + r, sy - r + 5), (sx - r, sy + r - 1, sx - r + 5, sy + r), (sx - r, sy + r - 5, sx - r + 1, sy + r), (sx + r - 5, sy + r - 1, sx + r, sy + r), (sx + r - 1, sy + r - 5, sx + r, sy + r)] {
                    rect(buf, w, h, a, b, c, d, (90, 255, 120), 255);
                }
            }
            if settings.hp_bars && u.selected && single {
                let (bw, top) = (2 * r, sy - r - 4);
                let fill = (bw as i64 * u.hp.max(0) as i64 / u.max_hp.max(1) as i64) as i32;
                health_line(buf, w, h, sx - r, top, bw, fill);
            }
        }
        // a building's health bar (`0x409400` -> `0x437140`): only while it is selected (clicked) or under the pointer
        let (mx, my) = self.to_map(self.cursor.0, self.cursor.1);
        let hovered = if self.edge_scroll && self.hud.as_ref().is_none_or(|l| l.hit(self.cursor.0, self.cursor.1).is_none()) { self.world.structure_at(mx, my) } else { None };
        for t in [self.shown_building(), hovered].into_iter().flatten().collect::<std::collections::BTreeSet<_>>() {
            let s = &self.world.structures[t];
            // width = max / scale (scale 144 for buildings, 72 for turrets, `0x40a981..0x40aa01`), centred on the footprint, at its top edge
            let scale = if matches!(s.kind, ObjectKind::Turret(_)) { 72 } else { 144 };
            let (fw, _) = s.size();
            let (cx, top) = (s.cell.0 * CELL + fw * CELL / 2, s.cell.1 * CELL);
            let (sx, sy) = view.to_screen(cx as f32, top as f32);
            let bw = ((s.max_hp / scale).max(1) as f32 * view.zoom) as i32;
            let fill = (((s.hp.max(0) / scale) as f32) * view.zoom) as i32;
            let col = if s.hp > s.max_hp / 2 { (98, 149, 66) } else if s.hp > s.max_hp / 4 { (181, 178, 25) } else { (197, 69, 8) };
            let (x0, y0) = (sx - bw / 2, sy);
            rect(buf, w, h, x0 - 1, y0 - 1, x0 + bw + 1, y0 + 5, (20, 20, 20), 255);
            rect(buf, w, h, x0, y0, x0 + bw, y0 + 4, (60, 60, 60), 255);
            rect(buf, w, h, x0, y0, x0 + fill, y0 + 4, col, 255);
            rect(buf, w, h, x0, y0, x0 + fill, y0 + 1, (col.0.saturating_add(90), col.1.saturating_add(70), col.2.saturating_add(60)), 255);
            // protected buildings (`[b+0x114]`) also get the string 146 (index 145) "SATELLITE PROTECTION" (`0x40952c..0x409650`): GDI text
            // (white, black shadow +2,+2) whose left edge is 46 px left of the footprint's middle, its top 6 px above the bar's first row
            // for the selected building (`0x40958a`) and 10 px for one that is only under the pointer (`0x40962c`)
            if s.protected {
                if let Some(text) = self.lang.as_ref().and_then(|l| l.get(opensow_formats::strings::SATELLITE_PROTECTION)) {
                    let z = view.zoom;
                    let lift = if self.shown_building() == Some(t) { 11.0 } else { 15.0 }; // the exe: 6 / 10; 5 px higher so the GDI stand-in does not cover the bar
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
    fn extra_frame(assets: &mut Assets, array: u32, index: usize) -> Option<(Set, usize)> {
        let (extra, offset) = extra_at(array)?;
        let set = assets.art.load(&format!("{}.ps6", extra.name))?;
        let i = offset + index;
        (i < set.len()).then_some((set, i))
    }

    /// Aircraft (placeholder shapes), drawn after the units.
    fn draw_aircraft(&self, buf: &mut [u32], w: usize, h: usize) {
        // air forces: a shadow on the ground and a body at the flight altitude (placeholder shapes; the art is not wired yet)
        for a in &self.world.aircraft {
            let (gx, gy) = self.view.to_screen(a.x as f32, a.y as f32);
            let (ax, ay) = self.view.to_screen(a.x as f32, (a.y - a.altitude) as f32);
            let r = ((14.0 * self.view.zoom) as i32).max(3);
            rect(buf, w, h, gx - r, gy - r / 3, gx + r, gy + r / 3, (0, 0, 0), 90);
            let c = if a.side == 0 { (90, 140, 255) } else { (120, 220, 120) };
            rect(buf, w, h, ax - r, ay - r / 2, ax + r, ay + r / 2, c, 255);
        }
    }

    /// Animation sprites (not the smoke puffs, which are drawn before the units: `draw_puffs`), then bullets (`0x4200f0..0x420232`):
    /// the exe draws both after the ground units and before the Advancers' bodies, so the Advancer's sparks come out from below it.
    fn draw_shots(&self, assets: &mut Assets, screen_mask: &[bool], buf: &mut [u32], w: usize, h: usize) {
        // the exe's screen mask (docs/effects.md): the smoke puffs (draw mode 8, `0x45b5e0`) and the darkening smoke (`0x45f4d0`: the
        // wrecks' smoke columns, mode 14) are drawn only where no building's mask frame and no foreground tile was stamped
        let masked = |routine: u32| matches!(routine, 0x45b5e0 | 0x45f4d0);
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
                    FrameRule::Variant268c(_) | FrameRule::Variant2680(_) => anim,
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
                let d = infobox::wrap_description(&font, &l.description);
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

    fn draw_hud(&self, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize, ui: f32) {
        let font = assets.font("small");
        let u = |v: f32| (v * ui).round() as i32;
        // countdown (`0x430d50`, called from `0x4209d3` with x = W/2, y = 13): "<string> mm:ss", left aligned at the screen's middle
        if let (Some((min, sec, _)), false) = (self.world.timer, self.world.timer_expired) {
            let label = self.lang.as_ref().and_then(|l| (0..400).filter_map(|i| l.get(i)).find(|t| t.trim().eq_ignore_ascii_case("remaining time"))).unwrap_or("Remaining Time");
            let text = format!("{} {min:02}:{sec:02}", label.trim()).to_uppercase();
            // `0x467060` passes text colour (255,255,255) and shadow (0,0,0) to `0x466f40` (shadow at +2,+2, as for messages)
            ui::text(buf, w, h, &font, &text, w as i32 / 2 + u(2.0), u(13.0) + u(2.0), ui, (0, 0, 0), (0, 0, 0));
            ui::text(buf, w, h, &font, &text, w as i32 / 2, u(13.0), ui, (255, 255, 255), (0, 0, 0));
        }
        // messages (`0x4036a0` draw, `0x404820` add, `0x4047a0` update, see `message_pose`): text colour (255,243,148),
        // shadow (74,4,0) at +2,+2
        let births: Vec<u32> = self.world.log.iter().map(|&(t, _)| t as u32).collect();
        for (k, &(t, id)) in self.world.log.iter().enumerate() {
            let Some(text) = opensow_formats::messages::lang_index(id).and_then(|i| self.lang.as_ref().and_then(|l| l.get(i))) else { continue };
            let text = text.trim();
            let tw = ui::text_width(&font, text, 1.0);
            let Some((x, y)) = message_pose(t as u32, tw, &births[k + 1..], self.world.tick as u32) else { continue };
            let y = h as i32 + u(y as f32);
            ui::text(buf, w, h, &font, text, u(x as f32) + u(2.0), y + u(2.0), ui, (74, 4, 0), (74, 4, 0));
            ui::text(buf, w, h, &font, text, u(x as f32), y, ui, (255, 243, 148), (0, 0, 0));
        }
        // the tutorial's instruction panel: the step's text word-wrapped to 300 px, the arrow button while it waits for a click
        if let (Some(step), true) = (self.tut_text, self.world.tutorial.is_some()) {
            if let Some(text) = self.lang.as_ref().and_then(|l| l.get(sim::tutorial_text_index(step))) {
                let (px, py, pw, ph) = self.tutorial_panel(w, h, ui);
                ui::dim_sub(buf, w, h, ui::Rect { x: px, y: py, w: pw, h: ph }, 40);
                let text = text.trim().trim_start_matches(['i', '!']).trim();
                let (mut line, mut y) = (String::new(), py + u(8.0));
                for word in text.split_whitespace() {
                    let next = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                    if ui::text_width(&font, &next, ui) > pw - u(16.0) && !line.is_empty() {
                        ui::text(buf, w, h, &font, &line, px + u(8.0), y, ui, (255, 255, 255), (0, 0, 0));
                        y += u(14.0);
                        line = word.to_string();
                    } else {
                        line = next;
                    }
                }
                ui::text(buf, w, h, &font, &line, px + u(8.0), y, ui, (255, 255, 255), (0, 0, 0));
                if self.world.tutorial.as_ref().is_some_and(|t| t.arrow) {
                    let a = u(28.0);
                    let r = ui::Rect { x: px + pw - a, y: py + ph - a, w: a, h: a };
                    ui::fill(buf, w, h, r, (40, 120, 40), 220);
                    ui::text(buf, w, h, &font, ">", r.x + a / 2 - u(3.0), r.y + a / 2 - u(6.0), ui, (255, 255, 255), (0, 0, 0));
                }
            }
        }
        // the hint box of missions 1 and 6 (`0x4303e0`): darkened box around the screen middle, white text (0xffffff)
        if let Some(hint) = self.world.hint {
            if let Some(text) = self.lang.as_ref().and_then(|l| l.get(hint.text)) {
                let (cx, cy) = (w as i32 / 2, h as i32 / 2);
                let r = ui::Rect { x: cx - u(hint.half as f32), y: cy - u(15.0), w: 2 * u(hint.half as f32), h: u(25.0) };
                ui::dim_sub(buf, w, h, r, 32);
                ui::text(buf, w, h, &font, text.trim(), cx - u(hint.text_dx as f32), cy - u(7.0), ui, (255, 255, 255), (0, 0, 0));
            }
        }
        if let Some(layout) = &self.hud {
            let state = hud::State { scene: &self.scene, view: &self.view, world: &self.world, placing: self.placing, strike: self.strike, hover: self.hud_hover, building: self.shown_building(), radar_black: self.radar_black };
            hud::draw(layout, &state, assets, buf, w, h);
            hud::draw_slots(layout, &state, assets, buf, w, h);
            let hs = layout.s;
            let (lines, desc) = self.info_text.as_ref().map_or((None, Vec::new()), |(_, l, d)| (Some(l.clone()), d.clone()));
            infobox::draw(&self.infobox, lines.as_ref(), &desc, assets, buf, w, h, hs);
        }
        if self.paused {
            self.draw_pause(assets, buf, w, h);
        }
    }

    /// The Space pause (`0x435330`, docs/pause.md): the frozen frame with every even pixel row black and the string 362 "P A U S E"
    /// (white, black shadow at +2,+2) with its left edge at `width / 2 - text width / 2` and its top at the screen's middle row. The exe
    /// uses a 28 px bold italic Times New Roman through GDI; the big bitmap font stands in.
    fn draw_pause(&self, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize) {
        for y in (0..h).step_by(2) {
            buf[y * w..(y + 1) * w].fill(0);
        }
        let font = assets.font("big");
        let text = self.lang.as_ref().and_then(|l| l.get(PAUSE_STRING - 1)).map_or("P A U S E", str::trim);
        let ui = (h as f32 / 768.0).max(1.0);
        let tw = ui::text_width(&font, text, ui);
        let (x, y) = ((w as i32 - tw) / 2, h as i32 / 2);
        ui::text(buf, w, h, &font, text, x + 2, y + 2, ui, (0, 0, 0), (0, 0, 0));
        ui::text(buf, w, h, &font, text, x, y, ui, (255, 255, 255), (0, 0, 0));
    }
}

/// The lang string of the pause (1-based id pushed at `0x43543a`).
const PAUSE_STRING: usize = 362;

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
fn health_line(buf: &mut [u32], w: usize, h: usize, x: i32, y: i32, width: i32, fill: i32) {
    rect(buf, w, h, x, y, x + width, y + 3, (10, 30, 10), 255);
    rect(buf, w, h, x, y, x + fill, y + 2, (170, 230, 150), 255);
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
