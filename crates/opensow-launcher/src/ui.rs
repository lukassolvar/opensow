//! Launcher state, layout and drawing. Pure CPU, no window: `main.rs` feeds it events and the `--screenshot` mode renders it headless.
//! The layout is a fixed 600x520 "logical" page scaled to fit the window and centred, so every window size shows the same page.

use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont};
use opensow_formats::lang::Language;
use opensow_formats::launcher::Settings;
use opensow_formats::{GameDir, DEFAULT_MAC_STEAM_DIR};
use std::path::{Path, PathBuf};

pub const PAGE_W: f32 = 600.0;
pub const PAGE_H: f32 = 520.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Original,
    Warmonger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Lang(Option<Language>),
    Browse(Slot),
    Clear(Slot),
    Detect,
    Play,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

pub struct Button {
    pub rect: Rect,
    pub action: Action,
    pub label: String,
    pub selected: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Unset,
    Found,
    Invalid,
}

pub fn status(slot: Slot, path: Option<&Path>) -> Status {
    let Some(p) = path else { return Status::Unset };
    let ok = match slot {
        Slot::Original => GameDir::new(p.to_path_buf()).resolve("Data").is_some_and(|d| d.is_dir()),
        Slot::Warmonger => GameDir::is_warmonger_dir(p),
    };
    if ok {
        Status::Found
    } else {
        Status::Invalid
    }
}

pub struct App {
    pub settings: Settings,
    pub message: String,
    pub font: Option<FontVec>,
    pub cursor: (f32, f32),
}

const FONTS: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
];

const NATIVE: [&str; 8] = ["English", "Slovenčina", "Čeština", "Polski", "Magyar", "Русский", "简体中文", "繁體中文"];

/// Colours as 0xRRGGBB.
const FLAG_W: f32 = 24.0;
const FLAG_H: f32 = 16.0;

/// Is `(x, y)` (relative to the centre, y down) inside a five-pointed star of outer radius `r`, turned by `rot` radians?
fn in_star(x: f32, y: f32, r: f32, rot: f32) -> bool {
    let pts: Vec<(f32, f32)> = (0..10)
        .map(|i| {
            let a = rot + i as f32 * std::f32::consts::PI / 5.0;
            let rr = if i % 2 == 0 { r } else { r * 0.382 };
            (rr * a.sin(), -rr * a.cos())
        })
        .collect();
    let mut inside = false;
    for i in 0..10 {
        let ((x1, y1), (x2, y2)) = (pts[i], pts[(i + 1) % 10]);
        if (y1 > y) != (y2 > y) && x < (x2 - x1) * (y - y1) / (y2 - y1) + x1 {
            inside = !inside;
        }
    }
    inside
}

/// The OpenSOW emblem (own drawing, no game art): a rounded badge with a star inside a crosshair ring. `(u, v)` in 0..1; `None` = outside.
pub fn emblem_pixel(u: f32, v: f32) -> Option<u32> {
    let (x, y) = (u - 0.5, v - 0.5);
    let (qx, qy) = (x.abs() - (0.5 - 0.22), y.abs() - (0.5 - 0.22));
    if (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) > 0.22 {
        return None;
    }
    let r = (x * x + y * y).sqrt();
    let rim = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) > 0.22 - 0.03;
    let tick = (x.abs() < 0.022 || y.abs() < 0.022) && r > 0.30 && r < 0.47;
    let ring = r > 0.355 && r < 0.395;
    Some(if rim {
        0x3a4560
    } else if tick || ring || in_star(x, y + 0.01, 0.25, 0.0) {
        ACCENT
    } else {
        // dark badge, lighter towards the top
        mix(0x2a3345, 0x161a21, (v * 255.0) as u32)
    })
}

/// RGBA bytes of the emblem at `size` x `size` (4 samples per pixel), for the window icon.
pub fn emblem_rgba(size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size * size * 4);
    for py in 0..size {
        for px in 0..size {
            let (mut rgb, mut n) = ([0u32; 3], 0u32);
            for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                if let Some(c) = emblem_pixel((px as f32 + sx) / size as f32, (py as f32 + sy) / size as f32) {
                    n += 1;
                    for (k, a) in rgb.iter_mut().enumerate() {
                        *a += c >> (16 - 8 * k) & 255;
                    }
                }
            }
            let div = n.max(1);
            out.extend([(rgb[0] / div) as u8, (rgb[1] / div) as u8, (rgb[2] / div) as u8, (n * 255 / 4) as u8]);
        }
    }
    out
}

/// Colour of a language's flag at `(u, v)` in 0..1 (3:2 box). English = United Kingdom, Chinese = PRC, traditional Chinese = ROC (Taiwan).
fn flag_color(l: Language, u: f32, v: f32) -> u32 {
    const WHITE: u32 = 0xffffff;
    const RED: u32 = 0xd22630;
    const BLUE: u32 = 0x1c3f94;
    let stripes = |cols: [u32; 3]| cols[((v * 3.0) as usize).min(2)];
    // inside a five-pointed star of radius r around (cx, cy), in box units where 1 unit = the box height
    let star = |cx: f32, cy: f32, r: f32, rot: f32| in_star((u - cx) * 1.5, v - cy, r, rot);
    match l {
        Language::English => {
            let (dx, dy) = ((u - 0.5).abs(), (v - 0.5).abs());
            if dx < 1.0 / 20.0 || dy < 1.0 / 10.0 {
                RED
            } else if dx < 1.0 / 12.0 || dy < 1.0 / 6.0 {
                WHITE
            } else if (dy - dx).abs() < 0.04 {
                RED
            } else if (dy - dx).abs() < 0.11 {
                WHITE
            } else {
                BLUE
            }
        }
        Language::Slovak => {
            // tricolour with a red shield (white double cross) on the hoist side
            let (sx, sy) = ((u - 0.33) * 1.5, v - 0.5);
            if sx.abs() < 0.19 && sy > -0.3 && sy < 0.3 + (0.19 - sx.abs()).min(0.0) {
                if (sx.abs() < 0.03 && sy.abs() < 0.2) || (sy > -0.12 && sy < -0.06 && sx.abs() < 0.11) || (sy > 0.0 && sy < 0.06 && sx.abs() < 0.08) {
                    WHITE
                } else {
                    RED
                }
            } else {
                stripes([WHITE, BLUE, RED])
            }
        }
        Language::Czech => {
            // blue wedge from the hoist to the middle
            if (v - 0.5).abs() < 0.5 - u * 0.5 * 1.0 / 0.5 * 0.5 && u < 0.5 && (v - 0.5).abs() < 0.5 - u {
                BLUE
            } else if v < 0.5 {
                WHITE
            } else {
                RED
            }
        }
        Language::Polish => if v < 0.5 { WHITE } else { RED },
        Language::Hungarian => stripes([RED, WHITE, 0x477050]),
        Language::Russian => stripes([WHITE, 0x0039a6, 0xd52b1e]),
        Language::ChineseSimplified => {
            const YELLOW: u32 = 0xffde00;
            let small = [(0.33, 0.12, 0.3), (0.40, 0.21, 0.6), (0.40, 0.33, 0.9), (0.33, 0.42, 0.0)];
            if star(0.17, 0.27, 0.19, 0.0) || small.iter().any(|&(cx, cy, _)| star(cx, cy, 0.06, 0.0)) {
                YELLOW
            } else {
                0xde2910
            }
        }
        Language::ChineseTraditional => {
            let (cx, cy) = ((u - 0.25) * 1.5, v - 0.25);
            let r = (cx * cx + cy * cy).sqrt();
            if u < 0.5 && v < 0.5 {
                // blue canton with the white sun (disc and rays)
                let a = cy.atan2(cx);
                let rays = 0.16 + 0.05 * (12.0 * a).cos().max(0.0).powi(3);
                if r < 0.1 || r < rays {
                    if r < 0.09 { 0x1c3f94 } else { WHITE }
                } else {
                    0x1c3f94
                }
            } else {
                0xde2910
            }
        }
    }
}

const CARD: u32 = 0x1f2530;
const BTN: u32 = 0x2b3342;
const BTN_HOVER: u32 = 0x38425a;
const ACCENT: u32 = 0xd9a441;
const TEXT: u32 = 0xe8ebf0;
const DIM: u32 = 0x8b93a3;
const GOOD: u32 = 0x62c370;
const BAD: u32 = 0xe36b5f;

impl App {
    pub fn new(settings: Settings) -> App {
        let font = FONTS.iter().find_map(|p| std::fs::read(p).ok().and_then(|d| FontVec::try_from_vec(d).ok()));
        // a saved language the port no longer offers (Polish, Hungarian, Russian) reads as "From game files"
        let mut settings = settings;
        settings.language = settings.language.filter(|l| l.supported());
        App { settings, message: String::new(), font, cursor: (-1.0, -1.0) }
    }

    pub fn path(&self, slot: Slot) -> Option<&PathBuf> {
        match slot {
            Slot::Original => self.settings.original.as_ref(),
            Slot::Warmonger => self.settings.warmonger.as_ref(),
        }
    }

    pub fn set_path(&mut self, slot: Slot, p: Option<PathBuf>) {
        match slot {
            Slot::Original => self.settings.original = p,
            Slot::Warmonger => self.settings.warmonger = p,
        }
    }

    fn card_y(slot: Slot) -> f32 {
        match slot {
            Slot::Original => 262.0,
            Slot::Warmonger => 366.0,
        }
    }

    fn lang_label(&self, l: Option<Language>) -> String {
        let Some(l) = l else { return "From game files".into() };
        let n = NATIVE[l.id() as usize];
        // a font without the script falls back to the code
        match &self.font {
            Some(f) if n.chars().all(|c| f.glyph_id(c).0 != 0) => n.into(),
            _ => format!("{} ({})", ["English", "Slovak", "Czech", "Polish", "Hungarian", "Russian", "Chinese (simpl.)", "Chinese (trad.)"][l.id() as usize], l.code()),
        }
    }

    /// Every button of the page in logical coordinates.
    pub fn buttons(&self) -> Vec<Button> {
        let mut out = vec![];
        let langs: Vec<Option<Language>> = std::iter::once(None).chain(Language::SUPPORTED.into_iter().map(Some)).collect();
        for (i, l) in langs.into_iter().enumerate() {
            let (col, row) = ((i % 3) as f32, (i / 3) as f32);
            out.push(Button { rect: Rect { x: 24.0 + col * 188.0, y: 96.0 + row * 44.0, w: 176.0, h: 36.0 }, action: Action::Lang(l), label: self.lang_label(l), selected: self.settings.language == l, enabled: true });
        }
        out.push(Button { rect: Rect { x: 456.0, y: 226.0, w: 120.0, h: 28.0 }, action: Action::Detect, label: "Auto-detect".into(), selected: false, enabled: true });
        for slot in [Slot::Original, Slot::Warmonger] {
            let y = Self::card_y(slot) + 54.0;
            let set = self.path(slot).is_some();
            let mk = |x: f32, w: f32, action, label: &str, enabled| Button { rect: Rect { x, y, w, h: 32.0 }, action, label: label.into(), selected: false, enabled };
            out.push(mk(40.0, 100.0, Action::Browse(slot), "Browse…", true));
            out.push(mk(148.0, 80.0, Action::Clear(slot), "Clear", set));
        }
        // one Play button: the campaign is switched on the world map, the game remembers it
        let valid = status(Slot::Original, self.settings.original.as_deref()) == Status::Found;
        out.push(Button { rect: Rect { x: 436.0, y: 472.0, w: 140.0, h: 36.0 }, action: Action::Play, label: "Play".into(), selected: valid, enabled: valid });
        out
    }

    pub fn click(&self, lx: f32, ly: f32) -> Option<Action> {
        self.buttons().into_iter().find(|b| b.enabled && b.rect.contains(lx, ly)).map(|b| b.action)
    }

    /// Which game card a logical point is over (drag and drop target).
    pub fn slot_at(&self, ly: f32) -> Option<Slot> {
        [Slot::Original, Slot::Warmonger].into_iter().find(|&s| ly >= Self::card_y(s) && ly < Self::card_y(s) + 94.0)
    }

    /// Try to use the dropped / chosen folder: a Warmonger install goes to its slot whatever card it landed on.
    pub fn offer_dir(&mut self, dir: PathBuf, hint: Option<Slot>) {
        let slot = if GameDir::is_warmonger_dir(&dir) { Slot::Warmonger } else { hint.unwrap_or(Slot::Original) };
        let st = status(slot, Some(&dir));
        self.set_path(slot, Some(dir.clone()));
        self.message = match st {
            Status::Found => format!("{} set.", if slot == Slot::Warmonger { "Warmonger" } else { "State of War" }),
            _ => format!("{} does not look like the {} folder.", dir.display(), if slot == Slot::Warmonger { "Warmonger" } else { "State of War" }),
        };
    }

    /// Fill the empty slots from the places the game is normally installed.
    pub fn detect(&mut self) {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut base: Option<PathBuf> = std::env::var_os(opensow_formats::GAME_DIR_ENV).map(PathBuf::from).filter(|p| p.join("Data").is_dir());
        if base.is_none() {
            base = home.map(|h| h.join(DEFAULT_MAC_STEAM_DIR)).filter(|p| p.join("Data").is_dir());
        }
        let mut found = 0;
        if let Some(b) = base {
            let sibling = GameDir::new(b.clone()).find_sibling_warmonger();
            if status(Slot::Original, self.settings.original.as_deref()) != Status::Found {
                self.settings.original = Some(b);
                found += 1;
            }
            if let Some(w) = sibling.filter(|_| status(Slot::Warmonger, self.settings.warmonger.as_deref()) != Status::Found) {
                self.settings.warmonger = Some(w);
                found += 1;
            }
        }
        self.message = if found == 0 { "Nothing new found; use Browse or drop a folder on a card.".into() } else { format!("Found {found} install(s).") };
    }

    // ---- drawing ----

    /// Draw the page into `buf` (w x h, 0x00RRGGBB), scaled to fit and centred.
    pub fn draw(&self, buf: &mut [u32], w: usize, h: usize) {
        let (scale, ox, oy) = Self::fit(w, h);
        paint_background(buf, w, h, scale, ox, oy);
        let mut c = Canvas { buf, w, h, scale, ox, oy };
        let font = self.font.as_ref();
        c.emblem(24.0, 14.0, 46.0);
        c.text(font, "OpenSOW", 82.0, 18.0, 30.0, ACCENT);
        c.text(font, "Launcher", 208.0, 30.0, 16.0, DIM);
        c.text(font, "Language", 24.0, 68.0, 17.0, TEXT);
        c.text(font, "Games", 24.0, 232.0, 17.0, TEXT);
        for slot in [Slot::Original, Slot::Warmonger] {
            let y = Self::card_y(slot);
            c.rrect(24.0, y, 552.0, 94.0, 8.0, CARD);
            let (name, sub) = match slot {
                Slot::Original => ("State of War", "folder with State of War.exe and Data"),
                Slot::Warmonger => ("State of War: Warmonger", "folder with State of War Warmonger.exe"),
            };
            c.text(font, name, 40.0, y + 10.0, 17.0, TEXT);
            let w_name = font.map_or(0.0, |f| text_width(f, name, 17.0));
            c.text(font, sub, 48.0 + w_name, y + 13.0, 12.0, DIM);
            let path = self.path(slot);
            let st = status(slot, path.map(|p| p.as_path()));
            let (label, col) = match st {
                Status::Unset => ("Not set", DIM),
                Status::Found => ("Found", GOOD),
                Status::Invalid => ("Not a valid folder", BAD),
            };
            let lw = font.map_or(0.0, |f| text_width(f, label, 13.0));
            c.text(font, label, 560.0 - lw, y + 13.0, 13.0, col);
            let shown = path.map(|p| p.display().to_string()).unwrap_or_else(|| "(drop a folder here or use Browse)".into());
            let shown = font.map_or(shown.clone(), |f| fit_tail(f, &shown, 13.0, 520.0));
            c.text(font, &shown, 40.0, y + 34.0, 13.0, if path.is_some() { TEXT } else { DIM });
        }
        for b in self.buttons() {
            let hover = b.enabled && b.rect.contains(self.cursor.0, self.cursor.1);
            let bg = if b.selected && matches!(b.action, Action::Lang(_)) { ACCENT } else if b.selected { 0x3f7d4a } else if hover { BTN_HOVER } else { BTN };
            let fg = if !b.enabled { 0x5b6272 } else if b.selected && matches!(b.action, Action::Lang(_)) { 0x16110a } else { TEXT };
            c.rrect(b.rect.x, b.rect.y, b.rect.w, b.rect.h, 6.0, bg);
            let tw = font.map_or(0.0, |f| text_width(f, &b.label, 14.0));
            let mut tx = b.rect.x + (b.rect.w - tw) / 2.0;
            if let Action::Lang(Some(l)) = b.action {
                // the flag sits at the left; the name is centred in what is left of the button
                c.flag(b.rect.x + 10.0, b.rect.y + (b.rect.h - FLAG_H) / 2.0, l);
                tx = b.rect.x + 10.0 + FLAG_W + (b.rect.w - 10.0 - FLAG_W - tw) / 2.0;
            }
            c.text(font, &b.label, tx, b.rect.y + 9.0, 14.0, fg);
        }
        let note = if self.message.is_empty() { "Switch between the two campaigns on the world map in the game." } else { &self.message };
        let note = font.map_or(note.to_owned(), |f| fit_tail(f, note, 13.0, 400.0));
        c.text(font, &note, 24.0, 482.0, 13.0, DIM);
        if font.is_none() {
            c.rrect(24.0, 300.0, 100.0, 10.0, 2.0, BAD); // no system font: the page is still usable but unlabelled
        }
    }

    /// (scale, x offset, y offset) of the page in a window of `w` x `h` pixels.
    pub fn fit(w: usize, h: usize) -> (f32, f32, f32) {
        let scale = (w as f32 / PAGE_W).min(h as f32 / PAGE_H);
        (scale, (w as f32 - PAGE_W * scale) / 2.0, (h as f32 - PAGE_H * scale) / 2.0)
    }
}

/// A dark map-like backdrop of our own making: a vertical gradient, faint contour lines of a smooth made-up height field, and a coarse grid.
/// Drawn in page coordinates so it scales with the page and fills the whole window.
fn paint_background(buf: &mut [u32], w: usize, h: usize, scale: f32, ox: f32, oy: f32) {
    for py in 0..h {
        let grad = mix(0x1b2231, 0x0f1116, (py * 255 / h.max(1)) as u32);
        let y = (py as f32 - oy) / scale;
        for px in 0..w {
            let x = (px as f32 - ox) / scale;
            // smooth height field -> iso-lines every 0.5 of its value
            let f = (x * 0.011).sin() * 1.6 + (y * 0.017 + 1.3).sin() * 1.3 + ((x + y) * 0.007).sin() * 1.8 + ((x - y) * 0.013 + 0.4).cos();
            let d = ((f * 2.0).fract() - 0.5).abs(); // 0.5 on the line
            let line = ((d - 0.46) / 0.04).clamp(0.0, 1.0) * 0.5;
            // grid every 40 page pixels, thinner than a pixel at scale 1
            let g = {
                let (gx, gy) = ((x / 40.0).fract().abs(), (y / 40.0).fract().abs());
                let near = |t: f32| (t.min(1.0 - t) * 40.0 * scale) < 0.7;
                if near(gx) || near(gy) { 0.35 } else { 0.0 }
            };
            let a = ((line * 22.0 + g * 14.0) as u32).min(255);
            buf[py * w + px] = mix(grad, 0x8fa3c8, a);
        }
    }
}

pub fn text_width(font: &FontVec, text: &str, size: f32) -> f32 {
    let s = font.as_scaled(PxScale::from(size));
    text.chars().map(|c| s.h_advance(s.glyph_id(c))).sum()
}

/// `text` shortened from the left with an ellipsis so it fits `max` logical pixels.
fn fit_tail(font: &FontVec, text: &str, size: f32, max: f32) -> String {
    if text_width(font, text, size) <= max {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for skip in 1..chars.len() {
        let s: String = std::iter::once('…').chain(chars[skip..].iter().copied()).collect();
        if text_width(font, &s, size) <= max {
            return s;
        }
    }
    "…".into()
}

struct Canvas<'a> {
    buf: &'a mut [u32],
    w: usize,
    h: usize,
    scale: f32,
    ox: f32,
    oy: f32,
}

fn mix(dst: u32, src: u32, a: u32) -> u32 {
    let ch = |s: u32| ((dst >> s & 255) * (255 - a) + (src >> s & 255) * a + 127) / 255;
    ch(16) << 16 | ch(8) << 8 | ch(0)
}

impl Canvas<'_> {
    /// Rounded rectangle in page coordinates.
    fn rrect(&mut self, x: f32, y: f32, rw: f32, rh: f32, r: f32, color: u32) {
        let (x0, y0, x1, y1) = (self.ox + x * self.scale, self.oy + y * self.scale, self.ox + (x + rw) * self.scale, self.oy + (y + rh) * self.scale);
        let r = r * self.scale;
        for py in (y0.floor().max(0.0) as usize)..(y1.ceil().max(0.0) as usize).min(self.h) {
            for px in (x0.floor().max(0.0) as usize)..(x1.ceil().max(0.0) as usize).min(self.w) {
                let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
                // distance outside the rounded shape (negative inside)
                let qx = (cx - (x0 + x1) / 2.0).abs() - ((x1 - x0) / 2.0 - r);
                let qy = (cy - (y0 + y1) / 2.0).abs() - ((y1 - y0) / 2.0 - r);
                let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
                let a = (0.5 - d).clamp(0.0, 1.0);
                if a > 0.0 {
                    let i = py * self.w + px;
                    self.buf[i] = mix(self.buf[i], color, (a * 255.0) as u32);
                }
            }
        }
    }

    /// The emblem with its top-left at page position (x, y), `size` page pixels square.
    fn emblem(&mut self, x: f32, y: f32, size: f32) {
        let (x0, y0, s) = (self.ox + x * self.scale, self.oy + y * self.scale, size * self.scale);
        for py in (y0.floor().max(0.0) as usize)..((y0 + s).ceil().max(0.0) as usize).min(self.h) {
            for px in (x0.floor().max(0.0) as usize)..((x0 + s).ceil().max(0.0) as usize).min(self.w) {
                let (mut rgb, mut n) = ([0u32; 3], 0u32);
                for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    if let Some(c) = emblem_pixel(((px as f32 + sx) - x0) / s, ((py as f32 + sy) - y0) / s) {
                        n += 1;
                        for (k, a) in rgb.iter_mut().enumerate() {
                            *a += c >> (16 - 8 * k) & 255;
                        }
                    }
                }
                if n > 0 {
                    let i = py * self.w + px;
                    self.buf[i] = mix(self.buf[i], (rgb[0] / n) << 16 | (rgb[1] / n) << 8 | rgb[2] / n, n * 255 / 4);
                }
            }
        }
    }

    /// A language's flag with its top-left at page position (x, y), `FLAG_W` x `FLAG_H` page pixels, with a thin dark outline.
    fn flag(&mut self, x: f32, y: f32, l: Language) {
        self.rrect(x - 1.0, y - 1.0, FLAG_W + 2.0, FLAG_H + 2.0, 3.0, 0x0e1014);
        let (x0, y0, x1, y1) = (self.ox + x * self.scale, self.oy + y * self.scale, self.ox + (x + FLAG_W) * self.scale, self.oy + (y + FLAG_H) * self.scale);
        for py in (y0.round().max(0.0) as usize)..(y1.round().max(0.0) as usize).min(self.h) {
            for px in (x0.round().max(0.0) as usize)..(x1.round().max(0.0) as usize).min(self.w) {
                // 2x2 samples smooth the diagonals and the stars
                let mut acc = [0u32; 3];
                for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    let c = flag_color(l, ((px as f32 + sx) - x0) / (x1 - x0), ((py as f32 + sy) - y0) / (y1 - y0));
                    for (k, a) in acc.iter_mut().enumerate() {
                        *a += c >> (16 - 8 * k) & 255;
                    }
                }
                self.buf[py * self.w + px] = (acc[0] / 4) << 16 | (acc[1] / 4) << 8 | acc[2] / 4;
            }
        }
    }

    /// Text with its top-left at page position (x, y); `size` is the page-space pixel height.
    fn text(&mut self, font: Option<&FontVec>, text: &str, x: f32, y: f32, size: f32, color: u32) {
        let Some(font) = font else { return };
        let sc = PxScale::from(size * self.scale);
        let s = font.as_scaled(sc);
        let mut pen = self.ox + x * self.scale;
        let base = self.oy + y * self.scale + s.ascent();
        for ch in text.chars() {
            let id = s.glyph_id(ch);
            let g = id.with_scale_and_position(sc, ab_glyph::point(pen, base));
            pen += s.h_advance(id);
            if let Some(o) = font.outline_glyph(g) {
                let b = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && (px as usize) < self.w && (py as usize) < self.h {
                        let i = py as usize * self.w + px as usize;
                        self.buf[i] = mix(self.buf[i], color, (cov.clamp(0.0, 1.0) * 255.0) as u32);
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new(Settings::default())
    }

    #[test]
    fn language_buttons_select_and_play_needs_a_valid_folder() {
        let a = app();
        let b = a.buttons();
        assert_eq!(b.iter().filter(|b| matches!(b.action, Action::Lang(_))).count(), 9);
        assert!(b.iter().find(|b| b.action == Action::Lang(None)).unwrap().selected);
        assert_eq!(b.iter().filter(|b| b.action == Action::Play).count(), 1);
        let play = b.iter().find(|b| b.action == Action::Play).unwrap();
        assert!(!play.enabled);
        let c = play.rect;
        assert_eq!(a.click(c.x + 1.0, c.y + 1.0), None);
        let cz = b.iter().find(|b| b.action == Action::Lang(Some(Language::Czech))).unwrap().rect;
        assert_eq!(a.click(cz.x + 5.0, cz.y + 5.0), Some(Action::Lang(Some(Language::Czech))));
    }

    #[test]
    fn dropped_folders_are_checked() {
        let mut a = app();
        let dir = std::env::temp_dir().join(format!("opensow-launcher-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        a.offer_dir(dir.clone(), Some(Slot::Original));
        assert_eq!(status(Slot::Original, a.path(Slot::Original).map(|p| p.as_path())), Status::Found); // `data` found case-insensitively
        a.offer_dir(std::env::temp_dir().join("opensow-no-such-folder"), Some(Slot::Warmonger));
        assert_eq!(status(Slot::Warmonger, a.path(Slot::Warmonger).map(|p| p.as_path())), Status::Invalid);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn every_window_size_keeps_the_page_inside() {
        for (w, h) in [(640, 480), (800, 600), (1920, 1080), (300, 900)] {
            let (s, ox, oy) = App::fit(w, h);
            assert!(ox >= 0.0 && oy >= 0.0 && ox + PAGE_W * s <= w as f32 + 0.01 && oy + PAGE_H * s <= h as f32 + 0.01);
        }
    }
}
