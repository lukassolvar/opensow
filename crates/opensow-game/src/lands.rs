//! The campaign map (`CMenuBriefing`, `0x441210`): the world map where the player chooses the next mission.
//!
//! "New game" first plays the `mA.sgv` BRIEFING animation (state 0), then the map (state 3) shows the 22 lands
//! (`lands_blue/red.tsp`, one per mission); `mB.sgv`, a spinning globe, loops in the top-left corner. A land the player may play
//! blinks (state 2) or is plain blue (1, passed); locked ones (0) are red. Clicking an open land zooms a frame onto it and types
//! its briefing (state 4) with ACCEPT / BACK buttons; ACCEPT starts the mission. Notes: docs/lands.md.

use crate::assets::{load_backdrop, Assets};
use crate::intro::Anim;
use crate::ui::{self, Image};
use opensow_formats::config::PassedLevels;
use opensow_formats::font::Font;
use opensow_formats::lang::{self, LangFile};
use opensow_formats::sprite::{self, rgb565_to_rgb8, Frame, Kind};
use opensow_viewer::render::pack;

/// The design resolution of the screen; everything is positioned relative to its centre and scaled to the window.
pub const W: usize = 640;
pub const H: usize = 480;
pub const LANDS: usize = 22;

/// Where `mB` (128x112) plays; found by matching it against the globe in the last frame of `mA`.
const GLOBE: (usize, usize) = (32, 0);
/// The briefing panel (`0x440b37..0x440b5d`: centre -0xe3, -0x58 .. +0xd7, +0x98) the selected land's text types into.
const PANEL: (i32, i32, i32, i32) = (320 - 0xe3, 240 - 0x58, 320 + 0xd7, 240 + 0x98);
/// Seconds the frame takes to grow from the land to the panel.
const GROW: f64 = 0.6;
/// Characters typed per second (the exe reveals one per two ticks of its 60 Hz clock per call; this is a guess).
const TYPE_RATE: f64 = 60.0;

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
    pub progress: PassedLevels,
    hover: Option<usize>,
    clock: f64,
    /// Mouse position in the 640x480 design space.
    cursor: (i32, i32),
    canvas: Vec<u32>,
    /// Effect ids to play (the game drains them): 41 location, 43 typing, 44/46 frame and flash, 29 accept.
    pub sounds: Vec<u8>,
    typed: usize,
}

fn frames(assets: &Assets, name: &str) -> Vec<Frame> {
    assets.game.resolve(&format!("Data\\Sprites2\\Menu\\{name}.tsp")).and_then(|p| std::fs::read(p).ok()).and_then(|d| sprite::parse(&d, Kind::Color).ok()).unwrap_or_default()
}

/// Window pixel -> design pixel for a 640x480 picture fitted into `w` x `h`.
fn fit(w: usize, h: usize) -> (f32, f32, f32) {
    let s = (w as f32 / W as f32).min(h as f32 / H as f32);
    ((w as f32 - W as f32 * s) / 2.0, (h as f32 - H as f32 * s) / 2.0, s)
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
            progress,
            hover: None,
            clock: 0.0,
            cursor: (-1, -1),
            canvas: vec![0; W * H],
            sounds: Vec::new(),
            typed: 0,
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
        match &mut self.stage {
            Stage::Video(a) => {
                a.update(dt);
                if a.finished() {
                    self.stage = Stage::Map;
                }
            }
            Stage::Selected { t, .. } => {
                *t += dt;
                // a tick of the typewriter sound for every four characters
                let chars = ((*t - GROW).max(0.0) * TYPE_RATE) as usize / 4;
                if chars > self.typed {
                    self.typed = chars;
                    if self.sounds.len() < 8 {
                        self.sounds.push(43);
                    }
                }
            }
            Stage::Map => {}
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

    pub fn mouse_moved(&mut self, x: i32, y: i32, w: usize, h: usize) {
        let (ox, oy, s) = fit(w, h);
        self.cursor = (((x as f32 - ox) / s) as i32, ((y as f32 - oy) / s) as i32);
        let hover = if matches!(self.stage, Stage::Map) { self.land_at(self.cursor) } else { None };
        if hover.is_some() && hover != self.hover {
            self.sounds.push(41); // briefing-location
        }
        self.hover = hover;
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
                    self.typed = 0;
                    self.sounds.extend([46, 41, 44]); // blik, location, rect01
                }
                Event::None
            }
            Stage::Selected { land, .. } => {
                if self.over_button(true) {
                    self.stage = Stage::Map;
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

    /// Render the 640x480 design picture and scale it into the window buffer with black bars.
    pub fn draw(&mut self, buf: &mut [u32], w: usize, h: usize) {
        self.render();
        buf.fill(0);
        let (ox, oy, s) = fit(w, h);
        let (dw, dh) = ((W as f32 * s) as usize, (H as f32 * s) as usize);
        for y in 0..dh.min(h) {
            let sy = ((y as f32 / s) as usize).min(H - 1);
            let dy = y + oy as usize;
            if dy >= h {
                break;
            }
            for x in 0..dw {
                let dx = x + ox as usize;
                if dx >= w {
                    break;
                }
                buf[dy * w + dx] = self.canvas[sy * W + ((x as f32 / s) as usize).min(W - 1)];
            }
        }
    }

    fn render(&mut self) {
        let mut canvas = std::mem::take(&mut self.canvas);
        if let Stage::Video(a) = &self.stage {
            canvas.fill(0);
            a.draw(&mut canvas, W, H, (0, 0, W, H));
            self.canvas = canvas;
            return;
        }
        match &self.bg {
            Some(img) => {
                for y in 0..H {
                    for x in 0..W {
                        let i = ((y * img.h / H) * img.w + x * img.w / W) * 3;
                        canvas[y * W + x] = pack(img.rgb[i], img.rgb[i + 1], img.rgb[i + 2]);
                    }
                }
            }
            None => canvas.fill(0x00001000),
        }
        let blink = self.clock * 60.0 % 60.0 <= 30.0;
        for l in 1..=LANDS {
            let s = self.progress.states[l];
            let Some(f) = (if s == 0 { self.red.get(l - 1) } else { self.blue.get(l - 1) }) else { continue };
            let lit = (self.hover == Some(l) && matches!(self.stage, Stage::Map)) || (s == 2 && blink);
            let (x, y) = Self::top_left(f);
            ui::sprite_lit(&mut canvas, W, H, f, x, y, 1.0, if lit { 60 } else { 0 });
        }
        if let Some(g) = &self.globe {
            g.draw(&mut canvas, W, H, (GLOBE.0, GLOBE.1, g.width, g.height));
        }
        let hot = |back: bool, me: &Lands| me.over_button(back);
        match self.stage {
            Stage::Map => {
                let (a, b) = self.hover.map_or_else(|| (String::new(), String::new()), |l| self.headings(l));
                self.heading(&mut canvas, &a, &b);
                self.draw_button(&mut canvas, true, hot(true, self));
            }
            Stage::Selected { land, t } => {
                let (a, b) = self.headings(land);
                self.heading(&mut canvas, &a, &b);
                self.draw_selection(&mut canvas, land, t);
                self.draw_button(&mut canvas, false, hot(false, self));
                self.draw_button(&mut canvas, true, hot(true, self));
            }
            Stage::Video(_) => {}
        }
        self.canvas = canvas;
    }

    fn draw_button(&self, canvas: &mut [u32], back: bool, hover: bool) {
        if let Some(f) = self.buttons.get(back as usize + if hover { 2 } else { 0 }).or_else(|| self.button(back)) {
            let (x, y) = Self::top_left(f);
            ui::sprite(canvas, W, H, f, x, y, 1.0);
        }
    }

    fn heading(&self, canvas: &mut [u32], a: &str, b: &str) {
        for (i, line) in [a, b].into_iter().enumerate() {
            let x = (W as i32 - self.big.measure(line, 1)) / 2;
            draw_text(canvas, &self.big, line, x, 81 + i as i32 * 24);
        }
    }

    /// The point marker, the growing frame and the typed text of a selected land (state 4).
    fn draw_selection(&self, canvas: &mut [u32], land: usize, t: f64) {
        let Some(p) = self.points.get(land - 1) else { return };
        let (px, py) = Self::top_left(p);
        ui::sprite(canvas, W, H, p, px, py, 1.0);
        let (cx, cy) = (px + p.width as i32 / 2, py + p.height as i32 / 2);
        let k = (t / GROW).min(1.0) as f32;
        let lerp = |a: i32, b: i32| a + ((b - a) as f32 * k) as i32;
        let (x0, y0, x1, y1) = (lerp(cx, PANEL.0), lerp(cy, PANEL.1), lerp(cx, PANEL.2), lerp(cy, PANEL.3));
        ui::dim(canvas, W, H, ui::Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }, 170);
        for (d, c) in [(-1, (0, 128, 0)), (1, (0, 128, 0)), (0, (0, 255, 0))] {
            outline(canvas, x0 - d, y0 - d, x1 + d, y1 + d, c);
        }
        if let Some(f) = (!self.reticle.is_empty()).then(|| &self.reticle[(t * 30.0) as usize % self.reticle.len()]) {
            ui::sprite(canvas, W, H, f, cx - f.hot_x as i32, cy - f.hot_y as i32, 1.0);
        }
        if t < GROW {
            return;
        }
        let (desc, goal) = self.briefing(land);
        let shown = ((t - GROW) * TYPE_RATE) as usize;
        let width = PANEL.2 - PANEL.0 - 40;
        let mut y = PANEL.1 + 11;
        let mut left = shown;
        for text in [desc, goal] {
            for line in wrap(&self.small, &text, width) {
                let n = line.chars().count().min(left);
                if n == 0 {
                    return;
                }
                draw_text(canvas, &self.small, &line.chars().take(n).collect::<String>(), PANEL.0 + 20, y);
                left -= n;
                y += 18;
            }
            y += 10;
        }
    }
}

fn outline(canvas: &mut [u32], x0: i32, y0: i32, x1: i32, y1: i32, c: (u8, u8, u8)) {
    let put = |canvas: &mut [u32], x: i32, y: i32| {
        if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H {
            canvas[y as usize * W + x as usize] = pack(c.0, c.1, c.2);
        }
    };
    for x in x0..=x1 {
        put(canvas, x, y0);
        put(canvas, x, y1);
    }
    for y in y0..=y1 {
        put(canvas, x0, y);
        put(canvas, x1, y);
    }
}

/// Greedy word wrap to `width` pixels.
fn wrap(font: &Font, text: &str, width: i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let next = if cur.is_empty() { word.to_owned() } else { format!("{cur} {word}") };
        if !cur.is_empty() && font.measure(&next, 1) > width {
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
fn draw_text(canvas: &mut [u32], font: &Font, text: &str, x: i32, y: i32) {
    let placed = font.layout(text, 1);
    let top = placed.iter().map(|p| p.y).min().unwrap_or(0);
    for p in placed {
        let g = p.glyph;
        for gy in 0..g.height as i32 {
            for gx in 0..g.width as i32 {
                let k = (gy * g.width as i32 + gx) as usize;
                let (dx, dy) = (x + p.x + gx, y + p.y - top + gy);
                if g.mask[k] && dx >= 0 && dy >= 0 && (dx as usize) < W && (dy as usize) < H {
                    let [r, gr, b] = rgb565_to_rgb8(g.pixels[k]);
                    canvas[dy as usize * W + dx as usize] = pack(r, gr, b);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensow_formats::GameDir;

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
        // ACCEPT sits bottom left, BACK bottom right
        assert_eq!(l.click(40, 450, W, H), Event::Accept(1));
        l.click(open.0, open.1, W, H);
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
