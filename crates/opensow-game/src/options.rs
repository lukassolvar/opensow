//! The options menu (`CMenuOptions`, `0x44c590`): the original art, controls and sounds. Notes: docs/options.md.

use crate::assets::{load_backdrop, Assets};
use crate::ui::{self, Image};
use opensow_formats::config::{Audio, Options, SettingsFile};
use opensow_formats::effects::Blit;
use opensow_formats::lang::{self, LangFile};
use opensow_formats::sprite::{self, Frame, Kind};
use opensow_viewer::render::pack;

/// Design resolution; frames are placed relative to its centre.
pub const W: usize = 640;
pub const H: usize = 480;

/// The screen sizes the resolution arrows step through (`0x49ce68`).
pub const RESOLUTIONS: [(u32, u32); 5] = [(640, 480), (800, 600), (1024, 768), (1600, 900), (1920, 1080)];

/// Everything the menu edits: the settings file (`COptions`) and the audio object's volumes and switches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameOptions {
    pub cfg: Options,
    pub music_volume: i32,
    pub voice_volume: i32,
    pub fx_volume: i32,
    pub music_on: bool,
    pub voice_on: bool,
    pub fx_on: bool,
}

impl Default for GameOptions {
    /// The first-run values the exe stores when `State of War_1.cfg` is missing (`0x452fd1`, `0x408510`; docs/options.md):
    /// radar and quake on, clouds off, vblank on, hardware cursor off, scroll speeds 3 (mouse) and 5 (keys), 1024 x 768, all
    /// volumes 100 and music, speech and effects on.
    fn default() -> Self {
        GameOptions::from_file(&SettingsFile::first_run())
    }
}

/// The settings file's name; the exe opens it in the current directory (`0x452a90` stores the name at `+4`).
pub const FILE: &str = "State of War_1.cfg";

impl GameOptions {
    pub fn from_file(f: &SettingsFile) -> GameOptions {
        let a = &f.audio;
        GameOptions { cfg: f.options.clone(), music_volume: a.music_volume as i32, voice_volume: a.voice_volume as i32, fx_volume: a.fx_volume as i32, music_on: a.music_on, voice_on: a.voice_on, fx_on: a.fx_on }
    }

    pub fn to_file(&self) -> SettingsFile {
        let audio = Audio {
            music_volume: self.music_volume.clamp(0, 100) as u32,
            voice_volume: self.voice_volume.clamp(0, 100) as u32,
            fx_volume: self.fx_volume.clamp(0, 100) as u32,
            music_on: self.music_on,
            voice_on: self.voice_on,
            fx_on: self.fx_on,
        };
        SettingsFile { options: self.cfg.clone(), audio }
    }

    /// `0x452c30`: the file's values, or the first-run ones when it is missing or unreadable. Tests never touch the disk.
    pub fn load() -> GameOptions {
        if cfg!(test) {
            return GameOptions::default();
        }
        std::fs::read(FILE).ok().and_then(|d| SettingsFile::parse(&d).ok()).map_or_else(GameOptions::default, |f| GameOptions::from_file(&f))
    }

    /// `0x453310` (O.K. in the options menu): writes the file; a write error is ignored like a read-only folder would be.
    pub fn save(&self) {
        if !cfg!(test) {
            let _ = std::fs::write(FILE, self.to_file().write());
        }
    }
}

/// Map scroll in pixels per second for a KEYS / MOUSE SCROLL setting (0..=10): the logic step scrolls `ticks * speed`
/// and `0x457f80` doubles it, 60 ticks per second (`0x415ef1`, `0x416b8b`).
pub fn scroll_px_per_second(speed: u32) -> f32 {
    2.0 * 60.0 * speed as f32
}

/// The two animated gears of the picture (`0x44ade0`): sprite `extras\kruh` (16 frames of a turning cog), each drawn as a
/// shadow (`Blit::Shade`, 3 px right/down) under the cog itself (`Blit::Additive`). Both steps happen every 4 ticks (a
/// counter `[0x71cdd4]` counts 3, 2, 1, 0, -1 and wraps), the first cog shows frame `a`, the second `15 - b`; `a` starts at
/// 0 and `b` at 8 (`[0x49ce90]`, so the cogs are half a turn apart), both globals that keep running between visits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gears {
    counter: i32,
    a: usize,
    b: usize,
}

impl Default for Gears {
    fn default() -> Self {
        Gears { counter: 0, a: 0, b: 8 }
    }
}

impl Gears {
    /// Runs `ticks` logic ticks (60 per second, the loop at `0x44ae78`).
    pub fn step(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.counter -= 1;
            if self.counter < 0 {
                self.counter = 3;
                self.a = (self.a + 1) % 16;
                self.b = (self.b + 1) % 16;
            }
        }
    }

    /// Frames of the first and second cog.
    pub fn frames(&self) -> (usize, usize) {
        (self.a, 15 - self.b)
    }
}

/// Where the cogs sit relative to the screen centre: (shadow, cog) for the first and second one (`0x44afea..0x44b093`).
const GEAR1: ((i32, i32), (i32, i32)) = ((0x6c, 0x53), (0x69, 0x50));
const GEAR2: ((i32, i32), (i32, i32)) = ((0xbd, 0x54), (0xba, 0x51));

/// Draws a frame onto the canvas with one of the exe's blit arithmetics (RGB565, `Blit::apply565`); `Shade` uses the sprite
/// only as a stencil.
fn blit(canvas: &mut [u32], f: &Frame, x: i32, y: i32, mode: Blit) {
    for sy in 0..f.height as i32 {
        let dy = y + sy;
        if !(0..H as i32).contains(&dy) {
            continue;
        }
        for sx in 0..f.width as i32 {
            let dx = x + sx;
            let i = (sy * f.width as i32 + sx) as usize;
            if !(0..W as i32).contains(&dx) || !f.mask[i] {
                continue;
            }
            let p = &mut canvas[dy as usize * W + dx as usize];
            let d = ((*p >> 19 & 31) << 11 | (*p >> 10 & 63) << 5 | (*p >> 3 & 31)) as u16;
            let o = mode.apply565(d, f.pixels[i]);
            let (r, g, b) = ((o >> 11) as u32, (o >> 5 & 63) as u32, (o & 31) as u32);
            *p = pack((r << 3 | r >> 2) as u8, (g << 2 | g >> 4) as u8, (b << 3 | b >> 2) as u8);
        }
    }
}

/// A clickable control: label buttons 0..=10, arrow buttons 0..=13, O.K. and CANCEL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Label(usize),
    Arrow(usize),
    Ok,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    None,
    /// O.K.: keep these options.
    Done(GameOptions),
    /// CANCEL: the settings file is reloaded (`0x452c30` reads the audio object from it too), so everything is back.
    Cancel(GameOptions),
}

pub struct OptionsMenu {
    pub opts: GameOptions,
    /// What the menu was opened with (CANCEL goes back to the file's values; the audio object is part of the file).
    saved: GameOptions,
    bg: Option<Image>,
    bg_full: Option<Image>,
    frames: Vec<Frame>,
    digits: Vec<Frame>,
    /// `extras\kruh`, the cog frames.
    gear_frames: Vec<Frame>,
    pub gears: Gears,
    /// Real time of the last drawn frame (the cogs advance 60 ticks per second).
    clock: Option<std::time::Instant>,
    tick_rest: f64,
    strings: Option<LangFile>,
    fonts: std::rc::Rc<(opensow_formats::font::Font, opensow_formats::font::Font)>,
    hover: Option<Hit>,
    /// Mouse position in the 640x480 design space.
    cursor: (i32, i32),
    canvas: Vec<u32>,
    /// Effect ids for the game to play: 30 hover, 33 arrow, 34 switch, 38 O.K., 39 CANCEL.
    pub sounds: Vec<u8>,
}

fn load(assets: &Assets, name: &str) -> Vec<Frame> {
    assets.game.resolve(&format!("Data\\Sprites2\\Menu\\{name}.tsp")).and_then(|p| std::fs::read(p).ok()).and_then(|d| sprite::parse(&d, Kind::Color).ok()).unwrap_or_default()
}

fn fit(w: usize, h: usize) -> (f32, f32, f32) {
    let s = (w as f32 / W as f32).min(h as f32 / H as f32);
    ((w as f32 - W as f32 * s) / 2.0, (h as f32 - H as f32 * s) / 2.0, s)
}

/// Frame of the label art that is the click area of label button `k` (frames 41..51).
const LABEL0: usize = 41;
/// Normal arrow frames start here; the highlighted ones are 14 later.
const ARROW0: usize = 13;
/// The "On" frames (2..9) mark where each switch's value is written.
const ON0: usize = 2;
/// The resolution caption frame (10..12 share one spot).
const RES_FRAME: usize = 10;

/// Language-file index (0-based) of each label button's text: RADAR, QUAKE, CLOUDS, FX, VOICE, MUSIC, VBLANK, HW MOUSE,
/// KEYS SCROLL, MOUSE SCROLL, RESOLUTION.
const LABEL_TEXT: [usize; 11] = [9, 10, 11, 14, 16, 15, 13, 12, 18, 19, 17];

impl OptionsMenu {
    pub fn new(assets: &mut Assets, opts: GameOptions) -> OptionsMenu {
        let language = assets.game.resolve("Data\\lang.dat").and_then(|p| std::fs::read(p).ok()).and_then(|d| lang::read_lang_dat(&d)).unwrap_or(lang::Language::English);
        let strings = assets.game.resolve(&format!("Data\\Language\\{}", language.file_name())).and_then(|p| std::fs::read(p).ok()).and_then(|d| LangFile::parse(&d).ok());
        OptionsMenu {
            saved: opts.clone(),
            opts,
            bg: load_backdrop(&assets.game, "Options"),
            bg_full: load_backdrop(&assets.game, "OptionsBackground"),
            frames: load(assets, "options"),
            digits: load(assets, "options_numbers"),
            gear_frames: assets.art.load("extras\\kruh.ps6").map(|f| (*f).clone()).unwrap_or_default(),
            gears: Gears::default(),
            clock: None,
            tick_rest: 0.0,
            strings,
            fonts: assets.font("medium"),
            hover: None,
            cursor: (-1, -1),
            canvas: vec![0; W * H],
            sounds: Vec::new(),
        }
    }

    fn string(&self, index: usize) -> &str {
        self.strings.as_ref().and_then(|s| s.get(index)).unwrap_or("")
    }

    fn top_left(f: &Frame) -> (i32, i32) {
        (W as i32 / 2 - f.hot_x as i32, H as i32 / 2 - f.hot_y as i32)
    }

    fn rect(&self, frame: usize) -> Option<ui::Rect> {
        let f = self.frames.get(frame)?;
        let (x, y) = Self::top_left(f);
        Some(ui::Rect { x, y, w: f.width as i32, h: f.height as i32 })
    }

    /// The control under a design-space point. Arrows win over the label rectangles they overlap.
    pub fn hit_at(&self, p: (i32, i32)) -> Option<Hit> {
        let inside = |frame: usize| self.rect(frame).is_some_and(|r| r.contains(p.0, p.1));
        if inside(1) {
            return Some(Hit::Ok);
        }
        if inside(0) {
            return Some(Hit::Cancel);
        }
        if let Some(k) = (0..14).find(|&k| inside(ARROW0 + k)) {
            return Some(Hit::Arrow(k));
        }
        (0..11).find(|&k| inside(LABEL0 + k)).map(Hit::Label)
    }

    /// Design-space point at the centre of a control (for scripted clicks).
    pub fn centre(&self, hit: Hit) -> (i32, i32) {
        let frame = match hit {
            Hit::Ok => 1,
            Hit::Cancel => 0,
            Hit::Arrow(k) => ARROW0 + k,
            Hit::Label(k) => LABEL0 + k,
        };
        self.rect(frame).map_or((0, 0), |r| (r.x + r.w / 2, r.y + r.h / 2))
    }

    pub fn mouse_moved(&mut self, x: i32, y: i32, w: usize, h: usize) {
        let (ox, oy, s) = fit(w, h);
        self.cursor = (((x as f32 - ox) / s) as i32, ((y as f32 - oy) / s) as i32);
        let hover = self.hit_at(self.cursor);
        if hover.is_some() && hover != self.hover {
            self.sounds.push(30); // mainmenu-move
        }
        self.hover = hover;
    }

    pub fn hovered(&self) -> Option<Hit> {
        self.hover
    }

    pub fn click(&mut self, x: i32, y: i32, w: usize, h: usize) -> Event {
        let (ox, oy, s) = fit(w, h);
        let p = (((x as f32 - ox) / s) as i32, ((y as f32 - oy) / s) as i32);
        match self.hit_at(p) {
            Some(hit) => self.press(hit),
            None => Event::None,
        }
    }

    /// Escape: the same as CANCEL.
    pub fn escape(&mut self) -> Event {
        self.press(Hit::Cancel)
    }

    /// What clicking a control does (the two jump tables `0x44c4b8` / `0x44c4d8`), with the control's sound.
    pub fn press(&mut self, hit: Hit) -> Event {
        let o = &mut self.opts;
        match hit {
            Hit::Ok => {
                self.sounds.push(38); // ingame-ano
                return Event::Done(self.opts.clone());
            }
            Hit::Cancel => {
                self.sounds.push(39); // ingame-nie
                let back = self.saved.clone();
                self.opts = back.clone();
                return Event::Cancel(back);
            }
            Hit::Label(k) => {
                self.sounds.push(34); // options-onoff
                match k {
                    0 => o.cfg.flag_a ^= true,
                    1 => o.cfg.flag_b ^= true,
                    2 => o.cfg.detail = if o.cfg.detail == 0 { 2 } else { 0 },
                    3 => o.fx_on ^= true,
                    4 => o.voice_on ^= true,
                    5 => o.music_on ^= true,
                    6 => o.cfg.flag_d ^= true,
                    7 => o.cfg.flag_c ^= true,
                    _ => {}
                }
            }
            Hit::Arrow(k) => {
                self.sounds.push(33); // options-arrow
                let step = |v: i32, up: bool| (v + if up { 5 } else { -5 }).clamp(0, 100);
                let up = k % 2 == 1;
                match k {
                    0 | 1 => o.cfg.detail = if up { (o.cfg.detail + 1).min(4) } else { o.cfg.detail.saturating_sub(1) },
                    2 | 3 => o.fx_volume = step(o.fx_volume, up),
                    4 | 5 => o.voice_volume = step(o.voice_volume, up),
                    6 | 7 => o.music_volume = step(o.music_volume, up),
                    8 | 9 => o.cfg.scroll_b = if up { (o.cfg.scroll_b + 1).min(10) } else { o.cfg.scroll_b.saturating_sub(1) },
                    10 | 11 => o.cfg.scroll_a = if up { (o.cfg.scroll_a + 1).min(10) } else { o.cfg.scroll_a.saturating_sub(1) },
                    _ => {
                        if let Some(i) = RESOLUTIONS.iter().position(|&r| r == (o.cfg.width, o.cfg.height)) {
                            let j = if up { (i + 1).min(4) } else { i.saturating_sub(1) };
                            (o.cfg.width, o.cfg.height) = RESOLUTIONS[j];
                        }
                    }
                }
            }
        }
        Event::None
    }

    fn text(&self, canvas: &mut [u32], s: &str, x: i32, y: i32, bright: bool) {
        let fg = if bright { (255, 255, 255) } else { (180, 180, 180) };
        ui::text(canvas, W, H, &self.fonts, s, x, y, 1.0, fg, (0, 30, 0));
    }

    fn number(&self, canvas: &mut [u32], x: i32, y: i32, value: i32) {
        let v = value.clamp(0, 999) as usize;
        let (h, t, u) = (v / 100, v / 10 % 10, v % 10);
        let mut put = |d: usize, dx: i32| {
            if let Some(f) = self.digits.get(d) {
                ui::sprite(canvas, W, H, f, x + dx - f.hot_x as i32, y - f.hot_y as i32, 1.0);
            }
        };
        if h > 0 {
            put(h, -12);
            put(t, 0);
            put(u, 12);
        } else if t > 0 {
            put(t, -6);
            put(u, 6);
        } else {
            put(u, 0);
        }
    }

    fn render(&mut self) {
        let mut canvas = std::mem::take(&mut self.canvas);
        canvas.fill(0);
        if let Some(bg) = &self.bg {
            for y in 0..H {
                for x in 0..W {
                    let (sx, sy) = (x * bg.w / W, y * bg.h / H);
                    let i = (sy * bg.w + sx) * 3;
                    canvas[y * W + x] = pack(bg.rgb[i], bg.rgb[i + 1], bg.rgb[i + 2]);
                }
            }
        }
        // the two turning cogs: both shadows, then both cogs (`0x44afea..0x44b093`), under every button
        let (f1, f2) = self.gears.frames();
        let (cx, cy) = (W as i32 / 2, H as i32 / 2);
        let place = |frame: Option<&Frame>, (dx, dy): (i32, i32)| frame.map(|f| (f.clone(), cx + dx - f.hot_x as i32, cy + dy - f.hot_y as i32));
        let cogs: Vec<(Frame, i32, i32, Blit)> = [
            (f1, GEAR1.0, Blit::Shade),
            (f2, GEAR2.0, Blit::Shade),
            (f1, GEAR1.1, Blit::Additive),
            (f2, GEAR2.1, Blit::Additive),
        ]
        .into_iter()
        .filter_map(|(k, at, mode)| place(self.gear_frames.get(k), at).map(|(f, x, y)| (f, x, y, mode)))
        .collect();
        for (f, x, y, mode) in &cogs {
            blit(&mut canvas, f, *x, *y, *mode);
        }
        let on = |k: usize| self.hover == Some(Hit::Label(k));
        // title and headings
        let t = self.string(4).to_owned();
        let tw = ui::text_width(&self.fonts, &t, 1.0);
        self.text(&mut canvas, &t, W as i32 / 2 - tw / 2 - 0x2a, 10, true);
        for (idx, x, y) in [(6, 21, 62), (7, 21, 255), (8, 343, 62)] {
            let s = self.string(idx).to_owned();
            self.text(&mut canvas, &s, x, y, true);
        }
        // switch rows: label and On/Off value
        let o = &self.opts;
        let switches = [o.cfg.flag_a, o.cfg.flag_b, o.cfg.detail != 0, o.fx_on, o.voice_on, o.music_on, o.cfg.flag_d, o.cfg.flag_c];
        let lh = ui::text_height(&self.fonts, 1.0);
        for k in 0..11 {
            let Some(r) = self.rect(LABEL0 + k) else { continue };
            let label = self.string(LABEL_TEXT[k]).to_owned();
            self.text(&mut canvas, &label, r.x, r.y + (r.h - lh) / 2, on(k));
            if let (Some(&state), Some(v)) = (switches.get(k), self.rect(ON0 + k)) {
                let word = self.string(if state { 267 } else { 268 }).to_owned();
                self.text(&mut canvas, &word, v.x, v.y + (v.h - lh) / 2, on(k));
            }
        }
        // resolution caption
        if let Some(r) = self.rect(RES_FRAME) {
            let cap = format!("{}x{}", o.cfg.width, o.cfg.height);
            self.text(&mut canvas, &cap, r.x, r.y + (r.h - lh) / 2, on(10));
        }
        // arrows
        for k in 0..14 {
            let f = if self.hover == Some(Hit::Arrow(k)) { ARROW0 + 14 + k } else { ARROW0 + k };
            if let Some(f) = self.frames.get(f) {
                let (x, y) = Self::top_left(f);
                ui::sprite(&mut canvas, W, H, f, x, y, 1.0);
            }
        }
        // numbers between the arrows
        let row_y = |k: usize| self.rect(ARROW0 + k).map_or(0, |r| r.y + r.h / 2);
        for (x, k, v) in [(255, 0, o.cfg.detail as i32), (550, 2, o.fx_volume), (550, 4, o.voice_volume), (550, 6, o.music_volume), (284, 8, o.cfg.scroll_b as i32), (284, 10, o.cfg.scroll_a as i32)] {
            self.number(&mut canvas, x, row_y(k), v);
        }
        for (i, hit) in [Hit::Cancel, Hit::Ok].into_iter().enumerate() {
            let _ = hit;
            if let Some(f) = self.frames.get(i) {
                let (x, y) = Self::top_left(f);
                ui::sprite(&mut canvas, W, H, f, x, y, 1.0);
            }
        }
        self.canvas = canvas;
    }

    pub fn draw(&mut self, buf: &mut [u32], w: usize, h: usize) {
        let now = std::time::Instant::now();
        if let Some(last) = self.clock.replace(now) {
            self.tick_rest += now.duration_since(last).as_secs_f64().min(0.25) * 60.0;
            let ticks = self.tick_rest as u32;
            self.tick_rest -= ticks as f64;
            self.gears.step(ticks);
        }
        self.render();
        buf.fill(0);
        let (ox, oy, s) = fit(w, h);
        // the larger background fills what the 640x480 picture does not cover (`0x44ade0`)
        if let Some(bg) = &self.bg_full {
            for y in 0..h {
                let sy = (y * bg.h / h).min(bg.h - 1);
                for x in 0..w {
                    let sx = (x * bg.w / w).min(bg.w - 1);
                    let i = (sy * bg.w + sx) * 3;
                    buf[y * w + x] = pack(bg.rgb[i] / 2, bg.rgb[i + 1] / 2, bg.rgb[i + 2] / 2);
                }
            }
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(opts: GameOptions) -> OptionsMenu {
        OptionsMenu {
            saved: opts.clone(),
            opts,
            bg: None,
            bg_full: None,
            frames: Vec::new(),
            digits: Vec::new(),
            gear_frames: Vec::new(),
            gears: Gears::default(),
            clock: None,
            tick_rest: 0.0,
            strings: None,
            fonts: std::rc::Rc::new((opensow_formats::font::Font::new(Default::default()), opensow_formats::font::Font::new(Default::default()))),
            hover: None,
            cursor: (0, 0),
            canvas: vec![0; W * H],
            sounds: Vec::new(),
        }
    }

    #[test]
    fn switches_follow_the_jump_table() {
        let mut m = menu(GameOptions::default());
        m.press(Hit::Label(0));
        assert!(!m.opts.cfg.flag_a);
        m.press(Hit::Label(1));
        assert!(!m.opts.cfg.flag_b);
        m.press(Hit::Label(2));
        assert_eq!(m.opts.cfg.detail, 2, "the clouds switch flips 0 <-> 2");
        m.press(Hit::Label(2));
        assert_eq!(m.opts.cfg.detail, 0);
        m.press(Hit::Label(3));
        m.press(Hit::Label(4));
        m.press(Hit::Label(5));
        assert!(!m.opts.fx_on && !m.opts.voice_on && !m.opts.music_on);
        m.press(Hit::Label(6));
        assert!(!m.opts.cfg.flag_d, "vblank starts on");
        m.press(Hit::Label(7));
        assert!(m.opts.cfg.flag_c);
        let before = m.opts.clone();
        for k in 8..11 {
            m.press(Hit::Label(k));
        }
        assert_eq!(m.opts, before, "the scroll and resolution labels do nothing");
        assert_eq!(m.sounds.iter().filter(|&&s| s == 34).count(), 12);
    }

    #[test]
    fn arrows_clamp_like_the_exe() {
        let mut m = menu(GameOptions::default());
        for _ in 0..30 {
            m.press(Hit::Arrow(3));
            m.press(Hit::Arrow(4));
            m.press(Hit::Arrow(7));
            m.press(Hit::Arrow(9));
            m.press(Hit::Arrow(11));
            m.press(Hit::Arrow(1));
        }
        assert_eq!((m.opts.fx_volume, m.opts.voice_volume, m.opts.music_volume), (100, 0, 100));
        assert_eq!((m.opts.cfg.scroll_b, m.opts.cfg.scroll_a, m.opts.cfg.detail), (10, 10, 4));
        m.press(Hit::Arrow(2));
        assert_eq!(m.opts.fx_volume, 95);
        for _ in 0..30 {
            m.press(Hit::Arrow(0));
            m.press(Hit::Arrow(8));
        }
        assert_eq!((m.opts.cfg.detail, m.opts.cfg.scroll_b), (0, 0));
    }

    #[test]
    fn resolution_steps_through_the_table() {
        let mut m = menu(GameOptions::default());
        assert_eq!((m.opts.cfg.width, m.opts.cfg.height), (1024, 768));
        m.press(Hit::Arrow(13));
        m.press(Hit::Arrow(13));
        m.press(Hit::Arrow(13));
        assert_eq!((m.opts.cfg.width, m.opts.cfg.height), (1920, 1080), "last entry stays");
        m.press(Hit::Arrow(12));
        assert_eq!((m.opts.cfg.width, m.opts.cfg.height), (1600, 900));
        for _ in 0..10 {
            m.press(Hit::Arrow(12));
        }
        assert_eq!((m.opts.cfg.width, m.opts.cfg.height), (640, 480));
        m.opts.cfg.width = 123;
        m.press(Hit::Arrow(13));
        assert_eq!(m.opts.cfg.width, 123, "a size not in the table does not move");
    }

    #[test]
    fn ok_keeps_cancel_reloads_the_whole_file() {
        let mut m = menu(GameOptions::default());
        m.press(Hit::Label(0));
        m.press(Hit::Arrow(3));
        m.press(Hit::Label(5));
        let Event::Cancel(c) = m.press(Hit::Cancel) else { panic!() };
        assert!(c.cfg.flag_a, "radar is back");
        assert!(c.music_on && c.fx_volume == 100, "the audio object is in the file too (0x452c30 -> 0x407ff0)");
        assert_eq!(m.opts, c);
        assert_eq!(m.sounds.last(), Some(&39));
        m.press(Hit::Label(0));
        let Event::Done(d) = m.press(Hit::Ok) else { panic!() };
        assert!(!d.cfg.flag_a);
        assert_eq!(m.sounds.last(), Some(&38));
    }

    #[test]
    fn scroll_speed_is_two_pixels_per_tick_and_setting() {
        assert_eq!(scroll_px_per_second(0), 0.0);
        assert_eq!(scroll_px_per_second(5), 600.0, "keys default");
        assert_eq!(scroll_px_per_second(3), 360.0, "mouse default");
    }

    #[test]
    fn first_run_defaults_are_the_exes() {
        let d = GameOptions::default();
        let c = &d.cfg;
        assert_eq!((c.flag_a, c.flag_b, c.detail, c.flag_d, c.flag_c), (true, true, 0, true, false));
        assert_eq!((c.scroll_a, c.scroll_b, c.width, c.height), (3, 5, 1024, 768));
        assert_eq!((d.music_volume, d.voice_volume, d.fx_volume, d.music_on, d.voice_on, d.fx_on), (100, 100, 100, true, true, true));
        assert_eq!(GameOptions::from_file(&d.to_file()), d);
    }

    #[test]
    fn gears_step_every_four_ticks_half_a_turn_apart() {
        let mut g = Gears::default();
        assert_eq!(g.frames(), (0, 7), "second cog: 15 - 8");
        g.step(1);
        assert_eq!(g.frames(), (1, 6), "the counter starts at 0, so the first tick already steps");
        g.step(3);
        assert_eq!(g.frames(), (1, 6));
        g.step(1);
        assert_eq!(g.frames(), (2, 5));
        // a full turn is 16 steps = 64 ticks; the cogs turn in opposite directions
        let mut g = Gears::default();
        g.step(64);
        assert_eq!(g.frames(), (0, 7));
        g.step(1 + 4 * 7);
        assert_eq!(g.frames(), (8, 15 - 0));
    }

    #[test]
    fn digit_layout() {
        // value 100 -> 3 digits, 7 -> 1; only checks it does not panic without sprites
        let m = menu(GameOptions::default());
        let mut c = vec![0; W * H];
        for v in [0, 7, 45, 100, 5000] {
            m.number(&mut c, 100, 100, v);
        }
    }
}
