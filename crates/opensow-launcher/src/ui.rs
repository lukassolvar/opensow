//! Launcher state, layout and drawing. Pure CPU, no window: `main.rs` feeds it events and the `--screenshot` mode renders it headless.
//! The page is laid out in logical units, 600 high and as wide as the window's shape allows (at least 800), and scaled to fit:
//! a sidebar (home, the two games, settings) and the chosen page on the right (docs/launcher.md).

use crate::settings::{Field, GameSettings, NATIVE};
use crate::text::{Message, Tx};
use crate::update::{Fail, Release};
use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont};
use opensow_formats::lang::Language;
use opensow_formats::launcher::Settings;
use opensow_formats::{GameDir, DEFAULT_MAC_STEAM_DIR};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// Logical page height; the width follows the window (`App::resize`).
pub const PAGE_H: f32 = 600.0;
/// The narrowest logical page; a narrower window shows the page smaller with bands above and below.
pub const MIN_W: f32 = 800.0;
/// Width of the sidebar.
pub const SIDE: f32 = 212.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Original,
    Warmonger,
}

impl Slot {
    pub fn name(self) -> &'static str {
        match self {
            Slot::Original => "State of War",
            Slot::Warmonger => "Warmonger",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Game(Slot),
    Settings,
}

/// The settings page's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Launcher,
    Game,
    Extras,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nav(Page),
    Tab(Tab),
    Lang(Option<Language>),
    Browse(Slot),
    Detect,
    /// Start the game (home page only) on the campaign used last.
    Play,
    Toggle(Field),
    Step(Field, i32),
    Reset,
    /// Ask GitHub for the latest release now.
    CheckUpdate,
    /// Download and install the release found.
    InstallUpdate,
    /// Open the release's page in the browser.
    ReleaseNotes,
    /// Start the new launcher and close this one.
    Restart,
    /// The "check when the launcher starts" switch.
    ToggleAutoCheck,
}

/// Where the update check stands (Settings, Launcher tab).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    /// Not checked in this session.
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    /// Downloading: the release and the bytes so far.
    Downloading(Release, u64),
    Installed(Release),
    Failed(Fail),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    #[cfg(test)]
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

/// How a button looks.
#[derive(Debug, Clone, PartialEq)]
pub enum Style {
    /// Big filled button in a colour.
    Primary(u32),
    /// Dark see-through button.
    Secondary,
    /// A sidebar entry.
    Nav,
    /// A settings tab.
    Tab,
    /// A language with its flag.
    Chip,
    /// An on/off switch.
    Switch(bool),
    /// A stepper arrow (left = true).
    Arrow(bool),
    /// A game row on the home page.
    Row,
}

pub struct Button {
    pub rect: Rect,
    pub action: Action,
    pub label: String,
    pub style: Style,
    pub selected: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// No folder chosen.
    Unset,
    Found,
    /// A folder is chosen but the game is not (or no longer) in it.
    Missing,
}

/// Is the game in `path`? Checked again when the window gets the focus and before every start, so files deleted between
/// launches show at once.
pub fn status(slot: Slot, path: Option<&Path>) -> Status {
    let Some(p) = path else { return Status::Unset };
    let ok = match slot {
        Slot::Original => GameDir::new(p.to_path_buf()).resolve("Data").is_some_and(|d| d.is_dir()),
        Slot::Warmonger => GameDir::is_warmonger_dir(p),
    };
    if ok {
        Status::Found
    } else {
        Status::Missing
    }
}

/// A decoded background picture.
pub struct Picture {
    w: usize,
    h: usize,
    px: Vec<u32>,
}

impl Picture {
    fn decode(bytes: &[u8]) -> Option<Picture> {
        let mut d = jpeg_decoder::Decoder::new(bytes);
        let data = d.decode().ok()?;
        let info = d.info()?;
        let (w, h) = (info.width as usize, info.height as usize);
        let px = match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => data.chunks_exact(3).map(|c| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32).collect(),
            jpeg_decoder::PixelFormat::L8 => data.iter().map(|&v| (v as u32) * 0x010101).collect(),
            _ => return None,
        };
        Some(Picture { w, h, px })
    }

    /// Bilinear sample at picture pixel (x, y), clamped to the edges.
    fn sample(&self, x: f32, y: f32) -> u32 {
        let x = x.clamp(0.0, (self.w - 1) as f32);
        let y = y.clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (((x - x0 as f32) * 255.0) as u32, ((y - y0 as f32) * 255.0) as u32);
        let top = mix(self.px[y0 * self.w + x0], self.px[y0 * self.w + x1], fx);
        let bot = mix(self.px[y1 * self.w + x0], self.px[y1 * self.w + x1], fx);
        mix(top, bot, fy)
    }
}

/// Screenshots of the game (staged headless battles, docs/launcher.md "Pictures"): the home page, State of War, Warmonger.
const ART: [&[u8]; 3] = [include_bytes!("../art/home.jpg"), include_bytes!("../art/original.jpg"), include_bytes!("../art/warmonger.jpg")];

/// The painted sidebar and picture for one page and window size (`App::draw` reuses it until either changes).
struct Backdrop {
    scene: Scene,
    w: usize,
    h: usize,
    px: Vec<u32>,
}

/// What fills the page's right side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scene {
    Home,
    Original,
    Warmonger,
    Settings,
}

/// Where the logical page sits in the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub scale: f32,
    pub ox: f32,
    pub oy: f32,
    /// Logical page width.
    pub pw: f32,
}

impl View {
    pub fn fit(w: usize, h: usize) -> View {
        let (w, h) = (w.max(1) as f32, h.max(1) as f32);
        let pw = (PAGE_H * w / h).max(MIN_W);
        let scale = (w / pw).min(h / PAGE_H);
        View { scale, ox: (w - pw * scale) / 2.0, oy: (h - PAGE_H * scale) / 2.0, pw }
    }
}

pub struct App {
    pub settings: Settings,
    pub game: GameSettings,
    /// Where the game settings live and the game is started (the per-user data folder).
    pub data_dir: PathBuf,
    pub page: Page,
    pub tab: Tab,
    pub message: Message,
    pub font: Option<FontVec>,
    bold: Option<FontVec>,
    pub cursor: (f32, f32),
    pub update: UpdateState,
    status: [Status; 2],
    view: View,
    art: Vec<Option<Picture>>,
    cache: RefCell<Option<Backdrop>>,
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

/// Headings and game titles; a text the bold font cannot draw falls back to the regular one.
const BOLD_FONTS: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/Library/Fonts/Arial Bold.ttf",
    "C:\\Windows\\Fonts\\segoeuib.ttf",
    "C:\\Windows\\Fonts\\arialbd.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
];

const NATIVE_NAMES: [&str; 8] = ["English", "Slovenčina", "Čeština", "Polski", "Magyar", "Русский", "简体中文", "繁體中文"];

const FLAG_W: f32 = 24.0;
const FLAG_H: f32 = 16.0;

const SIDEBAR: u32 = 0x0b0d11;
const SIDE_ACTIVE: u32 = 0x1c212b;
const SIDE_HOVER: u32 = 0x151920;
const BTN: u32 = 0x232a36;
const BTN_HOVER: u32 = 0x34405a;
const ACCENT: u32 = 0xe0a83e;
const GREEN: u32 = 0x4fcf7f;
const BLUE: u32 = 0x4ea8ff;
const RED: u32 = 0xe8473a;
const TEXT: u32 = 0xeef0f4;
const DIM: u32 = 0x9aa1ae;
const DISABLED: u32 = 0x3a3f48;
const SHADOW: u32 = 0x06070a;

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

/// The launcher's icon: a tank on a transparent background, 256x256 RGBA (`art/tank.png`).
const ICON_PNG: &[u8] = include_bytes!("../art/tank.png");

/// The icon decoded once: (side, premultiplied RGBA as f32).
fn icon() -> &'static (usize, Vec<[f32; 4]>) {
    static ICON: std::sync::OnceLock<(usize, Vec<[f32; 4]>)> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        let decoded = png::Decoder::new(ICON_PNG).read_info().ok().and_then(|mut r| {
            let mut buf = vec![0; r.output_buffer_size()];
            let info = r.next_frame(&mut buf).ok()?;
            (info.color_type == png::ColorType::Rgba && info.width == info.height).then_some((info.width as usize, buf))
        });
        let Some((n, buf)) = decoded else { return (1, vec![[0.0; 4]]) };
        let px = buf
            .chunks_exact(4)
            .map(|c| {
                let a = c[3] as f32 / 255.0;
                [c[0] as f32 * a, c[1] as f32 * a, c[2] as f32 * a, a]
            })
            .collect();
        (n, px)
    })
}

/// The icon's average over the box `(u0, v0)`..`(u1, v1)` (0..1): (0xRRGGBB, coverage 0..=1).
fn icon_area(u0: f32, v0: f32, u1: f32, v1: f32) -> (u32, f32) {
    let (n, px) = icon();
    let nf = *n as f32;
    let (x0, x1) = ((u0 * nf).floor().max(0.0) as usize, ((u1 * nf).ceil() as usize).clamp(1, *n));
    let (y0, y1) = ((v0 * nf).floor().max(0.0) as usize, ((v1 * nf).ceil() as usize).clamp(1, *n));
    let mut acc = [0.0f32; 4];
    let mut count = 0.0;
    for y in y0..y1.max(y0 + 1).min(*n) {
        for x in x0..x1.max(x0 + 1).min(*n) {
            for (a, v) in acc.iter_mut().zip(px[y * n + x]) {
                *a += v;
            }
            count += 1.0;
        }
    }
    if count == 0.0 || acc[3] <= 0.0 {
        return (0, 0.0);
    }
    let ch = |i: usize| (acc[i] / acc[3]).round().clamp(0.0, 255.0) as u32;
    (ch(0) << 16 | ch(1) << 8 | ch(2), acc[3] / count)
}

/// RGBA bytes of the icon at `size` x `size`, for the window icon.
pub fn icon_rgba(size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size * size * 4);
    let s = size as f32;
    for py in 0..size {
        for px in 0..size {
            let (c, a) = icon_area(px as f32 / s, py as f32 / s, (px + 1) as f32 / s, (py + 1) as f32 / s);
            out.extend([(c >> 16) as u8, (c >> 8) as u8, c as u8, (a * 255.0).round() as u8]);
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
            if u < 0.5 && (v - 0.5).abs() < 0.5 - u {
                BLUE
            } else if v < 0.5 {
                WHITE
            } else {
                RED
            }
        }
        Language::Polish => {
            if v < 0.5 {
                WHITE
            } else {
                RED
            }
        }
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
                if r >= 0.09 && (r < 0.1 || r < rays) {
                    WHITE
                } else {
                    0x1c3f94
                }
            } else {
                0xde2910
            }
        }
    }
}

/// The accent of a page: gold at home and in the settings, blue for State of War, red for Warmonger.
fn accent(page: Page) -> u32 {
    match page {
        Page::Game(Slot::Original) => BLUE,
        Page::Game(Slot::Warmonger) => RED,
        _ => ACCENT,
    }
}

impl App {
    pub fn new(settings: Settings, game: GameSettings, data_dir: PathBuf) -> App {
        let font = FONTS.iter().find_map(|p| std::fs::read(p).ok().and_then(|d| FontVec::try_from_vec(d).ok()));
        let bold = BOLD_FONTS.iter().find_map(|p| std::fs::read(p).ok().and_then(|d| FontVec::try_from_vec(d).ok()));
        // a saved language the port no longer offers (Polish, Hungarian, Russian) reads as "From game files"
        let mut settings = settings;
        settings.language = settings.language.filter(|l| l.supported());
        let art = ART.iter().map(|b| Picture::decode(b)).collect();
        let mut app = App {
            settings,
            game,
            data_dir,
            page: Page::Home,
            tab: Tab::Game,
            message: Message::None,
            font,
            bold,
            cursor: (-1.0, -1.0),
            update: UpdateState::Idle,
            status: [Status::Unset; 2],
            view: View::fit(1000, 620),
            art,
            cache: RefCell::new(None),
        };
        app.refresh();
        app
    }

    /// Looks at both folders again (files may have been deleted or put back since).
    pub fn refresh(&mut self) {
        for slot in [Slot::Original, Slot::Warmonger] {
            self.status[slot.index()] = status(slot, self.path(slot).map(|p| p.as_path()));
        }
    }

    pub fn status(&self, slot: Slot) -> Status {
        self.status[slot.index()]
    }

    /// Can this campaign start? Warmonger also needs the base game (it uses its art and sound, docs/warmonger.md).
    pub fn playable(&self, slot: Slot) -> bool {
        self.status(Slot::Original) == Status::Found && self.status(slot) == Status::Found
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
        self.refresh();
    }

    /// The window was resized to `w` x `h` pixels.
    pub fn resize(&mut self, w: usize, h: usize) {
        self.view = View::fit(w, h);
    }

    #[cfg(test)]
    pub fn view(&self) -> View {
        self.view
    }

    /// Window pixel -> logical page point.
    pub fn to_logical(&self, px: f32, py: f32) -> (f32, f32) {
        ((px - self.view.ox) / self.view.scale, (py - self.view.oy) / self.view.scale)
    }

    /// The language the page speaks: the chosen one, or with "From game files" the State of War folder's `Data\lang.dat` (a
    /// language the port does not offer, or none, is English); English too when the font lacks the script.
    pub fn tx(&self) -> Tx {
        let l = self.settings.language.or_else(|| {
            let dir = self.settings.original.as_ref()?;
            let p = GameDir::new(dir.clone()).resolve("Data\\lang.dat")?;
            opensow_formats::lang::read_lang_dat(&std::fs::read(p).ok()?)
        });
        let tx = l.filter(|l| l.supported()).map_or(Tx::english(), Tx::new);
        let drawable = |s: &str| self.font.as_ref().is_none_or(|f| s.chars().all(|c| c.is_whitespace() || f.glyph_id(c).0 != 0));
        if tx.all().iter().all(|s| drawable(s)) && tx.words().iter().all(|s| drawable(s)) {
            tx
        } else {
            Tx::english()
        }
    }

    fn lang_label(&self, l: Option<Language>) -> String {
        let Some(l) = l else { return self.tx().game_files_label().into() };
        let n = NATIVE_NAMES[l.id() as usize];
        // a font without the script falls back to the code
        match &self.font {
            Some(f) if n.chars().all(|c| f.glyph_id(c).0 != 0) => n.into(),
            _ => format!(
                "{} ({})",
                ["English", "Slovak", "Czech", "Polish", "Hungarian", "Russian", "Chinese (simpl.)", "Chinese (trad.)"][l.id() as usize],
                l.code()
            ),
        }
    }

    // ---- layout ----

    fn nav_rect(i: usize) -> Rect {
        let y = if i == 3 { PAGE_H - 70.0 } else { 86.0 + 62.0 * i as f32 };
        Rect { x: 10.0, y, w: SIDE - 20.0, h: 54.0 }
    }

    /// The play button's place (bottom right of the page).
    fn play_rect(&self) -> Rect {
        Rect { x: self.view.pw - 36.0 - 220.0, y: PAGE_H - 36.0 - 60.0, w: 220.0, h: 60.0 }
    }

    /// Home page: the "Your games" card and its rows.
    fn games_card(&self) -> Rect {
        Rect { x: SIDE + 40.0, y: 292.0, w: (self.view.pw - SIDE - 80.0).min(430.0), h: 158.0 }
    }

    fn game_row(&self, slot: Slot) -> Rect {
        let c = self.games_card();
        Rect { x: c.x + 8.0, y: c.y + 42.0 + 56.0 * slot.index() as f32, w: c.w - 16.0, h: 52.0 }
    }

    /// Settings page: the two columns' left edges and width.
    fn columns(&self) -> ([f32; 2], f32) {
        let w = (self.view.pw - SIDE - 64.0 - 40.0) / 2.0;
        ([SIDE + 32.0, SIDE + 32.0 + w + 40.0], w)
    }

    /// The Game tab's rows: (column, row, field).
    fn game_rows() -> Vec<(usize, usize, Field)> {
        let left = [Field::Resolution, Field::Clouds, Field::Radar, Field::Quake, Field::Vsync, Field::HwCursor, Field::KeysScroll, Field::MouseScroll];
        let right = [Field::Music, Field::Speech, Field::Effects];
        left.iter().enumerate().map(|(i, &f)| (0, i, f)).chain(right.iter().enumerate().map(|(i, &f)| (1, i, f))).collect()
    }

    const ROW_Y: f32 = 158.0;
    const ROW_H: f32 = 44.0;
    const EXTRA_H: f32 = 74.0;

    /// The volume that goes with a sound switch.
    fn volume_of(f: Field) -> Option<Field> {
        match f {
            Field::Music => Some(Field::MusicVolume),
            Field::Speech => Some(Field::SpeechVolume),
            Field::Effects => Some(Field::EffectsVolume),
            _ => None,
        }
    }

    /// Width of a stepper's value box.
    fn value_w(f: Field) -> f32 {
        if f == Field::Resolution {
            96.0
        } else {
            44.0
        }
    }

    /// Every button of the current page and the sidebar, in logical coordinates.
    pub fn buttons(&self) -> Vec<Button> {
        let tx = self.tx();
        let mut out = vec![];
        let mut push = |rect: Rect, action: Action, label: &str, style: Style, selected: bool, enabled: bool| {
            out.push(Button { rect, action, label: label.into(), style, selected, enabled });
        };
        // sidebar
        let nav = [
            (Page::Home, tx.home()),
            (Page::Game(Slot::Original), Slot::Original.name()),
            (Page::Game(Slot::Warmonger), Slot::Warmonger.name()),
            (Page::Settings, tx.settings()),
        ];
        for (i, (p, label)) in nav.into_iter().enumerate() {
            push(Self::nav_rect(i), Action::Nav(p), label, Style::Nav, self.page == p, true);
        }
        let pw = self.view.pw;
        match self.page {
            Page::Home => {
                if self.status(Slot::Original) != Status::Found || self.status(Slot::Warmonger) != Status::Found {
                    let w = self.button_w(tx.find_games(), 15.0, 150.0);
                    push(Rect { x: pw - 28.0 - w, y: 22.0, w, h: 38.0 }, Action::Detect, tx.find_games(), Style::Secondary, false, true);
                }
                for slot in [Slot::Original, Slot::Warmonger] {
                    push(self.game_row(slot), Action::Nav(Page::Game(slot)), slot.name(), Style::Row, false, true);
                }
                let ok = self.playable(Slot::Original);
                push(self.play_rect(), Action::Play, tx.play(), Style::Primary(ACCENT), false, ok);
            }
            Page::Game(slot) => {
                let found = self.status(slot) == Status::Found;
                let locate = if found { tx.change_folder() } else { tx.locate() };
                let lw = self.button_w(locate, 15.0, 130.0);
                let dw = self.button_w(tx.auto_detect(), 15.0, 110.0);
                push(Rect { x: pw - 28.0 - lw, y: 22.0, w: lw, h: 38.0 }, Action::Browse(slot), locate, Style::Secondary, false, true);
                push(Rect { x: pw - 28.0 - lw - 10.0 - dw, y: 22.0, w: dw, h: 38.0 }, Action::Detect, tx.auto_detect(), Style::Secondary, false, true);
                // Play lives on the home page only; a game's page offers the big Locate until its files are found
                if !found {
                    push(self.play_rect(), Action::Browse(slot), tx.locate(), Style::Primary(accent(self.page)), false, true);
                }
            }
            Page::Settings => {
                let mut x = SIDE + 32.0;
                for (t, label) in [(Tab::Launcher, tx.launcher()), (Tab::Game, tx.tab_game()), (Tab::Extras, tx.tab_extras())] {
                    let w = self.font.as_ref().map_or(100.0, |f| text_width(f, label, 16.0)) + 24.0;
                    push(Rect { x, y: 74.0, w, h: 34.0 }, Action::Tab(t), label, Style::Tab, self.tab == t, true);
                    x += w + 8.0;
                }
                let ([c0, c1], cw) = self.columns();
                match self.tab {
                    Tab::Launcher => {
                        let langs: Vec<Option<Language>> = std::iter::once(None).chain(Language::SUPPORTED.into_iter().map(Some)).collect();
                        let w = ((pw - SIDE - 64.0 - 24.0) / 3.0).min(210.0);
                        for (i, l) in langs.into_iter().enumerate() {
                            let (col, row) = ((i % 3) as f32, (i / 3) as f32);
                            let r = Rect { x: c0 + col * (w + 12.0), y: 160.0 + row * 50.0, w, h: 40.0 };
                            push(r, Action::Lang(l), &self.lang_label(l), Style::Chip, self.settings.language == l, true);
                        }
                        self.update_buttons(&mut push, c0);
                    }
                    Tab::Game => {
                        for (col, row, f) in Self::game_rows() {
                            let x1 = [c0, c1][col] + cw;
                            let y = Self::ROW_Y + row as f32 * Self::ROW_H;
                            let stepped = if let Some(v) = Self::volume_of(f) { Some(v) } else { self.game.switch(f).is_none().then_some(f) };
                            let mut right = x1;
                            if let Some(s) = stepped {
                                let vw = Self::value_w(s);
                                push(
                                    Rect { x: right - 28.0, y: y + 4.0, w: 28.0, h: 28.0 },
                                    Action::Step(s, 1),
                                    "",
                                    Style::Arrow(false),
                                    false,
                                    self.game.can_step(s, 1),
                                );
                                right -= 28.0 + vw;
                                push(
                                    Rect { x: right - 28.0, y: y + 4.0, w: 28.0, h: 28.0 },
                                    Action::Step(s, -1),
                                    "",
                                    Style::Arrow(true),
                                    false,
                                    self.game.can_step(s, -1),
                                );
                                right -= 28.0 + 14.0;
                            }
                            if let Some(on) = self.game.switch(f) {
                                push(Rect { x: right - 46.0, y: y + 5.0, w: 46.0, h: 26.0 }, Action::Toggle(f), "", Style::Switch(on), false, true);
                            }
                        }
                        let w = self.button_w(tx.reset(), 15.0, 150.0);
                        push(Rect { x: c1, y: Self::ROW_Y + 3.0 * Self::ROW_H + 30.0, w, h: 38.0 }, Action::Reset, tx.reset(), Style::Secondary, false, true);
                    }
                    Tab::Extras => {
                        for k in 0..crate::settings::EXTRAS.len() {
                            let (col, row) = (k / 4, k % 4);
                            let y = 158.0 + row as f32 * Self::EXTRA_H;
                            let on = self.game.extras[k];
                            push(
                                Rect { x: [c0, c1][col] + cw - 46.0, y: y + 2.0, w: 46.0, h: 26.0 },
                                Action::Toggle(Field::Extra(k)),
                                "",
                                Style::Switch(on),
                                false,
                                true,
                            );
                        }
                    }
                }
            }
        }
        out
    }

    /// Settings, Launcher tab: the update section's row of buttons and its switch.
    const UPDATE_Y: f32 = 380.0;

    fn update_buttons(&self, push: &mut impl FnMut(Rect, Action, &str, Style, bool, bool), x0: f32) {
        let tx = self.tx();
        let y = Self::UPDATE_Y;
        push(
            Rect { x: self.view.pw - 32.0 - 46.0, y: y - 2.0, w: 46.0, h: 26.0 },
            Action::ToggleAutoCheck,
            "",
            Style::Switch(self.settings.check_updates),
            false,
            true,
        );
        let busy = matches!(self.update, UpdateState::Checking | UpdateState::Downloading(..));
        let mut x = x0;
        let row = y + 82.0;
        let main = match &self.update {
            UpdateState::Available(r) if r.archive.is_some() && r.checksum.is_some() => Some((Action::InstallUpdate, tx.install(&r.version_text()))),
            UpdateState::Installed(_) => Some((Action::Restart, tx.restart().to_string())),
            _ => None,
        };
        if let Some((action, label)) = main {
            // the primary label is bold
            let w = self.width(&label, 15.5, true).max(140.0) + 44.0;
            push(Rect { x, y: row, w, h: 40.0 }, action, &label, Style::Primary(ACCENT), false, true);
            x += w + 10.0;
        } else {
            let w = self.button_w(tx.check_now(), 15.0, 150.0);
            push(Rect { x, y: row, w, h: 40.0 }, Action::CheckUpdate, tx.check_now(), Style::Secondary, false, !busy);
            x += w + 10.0;
        }
        if matches!(self.update, UpdateState::Available(_) | UpdateState::Downloading(..) | UpdateState::Installed(_)) {
            let w = self.button_w(tx.release_notes(), 15.0, 120.0);
            push(Rect { x, y: row, w, h: 40.0 }, Action::ReleaseNotes, tx.release_notes(), Style::Secondary, false, true);
        }
    }

    /// The update section's texts and progress bar.
    fn draw_updates(&self, c: &mut Canvas, tx: Tx, x0: f32) {
        let y = Self::UPDATE_Y;
        let right = self.view.pw - 32.0;
        self.text(c, tx.updates(), x0, y, 15.0, DIM, true);
        let label = self.fit(tx.check_at_start(), 13.5, right - 56.0 - x0 - 140.0);
        let lw = self.width(&label, 13.5, false);
        self.text(c, &label, right - 46.0 - 12.0 - lw, y + 3.0, 13.5, TEXT, false);
        let (line, col) = match &self.update {
            UpdateState::Idle => (String::new(), DIM),
            UpdateState::Checking => (tx.checking().into(), TEXT),
            UpdateState::UpToDate => (tx.up_to_date().into(), GREEN),
            UpdateState::Available(r) if r.archive.is_none() => (tx.update_fail(&Fail::NoArchive(crate::update::platform())), ACCENT),
            UpdateState::Available(r) if r.checksum.is_none() => (tx.update_fail(&Fail::NoChecksum), ACCENT),
            UpdateState::Available(r) => (tx.available(&r.version_text(), &r.name), ACCENT),
            UpdateState::Downloading(r, got) => {
                let size = r.archive.as_ref().map_or(0, |a| a.size);
                let pct = (size > 0).then(|| ((*got * 100) / size).min(100) as u32);
                (tx.downloading(&r.version_text(), pct), TEXT)
            }
            UpdateState::Installed(r) => (tx.installed(&r.version_text()), GREEN),
            UpdateState::Failed(f) => (tx.update_fail(f), RED),
        };
        let version = tx.version(crate::update::VERSION);
        self.text(c, &version, x0, y + 30.0, 14.0, TEXT, false);
        let room = right - x0;
        for (i, l) in self.wrap(&line, 13.5, room).iter().take(2).enumerate() {
            self.text(c, l, x0, y + 50.0 + i as f32 * 18.0, 13.5, col, false);
        }
        if let UpdateState::Downloading(r, got) = &self.update {
            let size = r.archive.as_ref().map_or(0, |a| a.size).max(1);
            let w = (room).min(360.0);
            c.rrect(x0, y + 136.0, w, 6.0, 3.0, 0x2a303b);
            c.rrect(x0, y + 136.0, w * (*got as f32 / size as f32).min(1.0), 6.0, 3.0, ACCENT);
        }
    }

    /// The sidebar's line under Settings: a new version waits.
    fn update_badge(&self, tx: Tx) -> Option<(String, u32)> {
        match &self.update {
            UpdateState::Available(r) | UpdateState::Downloading(r, _) => Some((tx.update_badge(&r.version_text()), ACCENT)),
            UpdateState::Installed(_) => Some((tx.restart().into(), GREEN)),
            _ => None,
        }
    }

    /// A button wide enough for its label, at least `min`.
    fn button_w(&self, label: &str, size: f32, min: f32) -> f32 {
        (self.font.as_ref().map_or(min, |f| text_width(f, label, size)) + 32.0).max(min)
    }

    pub fn click(&self, lx: f32, ly: f32) -> Option<Action> {
        self.buttons().into_iter().find(|b| b.enabled && b.rect.contains(lx, ly)).map(|b| b.action)
    }

    /// Where a dropped folder goes: the game whose page is shown.
    pub fn drop_slot(&self) -> Option<Slot> {
        match self.page {
            Page::Game(s) => Some(s),
            _ => None,
        }
    }

    /// Try to use the dropped / chosen folder: a Warmonger install goes to its slot whatever page it landed on.
    pub fn offer_dir(&mut self, dir: PathBuf, hint: Option<Slot>) {
        let slot = if GameDir::is_warmonger_dir(&dir) { Slot::Warmonger } else { hint.unwrap_or(Slot::Original) };
        let st = status(slot, Some(&dir));
        self.set_path(slot, Some(dir.clone()));
        self.message = match st {
            Status::Found => Message::Set(slot.name().into()),
            _ => Message::NotTheFolder(dir.display().to_string(), slot.name().into()),
        };
    }

    /// Fill the empty or broken slots from the places the game is normally installed.
    pub fn detect(&mut self) {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut base: Option<PathBuf> = std::env::var_os(opensow_formats::GAME_DIR_ENV).map(PathBuf::from).filter(|p| p.join("Data").is_dir());
        if base.is_none() {
            base = home.map(|h| h.join(DEFAULT_MAC_STEAM_DIR)).filter(|p| p.join("Data").is_dir());
        }
        let mut found = 0;
        if let Some(b) = base {
            let sibling = GameDir::new(b.clone()).find_sibling_warmonger();
            if self.status(Slot::Original) != Status::Found {
                self.settings.original = Some(b);
                found += 1;
            }
            if let Some(w) = sibling.filter(|_| self.status(Slot::Warmonger) != Status::Found) {
                self.settings.warmonger = Some(w);
                found += 1;
            }
        }
        self.refresh();
        self.message = if found == 0 { Message::NothingFound } else { Message::Found(found) };
    }

    // ---- drawing ----

    fn scene(&self) -> Scene {
        match self.page {
            Page::Home => Scene::Home,
            Page::Game(Slot::Original) => Scene::Original,
            Page::Game(Slot::Warmonger) => Scene::Warmonger,
            Page::Settings => Scene::Settings,
        }
    }

    /// Draw the page into `buf` (w x h, 0x00RRGGBB). Call [`App::resize`] with the same size first.
    pub fn draw(&self, buf: &mut [u32], w: usize, h: usize) {
        let v = self.view;
        let scene = self.scene();
        {
            let mut cache = self.cache.borrow_mut();
            let fresh = matches!(&*cache, Some(b) if b.scene == scene && b.w == w && b.h == h);
            if !fresh {
                let mut px = vec![0u32; w * h];
                self.paint_backdrop(&mut px, w, h, scene);
                *cache = Some(Backdrop { scene, w, h, px });
            }
            if let Some(b) = &*cache {
                buf[..w * h].copy_from_slice(&b.px[..w * h]);
            }
        }
        let mut c = Canvas { buf, w, h, scale: v.scale, ox: v.ox, oy: v.oy };
        let tx = self.tx();
        // the sidebar's highlights go under its labels, every other button over the page
        let (nav, rest): (Vec<Button>, Vec<Button>) = self.buttons().into_iter().partition(|b| b.style == Style::Nav);
        for b in &nav {
            self.draw_button(&mut c, b);
        }
        self.draw_sidebar(&mut c, tx);
        match self.page {
            Page::Home => self.draw_home(&mut c, tx),
            Page::Game(slot) => self.draw_game(&mut c, tx, slot),
            Page::Settings => self.draw_settings(&mut c, tx),
        }
        for b in &rest {
            self.draw_button(&mut c, b);
        }
        self.draw_message(&mut c, tx);
    }

    /// The sidebar colour and the page's picture, graded and darkened where text sits; cached per page and window size.
    fn paint_backdrop(&self, buf: &mut [u32], w: usize, h: usize, scene: Scene) {
        let v = self.view;
        let pic = match scene {
            Scene::Home => self.art.first(),
            Scene::Original => self.art.get(1),
            Scene::Warmonger => self.art.get(2),
            Scene::Settings => None,
        }
        .and_then(|p| p.as_ref());
        let cw = v.pw - SIDE;
        // cover the content area; the picture's part that shows is centred
        let (fit, offx, offy) = match pic {
            Some(p) => {
                let s = (cw / p.w as f32).max(PAGE_H / p.h as f32);
                (s, (cw - p.w as f32 * s) / 2.0, (PAGE_H - p.h as f32 * s) / 2.0)
            }
            None => (1.0, 0.0, 0.0),
        };
        for py in 0..h {
            let ly = (py as f32 + 0.5 - v.oy) / v.scale;
            for px in 0..w {
                let lx = (px as f32 + 0.5 - v.ox) / v.scale;
                buf[py * w + px] = if lx < SIDE {
                    if lx >= SIDE - 1.0 {
                        0x1d222b
                    } else {
                        SIDEBAR
                    }
                } else {
                    let x = lx - SIDE;
                    let base = match pic {
                        Some(p) => grade(p.sample((x - offx) / fit, (ly - offy) / fit), scene),
                        None => contour(lx, ly, v.scale),
                    };
                    let t = ly / PAGE_H;
                    let u = x / cw;
                    let (bottom, top, left) = match scene {
                        Scene::Settings => (0.0, 0.0, 0.0),
                        _ => (smooth(0.36, 1.0, t) * 0.92, smooth(0.2, 0.0, t) * 0.55, smooth(0.6, 0.0, u) * 0.5),
                    };
                    let a = 1.0 - (1.0 - bottom) * (1.0 - top) * (1.0 - left);
                    mix(base, SHADOW, (a * 255.0) as u32)
                };
            }
        }
    }

    fn draw_sidebar(&self, c: &mut Canvas, tx: Tx) {
        c.icon(14.0, 18.0, 44.0);
        self.text(c, "OpenSOW", 64.0, 19.0, 21.0, TEXT, true);
        self.text(c, tx.launcher(), 65.0, 44.0, 12.5, DIM, false);
        for (i, page) in [Page::Home, Page::Game(Slot::Original), Page::Game(Slot::Warmonger), Page::Settings].into_iter().enumerate() {
            let r = Self::nav_rect(i);
            let (icx, icy) = (r.x + 26.0, r.y + r.h / 2.0);
            match page {
                Page::Home => c.fill_fn(icx - 12.0, icy - 12.0, 24.0, 24.0, house, if self.page == page { ACCENT } else { DIM }),
                Page::Settings => c.fill_fn(icx - 12.0, icy - 12.0, 24.0, 24.0, gear, if self.page == page { ACCENT } else { DIM }),
                Page::Game(slot) => c.badge(icx, icy, 12.0, self.status(slot)),
            }
            let (label, sub) = match page {
                Page::Home => (tx.home().to_string(), None),
                Page::Settings => (tx.settings().to_string(), self.update_badge(tx)),
                Page::Game(slot) => (slot.name().to_string(), Some(self.state_line(tx, slot))),
            };
            match sub {
                Some((s, col)) => {
                    self.text(c, &label, r.x + 50.0, r.y + 8.0, 15.5, TEXT, true);
                    let s = self.fit(&s, 11.5, r.w - 56.0);
                    self.text(c, &s, r.x + 50.0, r.y + 30.0, 11.5, col, false);
                }
                None => self.text(c, &label, r.x + 50.0, r.y + (r.h - 16.0) / 2.0 - 1.0, 15.5, TEXT, true),
            }
        }
    }

    /// A game's state in words and colour.
    fn state_line(&self, tx: Tx, slot: Slot) -> (String, u32) {
        match self.status(slot) {
            Status::Found if slot == Slot::Warmonger && !self.playable(slot) => (tx.files_found().into(), ACCENT),
            Status::Found => (tx.ready().into(), GREEN),
            Status::Missing => (tx.files_missing().into(), RED),
            Status::Unset if slot == Slot::Warmonger => (format!("{} · {}", tx.not_located(), tx.optional()), DIM),
            Status::Unset => (tx.not_located().into(), DIM),
        }
    }

    fn draw_home(&self, c: &mut Canvas, tx: Tx) {
        let x = SIDE + 40.0;
        c.icon(x - 6.0, 96.0, 90.0);
        self.text(c, "OpenSOW", x + 92.0, 104.0, 50.0, TEXT, true);
        self.text(c, tx.tagline(), x + 95.0, 160.0, 18.0, 0xc9ced8, false);
        let card = self.games_card();
        c.rrect_a(card.x, card.y, card.w, card.h, 12.0, 0x10131a, 200);
        self.text(c, tx.your_games(), card.x + 18.0, card.y + 14.0, 14.0, DIM, true);
        for slot in [Slot::Original, Slot::Warmonger] {
            let r = self.game_row(slot);
            c.badge(r.x + 24.0, r.y + r.h / 2.0, 13.0, self.status(slot));
            let title = if slot == Slot::Warmonger { "State of War: Warmonger" } else { "State of War" };
            self.text(c, title, r.x + 50.0, r.y + 7.0, 16.0, TEXT, true);
            let (s, col) = self.state_line(tx, slot);
            let s = self.fit(&s, 12.5, r.w - 80.0);
            self.text(c, &s, r.x + 50.0, r.y + 29.0, 12.5, col, false);
            c.stroke(&[(r.x + r.w - 22.0, r.y + 19.0), (r.x + r.w - 15.0, r.y + 26.0), (r.x + r.w - 22.0, r.y + 33.0)], 2.0, DIM);
        }
        if !self.playable(Slot::Original) {
            let p = self.play_rect();
            let s = self.fit(tx.locate_first(), 13.5, p.w + 120.0);
            let w = self.width(&s, 13.5, false);
            self.text(c, &s, p.x + p.w - w, p.y - 26.0, 13.5, 0xc9ced8, false);
        }
    }

    fn draw_game(&self, c: &mut Canvas, tx: Tx, slot: Slot) {
        let x = SIDE + 40.0;
        let maxw = self.view.pw - x - 40.0;
        let col = accent(self.page);
        let title = if slot == Slot::Warmonger { "State of War: Warmonger" } else { "State of War" };
        let size = self.bold.as_ref().map_or(48.0, |f| fit_size(f, title, 48.0, maxw));
        self.text(c, title, x, PAGE_H - 262.0 + (48.0 - size) * 0.7, size, col, true);
        self.text(c, tx.subtitle(slot == Slot::Warmonger), x + 2.0, PAGE_H - 200.0, 17.0, 0xd5d9e0, false);
        let st = self.status(slot);
        let (line, lcol) = match st {
            Status::Found => (tx.files_found(), GREEN),
            Status::Missing => (tx.files_missing_long(), RED),
            Status::Unset => (tx.not_located_long(), TEXT),
        };
        c.badge(x + 10.0, PAGE_H - 152.0, 10.0, st);
        let line = self.fit(line, 15.0, maxw - 30.0);
        self.text(c, &line, x + 28.0, PAGE_H - 162.0, 15.0, lcol, false);
        let path = match (st, self.path(slot)) {
            (Status::Unset, _) | (_, None) => tx.folder_hint(slot == Slot::Warmonger).to_string(),
            (_, Some(p)) => p.display().to_string(),
        };
        let path = self.fit_left(&path, 12.5, maxw - 30.0);
        self.text(c, &path, x + 28.0, PAGE_H - 138.0, 12.5, DIM, false);
        // the line under it: what Warmonger needs, or how the two campaigns go together
        let note = match slot {
            Slot::Warmonger if st == Status::Found && !self.playable(slot) => Some((tx.needs_base(), ACCENT)),
            Slot::Warmonger => Some((tx.campaign_note(), DIM)),
            Slot::Original => None,
        };
        if let Some((n, ncol)) = note {
            let room = self.play_rect().x - x - 20.0;
            for (i, l) in self.wrap(n, 12.5, room).iter().take(2).enumerate() {
                self.text(c, l, x + 28.0, PAGE_H - 100.0 + i as f32 * 18.0, 12.5, ncol, false);
            }
        }
    }

    fn draw_settings(&self, c: &mut Canvas, tx: Tx) {
        let x = SIDE + 32.0;
        self.text(c, tx.settings(), x, 24.0, 28.0, TEXT, true);
        c.rrect(x, 108.0, self.view.pw - x - 32.0, 1.0, 0.0, 0x2a303b);
        let ([c0, c1], cw) = self.columns();
        match self.tab {
            Tab::Launcher => {
                self.text(c, tx.language(), c0, 128.0, 15.0, DIM, true);
                self.text(c, tx.stored_in(), c0, 290.0, 15.0, DIM, true);
                let p = self.fit_left(&self.data_dir.display().to_string(), 13.0, self.view.pw - c0 - 40.0);
                self.text(c, &p, c0, 316.0, 13.0, TEXT, false);
                self.draw_updates(c, tx, c0);
            }
            Tab::Game => {
                self.text(c, tx.graphics(), c0, 126.0, 15.0, DIM, true);
                self.text(c, tx.sound(), c1, 126.0, 15.0, DIM, true);
                let buttons = self.buttons();
                for (col, row, f) in Self::game_rows() {
                    let x0 = [c0, c1][col];
                    let y = Self::ROW_Y + row as f32 * Self::ROW_H;
                    c.rrect(x0, y + Self::ROW_H - 4.0, cw, 1.0, 0.0, 0x1f242d);
                    // the label gets what the controls leave
                    let first = buttons
                        .iter()
                        .filter(|b| b.rect.y >= y && b.rect.y < y + Self::ROW_H && b.rect.x >= x0 && b.rect.x < x0 + cw)
                        .map(|b| b.rect.x)
                        .fold(x0 + cw, f32::min);
                    let (label, _) = tx.field(f);
                    let room = first - x0 - 10.0;
                    let size = self.font.as_ref().map_or(14.5, |fo| fit_size(fo, label, 14.5, room).max(11.0));
                    let label = self.fit(label, size, room);
                    self.text(c, &label, x0, y + 18.0 - size / 2.0 - 1.0, size, TEXT, false);
                    // the stepped value between its arrows
                    let stepped = Self::volume_of(f).or_else(|| self.game.switch(f).is_none().then_some(f));
                    if let Some(s) = stepped {
                        let vw = Self::value_w(s);
                        let vx = x0 + cw - 28.0 - vw;
                        let text = match s {
                            Field::Resolution => match self.game.resolution() {
                                NATIVE => tx.native().to_string(),
                                (w, h) => format!("{w}×{h}"),
                            },
                            _ => self.game.number(s).unwrap_or(0).to_string(),
                        };
                        let dim = self.game.switch(f) == Some(false);
                        let tw = self.width(&text, 14.0, false);
                        self.text(c, &text, vx + (vw - tw) / 2.0, y + 10.0, 14.0, if dim { DIM } else { TEXT }, false);
                    }
                }
            }
            Tab::Extras => {
                self.text(c, &self.fit(tx.extras_note(), 13.5, self.view.pw - c0 - 40.0), c0, 126.0, 13.5, DIM, false);
                for k in 0..crate::settings::EXTRAS.len() {
                    let (col, row) = (k / 4, k % 4);
                    let (x0, y) = ([c0, c1][col], 158.0 + row as f32 * Self::EXTRA_H);
                    let (label, hint) = tx.field(Field::Extra(k));
                    let room = cw - 60.0;
                    let size = self.font.as_ref().map_or(15.0, |fo| fit_size(fo, label, 15.0, room).max(11.0));
                    self.text(c, label, x0, y + 4.0, size, TEXT, true);
                    for (i, l) in self.wrap(hint, 12.0, room).iter().take(2).enumerate() {
                        self.text(c, l, x0, y + 27.0 + i as f32 * 16.0, 12.0, DIM, false);
                    }
                }
            }
        }
    }

    fn draw_button(&self, c: &mut Canvas, b: &Button) {
        let r = b.rect;
        let hover = b.enabled && r.contains(self.cursor.0, self.cursor.1);
        match &b.style {
            Style::Nav => {
                if b.selected || hover {
                    c.rrect(r.x, r.y, r.w, r.h, 8.0, if b.selected { SIDE_ACTIVE } else { SIDE_HOVER });
                }
                if b.selected {
                    c.rrect(r.x - 10.0, r.y + 10.0, 4.0, r.h - 20.0, 2.0, accent(self.page));
                }
            }
            Style::Row => {
                if hover {
                    c.rrect_a(r.x, r.y, r.w, r.h, 8.0, 0xffffff, 18);
                }
            }
            Style::Primary(col) => {
                let bg = if !b.enabled {
                    DISABLED
                } else if hover {
                    mix(*col, 0xffffff, 40)
                } else {
                    *col
                };
                c.rrect_a(r.x + 2.0, r.y + 4.0, r.w, r.h, 10.0, 0x000000, 110);
                c.rrect(r.x, r.y, r.w, r.h, 10.0, bg);
                let fg = if b.enabled { 0x101215 } else { 0x7b818c };
                let size = self.font_fit(&b.label, (r.h * 0.37).min(22.0), r.w - 24.0);
                let tw = self.width(&b.label, size, true);
                self.text(c, &b.label, r.x + (r.w - tw) / 2.0, r.y + (r.h - size) / 2.0 - 1.0, size, fg, true);
            }
            Style::Secondary => {
                c.rrect_a(r.x, r.y, r.w, r.h, 8.0, if hover { BTN_HOVER } else { 0x0c0f14 }, if hover { 235 } else { 190 });
                let size = self.font_fit(&b.label, 15.0, r.w - 16.0);
                let tw = self.width(&b.label, size, false);
                self.text(c, &b.label, r.x + (r.w - tw) / 2.0, r.y + (r.h - size) / 2.0 - 1.0, size, if b.enabled { TEXT } else { 0x5b6272 }, false);
            }
            Style::Tab => {
                let col = if b.selected {
                    TEXT
                } else if hover {
                    0xc9ced8
                } else {
                    DIM
                };
                let tw = self.width(&b.label, 16.0, b.selected);
                self.text(c, &b.label, r.x + (r.w - tw) / 2.0, r.y + 6.0, 16.0, col, b.selected);
                if b.selected {
                    c.rrect(r.x + 6.0, r.y + r.h - 3.0, r.w - 12.0, 3.0, 1.5, ACCENT);
                }
            }
            Style::Chip => {
                let bg = if b.selected {
                    ACCENT
                } else if hover {
                    BTN_HOVER
                } else {
                    BTN
                };
                c.rrect(r.x, r.y, r.w, r.h, 8.0, bg);
                let fg = if b.selected { 0x16110a } else { TEXT };
                let flag = matches!(b.action, Action::Lang(Some(_)));
                let room = r.w - if flag { 12.0 + FLAG_W + 8.0 } else { 16.0 };
                let size = self.font_fit(&b.label, 14.5, room);
                let tw = self.width(&b.label, size, false);
                let mut lx = r.x + (r.w - tw) / 2.0;
                if let Action::Lang(Some(l)) = b.action {
                    c.flag(r.x + 12.0, r.y + (r.h - FLAG_H) / 2.0, l);
                    lx = r.x + 12.0 + FLAG_W + (r.w - 12.0 - FLAG_W - tw) / 2.0;
                }
                self.text(c, &b.label, lx, r.y + (r.h - size) / 2.0 - 1.0, size, fg, false);
            }
            Style::Switch(on) => {
                let track = if *on { ACCENT } else { 0x3a414d };
                c.rrect(r.x, r.y, r.w, r.h, r.h / 2.0, if hover { mix(track, 0xffffff, 30) } else { track });
                let k = r.h - 6.0;
                let kx = if *on { r.x + r.w - 3.0 - k } else { r.x + 3.0 };
                c.rrect(kx, r.y + 3.0, k, k, k / 2.0, 0xf6f7f9);
            }
            Style::Arrow(left) => {
                let col = if !b.enabled {
                    0x3d434e
                } else if hover {
                    TEXT
                } else {
                    0xb9bfca
                };
                c.rrect(r.x, r.y, r.w, r.h, 6.0, if hover { BTN_HOVER } else { BTN });
                let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                let d = if *left { 3.0 } else { -3.0 };
                c.stroke(&[(cx + d, cy - 6.0), (cx - d, cy), (cx + d, cy + 6.0)], 2.0, col);
            }
        }
    }

    /// The last action's note, bottom left of the page.
    fn draw_message(&self, c: &mut Canvas, tx: Tx) {
        if self.message == Message::None {
            return;
        }
        let x = SIDE + 24.0;
        let room = self.view.pw - x - 24.0 - if matches!(self.page, Page::Settings) { 0.0 } else { 260.0 };
        // up to two lines; a path too long even for that is cut at its start
        let mut lines = self.wrap(&tx.message(&self.message), 13.0, room - 24.0);
        if lines.len() > 2 || lines.iter().any(|l| self.width(l, 13.0, false) > room - 24.0) {
            lines = vec![self.fit_left(&tx.message(&self.message), 13.0, room - 24.0)];
        }
        let w = lines.iter().map(|l| self.width(l, 13.0, false)).fold(0.0, f32::max);
        let y = PAGE_H - 50.0 - 18.0 * (lines.len() as f32 - 1.0);
        c.rrect_a(x, y, w + 24.0, 32.0 + 18.0 * (lines.len() as f32 - 1.0), 8.0, 0x000000, 190);
        for (i, l) in lines.iter().enumerate() {
            self.text(c, l, x + 12.0, y + 8.0 + 18.0 * i as f32, 13.0, TEXT, false);
        }
    }

    // ---- text helpers ----

    /// The bold font when it has every glyph of `text`, else the regular one.
    fn face(&self, text: &str, bold: bool) -> Option<&FontVec> {
        let b = self.bold.as_ref().filter(|f| bold && text.chars().all(|ch| ch.is_whitespace() || f.glyph_id(ch).0 != 0));
        b.or(self.font.as_ref())
    }

    fn text(&self, c: &mut Canvas, text: &str, x: f32, y: f32, size: f32, color: u32, bold: bool) {
        c.text(self.face(text, bold), text, x, y, size, color);
    }

    fn width(&self, text: &str, size: f32, bold: bool) -> f32 {
        self.face(text, bold).map_or(0.0, |f| text_width(f, text, size))
    }

    /// The largest size up to `size` (at least 10) at which `text` fits `max`.
    fn font_fit(&self, text: &str, size: f32, max: f32) -> f32 {
        self.font.as_ref().map_or(size, |f| fit_size(f, text, size, max).max(10.0))
    }

    /// `text` cut at the end with an ellipsis to fit `max`.
    fn fit(&self, text: &str, size: f32, max: f32) -> String {
        self.font.as_ref().map_or(text.into(), |f| fit_end(f, text, size, max))
    }

    /// `text` cut at the start (paths: the end matters).
    fn fit_left(&self, text: &str, size: f32, max: f32) -> String {
        self.font.as_ref().map_or(text.into(), |f| fit_tail(f, text, size, max))
    }

    fn wrap(&self, text: &str, size: f32, max: f32) -> Vec<String> {
        self.font.as_ref().map_or(vec![text.into()], |f| wrap(f, text, size, max))
    }
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The picture's colour grade per page: home cool and muted, State of War cold blue, Warmonger red with harder contrast.
fn grade(c: u32, scene: Scene) -> u32 {
    let (r, g, b) = ((c >> 16 & 255) as f32, (c >> 8 & 255) as f32, (c & 255) as f32);
    let l = 0.299 * r + 0.587 * g + 0.114 * b;
    let (tint, amount, gain, contrast) = match scene {
        Scene::Home => ([0.82, 0.94, 1.12], 0.4, 0.78, 1.0),
        Scene::Original => ([0.62, 0.92, 1.32], 0.55, 0.8, 1.08),
        Scene::Warmonger => ([1.45, 0.42, 0.32], 0.7, 0.82, 1.18),
        Scene::Settings => ([1.0; 3], 0.0, 1.0, 1.0),
    };
    let ch = |v: f32, t: f32| {
        let x = (v * (1.0 - amount) + l * t * amount) * gain / 255.0;
        (((x - 0.45) * contrast + 0.45) * 255.0).clamp(0.0, 255.0) as u32
    };
    ch(r, tint[0]) << 16 | ch(g, tint[1]) << 8 | ch(b, tint[2])
}

/// The settings page's backdrop (own drawing): a dark gradient with faint contour lines of a made-up height field and a coarse grid,
/// in page coordinates.
fn contour(x: f32, y: f32, scale: f32) -> u32 {
    let grad = mix(0x161b26, 0x0d0f13, ((y / PAGE_H).clamp(0.0, 1.0) * 255.0) as u32);
    let f = (x * 0.011).sin() * 1.6 + (y * 0.017 + 1.3).sin() * 1.3 + ((x + y) * 0.007).sin() * 1.8 + ((x - y) * 0.013 + 0.4).cos();
    let d = ((f * 2.0).fract() - 0.5).abs(); // 0.5 on the line
    let line = ((d - 0.46) / 0.04).clamp(0.0, 1.0) * 0.5;
    let (gx, gy) = ((x / 40.0).fract().abs(), (y / 40.0).fract().abs());
    let near = |t: f32| (t.min(1.0 - t) * 40.0 * scale) < 0.7;
    let g = if near(gx) || near(gy) { 0.35 } else { 0.0 };
    mix(grad, 0x8fa3c8, ((line * 16.0 + g * 10.0) as u32).min(255))
}

/// The house of the Home entry, `(u, v)` in 0..1.
fn house(u: f32, v: f32) -> bool {
    let roof = (0.1..=0.5).contains(&v) && (u - 0.5).abs() <= (v - 0.1) * 1.15;
    let body = u > 0.2 && u < 0.8 && v > 0.45 && v < 0.88;
    let door = u > 0.42 && u < 0.58 && v > 0.62;
    roof || (body && !door)
}

/// The gear of the Settings entry, `(u, v)` in 0..1.
fn gear(u: f32, v: f32) -> bool {
    let (x, y) = (u - 0.5, v - 0.5);
    let r = (x * x + y * y).sqrt();
    let a = y.atan2(x);
    let outer = 0.34 + if (8.0 * a).cos() > 0.25 { 0.13 } else { 0.0 };
    r < outer && r > 0.14
}

pub fn text_width(font: &FontVec, text: &str, size: f32) -> f32 {
    let s = font.as_scaled(PxScale::from(size));
    text.chars().map(|c| s.h_advance(s.glyph_id(c))).sum()
}

/// The largest size up to `size` at which `text` fits `max`.
fn fit_size(font: &FontVec, text: &str, size: f32, max: f32) -> f32 {
    (size * max / text_width(font, text, size).max(1.0)).min(size)
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

/// `text` shortened at the end with an ellipsis so it fits `max` logical pixels.
fn fit_end(font: &FontVec, text: &str, size: f32, max: f32) -> String {
    if text_width(font, text, size) <= max {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (0..chars.len()).rev() {
        let s: String = chars[..keep].iter().copied().chain(std::iter::once('…')).collect();
        if text_width(font, &s, size) <= max {
            return s;
        }
    }
    "…".into()
}

/// Lines of at most `max` logical pixels, broken at spaces (or anywhere in text without spaces, such as Chinese).
fn wrap(font: &FontVec, text: &str, size: f32, max: f32) -> Vec<String> {
    let mut lines = vec![];
    let mut line = String::new();
    let words: Vec<String> = if text.contains(' ') { text.split(' ').map(|w| format!("{w} ")).collect() } else { text.chars().map(String::from).collect() };
    for w in words {
        let candidate = format!("{line}{w}");
        if !line.is_empty() && text_width(font, candidate.trim_end(), size) > max {
            lines.push(line.trim_end().to_owned());
            line = w;
        } else {
            line = candidate;
        }
    }
    if !line.trim().is_empty() {
        lines.push(line.trim_end().to_owned());
    }
    lines
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
    /// Window pixel bounds of a logical box, clipped.
    fn span(&self, x: f32, y: f32, w: f32, h: f32) -> (usize, usize, usize, usize) {
        let (x0, y0) = (self.ox + x * self.scale, self.oy + y * self.scale);
        let (x1, y1) = (self.ox + (x + w) * self.scale, self.oy + (y + h) * self.scale);
        (x0.floor().max(0.0) as usize, y0.floor().max(0.0) as usize, (x1.ceil().max(0.0) as usize).min(self.w), (y1.ceil().max(0.0) as usize).min(self.h))
    }

    fn rrect(&mut self, x: f32, y: f32, rw: f32, rh: f32, r: f32, color: u32) {
        self.rrect_a(x, y, rw, rh, r, color, 255);
    }

    /// Rounded rectangle in page coordinates at opacity `alpha` (0..=255).
    #[allow(clippy::too_many_arguments)]
    fn rrect_a(&mut self, x: f32, y: f32, rw: f32, rh: f32, r: f32, color: u32, alpha: u32) {
        let (x0, y0, x1, y1) = (self.ox + x * self.scale, self.oy + y * self.scale, self.ox + (x + rw) * self.scale, self.oy + (y + rh) * self.scale);
        let r = (r * self.scale).min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
        let (px0, py0, px1, py1) = self.span(x, y, rw, rh);
        for py in py0..py1 {
            for px in px0..px1 {
                let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
                // distance outside the rounded shape (negative inside); the coverage also handles boxes thinner than a pixel
                let qx = (cx - (x0 + x1) / 2.0).abs() - ((x1 - x0) / 2.0 - r);
                let qy = (cy - (y0 + y1) / 2.0).abs() - ((y1 - y0) / 2.0 - r);
                let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
                let a = (0.5 - d).clamp(0.0, 1.0);
                if a > 0.0 {
                    let i = py * self.w + px;
                    self.buf[i] = mix(self.buf[i], color, (a * alpha as f32) as u32);
                }
            }
        }
    }

    /// A shape given by `inside(u, v)` over a logical box, 4 samples per pixel.
    fn fill_fn(&mut self, x: f32, y: f32, w: f32, h: f32, inside: fn(f32, f32) -> bool, color: u32) {
        let (x0, y0, s) = (self.ox + x * self.scale, self.oy + y * self.scale, self.scale);
        let (px0, py0, px1, py1) = self.span(x, y, w, h);
        for py in py0..py1 {
            for px in px0..px1 {
                let n = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
                    .iter()
                    .filter(|(sx, sy)| inside((px as f32 + sx - x0) / (w * s), (py as f32 + sy - y0) / (h * s)))
                    .count() as u32;
                if n > 0 {
                    let i = py * self.w + px;
                    self.buf[i] = mix(self.buf[i], color, n * 255 / 4);
                }
            }
        }
    }

    /// A polyline `width` logical pixels wide with round ends.
    fn stroke(&mut self, pts: &[(f32, f32)], width: f32, color: u32) {
        let hw = width * self.scale / 2.0;
        let sp: Vec<(f32, f32)> = pts.iter().map(|&(x, y)| (self.ox + x * self.scale, self.oy + y * self.scale)).collect();
        let (lx, hx) = sp.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
        let (ly, hy) = sp.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        let (px0, py0) = ((lx - hw - 1.0).max(0.0) as usize, (ly - hw - 1.0).max(0.0) as usize);
        let (px1, py1) = (((hx + hw + 2.0).max(0.0) as usize).min(self.w), ((hy + hw + 2.0).max(0.0) as usize).min(self.h));
        for py in py0..py1 {
            for px in px0..px1 {
                let p = (px as f32 + 0.5, py as f32 + 0.5);
                let d = sp.windows(2).map(|s| seg_dist(p, s[0], s[1])).fold(f32::MAX, f32::min);
                let a = (hw + 0.5 - d).clamp(0.0, 1.0);
                if a > 0.0 {
                    let i = py * self.w + px;
                    self.buf[i] = mix(self.buf[i], color, (a * 255.0) as u32);
                }
            }
        }
    }

    /// A game's state as a round badge: a green tick, a red cross or an empty ring.
    fn badge(&mut self, cx: f32, cy: f32, r: f32, st: Status) {
        match st {
            Status::Found => {
                self.rrect(cx - r, cy - r, 2.0 * r, 2.0 * r, r, GREEN);
                self.stroke(&[(cx - r * 0.45, cy + r * 0.02), (cx - r * 0.1, cy + r * 0.38), (cx + r * 0.48, cy - r * 0.35)], r * 0.22, 0x0d1a12);
            }
            Status::Missing => {
                self.rrect(cx - r, cy - r, 2.0 * r, 2.0 * r, r, RED);
                let d = r * 0.38;
                self.stroke(&[(cx - d, cy - d), (cx + d, cy + d)], r * 0.22, 0x1d0b09);
                self.stroke(&[(cx - d, cy + d), (cx + d, cy - d)], r * 0.22, 0x1d0b09);
            }
            Status::Unset => {
                self.rrect(cx - r, cy - r, 2.0 * r, 2.0 * r, r, 0x5a616d);
                self.rrect(cx - r + 2.0, cy - r + 2.0, 2.0 * r - 4.0, 2.0 * r - 4.0, r - 2.0, SIDEBAR);
            }
        }
    }

    /// The icon with its top-left at page position (x, y), `size` page pixels square.
    fn icon(&mut self, x: f32, y: f32, size: f32) {
        let (x0, y0, s) = (self.ox + x * self.scale, self.oy + y * self.scale, size * self.scale);
        let (px0, py0, px1, py1) = self.span(x, y, size, size);
        for py in py0..py1 {
            for px in px0..px1 {
                let (u0, v0) = ((px as f32 - x0) / s, (py as f32 - y0) / s);
                let (c, a) = icon_area(u0, v0, u0 + 1.0 / s, v0 + 1.0 / s);
                if a > 0.0 {
                    let i = py * self.w + px;
                    self.buf[i] = mix(self.buf[i], c, (a * 255.0) as u32);
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
                self.buf[py * self.w + px] = (acc[0] / 4) << 16 | (acc[1] / 4) << 8 | (acc[2] / 4);
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

/// Distance from `p` to the segment `a`-`b`.
fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("opensow-launcher-{name}-{}", std::process::id()))
    }

    fn app() -> App {
        App::new(Settings::default(), GameSettings::default(), temp("unused"))
    }

    fn find(a: &App, act: Action) -> Option<Button> {
        a.buttons().into_iter().find(|b| b.action == act)
    }

    #[test]
    fn play_waits_for_the_base_game() {
        let mut a = app();
        let play = find(&a, Action::Play).unwrap();
        assert!(!play.enabled, "home: greyed out until State of War is located");
        assert_eq!(a.click(play.rect.x + 1.0, play.rect.y + 1.0), None);
        // a game page never has Play; without its folder it offers the big Locate
        a.page = Page::Game(Slot::Original);
        assert!(find(&a, Action::Play).is_none());
        assert!(a.buttons().iter().filter(|b| b.action == Action::Browse(Slot::Original)).count() == 2);
        let dir = temp("base");
        std::fs::create_dir_all(dir.join("data")).unwrap();
        a.offer_dir(dir.clone(), a.drop_slot());
        assert_eq!(a.status(Slot::Original), Status::Found, "`data` is found case-insensitively");
        assert!(find(&a, Action::Play).is_none());
        assert_eq!(a.buttons().iter().filter(|b| b.action == Action::Browse(Slot::Original)).count(), 1, "found: only Change folder");
        a.page = Page::Home;
        assert!(find(&a, Action::Play).unwrap().enabled);
        // the files go away between launches: checked again, the page offers to locate the game again
        std::fs::remove_dir_all(&dir).unwrap();
        a.refresh();
        assert_eq!(a.status(Slot::Original), Status::Missing);
        assert!(!find(&a, Action::Play).unwrap().enabled);
    }

    #[test]
    fn warmonger_is_optional_but_needs_the_base_game_to_play() {
        let mut a = app();
        a.offer_dir(temp("no-such-folder"), Some(Slot::Warmonger));
        assert_eq!(a.status(Slot::Warmonger), Status::Missing);
        assert!(!a.playable(Slot::Warmonger));
        let w = temp("wm");
        std::fs::create_dir_all(w.join("Data").join("Levels")).unwrap();
        std::fs::write(w.join("Data").join("Levels").join("M01.edt"), b"").unwrap();
        std::fs::write(w.join("State of War Warmonger.exe"), b"").unwrap();
        a.offer_dir(w.clone(), None);
        assert_eq!(a.status(Slot::Warmonger), Status::Found, "a Warmonger folder lands in its slot");
        assert!(!a.playable(Slot::Warmonger), "the base game is still missing");
        a.page = Page::Game(Slot::Warmonger);
        assert!(find(&a, Action::Play).is_none());
        std::fs::remove_dir_all(&w).unwrap();
    }

    #[test]
    fn sidebar_and_tabs_navigate() {
        let a = app();
        let b = a.buttons();
        let set = b.iter().find(|b| b.action == Action::Nav(Page::Settings)).unwrap().rect;
        assert_eq!(a.click(set.x + 5.0, set.y + 5.0), Some(Action::Nav(Page::Settings)));
        let mut a = a;
        a.page = Page::Settings;
        a.tab = Tab::Launcher;
        let langs: Vec<Option<Language>> = a.buttons().iter().filter_map(|b| if let Action::Lang(l) = b.action { Some(l) } else { None }).collect();
        assert_eq!(langs.len(), 1 + Language::SUPPORTED.len());
        assert!(!langs.contains(&Some(Language::Polish)) && !langs.contains(&Some(Language::Hungarian)), "copies and the damaged file are not offered");
        assert!(a.buttons().iter().find(|b| b.action == Action::Lang(None)).unwrap().selected);
    }

    #[test]
    fn every_page_fits_without_overlaps_at_every_width() {
        let mut a = app();
        for (w, h) in [(640, 480), (800, 600), (1000, 620), (1920, 1080), (2560, 1080), (300, 900)] {
            a.resize(w, h);
            let v = a.view();
            assert!(v.ox >= -0.01 && v.oy >= -0.01 && v.ox + v.pw * v.scale <= w as f32 + 0.01 && v.oy + PAGE_H * v.scale <= h as f32 + 0.01);
            for page in [Page::Home, Page::Game(Slot::Original), Page::Game(Slot::Warmonger), Page::Settings] {
                for tab in [Tab::Launcher, Tab::Game, Tab::Extras] {
                    a.page = page;
                    a.tab = tab;
                    let b = a.buttons();
                    for (i, x) in b.iter().enumerate() {
                        let r = x.rect;
                        assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= v.pw && r.y + r.h <= PAGE_H, "{page:?} {tab:?} {:?} outside at {w}x{h}", x.action);
                        for y in &b[i + 1..] {
                            assert!(!r.overlaps(&y.rect), "{page:?} {tab:?}: {:?} overlaps {:?} at {w}x{h}", x.action, y.action);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_update_state_fits_the_launcher_tab() {
        let mut a = app();
        a.page = Page::Settings;
        a.tab = Tab::Launcher;
        let asset = |n: &str| crate::update::Asset { name: n.into(), browser_download_url: String::new(), size: 10 };
        let r = Release {
            tag: "v9.0.0".into(),
            version: (9, 0, 0),
            name: "N".into(),
            url: String::new(),
            archive: Some(asset("a.zip")),
            checksum: Some(asset("a.zip.sha256")),
        };
        let no_sum = Release { checksum: None, ..r.clone() };
        let states = [
            UpdateState::Idle,
            UpdateState::Checking,
            UpdateState::UpToDate,
            UpdateState::Available(r.clone()),
            UpdateState::Available(no_sum),
            UpdateState::Downloading(r.clone(), 5),
            UpdateState::Installed(r.clone()),
            UpdateState::Failed(Fail::NoReleases),
        ];
        for (w, h) in [(640, 480), (1920, 1080)] {
            a.resize(w, h);
            for st in &states {
                a.update = st.clone();
                let b = a.buttons();
                for (i, x) in b.iter().enumerate() {
                    assert!(x.rect.x + x.rect.w <= a.view().pw && x.rect.y + x.rect.h <= PAGE_H, "{st:?} {:?}", x.action);
                    assert!(b[i + 1..].iter().all(|y| !x.rect.overlaps(&y.rect)), "{st:?}: {:?} overlaps", x.action);
                }
                let has = |act: Action| b.iter().any(|x| x.action == act && x.enabled);
                match st {
                    UpdateState::Available(r) if r.checksum.is_some() => assert!(has(Action::InstallUpdate)),
                    UpdateState::Installed(_) => assert!(has(Action::Restart)),
                    UpdateState::Checking | UpdateState::Downloading(..) => assert!(!has(Action::CheckUpdate) && !has(Action::InstallUpdate)),
                    _ => assert!(has(Action::CheckUpdate) && !has(Action::InstallUpdate)),
                }
            }
        }
    }

    #[test]
    fn settings_controls_change_the_values() {
        let mut a = app();
        a.page = Page::Settings;
        a.tab = Tab::Game;
        assert!(find(&a, Action::Toggle(Field::Radar)).is_some());
        assert!(find(&a, Action::Step(Field::MusicVolume, -1)).unwrap().enabled);
        assert!(!find(&a, Action::Step(Field::MusicVolume, 1)).unwrap().enabled, "100 is the top");
        a.tab = Tab::Extras;
        assert_eq!(a.buttons().iter().filter(|b| matches!(b.action, Action::Toggle(Field::Extra(_)))).count(), 8);
    }

    #[test]
    fn the_icon_is_a_cut_out() {
        let px = icon_rgba(64);
        let alpha = |x: usize, y: usize| px[(y * 64 + x) * 4 + 3];
        assert_eq!((alpha(0, 0), alpha(63, 0), alpha(0, 63), alpha(63, 63)), (0, 0, 0, 0), "transparent corners");
        assert_eq!(alpha(32, 32), 255, "the tank in the middle");
    }

    #[test]
    fn the_pictures_decode() {
        let a = app();
        assert!(a.art.iter().all(|p| p.as_ref().is_some_and(|p| p.w > 1000 && p.h > 600)));
    }
}
