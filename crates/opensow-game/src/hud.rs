//! The in-game HUD, laid out the way the exe does it (docs/hud.md has the addresses). Everything is positioned in the
//! original's own pixels (a 640x480 or 1024x768 screen) from the screen's edges and multiplied by one scale factor.
//!
//! - counters (`0x454570`, `0x430920`, `0x430b50`): credits tile at (3,3) with five digits at x+75+10k, research tile
//!   at (143,3) - or at (3,3) when the credits tile is hidden - with four digits at x+87+9k; digits at y+2; the tiles
//!   show once a player has credits / research or owns a gold mine / research lab;
//! - radar (`0x454610`): frame at x = W-11-mapW, y = -1 (frame 0 for the 64x64 missions, frame 1 otherwise), the map
//!   at 1 px per cell inside it at (+6,+6), picture `radar.ps6` frame 5+mission, buildings as blinking footprint
//!   rectangles, units as 5-pixel crosses, the camera as a white rectangle;
//! - air pills (`0x453e80`, `0x453f00`, `0x455520`): `ikony` frame 5+kind (kinds 30..34) centred at x=31, y=60+51n
//!   in the order they appeared (the level header's order at the start), the stock count in `panel2nu` digits;
//! - turret tabs (`0x4551f0`, `0x454e60`): `ikony` 25..29, centred at x = W-11-57j, y = H-21, one per tier the level
//!   allows (header bytes), j counting from the right.

use crate::assets::Assets;
use crate::sim::World;
use crate::ui::{self, Rect};
use opensow_formats::air::AirForce;
use opensow_formats::level::Owner;
use opensow_formats::rally;
use opensow_formats::sprite::rgb565_to_rgb8;
use opensow_viewer::render::{rect, View};

/// `radar.ps6` uses frame 0 (76x76) for these missions' 64x64 maps and frame 1 (108x92) for the rest (`0x45463e`).
pub const SMALL_RADAR: [u32; 9] = [1, 2, 6, 9, 11, 12, 14, 16, 20];
/// Warmonger's exe tests its own list for the small frame (`0x44eb11`, `0x44f5d9`).
pub const SMALL_RADAR_WARMONGER: [u32; 5] = [1, 4, 6, 11, 20];

/// Warmonger's radar picture: frame of `radar2.tsp` for a mission (byte table `0x44efc0` -> jump table `0x44ef90`): its eleven
/// missions in order, any other number falls through with frame 0.
pub fn radar2_frame(mission: u32) -> usize {
    [1, 2, 3, 4, 6, 7, 11, 12, 15, 20, 22].iter().position(|&m| m == mission).unwrap_or(0)
}

/// What a point of the HUD does when clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// Part of the HUD without an action (swallows the click).
    Panel,
    Radar,
    Turret(u32),
    Air(AirForce),
    /// Slot `i` of the selected building: its icon (hover only).
    Slot(usize),
    /// The arrow left of slot `i`'s icon (`0x454ba0`): the click that buys the tech-level.
    SlotBuy(usize),
    /// The "RALLY POINT" button above the top slot icon (widget 0x7b, `0x416cf9`).
    Rally,
}

/// What decides which HUD parts exist.
pub struct Inputs {
    /// The campaign mission number, `None` outside the campaign.
    pub mission: Option<u32>,
    /// The Warmonger campaign is played (its own radar list and pictures).
    pub warmonger: bool,
    /// Map size in cells (for the radar).
    pub map_cells: (i32, i32),
    /// Whether the credits readout exists.
    pub show_credits: bool,
    /// Whether the research readout exists.
    pub show_research: bool,
    /// Turret tiers (1-5) the level allows, in tab order from the right.
    pub tiers: Vec<u32>,
    /// Air forces in pill order from the top.
    pub pills: Vec<AirForce>,
    /// The info box frame while it is on screen (it swallows clicks).
    pub info: Option<Rect>,
    /// Production slots of the selected building shown as icons along the right edge.
    pub slots: usize,
    /// Per slot: the tech-level arrow exists (level < limit, `[slot+0x20] < [slot+0x24]`).
    pub buy: Vec<bool>,
    /// The selected building is an own factory: the rally point button exists (`0x455dbe`).
    pub rally_button: bool,
}

/// Where every HUD part sits for one window size, in window pixels.
#[derive(Clone, Debug)]
pub struct Layout {
    /// Window pixels per pixel of the original's HUD art.
    pub s: f32,
    /// The campaign mission number, `None` outside the campaign.
    pub mission: Option<u32>,
    /// Position of the credits readout, when it exists.
    pub credits: Option<(i32, i32)>,
    /// Position of the research readout, when it exists.
    pub research: Option<(i32, i32)>,
    /// Top-left corner of the radar frame.
    pub radar_frame: (i32, i32),
    /// Which `radar.ps6` frame is the radar frame (0 or 1).
    pub radar_frame_index: usize,
    /// Options RADAR switch (`+8`): the exe creates, draws, updates and hit-tests the radar widget only when it is on.
    pub radar_on: bool,
    /// Map area inside the radar frame, 1 cell = `s` pixels.
    pub radar: Rect,
    /// Map size in cells.
    pub map_cells: (i32, i32),
    /// Tabs from the right: (tier, click rect, sprite top-left).
    pub tabs: Vec<(u32, Rect, (i32, i32))>,
    /// Pills from the top: (force, click rect, centre x, centre y).
    pub pills: Vec<(AirForce, Rect, i32, i32)>,
    /// The pills are disabled (3 s after a launch, widget state 2, `0x454e4e`): drawn half-additive (`0x45c220`), no info box and
    /// no click (`0x454d02`), the hover glow and sound stay (`0x456350` updates the widget regardless of its state).
    pub pills_locked: bool,
    /// The info box's area, when it is open.
    pub info: Option<Rect>,
    /// Slot icons: (click rect, centre x, centre y).
    pub slots: Vec<(Rect, i32, i32)>,
    /// Tech-level arrows: (click rect, centre x, centre y); `None` where the slot has no arrow.
    pub arrows: Vec<Option<(Rect, i32, i32)>>,
    /// The rally point button: (click rect, hot spot x, hot spot y).
    pub rally: Option<(Rect, i32, i32)>,
}

/// `ikony` frame of an air-force pill (`0x453e80`: kind 30..34 -> 35..39).
pub fn pill_frame(f: AirForce) -> usize {
    35 + (f.code() - 30) as usize
}

impl Layout {
    /// Lays the HUD out for a `w` x `h` window at scale `s`.
    pub fn new(w: usize, h: usize, s: f32, i: &Inputs) -> Layout {
        let u = |v: i32| (v as f32 * s).round() as i32;
        let (w, h) = (w as i32, h as i32);
        let small: &[u32] = if i.warmonger { &SMALL_RADAR_WARMONGER } else { &SMALL_RADAR };
        let frame_idx = if i.mission.is_some_and(|m| small.contains(&m)) { 0 } else { 1 };
        let radar_frame = (w - u(11) - u(i.map_cells.0), -u(1));
        let radar = Rect { x: radar_frame.0 + u(6), y: radar_frame.1 + u(6), w: u(i.map_cells.0), h: u(i.map_cells.1) };
        let credits = i.show_credits.then_some((u(3), u(3)));
        let research = i.show_research.then(|| if i.show_credits { (u(143), u(3)) } else { (u(3), u(3)) });
        let tabs = i
            .tiers
            .iter()
            .enumerate()
            .map(|(j, &t)| {
                let (cx, cy) = (w - u(11) - u(57) * j as i32, h - u(21));
                (t, Rect { x: cx - u(44), y: cy - u(21), w: u(57), h: u(42) }, (cx - u(46), cy - u(21)))
            })
            .collect();
        let pills = i
            .pills
            .iter()
            .enumerate()
            .map(|(n, &f)| {
                let (cx, cy) = (u(31), u(60) + u(51) * n as i32);
                (f, Rect { x: cx - u(30), y: cy - u(26), w: u(58), h: u(49) }, cx, cy)
            })
            .collect();
        // slot icons (`0x455d03..0x455d91`): a column at x = W-28, centred on the screen's middle line, 60 px apart
        let n = i.slots as i32;
        // y0 is the exe's H/2 + 50 + ..., with H the screen height: the middle line stays in screen pixels, the offsets scale
        let mut off = 0x32 + (0x3c - 60 * n) / 2;
        if (w as f32 / s).round() as i32 == 640 {
            off += 0xf;
            if n >= 4 {
                off += 0xa;
            }
        }
        let slots: Vec<(Rect, i32, i32)> = (0..n)
            .map(|k| {
                let (cx, cy) = (w - u(28), h / 2 + u(off) + u(60) * k);
                (Rect { x: cx - u(26), y: cy - u(29), w: u(54), h: u(56) }, cx, cy)
            })
            .collect();
        // the arrow (`0x455d68`): centre 40 left of the icon and 3 above, click rect x -15..14, y -16..20 (`0x4551ae`)
        let arrows = slots
            .iter()
            .enumerate()
            .map(|(k, (_, cx, cy))| {
                i.buy.get(k).copied().unwrap_or(false).then(|| {
                    let (ax, ay) = (cx - u(40), cy - u(3));
                    (Rect { x: ax - u(15), y: ay - u(16), w: u(30), h: u(37) }, ax, ay)
                })
            })
            .collect();
        // the rally button (`0x455dcf`): hot spot = the top icon's position + (4, -41), exclusive hit box (-25..25, -11..12)
        let rally = (i.rally_button && n > 0).then(|| {
            let (_, cx, cy) = slots[0];
            let (px, py) = (cx + u(rally::BUTTON_OFFSET.0), cy + u(rally::BUTTON_OFFSET.1));
            let (x0, y0, x1, y1) = rally::BUTTON_RECT;
            (Rect { x: px + u(x0) + 1, y: py + u(y0) + 1, w: u(x1 - x0) - 1, h: u(y1 - y0) - 1 }, px, py)
        });
        Layout {
            radar_on: true,
            s,
            mission: i.mission,
            credits,
            research,
            radar_frame,
            radar_frame_index: frame_idx,
            radar,
            map_cells: i.map_cells,
            tabs,
            pills,
            info: i.info,
            slots,
            arrows,
            rally,
            pills_locked: false,
        }
    }

    /// What lies at a window position; `None` outside the HUD.
    pub fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        if self.rally.is_some_and(|(r, ..)| r.contains(x, y)) {
            return Some(Hit::Rally);
        }
        if let Some(i) = self.arrows.iter().position(|a| a.is_some_and(|(r, ..)| r.contains(x, y))) {
            return Some(Hit::SlotBuy(i));
        }
        if let Some(i) = self.slots.iter().position(|(r, ..)| r.contains(x, y)) {
            return Some(Hit::Slot(i));
        }
        if self.info.is_some_and(|f| f.contains(x, y)) {
            return Some(Hit::Panel);
        }
        let u = |v: i32| (v as f32 * self.s).round() as i32;
        if self.radar_on {
            if self.radar.contains(x, y) {
                return Some(Hit::Radar);
            }
            let (fw, fh) = if self.radar_frame_index == 0 { (76, 76) } else { (108, 92) };
            if (Rect { x: self.radar_frame.0, y: self.radar_frame.1, w: u(fw), h: u(fh) }).contains(x, y) {
                return Some(Hit::Panel);
            }
        }
        // the topmost tab wins where tabs overlap: later (further left) tabs are drawn over earlier ones
        if let Some((t, ..)) = self.tabs.iter().rev().find(|(_, r, _)| r.contains(x, y)) {
            return Some(Hit::Turret(*t));
        }
        if let Some((f, ..)) = self.pills.iter().find(|(_, r, ..)| r.contains(x, y)) {
            return Some(Hit::Air(*f));
        }
        for (pos, tile_w) in [(self.credits, 133), (self.research, 133)] {
            if let Some((px, py)) = pos {
                if (Rect { x: px, y: py, w: u(tile_w), h: u(19) }).contains(x, y) {
                    return Some(Hit::Panel);
                }
            }
        }
        None
    }

    /// Whether a screen position lies on the radar widget (picture or frame).
    pub fn on_radar_frame(&self, x: i32, y: i32) -> bool {
        let u = |v: i32| (v as f32 * self.s).round() as i32;
        let (fw, fh) = if self.radar_frame_index == 0 { (76, 76) } else { (108, 92) };
        self.radar_on && (Rect { x: self.radar_frame.0, y: self.radar_frame.1, w: u(fw), h: u(fh) }).contains(x, y)
    }

    /// Map pixel under a screen position on the radar (clamped to the map; one radar pixel is one 32 px cell).
    pub fn radar_to_map(&self, x: i32, y: i32) -> (f32, f32) {
        let fx = ((x - self.radar.x) as f32 / self.s).clamp(0.0, self.map_cells.0 as f32);
        let fy = ((y - self.radar.y) as f32 / self.s).clamp(0.0, self.map_cells.1 as f32);
        (fx * 32.0, fy * 32.0)
    }
}

/// What the HUD needs to know about the play screen.
pub struct State<'a> {
    /// The camera.
    pub view: &'a View,
    /// The mission's state.
    pub world: &'a World,
    /// The air strike that is armed and waits for a target click.
    pub strike: Option<AirForce>,
    /// What the pointer is over.
    pub hover: Option<Hit>,
    /// Selected building (index into `world.structures`).
    pub building: Option<usize>,
    /// Black radar (`[0x5baad8]`): a plain black plate (`radar` frame 4 or 5) instead of the mission picture.
    pub radar_black: bool,
}

fn dim(buf: &mut [u32], w: usize, h: usize, r: Rect, alpha: u32) {
    ui::fill(buf, w, h, r, (0, 0, 0), alpha);
}

fn frame_box(buf: &mut [u32], w: usize, h: usize, r: Rect, c: (u8, u8, u8)) {
    for (a, b, cc, d) in
        [(r.x, r.y, r.x + r.w, r.y + 1), (r.x, r.y + r.h - 1, r.x + r.w, r.y + r.h), (r.x, r.y, r.x + 1, r.y + r.h), (r.x + r.w - 1, r.y, r.x + r.w, r.y + r.h)]
    {
        rect(buf, w, h, a, b, cc, d, c, 255);
    }
}

/// A counter (`money.ps6`: frame `base` is the tile, `base+1+d` the digit d): the tile at `pos`, `n` zero-padded digits
/// of the value from x+`first`, `step` apart, y+2 (frames are drawn by their top-left corner, the hotspot is ignored).
fn counter(assets: &mut Assets, buf: &mut [u32], w: usize, h: usize, pos: (i32, i32), base: usize, value: i32, n: usize, first: i32, step: i32, s: f32) {
    let Some(set) = assets.art.load("extras\\money.ps6") else { return };
    if set.len() < base + 11 {
        return;
    }
    let u = |v: i32| (v as f32 * s).round() as i32;
    ui::sprite(buf, w, h, &set[base], pos.0, pos.1, s);
    // the language packs' tiles are wider when the label is longer (SK: 145 / 184 instead of 133); the digit slots sit at the right end
    let first = first + (set[base].width as i32 - 133).max(0);
    let modulus = 10i32.pow(n as u32);
    let v = value.max(0) % modulus;
    for k in 0..n {
        let d = (v / 10i32.pow((n - 1 - k) as u32) % 10) as usize;
        ui::sprite(buf, w, h, &set[base + 1 + d], pos.0 + u(first + step * k as i32), pos.1 + u(2), s);
    }
}

/// Draws the HUD's fixed parts: readouts, radar, turret tabs and air pills.
pub fn draw(l: &Layout, st: &State, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize) {
    let s = l.s;
    let u = |v: i32| (v as f32 * s).round() as i32;

    // radar: the mission's picture (`radar.ps6` frame 5+mission), then the markers, then the frame
    let radar = assets.art.load("extras\\radar.ps6");
    if l.radar_on {
        if st.radar_black {
            // `0x4546a8`: frame 4 (66x66) for the 64x64 maps, else frame 5 (98x82), one pixel up and left of the map
            let plate = if l.radar_frame_index == 0 { 4 } else { 5 };
            if let Some(pic) = radar.as_ref().and_then(|set| set.get(plate)) {
                ui::sprite(buf, w, h, pic, l.radar.x - u(1), l.radar.y - u(1), s);
            }
        } else if let Some(m) = l.mission.filter(|&m| m >= 1) {
            // Warmonger: its own `radar2.tsp` (`0x44ebb4`); the base game: `radar.ps6` frame 5 + mission
            let (set, k) = if assets.game.edition == opensow_formats::Edition::Warmonger {
                (assets.warmonger_radar(), radar2_frame(m))
            } else {
                (radar.clone(), 5 + m as usize)
            };
            if let Some(pic) = set.as_ref().and_then(|set| set.get(k)) {
                // `0x454697`: additive over the world, plain when the level's weather word `[0x49a354]` is 2
                ui::sprite_add_or_plain_mode(buf, w, h, pic, l.radar.x, l.radar.y, s, st.world.weather != 2);
            }
        }
        let px = (s.ceil() as i32).max(1);
        let at = |cx: i32, cy: i32| (l.radar.x + (cx as f32 * s) as i32, l.radar.y + (cy as f32 * s) as i32);
        // the exe's blue and green, or the skirmish's colours (docs/skirmish.md)
        let [blue, green] = assets.art.team().map(|c| c.radar());
        // units: a plus of five cells (`0x45481b`)
        for un in st.world.units.iter().filter(|un| un.hp > 0) {
            let c = if un.player == 0 { blue } else { green };
            let (cx, cy) = ((un.x as i32).div_euclid(32), (un.y as i32).div_euclid(32));
            for (dx, dy) in [(0, -1), (-1, 0), (0, 0), (1, 0), (0, 1)] {
                let (x, y) = at(cx + dx, cy + dy);
                rect(buf, w, h, x, y, x + px, y + px, c, 255);
            }
        }
        // buildings blink: shown 45 of every 60 ticks (`0x454966`), a rectangle over the footprint
        if st.world.tick % 60 < 45 {
            for b in st.world.structures.iter().filter(|b| b.alive()) {
                let c = match b.owner {
                    Owner::Player => blue,
                    Owner::Enemy => green,
                    _ => (255, 255, 255),
                };
                let (bw, bh) = b.size();
                let (x0, y0) = at(b.cell.0, b.cell.1);
                let (x1, y1) = at(b.cell.0 + bw, b.cell.1 + bh);
                rect(buf, w, h, x0, y0, x1.max(x0 + px), y1.max(y0 + px), c, 255);
            }
        }
        // the camera: a white rectangle of screen size in cells (`0x454779`)
        let (vx, vy) = (st.view.x / 32.0, st.view.y / 32.0);
        let (cw, ch) = (w as f32 / st.view.zoom / 32.0, h as f32 / st.view.zoom / 32.0);
        let (x0, y0) = at(vx.max(0.0) as i32, vy.max(0.0) as i32);
        let (x1, y1) = at((vx + cw).min(l.map_cells.0 as f32) as i32, (vy + ch).min(l.map_cells.1 as f32) as i32);
        frame_box(buf, w, h, Rect { x: x0, y: y0, w: (x1 - x0).max(2), h: (y1 - y0).max(2) }, (255, 255, 255));
        if let Some(f) = radar.as_ref().and_then(|set| set.get(l.radar_frame_index)) {
            ui::sprite(buf, w, h, f, l.radar_frame.0, l.radar_frame.1, s);
        }
    }

    // counters
    if let Some(p) = l.credits {
        counter(assets, buf, w, h, p, 0, st.world.credits[0], 5, 72, 10, s);
    }
    if let Some(p) = l.research {
        counter(assets, buf, w, h, p, 11, st.world.research[0], 4, 87, 9, s);
    }

    let icons = assets.art.load("extras\\ikony.ps6");
    let digits = assets.art.load("__extras\\panel2nu.ps6");
    let aura = assets.art.load("extras\\ikonyaura.ps6");
    // air pills: the force's pill, its stock in small digits (`0x4543a2`..`0x4544e9`)
    for (f, _r, cx, cy) in &l.pills {
        let g = opensow_formats::air::HEADER_ORDER.iter().position(|x| x == f).unwrap_or(0);
        let stock = st.world.air_stock[0][g];
        if let Some(fr) = icons.as_ref().and_then(|set| set.get(pill_frame(*f))) {
            // state 2 (disabled) draws through the half-additive blit `0x45c220`, like a locked slot icon (`0x456140`, `0x4561d0`)
            ui::sprite_add_or_plain(buf, w, h, fr, cx - u(31), cy - u(26), s, l.pills_locked);
        }
        // the pill's `+0x34` is `ikonyaura` frame 2 (`0x454014`: `[0x5e7178]`), added while the pointer is on it; our armed pill keeps it
        if st.strike == Some(*f) || st.hover == Some(Hit::Air(*f)) {
            if let Some(a) = aura.as_ref().and_then(|set| set.get(2)) {
                ui::sprite_add(buf, w, h, a, cx - u(a.hot_x as i32), cy - u(a.hot_y as i32), s, false);
            }
        }
        let (hun, ten, one) = ((stock / 100 % 10) as usize, (stock / 10 % 10) as usize, (stock % 10) as usize);
        let k = if one != 1 { -1 } else { 0 };
        let mut put = |d: usize, dx: i32| {
            if let Some(fr) = digits.as_ref().and_then(|set| set.get(d)) {
                ui::sprite(buf, w, h, fr, *cx + u(dx + k), cy + u(12), s);
            }
        };
        if hun != 0 {
            put(hun, 7);
            put(ten, 12);
            put(one, 17);
        } else if ten != 0 {
            put(ten, 10);
            put(one, 15);
        } else {
            put(one, 12);
        }
    }
    // turret tabs: later tabs (further left) are drawn over the earlier ones, so draw from the right
    let small = assets.font("small");
    for (tier, r, tl) in l.tabs.iter() {
        if let Some(fr) = icons.as_ref().and_then(|set| set.get(24 + *tier as usize)) {
            // widget state 2 (`0x454e60`: credits < 200 * tier) draws through the half-additive blit `0x45c220`, like a locked slot icon
            let poor = st.world.credits[0] < opensow_formats::production::turret_price(*tier).unwrap_or(0);
            ui::sprite_add_or_plain(buf, w, h, fr, tl.0, tl.1, s, poor);
            // the tab's `ikonyaura` frame 1 (`+0x34`, `0x455400`) is added while the pointer is on it
            if let Some(a) = aura.as_ref().filter(|_| st.hover == Some(Hit::Turret(*tier))).and_then(|set| set.get(1)) {
                let (cx, cy) = (tl.0 + u(fr.hot_x as i32), tl.1 + u(fr.hot_y as i32));
                ui::sprite_add(buf, w, h, a, cx - u(a.hot_x as i32), cy - u(a.hot_y as i32), s, false);
            }
        }
        // no selection mark: the original only dims a tab it cannot afford (`0x454e60`)
        if st.hover == Some(Hit::Turret(*tier)) {
            let price = opensow_formats::production::turret_price(*tier).unwrap_or(0);
            ui::text(buf, w, h, &small, &format!("{price}"), r.x + u(8), r.y - u(14), s, (255, 240, 170), (0, 0, 0));
        }
    }
    let _ = dim;
}

/// The production slots of the selected building (`0x4541b0`, the loop at `0x4542a6`): per slot the `ikony` icon
/// (plain when active, half-additive `0x45c220` when locked), the `ikonyaura` frame-0 glow added on top, the tech-level
/// arrow (`ikonyrpoints` frame 0; half-additive when locked or unaffordable) and the bar of `0x437390`.
///
/// The icon art already carries a full green bar 33 px wide at (x-8, y+21); the exe paints over it:
/// an active slot paints the unfinished remainder grey, and the first locked slot paints its upgrade progress green.
pub fn draw_slots(l: &Layout, st: &State, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize) {
    let Some(b) = st.building.and_then(|i| st.world.structures.get(i)) else { return };
    let icons = assets.art.load("extras\\ikony.ps6");
    let aura = assets.art.load("extras\\ikonyaura.ps6");
    let arrow = assets.art.load("extras\\ikonyrpoints.ps6");
    let s = l.s;
    let u = |v: i32| (v as f32 * s).round() as i32;
    let mut first_locked_done = false;
    for (i, (_, cx, cy)) in l.slots.iter().enumerate() {
        let Some(sl) = b.slots.get(i) else { continue };
        let locked = i >= b.active;
        if let Some(fr) = icons.as_ref().and_then(|set| set.get(sl.kind.max(0) as usize)) {
            ui::sprite_add_or_plain(buf, w, h, fr, cx - u(28), cy - u(30), s, locked);
        }
        // the glow is a hover highlight, not always on
        let hovered = matches!(st.hover, Some(Hit::Slot(j) | Hit::SlotBuy(j)) if j == i);
        if let Some(fr) = aura.as_ref().filter(|_| hovered).and_then(|set| set.first()) {
            ui::sprite_add(buf, w, h, fr, cx - u(28), cy - u(30), s, false);
        }
        if let (Some(Some((_, ax, ay))), Some(fr)) = (l.arrows.get(i), arrow.as_ref().and_then(|set| set.first())) {
            let affordable = !locked && st.world.research[0] >= opensow_formats::production::slot_unlock_cost(sl.kind).unwrap_or(i32::MAX as u32) as i32;
            ui::sprite_add_or_plain(buf, w, h, fr, ax - u(15), ay - u(22), s, !affordable);
        }
        // the bar (`0x4542c5`..`0x454339`); the owner/kind exclusions are the exe's (blue/green side, not mines, labs, wind)
        let (x0, y0) = (cx - u(8), cy + u(21));
        let (remainder, width) = if !locked {
            if !matches!(b.owner, Owner::Player | Owner::Enemy) || matches!(b.kind_id, 105..=107) || sl.target == 0 {
                continue;
            }
            (true, (sl.progress as u64 * 33 / sl.target as u64) as i32)
        } else if !first_locked_done && sl.target > 0 {
            first_locked_done = true;
            (false, (b.upgrade.max(0) as u64 * 33 / sl.unlock_points().max(1) as u64) as i32)
        } else {
            continue;
        };
        slot_bar(buf, w, h, x0, y0, width, remainder, s);
    }
    // the rally point button: `ikonyrpoints` frame 1, frame 3 while the pointer is on it (widget draw mode 0, a plain copy)
    if let (Some((_, px, py)), Some(set)) = (l.rally, arrow.as_ref()) {
        let frame = if st.hover == Some(Hit::Rally) { rally::BUTTON_HOVER_FRAME } else { rally::BUTTON_FRAME };
        if let Some(fr) = set.get(frame) {
            ui::sprite(buf, w, h, fr, px - u(fr.hot_x as i32), py - u(fr.hot_y as i32), s);
        }
    }
}

/// `0x437390` in the exe's 565 colour set: four rows, 33 px wide; `remainder` paints from `width` to the end in
/// grey, otherwise the first `width` px in green.
fn slot_bar(buf: &mut [u32], w: usize, h: usize, x0: i32, y0: i32, width: i32, remainder: bool, s: f32) {
    const FILLED: [u16; 4] = [0x960c, 0xd7f4, 0x6467, 0x3ae3];
    const EMPTY: [u16; 4] = [0x632c, 0xa534, 0x528a, 0x39c7];
    let u = |v: i32| (v as f32 * s).round() as i32;
    let width = width.clamp(0, 33);
    let (from, to, colours) = if remainder { (width, 33, EMPTY) } else { (0, width, FILLED) };
    for (row, c) in colours.iter().enumerate() {
        let [r, g, b] = rgb565_to_rgb8(*c);
        rect(buf, w, h, x0 + u(from), y0 + u(row as i32), x0 + u(to), y0 + u(row as i32 + 1), (r, g, b), 255);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> Inputs {
        Inputs {
            mission: Some(12),
            warmonger: false,
            map_cells: (64, 64),
            show_credits: true,
            show_research: true,
            tiers: vec![1, 2, 3],
            pills: vec![AirForce::Bomber, AirForce::Fighter],
            info: None,
            slots: 0,
            buy: Vec::new(),
            rally_button: false,
        }
    }

    #[test]
    fn radar_switch_removes_the_widget() {
        let mut l = Layout::new(640, 480, 1.0, &inputs());
        let (x, y) = (l.radar.x + 2, l.radar.y + 2);
        assert_eq!(l.hit(x, y), Some(Hit::Radar));
        l.radar_on = false;
        assert_eq!(l.hit(x, y), None);
        assert_eq!(l.hit(l.radar_frame.0 + 1, l.radar_frame.1 + 1), None);
    }

    #[test]
    fn positions_follow_the_exes_anchors() {
        let l = Layout::new(640, 480, 1.0, &inputs());
        assert_eq!(l.credits, Some((3, 3)));
        assert_eq!(l.research, Some((143, 3)));
        assert_eq!(l.radar_frame, (640 - 11 - 64, -1));
        assert_eq!(l.radar_frame_index, 0);
        assert_eq!((l.radar.x, l.radar.y), (640 - 11 - 64 + 6, 5));
        // tab 0 is the rightmost, centred 11 px from the right edge and 21 px above the bottom
        assert_eq!(l.tabs[0].2, (640 - 11 - 46, 480 - 21 - 21));
        assert_eq!(l.tabs[1].2 .0, 640 - 11 - 57 - 46);
        // pills: centre x 31, y 60 + 51 n
        assert_eq!((l.pills[0].2, l.pills[0].3), (31, 60));
        assert_eq!(l.pills[1].3, 111);
        assert_eq!(l.hit(l.tabs[0].1.x + 2, l.tabs[0].1.y + 2), Some(Hit::Turret(1)));
        assert_eq!(l.hit(l.pills[1].1.x + 2, l.pills[1].1.y + 2), Some(Hit::Air(AirForce::Fighter)));
        let mut locked = Layout::new(640, 480, 1.0, &inputs());
        locked.pills_locked = true;
        assert_eq!(locked.hit(locked.pills[1].1.x + 2, locked.pills[1].1.y + 2), Some(Hit::Air(AirForce::Fighter)), "the hover still reaches a disabled pill");
        assert_eq!(l.hit(l.radar.x + 3, l.radar.y + 3), Some(Hit::Radar));
        assert_eq!(l.hit(320, 240), None);
    }

    #[test]
    fn slot_arrows_sit_left_of_the_icon_and_take_the_click() {
        let mut i = inputs();
        i.slots = 2;
        i.buy = vec![true, false];
        let l = Layout::new(1024, 768, 1.0, &i);
        let (_, cx, cy) = l.slots[0];
        assert_eq!(cx, 1024 - 28);
        let (r, ax, ay) = l.arrows[0].unwrap();
        assert_eq!((ax, ay), (cx - 40, cy - 3));
        assert!(l.arrows[1].is_none());
        // the arrow is checked before the icon; a slot without an arrow only has its icon
        assert_eq!(l.hit(r.x + 2, r.y + 2), Some(Hit::SlotBuy(0)));
        assert_eq!(l.hit(cx, cy), Some(Hit::Slot(0)));
        assert_eq!(l.hit(l.slots[1].1 - 40, l.slots[1].2 - 3), None);
    }

    #[test]
    fn slots_stay_on_screen_when_the_hud_is_scaled() {
        let mut i = inputs();
        i.slots = 2;
        i.buy = vec![true, true];
        // 2560x1600 is scale 2.08: the column is centred on the screen's middle line, not on the scaled one
        let (w, h) = (2560, 1600);
        let l = Layout::new(w, h, h as f32 / 768.0, &i);
        for (r, cx, cy) in &l.slots {
            assert!(*cx < w as i32 && *cy > h as i32 / 2 - 200 && *cy < h as i32 / 2 + 300, "slot at {cx},{cy} {r:?}");
        }
    }

    #[test]
    fn slot_bar_paints_the_remainder_grey_or_the_progress_green() {
        let (w, h) = (40, 8);
        let mut buf = vec![0u32; w * h];
        slot_bar(&mut buf, w, h, 2, 2, 10, true, 1.0);
        // rows 2..6, grey from x = 2 + 10 to the end of the 33 px bar; nothing left of it
        assert_eq!(buf[2 * w + 5], 0);
        let [r, g, b] = rgb565_to_rgb8(0x632c);
        assert_eq!(buf[2 * w + 20], opensow_viewer::render::pack(r, g, b));
        let mut buf = vec![0u32; w * h];
        slot_bar(&mut buf, w, h, 2, 2, 10, false, 1.0);
        let [r, g, b] = rgb565_to_rgb8(0xd7f4);
        assert_eq!(buf[3 * w + 5], opensow_viewer::render::pack(r, g, b));
        assert_eq!(buf[3 * w + 20], 0);
        // a full bar leaves no remainder
        let mut buf = vec![0u32; w * h];
        slot_bar(&mut buf, w, h, 2, 2, 33, true, 1.0);
        assert!(buf.iter().all(|&p| p == 0));
    }

    #[test]
    fn research_moves_left_when_there_is_no_credits_tile() {
        let mut i = inputs();
        i.show_credits = false;
        i.mission = Some(1);
        let l = Layout::new(1024, 768, 1.0, &i);
        assert_eq!(l.credits, None);
        assert_eq!(l.research, Some((3, 3)));
    }

    #[test]
    fn warmonger_uses_its_own_small_radar_list_and_pictures() {
        let at = |m: u32, warmonger: bool| Layout::new(640, 480, 1.0, &Inputs { mission: Some(m), warmonger, ..inputs() }).radar_frame_index;
        assert_eq!([2, 4, 12].map(|m| at(m, false)), [0, 1, 0], "the base game's list");
        assert_eq!([2, 4, 12].map(|m| at(m, true)), [1, 0, 1], "Warmonger's list (0x44eb11)");
        assert_eq!([1, 2, 3, 4, 6, 7, 11, 12, 15, 20, 22].map(radar2_frame), [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(radar2_frame(5), 0, "other numbers fall through with frame 0");
    }

    #[test]
    fn radar_maps_corners() {
        let l = Layout::new(640, 480, 1.0, &inputs());
        assert_eq!(l.radar_to_map(l.radar.x, l.radar.y), (0.0, 0.0));
        assert_eq!(l.radar_to_map(l.radar.x + 64, l.radar.y + 64), (2048.0, 2048.0));
    }
}
