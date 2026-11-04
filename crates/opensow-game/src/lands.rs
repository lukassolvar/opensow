//! The campaign map (`CMenuBriefing`, `0x441210`): the world map where the player chooses the next mission.
//!
//! "New game" first plays the `mA.sgv` BRIEFING animation (state 0), then the map (state 3) shows the 22 lands
//! (`lands_blue/red.tsp`, one per mission); `mB.sgv`, a spinning globe, loops in the top-left corner. A land the player may play
//! blinks (state 2) or is plain blue (1, passed); locked ones (0) are red. Clicking an open land zooms a frame onto it and types
//! its briefing (state 4) with ACCEPT / BACK buttons; ACCEPT starts the mission. Notes: docs/lands.md.

use crate::assets::{load_backdrop, Assets};
use crate::gdi::Gdi;
use crate::intro::Anim;
use crate::ui::{self, Image};
use opensow_formats::config::PassedLevels;
use opensow_formats::font::Font;
use opensow_formats::lang::{self, LangFile};
use opensow_formats::sprite::{self, rgb565_to_rgb8, Frame, Kind};
use opensow_viewer::render::pack;

/// The design resolution of the map picture (`Briefing.srf`); it is drawn 1:1 at the centre of the screen.
pub const W: usize = 640;
pub const H: usize = 480;
pub const LANDS: usize = 22;
/// The size of the backdrop art `Briefing2.srf` (`0x780` x `0x438` in the exe).
const ART: (usize, usize) = (1920, 1080);

/// Where `mB` (128x112) plays: the exe opens it with x = 0x20, y = 1 (`0x4413e6`, `0x441c7f`: pushes 1, 0x20, then the player's open
/// `0x4573e0` builds the rect left = x, top = y from them); `mA` gets (0, 0).
const GLOBE: (i32, i32) = (32, 1);
/// Natural rows kept on screen when the window is bigger than the picture: the 480-row map picture plus a margin, so the text stays
/// readable (a window of height `h` is zoomed by `h / 560`).
const ZOOM_ROWS: f32 = 560.0;
/// Wrap width of the briefing texts: panel width (0xe3 + 0xd7) minus `[0x48f400]` = 50 (`0x440b5d..0x440bec`).
const WRAP: i32 = 400;
/// Ticks of the 60 Hz clock per second.
const TICKS: f64 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    None,
    /// BACK on the map: return to the main menu.
    Back,
    /// ACCEPT in a land's briefing: play mission `1..=22`.
    Accept(usize),
}

enum Stage {
    /// `mA` plays (state 0).
    Video(Box<Anim>),
    /// The interactive map (state 3).
    Map,
    /// A land is selected and its text types (state 4); seconds since the click.
    Selected { land: usize, t: f64 },
}

pub struct Lands {
    stage: Stage,
    /// `Briefing2.srf`, the full-screen art (globe on the left), and `Briefing.srf`, the 640x480 map drawn over it.
    art: Option<Image>,
    bg: Option<Image>,
    blue: Vec<Frame>,
    red: Vec<Frame>,
    points: Vec<Frame>,
    buttons: Vec<Frame>,
    reticle: Vec<Frame>,
    globe: Option<Anim>,
    strings: Option<LangFile>,
    big: Font,
    small: Font,
    gdi: Option<Gdi>,
    pub progress: PassedLevels,
    hover: Option<usize>,
    /// Ticks since the hovered land changed (`[+0x338]` of the exe object).
    hover_ticks: f64,
    clock: f64,
    /// Mouse position relative to the top-left of the centred 640x480 picture.
    cursor: (i32, i32),
    /// Window size of the last frame / mouse event.
    size: (usize, usize),
    /// Effect ids to play (the game drains them).
    pub sounds: Vec<u8>,
    /// Typing-sound accumulator (`[+0x380]`) and the once-only sounds of a selection (`[+0x375..0x378]`).
    type_acc: f64,
    once: [bool; 4],
    /// Scratch buffer for windows larger than the art (rendered at natural size, then scaled up).
    virt: Vec<u32>,
    /// The window is larger than the art and the frame is scaled up (the art's own globe is visible).
    scaled: bool,
    /// GDI-style text of the frame in natural pixels, drawn after the upscale so it stays sharp.
    texts: std::cell::RefCell<Vec<(String, i32, i32)>>,
}

fn frames(assets: &Assets, name: &str) -> Vec<Frame> {
    assets.game.resolve(&format!("Data\\Sprites2\\Menu\\{name}.tsp")).and_then(|p| std::fs::read(p).ok()).and_then(|d| sprite::parse(&d, Kind::Color).ok()).unwrap_or_default()
}

/// The aspect-preserving rectangle of a 640x480 video in a `w` x `h` window.
fn fit(w: usize, h: usize) -> (usize, usize, usize, usize) {
    let s = (w as f32 / W as f32).min(h as f32 / H as f32);
    let (dw, dh) = (((W as f32 * s) as usize).max(1), ((H as f32 * s) as usize).max(1));
    ((w.saturating_sub(dw)) / 2, (h.saturating_sub(dh)) / 2, dw, dh)
}

/// Everything about a selected land that depends on its texts and the screen size (`0x440b37..0x440d6a`).
struct Geometry {
    desc: Vec<String>,
    goal: Vec<String>,
    /// Description panel (left, top, right, bottom) and objectives panel (top, bottom, centre y).
    panel: (i32, i32, i32, i32),
    goal_panel: (i32, i32, i32),
    /// Top-left of the land's point (`[+0x344]`, `[+0x348]`) and the box growth speed factor `W / 640`.
    point: (i32, i32),
    speed: i32,
}

/// What the selection animation shows at `c` ticks after the click (`0x43ebe0`).
#[derive(Default)]
struct Timeline {
    reticle: usize,
    buttons: bool,
    /// The growing description box (left, top, right, bottom).
    box1: Option<(i32, i32, i32, i32)>,
    /// Characters typed in each description line.
    desc: Vec<usize>,
    /// The growing objectives box (top, bottom).
    box2: Option<(i32, i32)>,
    goal: Vec<usize>,
    /// True while the objectives are still typing (the exe plays `briefing-type` then).
    typing_goal: bool,
}

/// `Left(c / 2)` of every line in turn, each shown character costing 2 ticks (`0x43f430`).
fn type_lines(lines: &[String], mut c: i64) -> (Vec<usize>, i64, bool) {
    let mut shown = Vec::new();
    let mut typing = false;
    for l in lines {
        let n = ((c.max(0) / 2) as usize).min(l.chars().count());
        if n == 0 {
            break;
        }
        typing |= n < l.chars().count();
        shown.push(n);
        c -= 2 * n as i64;
    }
    (shown, c, typing)
}

fn timeline(g: &Geometry, c: i64) -> Timeline {
    let mut t = Timeline::default();
    let mut c = c;
    let k = (c / 2).clamp(0, 39);
    t.reticle = k as usize;
    c -= 2 * k;
    t.buttons = c > 2;
    if c <= 2 {
        return t;
    }
    let n = (c - 2) as i32;
    let (px, py) = g.point;
    let (l, tp, r, b) = g.panel;
    let (left, right) = ((px - 8 * n * g.speed).max(l), (px + 8 * n * g.speed).min(r));
    let (top, bottom) = ((py - 4 * n * g.speed).max(tp), (py + 4 * n * g.speed).min(b));
    t.box1 = Some((left, top, right, bottom));
    let steps = ((px - left).abs() >> 3).max((px - right).abs() >> 3).max((py - top).abs() >> 2).max((py - bottom).abs() >> 2);
    let mut c = n as i64 - steps as i64;
    let (shown, rest, _) = type_lines(&g.desc, c);
    t.desc = shown;
    c = rest;
    if c > 0xf {
        let n2 = (c - 0xf) as i32;
        let (gt, gb, gc) = g.goal_panel;
        let (top, bottom) = ((gc - 2 * n2).max(gt), (gc + 2 * n2).min(gb));
        t.box2 = Some((top, bottom));
        let c2 = n2 as i64 - (gc - top).abs().max((gc - bottom).abs()) as i64;
        let (shown, _, typing) = type_lines(&g.goal, c2);
        t.goal = shown;
        t.typing_goal = typing;
    }
    t
}

impl Lands {
    /// `video`: play the `mA` animation first (New game); `mute` opens no audio device.
    pub fn new(assets: &mut Assets, progress: PassedLevels, video: bool, mute: bool) -> Lands {
        let language = assets.game.resolve("Data\\lang.dat").and_then(|p| std::fs::read(p).ok()).and_then(|d| lang::read_lang_dat(&d)).unwrap_or(lang::Language::English);
        let strings = assets.game.resolve(&format!("Data\\Language\\{}", language.file_name())).and_then(|p| std::fs::read(p).ok()).and_then(|d| LangFile::parse(&d).ok());
        let mut font = |n: &str| Font::new(assets.art.load(&format!("extras\\fonts\\{n}.ps6")).map(|f| (*f).clone()).unwrap_or_default());
        let (big, small) = (font("font_briefing_big"), font("font_briefing_small"));
        let stage = match video.then(|| Anim::open(&assets.game, "mA")).and_then(Result::ok) {
            Some(mut a) => {
                a.once = true;
                if !mute {
                    a.start_sound();
                }
                Stage::Video(Box::new(a))
            }
            None => Stage::Map,
        };
        Lands {
            stage,
            art: load_backdrop(&assets.game, "Briefing2"),
            bg: load_backdrop(&assets.game, "Briefing"),
            blue: frames(assets, "lands_blue"),
            red: frames(assets, "lands_red"),
            points: frames(assets, "lands_points"),
            buttons: frames(assets, "lands_buttons"),
            reticle: frames(assets, "briefing"),
            globe: Anim::open(&assets.game, "mB").ok(),
            strings,
            big,
            small,
            gdi: Gdi::load(),
            progress,
            hover: None,
            hover_ticks: 0.0,
            clock: 0.0,
            cursor: (-1, -1),
            size: (W, H),
            sounds: Vec::new(),
            type_acc: 0.0,
            once: [false; 4],
            virt: Vec::new(),
            scaled: false,
            texts: Default::default(),
        }
    }

    pub fn playing_video(&self) -> bool {
        matches!(self.stage, Stage::Video(_))
    }

    pub fn selected(&self) -> Option<usize> {
        match self.stage {
            Stage::Selected { land, .. } => Some(land),
            _ => None,
        }
    }

    pub fn hovered(&self) -> Option<usize> {
        self.hover
    }

    /// Headless: jump to a stage. `land` 0 = the plain map, otherwise that land selected `t` seconds ago.
    pub fn show(&mut self, land: usize, t: f64) {
        self.stage = if land == 0 { Stage::Map } else { Stage::Selected { land, t } };
        self.once = [false; 4];
    }

    pub fn set_hover(&mut self, land: Option<usize>) {
        self.hover = land;
    }

    pub fn skip_video(&mut self) {
        if self.playing_video() {
            self.stage = Stage::Map;
        }
    }

    pub fn update(&mut self, dt: f64) {
        self.clock += dt;
        if let Some(g) = self.globe.as_mut() {
            g.update(dt);
        }
        let ticks = dt * TICKS;
        let mut typing = false;
        match &mut self.stage {
            Stage::Video(a) => {
                a.update(dt);
                if a.finished() {
                    self.stage = Stage::Map;
                }
            }
            Stage::Selected { t, .. } => {
                *t += dt;
            }
            Stage::Map => self.hover_ticks += ticks,
        }
        match self.stage {
            Stage::Selected { land, t } => {
                let tl = timeline(&self.geometry(land), (t * TICKS) as i64);
                // `0x43ebe0`: blik, then location on the first frame, rect01 when the description box opens, rect02 for the objectives'
                let wanted = [(46, true), (41, true), (44, tl.box1.is_some()), (45, tl.box2.is_some())];
                for (i, (id, on)) in wanted.into_iter().enumerate() {
                    if on && !self.once[i] {
                        self.once[i] = true;
                        self.sounds.push(id);
                    }
                }
                typing = tl.typing_goal; // briefing-type: only the objectives make a sound, the description is silent
            }
            Stage::Map => {
                if let Some(land) = self.hover {
                    let (a, b) = self.headings(land);
                    let c = self.hover_ticks as i64;
                    let (_, _, typing_a) = type_lines(&[a.clone()], c);
                    let used = type_lines(&[a], c).1;
                    let (_, _, typing_b) = type_lines(&[b], used);
                    typing = typing_a || typing_b; // briefing-type02 under the headings
                }
            }
            Stage::Video(_) => {}
        }
        if typing {
            // `[+0x380]` = min(it, 4) + frame ticks; every 4 ticks one sound
            self.type_acc = self.type_acc.min(4.0) + ticks;
            if self.type_acc >= 4.0 {
                self.type_acc -= 4.0;
                if self.sounds.len() < 8 {
                    self.sounds.push(if matches!(self.stage, Stage::Map) { 43 } else { 42 });
                }
            }
        }
    }

    fn top_left(f: &Frame) -> (i32, i32) {
        (W as i32 / 2 - f.hot_x as i32, H as i32 / 2 - f.hot_y as i32)
    }

    fn hits(f: &Frame, (x, y): (i32, i32)) -> bool {
        let (l, t) = Self::top_left(f);
        let (dx, dy) = (x - l, y - t);
        dx >= 0 && dy >= 0 && dx < f.width as i32 && dy < f.height as i32 && f.mask[(dy * f.width as i32 + dx) as usize]
    }

    /// First land (lowest number) whose pixels are under the cursor, like the exe's loop; locked lands count too.
    fn land_at(&self, p: (i32, i32)) -> Option<usize> {
        (1..=LANDS).find(|&l| {
            let f = if self.progress.states[l] == 0 { self.red.get(l - 1) } else { self.blue.get(l - 1) };
            f.is_some_and(|f| Self::hits(f, p))
        })
    }

    fn button(&self, back: bool) -> Option<&Frame> {
        // frames: 0 ACCEPT, 1 BACK, 2/3 their highlighted versions
        self.buttons.get(back as usize)
    }

    fn over_button(&self, back: bool) -> bool {
        self.buttons.get(2 + back as usize).is_some_and(|f| Self::hits(f, self.cursor))
    }

    /// The window pixel of the picture's top-left corner.
    fn origin(w: usize, h: usize) -> (i32, i32) {
        (w as i32 / 2 - W as i32 / 2, h as i32 / 2 - H as i32 / 2)
    }

    /// Windows bigger than the 640x480 picture are drawn at natural size and scaled up uniformly (the art and the 1:1 map picture
    /// stay aligned; the art is cropped to cover): `(scale, natural width, natural height)`.
    fn natural(w: usize, h: usize) -> (f32, usize, usize) {
        let s = (w as f32 / ART.0 as f32).max(h as f32 / ZOOM_ROWS).max(1.0);
        (s, (w as f32 / s).ceil() as usize, (h as f32 / s).ceil() as usize)
    }

    pub fn mouse_moved(&mut self, x: i32, y: i32, w: usize, h: usize) {
        let (s, vw, vh) = Self::natural(w, h);
        let (x, y) = ((x as f32 / s) as i32, (y as f32 / s) as i32);
        let (w, h) = (vw, vh);
        self.size = (w, h);
        let (ox, oy) = Self::origin(w, h);
        self.cursor = (x - ox, y - oy);
        let hover = if matches!(self.stage, Stage::Map) { self.land_at(self.cursor) } else { None };
        if hover != self.hover {
            self.hover_ticks = 0.0; // `0x441982`: the headings type again
        }
        self.hover = hover;
    }

    /// Selection animation state of the current land at the current time.
    fn buttons_shown(&self) -> bool {
        match self.stage {
            Stage::Selected { land, t } => timeline(&self.geometry(land), (t * TICKS) as i64).buttons,
            _ => false,
        }
    }

    /// A left click (press) at window pixel `(x, y)`.
    pub fn click(&mut self, x: i32, y: i32, w: usize, h: usize) -> Event {
        self.mouse_moved(x, y, w, h);
        match self.stage {
            Stage::Video(_) => {
                self.skip_video();
                Event::None
            }
            Stage::Map => {
                if self.over_button(true) {
                    return Event::Back;
                }
                if let Some(l) = self.hover.filter(|&l| self.progress.states[l] != 0) {
                    self.stage = Stage::Selected { land: l, t: 0.0 };
                    self.once = [false; 4];
                    self.hover_ticks = 0.0;
                }
                Event::None
            }
            Stage::Selected { land, .. } => {
                let shown = self.buttons_shown();
                if !(shown && (self.over_button(true) || self.over_button(false))) {
                    // a click elsewhere finishes the animation at once, silently (not found in the exe, added on request)
                    if let Stage::Selected { t, .. } = &mut self.stage {
                        *t = t.max(1000.0);
                    }
                    self.once = [true; 4];
                    return Event::None;
                }
                if self.over_button(true) {
                    self.stage = Stage::Map;
                    self.hover_ticks = 0.0;
                } else if self.over_button(false) {
                    self.sounds.push(29);
                    return Event::Accept(land);
                }
                Event::None
            }
        }
    }

    /// Escape / BACK from the keyboard.
    pub fn back(&mut self) -> Event {
        match self.stage {
            Stage::Video(_) => {
                self.skip_video();
                Event::None
            }
            Stage::Map => Event::Back,
            Stage::Selected { .. } => {
                self.stage = Stage::Map;
                self.hover_ticks = 0.0;
                Event::None
            }
        }
    }

    fn string(&self, index: usize) -> &str {
        self.strings.as_ref().and_then(|s| s.get(index)).unwrap_or("")
    }

    /// The two heading lines of a land: `TERRITORY : nn DIFFICULTY : n from 10` and `Coordinate: ... Battle ground: ...`
    /// (strings 30+L and 53+L; the exe's string numbers are one higher); a locked land only says "No data available" (53).
    pub fn headings(&self, land: usize) -> (String, String) {
        if self.progress.states[land] == 0 {
            (self.string(53).to_owned(), String::new())
        } else {
            (self.string(30 + land).to_owned(), self.string(53 + land).to_owned())
        }
    }

    /// The briefing and the objectives of a land (strings 75+L and 97+L).
    pub fn briefing(&self, land: usize) -> (String, String) {
        (self.string(75 + land).replace("\r\n", " "), self.string(97 + land).replace("\r\n", " "))
    }

    /// Width of a line in the text face (GDI Times New Roman when installed, else the bitmap font).
    fn width_of(&self, s: &str, heading: bool) -> i32 {
        match &self.gdi {
            Some(g) => g.width(s),
            None => (if heading { &self.big } else { &self.small }).measure(s, 1),
        }
    }

    fn put(&self, buf: &mut [u32], (w, h): (usize, usize), s: &str, x: i32, y: i32, heading: bool) {
        if s.is_empty() {
            return;
        }
        match &self.gdi {
            Some(_) => self.texts.borrow_mut().push((s.to_owned(), x, y)),
            None => draw_text(buf, w, h, if heading { &self.big } else { &self.small }, s, x, y),
        }
    }

    /// Panels, point and wrapped texts of a land on a screen of the stored size (`0x440a88..0x440d6a`).
    fn geometry(&self, land: usize) -> Geometry {
        let (w, h) = self.size;
        let (cx, cy) = (w as i32 / 2, h as i32 / 2);
        let (desc, goal) = self.briefing(land);
        // the exe wraps English text with the bitmap face `font_briefing_small` (`0x440f70` measures with `0x45ed50`, font 11)
        let (desc, goal) = (wrap(&self.small, &desc, WRAP), wrap(&self.small, &goal, WRAP));
        let lines = goal.len() as i32;
        let (l, t, r) = (cx - 0xe3, cy - 0x58, cx + 0xd7);
        let gtop = cy - 20 * lines + 0xcf;
        let gbottom = cy + 0xe3;
        let p = self.points.get(land - 1);
        Geometry {
            desc,
            goal,
            panel: (l, t, r, cy - 20 * lines + 0xb1),
            goal_panel: (gtop, gbottom, (gtop + gbottom) / 2),
            point: p.map_or((cx, cy), |p| (cx - p.hot_x as i32 + 3, cy - p.hot_y as i32 + 2)),
            speed: (w as i32 / 640).max(1),
        }
    }

    /// Draw the screen into the window buffer: the art backdrop, the 640x480 map picture at 1:1 in the centre, text relative to the
    /// window's centre or its top edge exactly like the exe.
    pub fn draw(&mut self, buf: &mut [u32], w: usize, h: usize) {
        buf.fill(0);
        if let Stage::Video(a) = &self.stage {
            let (x, y, dw, dh) = fit(w, h);
            a.draw(buf, w, h, (x, y, dw, dh));
            return;
        }
        let (s, vw, vh) = Self::natural(w, h);
        self.scaled = s > 1.0;
        self.texts.borrow_mut().clear();
        if !self.scaled {
            self.draw_native(buf, w, h);
            self.draw_texts(buf, w, h, 1.0);
            return;
        }
        let mut virt = std::mem::take(&mut self.virt);
        virt.clear();
        virt.resize(vw * vh, 0);
        self.draw_native(&mut virt, vw, vh);
        for y in 0..h {
            let sy = ((y as f32 / s) as usize).min(vh - 1);
            for x in 0..w {
                buf[y * w + x] = virt[sy * vw + ((x as f32 / s) as usize).min(vw - 1)];
            }
        }
        self.virt = virt;
        self.draw_texts(buf, w, h, s);
    }

    fn draw_texts(&self, buf: &mut [u32], w: usize, h: usize, zoom: f32) {
        if let Some(g) = &self.gdi {
            for (t, x, y) in self.texts.borrow().iter() {
                g.draw(buf, w, h, t, *x, *y, zoom);
            }
        }
    }

    fn draw_native(&mut self, buf: &mut [u32], w: usize, h: usize) {
        self.size = (w, h);
        self.draw_backdrop(buf, w, h);
        let (ox, oy) = Self::origin(w, h);
        let blink = self.clock * 60.0 % 60.0 <= 30.0;
        // `0x441f06`: every land (blue when open or passed, red when locked), each followed by its yellow point, baked into the picture
        for l in 1..=LANDS {
            let f = if self.progress.states[l] == 0 { self.red.get(l - 1) } else { self.blue.get(l - 1) };
            if let Some(f) = f {
                let (x, y) = Self::top_left(f);
                ui::sprite(buf, w, h, f, ox + x, oy + y, 1.0);
            }
            if let Some(p) = self.points.get(l - 1) {
                let (x, y) = Self::top_left(p);
                ui::sprite(buf, w, h, p, ox + x, oy + y, 1.0);
            }
        }
        // per frame (`0x441700`): the hovered land and a blinking open one are drawn again additively
        for l in 1..=LANDS {
            let s = self.progress.states[l];
            let lit = (self.hover == Some(l) && matches!(self.stage, Stage::Map)) || (s == 2 && blink);
            if !lit {
                continue;
            }
            if let Some(f) = if s == 0 { self.red.get(l - 1) } else { self.blue.get(l - 1) } {
                let (x, y) = Self::top_left(f);
                ui::sprite_add(buf, w, h, f, ox + x, oy + y, 1.0, false);
            }
        }
        // the spinning globe stands in for the art's globe on screens the art does not fill (it is not visible at 1920x1080)
        if let Some(g) = &self.globe {
            if !self.scaled && (w < ART.0 || h < ART.1) {
                g.draw(buf, w, h, (GLOBE.0 as usize, GLOBE.1 as usize, g.width, g.height));
            }
        }
        match self.stage {
            Stage::Map => {
                if let Some(land) = self.hover {
                    let (a, b) = self.headings(land);
                    self.typed_headings(buf, (w, h), &a, &b);
                }
                self.draw_button(buf, (w, h), true, self.over_button(true));
            }
            Stage::Selected { land, t } => {
                let (a, b) = self.headings(land);
                self.heading_lines(buf, (w, h), [(&a, &a), (&b, &b)]);
                self.draw_selection(buf, (w, h), land, t);
            }
            Stage::Video(_) => {}
        }
    }

    /// `Briefing2.srf` over the whole screen; a screen smaller than the art shows its middle (`0x4412ab..0x441314`).
    fn draw_backdrop(&self, buf: &mut [u32], w: usize, h: usize) {
        if let Some(img) = &self.art {
            // 1:1 and centred, so the art's band stays under the 640x480 picture at every size: a smaller screen shows the middle
            // (`0x4412ab..0x441314`), a larger one gets black around it (the exe's modes never exceed the art)
            let (ox, oy) = (w as i32 / 2 - img.w as i32 / 2, h as i32 / 2 - img.h as i32 / 2);
            for y in 0..h as i32 {
                let sy = y - oy;
                if sy < 0 || sy >= img.h as i32 {
                    continue;
                }
                for x in 0..w as i32 {
                    let sx = x - ox;
                    if sx >= 0 && sx < img.w as i32 {
                        let i = (sy as usize * img.w + sx as usize) * 3;
                        buf[y as usize * w + x as usize] = pack(img.rgb[i], img.rgb[i + 1], img.rgb[i + 2]);
                    }
                }
            }
        }
        if let Some(img) = &self.bg {
            let (ox, oy) = Self::origin(w, h);
            for y in 0..img.h as i32 {
                let dy = oy + y;
                if dy < 0 || dy >= h as i32 {
                    continue;
                }
                for x in 0..img.w as i32 {
                    let dx = ox + x;
                    if dx >= 0 && dx < w as i32 {
                        let i = (y as usize * img.w + x as usize) * 3;
                        buf[dy as usize * w + dx as usize] = pack(img.rgb[i], img.rgb[i + 1], img.rgb[i + 2]);
                    }
                }
            }
        }
    }

    fn draw_button(&self, buf: &mut [u32], (w, h): (usize, usize), back: bool, hover: bool) {
        if let Some(f) = self.buttons.get(back as usize + if hover { 2 } else { 0 }).or_else(|| self.button(back)) {
            let (ox, oy) = Self::origin(w, h);
            let (x, y) = Self::top_left(f);
            ui::sprite(buf, w, h, f, ox + x, oy + y, 1.0);
        }
    }

    /// Heading lines, each centred on the screen by its full width, at y 81 and 101 of the window (`0x467060` after `GetTextExtentPoint32`).
    /// `lines` = (full text, text to show).
    fn heading_lines(&self, buf: &mut [u32], size: (usize, usize), lines: [(&String, &String); 2]) {
        for (i, (full, shown)) in lines.into_iter().enumerate() {
            let x = (size.0 as i32 - self.width_of(full, true)) / 2;
            self.put(buf, size, shown, x, 81 + 20 * i as i32, true);
        }
    }

    /// The map's hover headings type in: `Left(c / 2)` of line one, then what is left for line two.
    fn typed_headings(&self, buf: &mut [u32], size: (usize, usize), a: &str, b: &str) {
        let c = self.hover_ticks as i64;
        let (n1, ..) = type_lines(&[a.to_owned()], c);
        let used = n1.first().copied().unwrap_or(0);
        let (n2, ..) = type_lines(&[b.to_owned()], c - 2 * used as i64);
        let part = |s: &str, n: Option<&usize>| s.chars().take(n.copied().unwrap_or(0)).collect::<String>();
        let (sa, sb) = (part(a, n1.first()), part(b, n2.first()));
        self.heading_lines(buf, size, [(&a.to_owned(), &sa), (&b.to_owned(), &sb)]);
    }

    /// The point marker's reticle, the two boxes and the typed text of a selected land (state 4, `0x43ebe0`).
    fn draw_selection(&self, buf: &mut [u32], size: (usize, usize), land: usize, t: f64) {
        let (w, h) = size;
        let g = self.geometry(land);
        let tl = timeline(&g, (t * TICKS) as i64);
        if let Some(f) = self.reticle.get(tl.reticle) {
            // drawn twice, additively, with its own hot spot at the point
            for _ in 0..2 {
                ui::sprite_add(buf, w, h, f, g.point.0 - f.hot_x as i32, g.point.1 - f.hot_y as i32, 1.0, false);
            }
        }
        if tl.buttons {
            self.draw_button(buf, size, true, self.over_button(true));
            self.draw_button(buf, size, false, self.over_button(false));
        }
        let (l, _, r, _) = g.panel;
        if let Some((bl, bt, br, bb)) = tl.box1 {
            ui::dim_sub(buf, w, h, ui::Rect { x: bl, y: bt, w: br - bl, h: bb - bt }, 48);
            for (v, x) in [(true, bl), (true, br), (false, bt), (false, bb)] {
                glow_line(buf, w, h, v, x, [0x200, 0x3e0]);
            }
        }
        for (i, n) in tl.desc.iter().enumerate() {
            let s: String = g.desc[i].chars().take(*n).collect();
            self.put(buf, size, &s, l + 20, g.panel.1 + 11 + 20 * i as i32, false);
        }
        if let Some((bt, bb)) = tl.box2 {
            ui::dim_sub(buf, w, h, ui::Rect { x: l, y: bt, w: r - l, h: bb - bt }, 48);
            glow_line(buf, w, h, false, bt, [0x200, 0x3e0]);
            glow_line(buf, w, h, false, bb, [0x400, 0x620]);
        }
        for (i, n) in tl.goal.iter().enumerate() {
            let s: String = g.goal[i].chars().take(*n).collect();
            self.put(buf, size, &s, l + 20, g.goal_panel.0 + 11 + 20 * i as i32, false);
        }
    }
}

/// Three additive green lines across the whole screen (`0x465c60`): the edge itself with `[outer, centre]` RGB565 values (the green
/// field of `0x03e0` = 31 of 63) on the line and its two neighbours.
fn glow_line(buf: &mut [u32], w: usize, h: usize, vertical: bool, at: i32, [outer, centre]: [u32; 2]) {
    for (d, v) in [(-1, outer), (0, centre), (1, outer)] {
        let g = (((v >> 5) & 63) * 255 / 63) as u32;
        let (lo, hi) = (0, if vertical { h } else { w } as i32);
        let line = at + d;
        if line < 0 || line >= if vertical { w } else { h } as i32 {
            continue;
        }
        for k in lo..hi {
            let (x, y) = if vertical { (line as usize, k as usize) } else { (k as usize, line as usize) };
            let p = &mut buf[y * w + x];
            let green = (((*p >> 8) & 255) + g).min(255);
            *p = (*p & 0xffff_00ff) | green << 8;
        }
    }
}

/// Greedy word wrap: a word joins the line while the line stays narrower than `width` pixels (`0x440f70`).
fn wrap(font: &Font, text: &str, width: i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let next = if cur.is_empty() { word.to_owned() } else { format!("{cur} {word}") };
        if !cur.is_empty() && font.measure(&next, 1) >= width {
            lines.push(std::mem::replace(&mut cur, word.to_owned()));
        } else {
            cur = next;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// A colour-font string with its top edge at `y` (the colour fonts keep their own pixels).
fn draw_text(canvas: &mut [u32], w: usize, h: usize, font: &Font, text: &str, x: i32, y: i32) {
    let placed = font.layout(text, 1);
    let top = placed.iter().map(|p| p.y).min().unwrap_or(0);
    for p in placed {
        let g = p.glyph;
        for gy in 0..g.height as i32 {
            for gx in 0..g.width as i32 {
                let k = (gy * g.width as i32 + gx) as usize;
                let (dx, dy) = (x + p.x + gx, y + p.y - top + gy);
                if g.mask[k] && dx >= 0 && dy >= 0 && (dx as usize) < w && (dy as usize) < h {
                    let [r, gr, b] = rgb565_to_rgb8(g.pixels[k]);
                    canvas[dy as usize * w + dx as usize] = pack(r, gr, b);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensow_formats::GameDir;

    fn l_t(l: &Lands) -> f64 {
        match l.stage {
            Stage::Selected { t, .. } => t,
            _ => 0.0,
        }
    }

    fn lands(video: bool) -> Option<(Assets, Lands)> {
        let game = GameDir::locate()?;
        let mut assets = Assets::load(game).ok()?;
        let l = Lands::new(&mut assets, PassedLevels::initial(), video, true);
        Some((assets, l))
    }

    #[test]
    fn locked_lands_say_no_data_and_open_ones_show_their_headings() {
        let Some((_, l)) = lands(false) else { return };
        assert_eq!(l.headings(3).0, "No data available");
        assert!(l.headings(1).0.starts_with("TERRITORY : 01"), "{:?}", l.headings(1));
        assert!(l.headings(1).1.contains("Canada"));
        assert!(l.briefing(1).0.starts_with("First, seize Research Lab"));
        assert!(l.briefing(1).1.starts_with("Objectives:"));
    }

    #[test]
    fn land_one_wraps_like_the_steam_screenshot_and_the_timeline_follows_the_exe() {
        let Some((_, mut l)) = lands(false) else { return };
        l.size = (1920, 1080);
        let g = l.geometry(1);
        assert_eq!(g.desc.len(), 10, "{:?}", g.desc);
        assert_eq!(g.desc[5], "machinery. As soon as you finish this task");
        assert_eq!(g.desc[8], "to defuse Anti-Air turrets so that our caryalls can safely");
        assert_eq!(g.goal.len(), 3, "{:?}", g.goal);
        assert!(g.goal[2].starts_with("Headquarters"));
        // objectives box: bottom 0xe3 below the centre, top 20 px per line higher than the 0xcf offset
        assert_eq!(g.goal_panel.1, 540 + 0xe3);
        assert_eq!(g.goal_panel.0, 540 - 60 + 0xcf);
        // reticle first (2 ticks a frame, 40 frames), then the box, the description, a pause and the objectives
        assert!(timeline(&g, 40).box1.is_none() && !timeline(&g, 40).buttons);
        assert_eq!(timeline(&g, 200).reticle, 39);
        assert!(timeline(&g, 90).box1.is_some() && timeline(&g, 90).desc.is_empty());
        assert_eq!(timeline(&g, 10_000).goal.len(), 3);
        assert!(timeline(&g, 10_000).box2.is_some());
    }

    #[test]
    fn clicking_an_open_land_selects_it_and_accept_starts_the_mission() {
        let Some((_, mut l)) = lands(false) else { return };
        // find a pixel of land 1 and of the locked land 3 in design space and click the window 1:1
        let hit = |l: &Lands, land: usize| {
            (0..H as i32).flat_map(|y| (0..W as i32).map(move |x| (x, y))).find(|&p| l.land_at(p) == Some(land))
        };
        let locked = hit(&l, 3).expect("land 3 has pixels");
        assert_eq!(l.click(locked.0, locked.1, W, H), Event::None);
        assert_eq!(l.selected(), None, "locked lands cannot be selected");
        let open = hit(&l, 1).expect("land 1 has pixels");
        l.click(open.0, open.1, W, H);
        assert_eq!(l.selected(), Some(1));
        assert_eq!(l.click(40, 450, W, H), Event::None, "the buttons only appear after the reticle");
        // that click skipped the animation: everything is typed and the buttons are there
        assert!(timeline(&l.geometry(1), (l_t(&l) * TICKS) as i64).buttons);
        l.update(0.0);
        // ACCEPT sits bottom left, BACK bottom right
        assert_eq!(l.click(40, 450, W, H), Event::Accept(1));
        l.click(open.0, open.1, W, H);
        l.update(3.0);
        l.click(580, 450, W, H);
        assert_eq!(l.selected(), None);
        assert_eq!(l.click(580, 450, W, H), Event::Back);
    }

    #[test]
    fn the_briefing_animation_plays_once_then_the_map_appears() {
        let Some((_, mut l)) = lands(true) else { return };
        assert!(l.playing_video());
        l.update(4.0);
        assert!(l.playing_video());
        l.update(5.0); // 104 frames at 12.5 fps = 8.32 s
        assert!(!l.playing_video());
        let mut buf = vec![0u32; 1280 * 800];
        l.draw(&mut buf, 1280, 800);
        assert!(buf.iter().any(|&p| p != 0));
    }
}
