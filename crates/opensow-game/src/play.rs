//! The in-game screen: camera, selection, orders, and drawing the world on top of the viewer's map renderer.

use crate::assets::Assets;
use crate::menu::Settings;
use crate::sim::{self, Outcome, World, BLAST_TICKS, CELL};
use crate::ui::{self, Rect};
use opensow_formats::level::{ObjectKind, Owner};
use opensow_formats::recoil::FLASH_SETS;
use opensow_formats::rings::Rings;
use opensow_formats::sprite::Frame;
use opensow_formats::units::unit_art;
use opensow_viewer::art::Source;
use opensow_viewer::render::{blit, pack, rect, render, Overlays, View};
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

pub struct Play {
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
    /// Air force waiting for a click on the map (keys B / F / C / Y / M).
    pub strike: Option<opensow_formats::air::AirForce>,
    /// The language strings (messages are shown for a few seconds in the HUD).
    lang: Option<opensow_formats::lang::LangFile>,
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
        // Units are drawn by the game from the simulation; the loader's static unit sprites go away.
        scene.placed.retain(|p| matches!(p.source, Source::Object(_)));
        let mut play = Play {
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
            strike: None,
            lang: assets.game.resolve("Data\\Language\\langEN.txt").and_then(|p| std::fs::read(p).ok()).and_then(|d| opensow_formats::lang::LangFile::parse(&d).ok()),
        };
        play.world.mission_no = level.trim_start_matches(['M', 'm']).parse().ok();
        // the tutorial (M00) runs its own script and has no computer opponent
        if level.eq_ignore_ascii_case("M00") {
            play.world.tutorial_step = Some(0);
            play.world.ai = false;
        }
        play.world.select_all();
        play.world.clear_selection();
        Ok(play)
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
        self.drag = Some((x, y));
    }

    pub fn left_up(&mut self, x: i32, y: i32, additive: bool) {
        let Some((sx, sy)) = self.drag.take() else { return };
        if (x - sx).abs() > 6 || (y - sy).abs() > 6 {
            let (a, b) = (self.to_map(sx, sy), self.to_map(x, y));
            self.world.select_rect(a.0, a.1, b.0, b.1, additive);
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
            // a click on a building with the Advancer selected sends it there (capture / upgrade / repair)
            if let Some(t) = self.world.structure_at(p.0, p.1) {
                let advancers: Vec<u32> = self.world.player_units().filter(|u| u.selected && u.kind == 40).map(|u| u.id).collect();
                let mut ordered = false;
                for id in advancers {
                    ordered |= self.world.order_action(id, t);
                }
                if ordered {
                    return;
                }
            }
            self.world.select_at(p.0, p.1, 22.0, additive);
        }
    }

    pub fn right_click(&mut self, x: i32, y: i32) {
        let (mx, my) = self.to_map(x, y);
        // right-click on an enemy attacks it, anywhere else moves
        match self.world.enemy_at(mx, my) {
            Some(t) => self.world.order_attack_selected(t),
            None => self.world.order_move_selected(mx, my),
        }
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
        let speed = 700.0 * dt / self.view.zoom;
        let (mut dx, mut dy) = (0.0, 0.0);
        let edge = 6;
        let at = self.edge_scroll;
        if self.keys.contains("left") || (at && self.cursor.0 < edge) {
            dx -= speed;
        }
        if self.keys.contains("right") || (at && self.cursor.0 >= w as i32 - edge) {
            dx += speed;
        }
        if self.keys.contains("up") || (at && self.cursor.1 < edge) {
            dy -= speed;
        }
        if self.keys.contains("down") || (at && self.cursor.1 >= h as i32 - edge) {
            dy += speed;
        }
        if dx != 0.0 || dy != 0.0 {
            self.view.x += dx;
            self.view.y += dy;
        }
        self.view.clamp(&self.scene, w, h);
        if self.paused || self.world.outcome.is_some() {
            return;
        }
        self.time += dt;
        let tick = 1.0 / settings.ticks_per_second as f32;
        let mut n = 0;
        while self.time >= tick && n < 8 {
            self.time -= tick;
            self.world.step();
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

    pub fn draw(&mut self, assets: &mut Assets, settings: &Settings, buf: &mut [u32], w: usize, h: usize, ui_scale: f32) {
        self.limit_zoom(w, h);
        if self.fit {
            self.centre_on_player(w, h);
            self.fit = false;
        }
        // destroyed buildings and turrets disappear
        let dead: HashSet<usize> = self.world.structures.iter().filter(|s| !s.alive()).map(|s| s.object).collect();
        if !dead.is_empty() {
            self.scene.placed.retain(|p| !matches!(p.source, Source::Object(i) if dead.contains(&i)));
        }
        let overlays = Overlays { blocked: false, grid: false, objects: false, markers: false, sprites: true, muzzles: false, walk: None, fire: None };
        render(&self.scene, &self.view, overlays, buf, w, h);

        if settings.show_range {
            self.draw_ranges(buf, w, h);
        }
        self.draw_units(assets, settings, buf, w, h);
        self.draw_bullets(buf, w, h);
        self.draw_hud(assets, buf, w, h, ui_scale);

        if let Some((sx, sy)) = self.drag {
            let (x0, y0, x1, y1) = (sx.min(self.cursor.0), sy.min(self.cursor.1), sx.max(self.cursor.0), sy.max(self.cursor.1));
            if x1 - x0 > 6 || y1 - y0 > 6 {
                rect(buf, w, h, x0, y0, x1, y1, (80, 255, 120), 40);
                for (a, b, c, d) in [(x0, y0, x1, y0 + 1), (x0, y1 - 1, x1, y1), (x0, y0, x0 + 1, y1), (x1 - 1, y0, x1, y1)] {
                    rect(buf, w, h, a, b, c, d, (120, 255, 150), 255);
                }
            }
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

    fn draw_units(&mut self, assets: &mut Assets, settings: &Settings, buf: &mut [u32], w: usize, h: usize) {
        let mut order: Vec<usize> = (0..self.world.units.len()).collect();
        order.sort_by(|&a, &b| self.world.units[a].y.total_cmp(&self.world.units[b].y));
        for &i in &order {
            let (kind, player) = (self.world.units[i].kind, self.world.units[i].player);
            self.sprites_for(assets, kind, player);
        }
        let view = self.view;
        for &i in &order {
            let u = &self.world.units[i];
            let Some(sp) = self.sprites.get(&(u.kind, u.player == 0)) else { continue };
            let (ax, ay) = u.px();
            let cframe = u.chassis.as_ref().map_or(0, |c| c.frame() as usize);
            if let Some(set) = &sp.chassis_shadow {
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
                if let Some(set) = &sp.top_shadow {
                    blit(buf, w, h, &view, &set[frame.min(set.len() - 1)], ax, ay, true);
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
        // overlays above all sprites: selection and health
        for u in &self.world.units {
            let (sx, sy) = view.to_screen(u.x, u.sprite_y());
            let r = (16.0 * view.zoom) as i32;
            if u.selected {
                for (a, b, c, d) in [(sx - r, sy - r, sx - r + 5, sy - r + 1), (sx - r, sy - r, sx - r + 1, sy - r + 5), (sx + r - 5, sy - r, sx + r, sy - r + 1), (sx + r - 1, sy - r, sx + r, sy - r + 5), (sx - r, sy + r - 1, sx - r + 5, sy + r), (sx - r, sy + r - 5, sx - r + 1, sy + r), (sx + r - 5, sy + r - 1, sx + r, sy + r), (sx + r - 1, sy + r - 5, sx + r, sy + r)] {
                    rect(buf, w, h, a, b, c, d, (90, 255, 120), 255);
                }
            }
            if settings.hp_bars && (u.selected || u.hp < u.max_hp) {
                let (bw, top) = (2 * r, sy - r - 4);
                let fill = (bw as i64 * u.hp.max(0) as i64 / u.max_hp.max(1) as i64) as i32;
                health_line(buf, w, h, sx - r, top, bw, fill);
            }
        }
        // production progress bars over the player's factories
        for s in self.world.structures.iter().filter(|s| s.alive() && s.owner == Owner::Player && s.active > 0) {
            let (cx, cy) = s.center();
            let (sx, sy) = view.to_screen(cx as f32, cy as f32);
            let r = (22.0 * view.zoom) as i32;
            for (j, slot) in s.slots.iter().take(s.active).enumerate() {
                let y = sy + r + 4 + j as i32 * 4;
                let fill = (2 * r as i64 * slot.progress as i64 / slot.target.max(1) as i64) as i32;
                rect(buf, w, h, sx - r, y, sx + r, y + 3, (10, 30, 10), 255);
                rect(buf, w, h, sx - r, y, sx - r + fill, y + 2, (255, 200, 80), 255);
            }
        }
        // the Advancer's beam while it works on a building
        for u in self.world.units.iter().filter(|u| u.kind == 40 && u.hp > 0) {
            let Some((action, t)) = u.action else { continue };
            if !u.path.is_empty() || u.fire_cooldown + 4 < sim::ADVANCER_BEAM_TICKS {
                continue;
            }
            let (cx, cy) = self.world.structures[t].center();
            let (x0, y0) = view.to_screen(u.x, u.sprite_y());
            let (x1, y1) = view.to_screen(cx as f32, cy as f32);
            let color = match action {
                opensow_formats::advancer::Action::Destroy => (255, 90, 60),
                opensow_formats::advancer::Action::Repair => (120, 255, 140),
                opensow_formats::advancer::Action::Upgrade => (255, 220, 90),
            };
            let n = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
            for k in 0..=n {
                let (px, py) = (x0 + (x1 - x0) * k / n, y0 + (y1 - y0) * k / n);
                rect(buf, w, h, px - 1, py - 1, px + 1, py + 1, color, 230);
            }
        }
        for s in self.world.structures.iter().filter(|s| s.alive() && s.hp < s.max_hp) {
            let (cx, cy) = s.center();
            let (sx, sy) = view.to_screen(cx as f32, cy as f32);
            let r = (22.0 * view.zoom) as i32;
            let fill = (2 * r as i64 * s.hp as i64 / s.max_hp.max(1) as i64) as i32;
            health_line(buf, w, h, sx - r, sy - r - 6, 2 * r, fill);
        }
    }

    fn draw_bullets(&self, buf: &mut [u32], w: usize, h: usize) {
        // air forces: a shadow on the ground and a body at the flight altitude (placeholder shapes; the art is not wired yet)
        for a in &self.world.aircraft {
            let (gx, gy) = self.view.to_screen(a.x as f32, a.y as f32);
            let (ax, ay) = self.view.to_screen(a.x as f32, (a.y - a.altitude) as f32);
            let r = ((14.0 * self.view.zoom) as i32).max(3);
            rect(buf, w, h, gx - r, gy - r / 3, gx + r, gy + r / 3, (0, 0, 0), 90);
            let c = if a.side == 0 { (90, 140, 255) } else { (120, 220, 120) };
            rect(buf, w, h, ax - r, ay - r / 2, ax + r, ay + r / 2, c, 255);
        }
        for b in &self.world.bullets {
            let (x, y) = b.position();
            let (sx, sy) = self.view.to_screen(x as f32, y as f32);
            let r = ((2.5 * self.view.zoom) as i32).max(1);
            rect(buf, w, h, sx - r, sy - r, sx + r + 1, sy + r + 1, (255, 235, 120), 255);
        }
        for b in &self.world.blasts {
            let (sx, sy) = self.view.to_screen(b.x as f32, b.y as f32);
            let t = b.age as f32 / BLAST_TICKS as f32;
            let r = ((6.0 + 22.0 * t) * self.view.zoom) as i32;
            rect(buf, w, h, sx - r, sy - r, sx + r, sy + r, (255, (190.0 - 120.0 * t) as u8, 40), ((1.0 - t) * 170.0) as u32);
            let r2 = r / 2;
            rect(buf, w, h, sx - r2, sy - r2, sx + r2, sy + r2, (255, 245, 190), ((1.0 - t) * 220.0) as u32);
        }
    }

    fn draw_hud(&self, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize, ui: f32) {
        let font = assets.font("small");
        let player = self.world.units.iter().filter(|u| u.player == 0).count();
        let enemy = self.world.units.iter().filter(|u| u.player != 0).count();
        let turrets = self.world.structures.iter().filter(|s| s.alive() && s.owner == opensow_formats::level::Owner::Enemy && matches!(s.kind, ObjectKind::Turret(_))).count();
        let selected = self.world.units.iter().filter(|u| u.selected).count();
        let st = self.world.air_stock[0];
        let line = format!("Credits {}   Research {}   Air B{} M{} C{} Y{} F{}   Mission {}   your units {player}   enemy units {enemy}   enemy turrets {turrets}   selected {selected}{}{}", self.world.credits[0], self.world.research[0], st[0], st[1], st[2], st[3], st[4], self.level.trim_start_matches(['M', 'm']), self.placing.map_or(String::new(), |t| format!("   PLACE TURRET {t}")) + &self.strike.map_or(String::new(), |f| format!("   AIR STRIKE: {}", f.name())), if self.paused { "   PAUSED" } else { "" });
        let bar = (26.0 * ui) as i32;
        ui::fill(buf, w, h, Rect { x: 0, y: 0, w: w as i32, h: bar }, (0, 0, 0), 150);
        ui::text(buf, w, h, &font, &line, (10.0 * ui) as i32, (5.0 * ui) as i32, ui, (235, 255, 235), (0, 0, 0));
        let hint = "Left click/drag select   Right click move/attack   Advancer: click a building, R repair/upgrade   1-5 turret   T tech-level   B/M/C/Y/F air strike   WASD/arrows scroll   wheel zoom   Q select all   Space pause   Esc menu";
        let foot = (22.0 * ui) as i32;
        ui::fill(buf, w, h, Rect { x: 0, y: h as i32 - foot, w: w as i32, h: foot }, (0, 0, 0), 130);
        ui::text(buf, w, h, &font, hint, (10.0 * ui) as i32, h as i32 - foot + (4.0 * ui) as i32, 0.9 * ui, (190, 210, 190), (0, 0, 0));
        // the newest message of the last five seconds
        if let Some(&(t, id)) = self.world.log.last() {
            if self.world.tick.saturating_sub(t) < 300 {
                if let Some(text) = opensow_formats::messages::lang_index(id).and_then(|i| self.lang.as_ref().and_then(|l| l.get(i))) {
                    ui::text(buf, w, h, &font, text.trim(), (10.0 * ui) as i32, (30.0 * ui) as i32, 1.2 * ui, (255, 230, 120), (0, 0, 0));
                }
            }
        }
        let _ = pack;
    }
}

/// The original's health bar: a thin pale-green line over a dark one, no frame; the lost part is dark.
fn health_line(buf: &mut [u32], w: usize, h: usize, x: i32, y: i32, width: i32, fill: i32) {
    rect(buf, w, h, x, y, x + width, y + 3, (10, 30, 10), 255);
    rect(buf, w, h, x, y, x + fill, y + 2, (170, 230, 150), 255);
}
