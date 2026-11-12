//! Developer tools: an overlay panel for testing. It does not exist in the original game.
//!
//! `F12` or the backquote key shows / hides the panel (hidden, only a small badge remains while a tool is active).
//! The panel can be dragged by its title bar. Tools:
//! - unlock every mission on the campaign map;
//! - game speed 1x / 2x / 5x / 10x;
//! - invulnerable units, infinite credits and research, frozen enemy (`sim::Cheats`);
//! - add air forces (bomber, asteroid, carryall, tripler, fighter) to the stock of either side (`World::air_stock`);
//! - place any unit, turret or building for either side with a click on the map (`Play::dev_spawn`; right click or Esc cancels).

use crate::assets::Assets;
use crate::sim::Cheats;
use crate::ui::{self, Rect};
use opensow_formats::font::Font;
use opensow_formats::level::UnitKind;
use std::rc::Rc;

/// A pair of bitmap fonts: inner mask and outline mask.
pub type Fonts = Rc<(Font, Font)>;

/// The game-speed multipliers the panel cycles through.
pub const SPEEDS: [u32; 4] = [1, 2, 5, 10];

/// Unit kinds that can be placed, with the short label of their button.
pub const KINDS: [(i32, &str); 22] = [
    (1, "AA 1"),
    (2, "AA 2"),
    (3, "AA 3"),
    (4, "ART 1"),
    (5, "ART 2"),
    (6, "ART 3"),
    (7, "CAN 1"),
    (8, "CAN 2"),
    (9, "CAN 3"),
    (10, "FLM 1"),
    (11, "FLM 2"),
    (12, "FLM 3"),
    (13, "SPC 1"),
    (14, "SPC 2"),
    (15, "SPC 3"),
    (40, "ADV"),
    (41, "WLK 1"),
    (42, "WLK 2"),
    (43, "WLK 3"),
    (44, "WLK 4"),
    (45, "WLK 5"),
    (46, "POD"),
];

/// Turrets and buildings that can be placed (object kinds, `ObjectKind::from_id`), with the label of their button.
pub const STRUCTS: [(i32, &str); 14] = [
    (25, "TUR 1"),
    (26, "TUR 2"),
    (27, "TUR 3"),
    (28, "TUR 4"),
    (29, "TUR 5"),
    (100, "HQ"),
    (101, "LFAC"),
    (102, "MFAC"),
    (103, "HFAC"),
    (104, "AIR"),
    (105, "GOLD"),
    (106, "LAB"),
    (107, "WIND"),
    (108, "WFAC"),
];

/// Whether an armed kind is a turret or building (placed with `World::dev_place_structure`) rather than a unit.
pub fn is_structure(kind: i32) -> bool {
    STRUCTS.iter().any(|e| e.0 == kind)
}

/// Air forces that can be added, in the order of `World::air_stock` (`HEADER_ORDER`).
pub const AIR: [&str; 5] = ["BOMBER", "ASTEROID", "CARRYALL", "TRIPLER", "FIGHTER"];

const COLUMNS: usize = 4;

/// The things on the panel that can be pointed at or clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Badge,
    Close,
    Title,
    UnlockAll,
    Invulnerable,
    Rich,
    Freeze,
    Speed(u32),
    Side(usize),
    Kind(i32),
    /// Adds one air force (index into `AIR`) to the stock of the chosen side.
    Air(usize),
}

/// The developer panel (key `` ` `` or F12): cheats, game speed and placing things.
#[derive(Debug, Clone)]
pub struct DevTools {
    /// The panel is shown.
    pub visible: bool,
    /// Every tech-level and turret is unlocked.
    pub unlock_all: bool,
    /// The player's units take no damage.
    pub invulnerable: bool,
    /// The player's credits and research stay at the maximum.
    pub rich: bool,
    /// The enemy does nothing.
    pub freeze_enemy: bool,
    /// Ticks per second multiplier (one of [`SPEEDS`]).
    pub speed: u32,
    /// Side of the placed units: 0 = the player, 1 = the enemy.
    pub side: usize,
    /// Top-left corner of the panel (None = default place).
    pos: Option<(i32, i32)>,
    /// Pointer offset inside the panel while the title bar is dragged.
    drag: Option<(i32, i32)>,
    /// The item under the pointer.
    pub hover: Option<Item>,
    /// The item a left press started on (it fires when the button is released over it).
    pressed: Option<Item>,
}

impl Default for DevTools {
    fn default() -> Self {
        DevTools {
            visible: false,
            unlock_all: false,
            invulnerable: false,
            rich: false,
            freeze_enemy: false,
            speed: 1,
            side: 0,
            pos: None,
            drag: None,
            hover: None,
            pressed: None,
        }
    }
}

struct Laid {
    panel: Rect,
    items: Vec<(Rect, Item)>,
    /// Section headers: (text, position).
    heads: Vec<(&'static str, i32, i32)>,
    hint: (i32, i32),
}

impl DevTools {
    /// The switches as the simulation's cheat flags.
    pub fn cheats(&self) -> Cheats {
        Cheats { invulnerable: self.invulnerable, rich: self.rich, freeze_enemy: self.freeze_enemy }
    }

    /// Anything switched on (the badge is shown while the panel is hidden).
    pub fn active(&self) -> bool {
        self.unlock_all || self.invulnerable || self.rich || self.freeze_enemy || self.speed != 1
    }

    /// Shows or hides the panel.
    pub fn toggle_visible(&mut self) {
        self.visible ^= true;
        self.drag = None;
        self.pressed = None;
        self.hover = None;
    }

    fn scale(h: usize) -> f32 {
        (h as f32 / 768.0).max(1.0)
    }

    fn layout(&self, fonts: &Fonts, w: usize, h: usize) -> Laid {
        let l = self.layout_at(fonts, w, h, None);
        // the default place moves up when the panel would run off the bottom (small windows)
        if self.pos.is_none() && l.panel.y + l.panel.h > h as i32 {
            let top = (h as i32 - l.panel.h).max(0);
            return self.layout_at(fonts, w, h, Some(top));
        }
        l
    }

    fn layout_at(&self, fonts: &Fonts, _w: usize, h: usize, top: Option<i32>) -> Laid {
        let s = Self::scale(h);
        let u = |v: i32| (v as f32 * s).round() as i32;
        let th = ui::text_height(fonts, s);
        let row = th + u(8);
        let gap = u(3);
        let pad = u(8);
        let pw = u(244);
        let inner = pw - 2 * pad;
        let mut items = Vec::new();
        let mut heads = Vec::new();
        let (px, py) = self.pos.unwrap_or((u(78), top.unwrap_or(u(70))));
        let mut y = py;
        let title = Rect { x: px, y, w: pw, h: row + u(4) };
        items.push((title, Item::Title));
        items.push((Rect { x: px + pw - row - u(2), y: y + u(2), w: row, h: row }, Item::Close));
        y += title.h + pad;
        let x0 = px + pad;
        let head = |heads: &mut Vec<(&'static str, i32, i32)>, y: &mut i32, text: &'static str| {
            heads.push((text, x0, *y));
            *y += th + u(4);
        };
        head(&mut heads, &mut y, "CAMPAIGN");
        items.push((Rect { x: x0, y, w: inner, h: row }, Item::UnlockAll));
        y += row + gap + u(4);
        head(&mut heads, &mut y, "CHEATS");
        for it in [Item::Invulnerable, Item::Rich, Item::Freeze] {
            items.push((Rect { x: x0, y, w: inner, h: row }, it));
            y += row + gap;
        }
        y += u(4);
        head(&mut heads, &mut y, "GAME SPEED");
        let bw = (inner - gap * (SPEEDS.len() as i32 - 1)) / SPEEDS.len() as i32;
        for (i, &sp) in SPEEDS.iter().enumerate() {
            items.push((Rect { x: x0 + i as i32 * (bw + gap), y, w: bw, h: row }, Item::Speed(sp)));
        }
        y += row + gap + u(4);
        head(&mut heads, &mut y, "PLACE UNIT");
        let bw = (inner - gap) / 2;
        for side in 0..2 {
            items.push((Rect { x: x0 + side as i32 * (bw + gap), y, w: bw, h: row }, Item::Side(side)));
        }
        y += row + gap;
        let bw = (inner - gap * (COLUMNS as i32 - 1)) / COLUMNS as i32;
        for (i, &(kind, _)) in KINDS.iter().enumerate() {
            let (c, r) = ((i % COLUMNS) as i32, (i / COLUMNS) as i32);
            items.push((Rect { x: x0 + c * (bw + gap), y: y + r * (row + gap), w: bw, h: row }, Item::Kind(kind)));
        }
        y += (KINDS.len().div_ceil(COLUMNS)) as i32 * (row + gap) + u(4);
        head(&mut heads, &mut y, "PLACE TURRET / BUILDING");
        for (i, &(kind, _)) in STRUCTS.iter().enumerate() {
            let (c, r) = ((i % COLUMNS) as i32, (i / COLUMNS) as i32);
            items.push((Rect { x: x0 + c * (bw + gap), y: y + r * (row + gap), w: bw, h: row }, Item::Kind(kind)));
        }
        y += (STRUCTS.len().div_ceil(COLUMNS)) as i32 * (row + gap) + u(4);
        head(&mut heads, &mut y, "ADD AIR FORCE");
        let bw = (inner - gap) / 2;
        for i in 0..AIR.len() {
            let (c, r) = ((i % 2) as i32, (i / 2) as i32);
            items.push((Rect { x: x0 + c * (bw + gap), y: y + r * (row + gap), w: bw, h: row }, Item::Air(i)));
        }
        y += (AIR.len().div_ceil(2)) as i32 * (row + gap) + u(2);
        let hint = (x0, y);
        y += th * 2 + u(6);
        let panel = Rect { x: px, y: py, w: pw, h: y + pad - py };
        Laid { panel, items, heads, hint }
    }

    /// Screen rectangle of an item of the shown panel (tests click on it).
    #[cfg(test)]
    pub fn rect_of(&self, fonts: &Fonts, item: Item, w: usize, h: usize) -> Option<Rect> {
        self.layout(fonts, w, h).items.iter().find(|e| e.1 == item).map(|e| e.0)
    }

    fn hit(&self, fonts: &Fonts, x: i32, y: i32, w: usize, h: usize) -> Option<Item> {
        if !self.visible {
            return self.badge(fonts, w, h).filter(|(r, _)| r.contains(x, y)).map(|_| Item::Badge);
        }
        let l = self.layout(fonts, w, h);
        l.items.iter().find(|(r, _)| r.contains(x, y)).map(|&(_, i)| i)
    }

    /// Is the pointer on the panel (or badge)? Clicks there never reach the game.
    pub fn covers(&self, fonts: &Fonts, x: i32, y: i32, w: usize, h: usize) -> bool {
        if self.visible {
            self.layout(fonts, w, h).panel.contains(x, y)
        } else {
            self.hit(fonts, x, y, w, h).is_some()
        }
    }

    /// Text and place of the badge shown while the panel is hidden and a tool is on.
    fn badge(&self, fonts: &Fonts, w: usize, h: usize) -> Option<(Rect, String)> {
        if self.visible || !self.active() {
            return None;
        }
        let s = Self::scale(h);
        let u = |v: i32| (v as f32 * s).round() as i32;
        let mut parts = vec!["DEV".to_owned()];
        if self.speed != 1 {
            parts.push(format!("{}X", self.speed));
        }
        for (on, t) in [(self.unlock_all, "UNLOCK"), (self.invulnerable, "GOD"), (self.rich, "$$$"), (self.freeze_enemy, "FREEZE")] {
            if on {
                parts.push(t.to_owned());
            }
        }
        let text = parts.join(" ");
        let (tw, th) = (ui::text_width(fonts, &text, s), ui::text_height(fonts, s));
        let r = Rect { x: u(8), y: h as i32 - th - u(14), w: tw + u(14), h: th + u(8) };
        let _ = w;
        Some((r, text))
    }

    /// Pointer moved; `true` when the panel took it. Dragging the title bar moves the panel.
    pub fn mouse_moved(&mut self, fonts: &Fonts, x: i32, y: i32, w: usize, h: usize) -> bool {
        if let Some((ox, oy)) = self.drag {
            let l = self.layout(fonts, w, h);
            let (nx, ny) = (x - ox, y - oy);
            self.pos = Some((nx.clamp(0, (w as i32 - l.panel.w).max(0)), ny.clamp(0, (h as i32 - l.panel.h).max(0))));
            return true;
        }
        self.hover = self.hit(fonts, x, y, w, h);
        self.covers(fonts, x, y, w, h)
    }

    /// Left button pressed; `true` when the panel took it.
    pub fn press(&mut self, fonts: &Fonts, x: i32, y: i32, w: usize, h: usize) -> bool {
        if !self.covers(fonts, x, y, w, h) {
            return false;
        }
        self.pressed = self.hit(fonts, x, y, w, h);
        if self.pressed == Some(Item::Title) {
            let l = self.layout(fonts, w, h);
            self.drag = Some((x - l.panel.x, y - l.panel.y));
        }
        true
    }

    /// Left button released; returns the item that was clicked, if any. `consumed` is whether the release belongs to the panel.
    pub fn release(&mut self, fonts: &Fonts, x: i32, y: i32, w: usize, h: usize) -> (bool, Option<Item>) {
        let started = self.pressed.take();
        self.drag = None;
        let Some(item) = started else { return (false, None) };
        if self.hit(fonts, x, y, w, h) != Some(item) {
            return (true, None);
        }
        match item {
            Item::Badge => self.visible = true,
            Item::Close => self.toggle_visible(),
            Item::UnlockAll => self.unlock_all ^= true,
            Item::Invulnerable => self.invulnerable ^= true,
            Item::Rich => self.rich ^= true,
            Item::Freeze => self.freeze_enemy ^= true,
            Item::Speed(s) => self.speed = s,
            Item::Side(s) => self.side = s,
            Item::Title | Item::Kind(_) | Item::Air(_) => {}
        }
        (true, Some(item))
    }

    /// Draws the panel (or the badge) over the finished frame. `armed` is the unit kind waiting for a map click.
    pub fn draw(&self, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize, armed: Option<i32>, in_mission: bool) {
        let fonts = assets.font("small");
        let s = Self::scale(h);
        let u = |v: i32| (v as f32 * s).round() as i32;
        let (white, ink) = ((255, 255, 255), (0, 0, 0));
        let (green, blue, yellow, grey) = ((0, 255, 0), (99, 150, 255), (255, 255, 120), (140, 150, 140));
        if let Some((r, text)) = self.badge(&fonts, w, h) {
            ui::fill(buf, w, h, r, (0, 14, 0), 200);
            frame(buf, w, h, r, (60, 200, 90));
            let th = ui::text_height(&fonts, s);
            ui::text(buf, w, h, &fonts, &text, r.x + u(7), r.y + (r.h - th) / 2, s, if self.hover == Some(Item::Badge) { yellow } else { green }, ink);
            return;
        }
        if !self.visible {
            return;
        }
        let l = self.layout(&fonts, w, h);
        let th = ui::text_height(&fonts, s);
        ui::fill(buf, w, h, l.panel, (0, 12, 0), 205);
        frame(buf, w, h, l.panel, (60, 200, 90));
        let label_y = |r: Rect| r.y + (r.h - th) / 2;
        let mut hint = "Pick a unit or building, click the map".to_owned();
        for &(r, item) in &l.items {
            let hot = self.hover == Some(item);
            let on = match item {
                Item::UnlockAll => self.unlock_all,
                Item::Invulnerable => self.invulnerable,
                Item::Rich => self.rich,
                Item::Freeze => self.freeze_enemy,
                Item::Speed(sp) => self.speed == sp,
                Item::Side(sd) => self.side == sd,
                Item::Kind(k) => armed == Some(k),
                _ => false,
            };
            let usable = !matches!(item, Item::Kind(_) | Item::Air(_)) || in_mission;
            match item {
                Item::Title => {
                    ui::fill(buf, w, h, r, (30, 110, 50), 150);
                    ui::text(buf, w, h, &fonts, "DEV TOOLS", r.x + u(8), label_y(r), s, yellow, ink);
                    ui::text(buf, w, h, &fonts, "F12", r.x + r.w - u(60) - ui::text_width(&fonts, "F12", s), label_y(r), s, grey, ink);
                }
                Item::Close => {
                    ui::fill(buf, w, h, r, (60, 220, 90), if hot { 90 } else { 30 });
                    let c = if hot { yellow } else { white };
                    ui::text(buf, w, h, &fonts, "X", r.x + (r.w - ui::text_width(&fonts, "X", s)) / 2, label_y(r), s, c, ink);
                }
                Item::UnlockAll | Item::Invulnerable | Item::Rich | Item::Freeze => {
                    if hot {
                        ui::fill(buf, w, h, r, (60, 220, 90), 70);
                    }
                    let b = Rect { x: r.x + u(4), y: r.y + (r.h - u(12)) / 2, w: u(12), h: u(12) };
                    frame(buf, w, h, b, if on { green } else { (60, 200, 90) });
                    if on {
                        ui::fill(buf, w, h, Rect { x: b.x + u(3), y: b.y + u(3), w: b.w - u(6), h: b.h - u(6) }, green, 255);
                    }
                    let text = match item {
                        Item::UnlockAll => "Unlock all missions",
                        Item::Invulnerable => "Invulnerable units",
                        Item::Rich => "Infinite credits + research",
                        _ => "Freeze enemy",
                    };
                    ui::text(buf, w, h, &fonts, text, r.x + u(22), label_y(r), s, if on { white } else { (200, 210, 200) }, ink);
                }
                Item::Speed(_) | Item::Side(_) | Item::Kind(_) | Item::Air(_) | Item::Badge => {
                    let text = match item {
                        Item::Speed(sp) => format!("{sp}X"),
                        Item::Side(0) => "MINE".into(),
                        Item::Side(_) => "ENEMY".into(),
                        Item::Kind(k) => KINDS.iter().chain(STRUCTS.iter()).find(|e| e.0 == k).map_or("?", |e| e.1).into(),
                        Item::Air(i) => AIR[i].into(),
                        Item::Badge => String::new(),
                        _ => unreachable!(),
                    };
                    let base = if matches!(item, Item::Side(1)) { (220, 70, 60) } else { (60, 200, 90) };
                    ui::fill(buf, w, h, r, if on { base } else { (0, 40, 10) }, if on { 110 } else { 150 });
                    frame(buf, w, h, r, if on { green } else { (40, 120, 60) });
                    if hot && usable {
                        ui::fill(buf, w, h, r, (60, 220, 90), 60);
                    }
                    let colour = if !usable {
                        (90, 100, 90)
                    } else if on {
                        yellow
                    } else {
                        white
                    };
                    ui::text(buf, w, h, &fonts, &text, r.x + (r.w - ui::text_width(&fonts, &text, s)) / 2, label_y(r), s, colour, ink);
                    if hot {
                        if let Item::Air(_) = item {
                            hint = if in_mission { "Click: +1 in the stock".to_owned() } else { "Start a mission first".to_owned() };
                        }
                        if let Item::Kind(k) = item {
                            hint = if in_mission { kind_label(k) } else { "Start a mission first".to_owned() };
                        }
                    }
                }
            }
        }
        if armed.is_some() {
            hint = "Click the map to place. Right click: stop".to_owned();
        }
        for &(text, x, y) in &l.heads {
            ui::text(buf, w, h, &fonts, text, x, y, s, blue, ink);
        }
        let sc = s * 0.9;
        ui::text(buf, w, h, &fonts, &hint, l.hint.0, l.hint.1, sc, if armed.is_some() { yellow } else { grey }, ink);
        let side = if self.side == 0 { "own side" } else { "ENEMY side" };
        ui::text(buf, w, h, &fonts, &format!("Units, buildings and air forces: {side}"), l.hint.0, l.hint.1 + th + u(2), sc, grey, ink);
    }
}

fn frame(buf: &mut [u32], w: usize, h: usize, r: Rect, c: (u8, u8, u8)) {
    for edge in [
        Rect { x: r.x, y: r.y, w: r.w, h: 1 },
        Rect { x: r.x, y: r.y + r.h - 1, w: r.w, h: 1 },
        Rect { x: r.x, y: r.y, w: 1, h: r.h },
        Rect { x: r.x + r.w - 1, y: r.y, w: 1, h: r.h },
    ] {
        ui::fill(buf, w, h, edge, c, 255);
    }
}

fn building_name(kind: i32) -> &'static str {
    match kind {
        100 => "Headquarters",
        101 => "Light Factory",
        102 => "Medium Factory",
        103 => "Heavy Factory",
        104 => "Air Base",
        105 => "Gold Mine",
        106 => "Research Lab",
        107 => "Wind Trap",
        _ => "Walker Factory",
    }
}

/// Name of an armed unit kind shown next to the pointer cell.
pub fn kind_label(kind: i32) -> String {
    match STRUCTS.iter().find(|e| e.0 == kind) {
        Some(&(k, _)) if k < 100 => format!("Turret {} (any cell, full hit points)", k - 24),
        Some(&(k, _)) => format!("{} (any cell, no production slots)", building_name(k)),
        None => UnitKind::describe(kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cheats_follow_the_switches() {
        let mut d = DevTools::default();
        assert!(!d.active());
        d.rich = true;
        d.freeze_enemy = true;
        assert!(d.active());
        let c = d.cheats();
        assert!(c.rich && c.freeze_enemy && !c.invulnerable);
        d.rich = false;
        d.freeze_enemy = false;
        d.speed = 5;
        assert!(d.active(), "a speed-up counts as active");
    }

    #[test]
    fn every_listed_structure_is_a_known_object() {
        for (k, label) in STRUCTS {
            assert!(opensow_formats::level::ObjectKind::from_id(k) != opensow_formats::level::ObjectKind::Unknown(k), "{label}");
            assert!(opensow_formats::buildings::footprint(k).is_some(), "{label}");
            assert!(is_structure(k) && !KINDS.iter().any(|e| e.0 == k));
        }
    }

    #[test]
    fn every_listed_kind_has_a_name() {
        for (k, label) in KINDS {
            assert!(opensow_formats::units::unit_art(k).is_some(), "{label}");
            assert!(opensow_formats::level::UnitKind::max_hp(k).is_some(), "{label}");
        }
    }
}
