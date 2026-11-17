//! Launcher state, layout and drawing. Pure CPU, no window: `main.rs` feeds it events and the `--screenshot` mode renders it headless.
//! The page is laid out in logical units, 600 high and as wide as the window's shape allows (at least 800), and scaled to fit:
//! a sidebar (home, the two games, settings) and the chosen page on the right (docs/launcher.md).

use crate::settings::{Field, GameSettings, NATIVE};
use crate::text::{Message, Tx};
use crate::update::{Fail, Note, Release};
use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont};
use opensow_formats::lang::Language;
use opensow_formats::launcher::Settings;
use opensow_formats::{default_steam_dirs, sysfont, GameDir};
use std::cell::RefCell;
use std::collections::HashMap;
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
    /// Open the new release's page in the browser, or all releases when there is none.
    ReleaseNotes,
    /// The sidebar header while an update waits: Settings, Launcher tab.
    Updates,
    /// Open the project's GitHub page.
    Repo,
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
    /// The sidebar header (icon, name, version) while an update waits.
    Header,
    /// Small text link.
    Link,
    /// The sidebar's link to the project's GitHub page.
    SideLink,
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

/// A value easing to its target ([`App::tween`]).
#[derive(Debug, Clone, Copy)]
struct Tween {
    from: f32,
    to: f32,
    start: f32,
    dur: f32,
}

impl Tween {
    fn done(&self, t: f32) -> bool {
        t - self.start >= self.dur
    }

    /// Ease-out cubic: fast start, soft landing.
    fn value(&self, t: f32) -> f32 {
        if self.dur <= 0.0 {
            return self.to;
        }
        let k = ((t - self.start) / self.dur).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * (1.0 - (1.0 - k).powi(3))
    }
}

/// How a change of the whole page animates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeKind {
    /// The window's first appearance: from the bare picture, the sidebar first, then the page top to bottom, rising into place.
    Intro,
    /// Another page or tab: the old one fades out while the new content rises into place top to bottom; the sidebar keeps its
    /// own animation.
    Page,
    /// The same page in other words (language, the update card): a plain cross-fade.
    Swap,
}

/// A cross-fade from an earlier frame (`None`: from the bare backdrop).
struct Fade {
    start: f32,
    kind: FadeKind,
    from: Option<Vec<u32>>,
}

impl FadeKind {
    /// (wait before it starts, length of each row's fade, extra wait from the page's top to its bottom, rise in page units)
    fn timing(self) -> (f32, f32, f32, f32) {
        match self {
            // the window takes a moment to open
            FadeKind::Intro => (0.2, 0.45, 0.25, 14.0),
            FadeKind::Page => (0.0, 0.3, 0.12, 10.0),
            FadeKind::Swap => (0.0, 0.22, 0.0, 0.0),
        }
    }

    fn total(self) -> f32 {
        let (wait, len, stagger, _) = self.timing();
        wait + len + stagger + if self == FadeKind::Intro { 0.08 } else { 0.0 }
    }
}

/// Animation lengths in seconds.
const HOVER_S: f32 = 0.16;
const SLIDE_S: f32 = 0.3;

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
    /// Seconds since the window opened, for the update badge's pulse.
    pub time: f32,
    /// Eased values by name (hover glows, the sliding highlights, switches, ...).
    anims: RefCell<HashMap<String, Tween>>,
    fade: RefCell<Option<Fade>>,
    /// The settings gear: (angle, time of the last frame, how fast it turns 0..1 while hovered).
    spin: RefCell<(f32, f32, f32)>,
    /// The message on screen and when it appeared (it slides in).
    shown_msg: RefCell<(Message, f32)>,
    /// The window's size in pixels (the last `resize`).
    size: (usize, usize),
    /// This version's release notes in each language, `Tx::index` order (`update::OWN_NOTES`).
    own_notes: [Vec<Note>; 5],
    status: [Status; 2],
    view: View,
    art: Vec<Option<Picture>>,
    cache: RefCell<Option<Backdrop>>,
}

/// The text face, best first; a file missing at its path is also looked up by name in the font folders (`sysfont::find`), and
/// with none of them any plain sans-serif face is used.
const FONTS: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
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
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
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
/// The update badge's pulse period in seconds.
const PULSE: f32 = 2.4;

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

/// The application icon, 64x64 straight RGBA (build.rs makes it from `art/icon.png`, docs/release.md "Icons"): drawn in
/// the sidebar header and on the home page, and the window icon.
pub const ICON_RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon64.rgba"));
const ICON_SIDE: usize = 64;

/// The icon as (side, premultiplied RGBA as f32), converted once.
fn icon() -> &'static (usize, Vec<[f32; 4]>) {
    static ICON: std::sync::OnceLock<(usize, Vec<[f32; 4]>)> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        let px = ICON_RGBA
            .chunks_exact(4)
            .map(|c| {
                let a = c[3] as f32 / 255.0;
                [c[0] as f32 * a, c[1] as f32 * a, c[2] as f32 * a, a]
            })
            .collect();
        (ICON_SIDE, px)
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
        let first = |paths: Vec<PathBuf>| paths.iter().find_map(|p| std::fs::read(p).ok().and_then(|d| FontVec::try_from_vec(d).ok()));
        let font = first(sysfont::find(FONTS)).or_else(|| first(sysfont::any_sans()));
        let bold = first(sysfont::find(BOLD_FONTS));
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
            time: 0.0,
            anims: RefCell::new(HashMap::new()),
            fade: RefCell::new(None),
            spin: RefCell::new((0.0, 0.0, 0.0)),
            shown_msg: RefCell::new((Message::None, -10.0)),
            size: (1000, 620),
            own_notes: crate::update::OWN_NOTES.map(crate::update::parse_notes),
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
        self.size = (w, h);
    }

    // ---- motion ----
    // Every animation is short and runs on `time`; the window redraws only while one runs ([`App::frame_interval`]). A value
    // seen for the first time starts at its target, so headless screenshots show the resting state.

    /// The value named `key` easing to `target` over `dur` seconds.
    fn tween(&self, key: &str, target: f32, dur: f32) -> f32 {
        let t = self.time;
        let mut m = self.anims.borrow_mut();
        let e = m.entry(key.to_string()).or_insert(Tween { from: target, to: target, start: t - dur, dur });
        if (e.to - target).abs() > 1e-4 {
            *e = Tween { from: e.value(t), to: target, start: t, dur };
        }
        e.value(t)
    }

    /// Animate from what is on screen now to whatever the next frame shows (page, tab, language or update card changes).
    pub fn begin_fade(&mut self, kind: FadeKind) {
        let (w, h) = self.size;
        let mut px = vec![0u32; w * h];
        self.draw(&mut px, w, h);
        *self.fade.borrow_mut() = Some(Fade { start: self.time, kind, from: Some(px) });
    }

    /// The window's first frames: the page appears over its picture.
    pub fn intro(&mut self) {
        *self.fade.borrow_mut() = Some(Fade { start: self.time, kind: FadeKind::Intro, from: None });
    }

    /// How soon the next frame is due: 1/60 s while something moves, 1/30 s while only the update badge pulses, none when all
    /// is still.
    pub fn frame_interval(&self) -> Option<f32> {
        let t = self.time;
        let moving = self.anims.borrow().values().any(|a| !a.done(t))
            || self.fade.borrow().as_ref().is_some_and(|f| t - f.start < f.kind.total())
            || t - self.shown_msg.borrow().1 < SLIDE_S
            || self.spin.borrow().2 > 0.001;
        if moving {
            Some(1.0 / 60.0)
        } else if self.animating() {
            Some(1.0 / 30.0)
        } else {
            None
        }
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

    /// The sidebar's GitHub link, above Settings.
    fn repo_rect() -> Rect {
        let s = Self::nav_rect(3);
        Rect { x: s.x, y: s.y - 40.0, w: s.w, h: 32.0 }
    }

    /// The play button's place (bottom right of the page).
    fn play_rect(&self) -> Rect {
        Rect { x: self.view.pw - 36.0 - 220.0, y: PAGE_H - 36.0 - 60.0, w: 220.0, h: 60.0 }
    }

    /// Home page: the "Your games" card and its rows.
    fn games_card(&self) -> Rect {
        Rect { x: SIDE + 40.0, y: 200.0, w: (self.view.pw - SIDE - 80.0).min(430.0), h: 158.0 }
    }

    /// The release the home page's notes card shows: a newer one found, else none (this version's own notes).
    fn new_release(&self) -> Option<&Release> {
        match &self.update {
            UpdateState::Available(r) | UpdateState::Downloading(r, _) | UpdateState::Installed(r) => Some(r),
            _ => None,
        }
    }

    /// The notes in the page's language, English where they are not translated.
    fn card_notes(&self) -> &[Note] {
        let i = self.tx().index();
        match self.new_release() {
            Some(r) => r.notes_in(i),
            None if self.own_notes[i].is_empty() => &self.own_notes[0],
            None => &self.own_notes[i],
        }
    }

    /// Home page: the "What's new" card under "Your games", as wide as it but clear of Play; none without notes or update.
    fn notes_card(&self) -> Option<Rect> {
        if self.card_notes().is_empty() && self.new_release().is_none() {
            return None;
        }
        let (g, p) = (self.games_card(), self.play_rect());
        let y = g.y + g.h + 16.0;
        Some(Rect { x: g.x, y, w: g.w.min(p.x - 20.0 - g.x), h: PAGE_H - 36.0 - y })
    }

    /// Is the update badge pulsing (the window then redraws about 30 times a second)?
    pub fn animating(&self) -> bool {
        matches!(self.update, UpdateState::Available(_) | UpdateState::Downloading(..))
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
    /// Rows per column of the Extras tab (two columns).
    const EXTRA_ROWS: usize = crate::settings::EXTRAS.len().div_ceil(2);

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
        if self.new_release().is_some() {
            push(Rect { x: 8.0, y: 10.0, w: SIDE - 16.0, h: 60.0 }, Action::Updates, "", Style::Header, false, true);
        }
        push(Self::repo_rect(), Action::Repo, "GitHub", Style::SideLink, false, true);
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
                if let Some(n) = self.notes_card() {
                    let fy = n.y + n.h - 42.0;
                    let link = match self.new_release() {
                        Some(_) => format!("{} ›", tx.release_notes()),
                        None => tx.all_notes().into(),
                    };
                    let lw = (self.width(&link, 12.5, false) + 8.0).min(n.w * 0.48);
                    push(Rect { x: n.x + n.w - 14.0 - lw, y: fy, w: lw, h: 30.0 }, Action::ReleaseNotes, &link, Style::Link, false, true);
                    let main = match &self.update {
                        UpdateState::Available(r) if r.archive.is_some() && r.checksum.is_some() => {
                            Some((Action::InstallUpdate, tx.install_short(&r.version_text())))
                        }
                        UpdateState::Installed(_) => Some((Action::Restart, tx.restart().to_string())),
                        _ => None,
                    };
                    if let Some((action, label)) = main {
                        let w = (self.width(&label, 12.5, true) + 28.0).min(n.w - 28.0 - lw - 10.0);
                        push(Rect { x: n.x + 14.0, y: fy, w, h: 30.0 }, action, &label, Style::Primary(ACCENT), false, true);
                    }
                }
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
                            let (col, row) = (k / Self::EXTRA_ROWS, k % Self::EXTRA_ROWS);
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
            UpdateState::Downloading(r, got) => (tx.downloading(&r.version_text(), download_pct(r, *got)), TEXT),
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
            let p = self.tween("download", (*got as f32 / size as f32).min(1.0), 0.3);
            c.rrect(x0, y + 136.0, w * p, 6.0, 3.0, ACCENT);
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
        let mut base: Option<PathBuf> = std::env::var_os(opensow_formats::GAME_DIR_ENV).map(PathBuf::from).filter(|p| p.join("Data").is_dir());
        if base.is_none() {
            base = default_steam_dirs().into_iter().next();
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
        let (nav, rest): (Vec<Button>, Vec<Button>) = self.buttons().into_iter().partition(|b| matches!(b.style, Style::Nav | Style::Header | Style::SideLink));
        // the selected entry's highlight slides from the old one
        if let Some(i) = [Page::Home, Page::Game(Slot::Original), Page::Game(Slot::Warmonger), Page::Settings].iter().position(|p| *p == self.page) {
            let r = Self::nav_rect(i);
            let y = self.tween("nav", r.y, SLIDE_S);
            c.rrect(r.x, y, r.w, r.h, 8.0, SIDE_ACTIVE);
            c.rrect(r.x - 10.0, y + 10.0, 4.0, r.h - 20.0, 2.0, accent(self.page));
        }
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
        // the settings tab's underline slides to the chosen tab
        if let Some(b) = rest.iter().find(|b| b.style == Style::Tab && b.selected) {
            let (x, w) = (self.tween("tab-x", b.rect.x, SLIDE_S), self.tween("tab-w", b.rect.w, SLIDE_S));
            c.rrect(x + 6.0, b.rect.y + b.rect.h - 3.0, w - 12.0, 3.0, 1.5, ACCENT);
        }
        self.draw_message(&mut c, tx);
        self.blend_fade(buf, w, h);
    }

    /// Blends the frame from the earlier one: each row of the new page fades in a little after the one above, and what the page
    /// draws over its picture (texts, cards, buttons) rises into place; the picture itself stays put.
    fn blend_fade(&self, buf: &mut [u32], w: usize, h: usize) {
        let mut fade = self.fade.borrow_mut();
        let Some(f) = fade.as_ref() else { return };
        let t = self.time - f.start;
        let cache = self.cache.borrow();
        let bg = cache.as_ref().filter(|b| b.w == w && b.h == h).map(|b| b.px.as_slice());
        let (Some(bg), true) = (bg, t < f.kind.total()) else {
            *fade = None;
            return;
        };
        let from = f.from.as_deref().filter(|p| p.len() >= w * h).unwrap_or(bg);
        let (wait, len, stagger, rise) = f.kind.timing();
        let v = self.view;
        let side_px = ((v.ox + SIDE * v.scale).max(0.0) as usize).min(w);
        // the new frame, read from below while rows rise (only needed while some row still moves)
        let new = if rise > 0.0 { buf[..w * h].to_vec() } else { vec![] };
        let ease = |k: f32| 1.0 - (1.0 - k.clamp(0.0, 1.0)).powi(3);
        for py in 0..h {
            let ly = ((py as f32 - v.oy) / v.scale / PAGE_H).clamp(0.0, 1.0);
            // (first pixel, last pixel, progress) of the sidebar and of the page in this row
            let side_k = match f.kind {
                FadeKind::Intro => ease((t - wait) / len),
                FadeKind::Page => 1.0,
                FadeKind::Swap => ease(t / len),
            };
            let page_k = ease((t - wait - if f.kind == FadeKind::Intro { 0.08 } else { 0.0 } - stagger * ly) / len);
            for (x0, x1, k) in [(0, side_px, side_k), (side_px, w, page_k)] {
                let a = (k * 255.0) as u32;
                // the overlay comes from a little lower and rises as it fades in
                let off = ((1.0 - k) * rise * v.scale) as usize;
                let sy = (py + if x0 == 0 { 0 } else { off }).min(h - 1);
                let row = py * w;
                // finished rows stay as drawn, rows not started yet show the old frame
                if a >= 255 && sy == py {
                    continue;
                }
                if a == 0 {
                    buf[row + x0..row + x1].copy_from_slice(&from[row + x0..row + x1]);
                    continue;
                }
                for px in x0..x1 {
                    let i = py * w + px;
                    let target = if sy == py {
                        buf[i]
                    } else {
                        let j = sy * w + px;
                        // the page's own drawing taken from below, over this row's picture
                        let ch = |s: u32| ((bg[i] >> s & 255) as i32 + (new[j] >> s & 255) as i32 - (bg[j] >> s & 255) as i32).clamp(0, 255) as u32;
                        ch(16) << 16 | ch(8) << 8 | ch(0)
                    };
                    buf[i] = mix(from[i], target, a);
                }
            }
        }
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
        if self.animating() {
            // an amber dot with an up arrow on the icon's corner; a ring pulses out of it and it gives a small hop
            let (dx, dy) = (53.0, 21.0 + self.hop());
            // it pops in with a small overshoot when the update is found
            let s = back_out(self.tween("badge", 1.0, 0.4));
            self.pulse_ring(c, dx, dy, 8.5);
            c.rrect(dx - 10.5 * s, dy - 10.5 * s, 21.0 * s, 21.0 * s, 10.5 * s, SIDEBAR);
            c.rrect(dx - 8.5 * s, dy - 8.5 * s, 17.0 * s, 17.0 * s, 8.5 * s, ACCENT);
            if s > 0.5 {
                c.stroke(&[(dx, dy + 4.0 * s), (dx, dy - 4.0 * s)], 1.8, 0x1a1405);
                c.stroke(&[(dx - 3.5 * s, dy - 0.5 * s), (dx, dy - 4.0 * s), (dx + 3.5 * s, dy - 0.5 * s)], 1.8, 0x1a1405);
            }
        } else {
            self.tween("badge", 0.0, 0.0);
        }
        self.text(c, "OpenSOW", 64.0, 19.0, 21.0, TEXT, true);
        // the version, and the one waiting
        let v = format!("v{}", crate::update::VERSION);
        self.text(c, &v, 65.0, 44.0, 12.5, DIM, false);
        if let Some(r) = self.new_release() {
            let col = if matches!(self.update, UpdateState::Installed(_)) { GREEN } else { ACCENT };
            self.text(c, &format!(" → {}", r.version_text()), 65.0 + self.width(&v, 12.5, false), 44.0, 12.5, col, false);
        }
        for (i, page) in [Page::Home, Page::Game(Slot::Original), Page::Game(Slot::Warmonger), Page::Settings].into_iter().enumerate() {
            let r = Self::nav_rect(i);
            let (icx, icy) = (r.x + 26.0, r.y + r.h / 2.0);
            let k = self.hovered(Action::Nav(page), r, true);
            let col = mix(DIM, if self.page == page { ACCENT } else { TEXT }, if self.page == page { 255 } else { (k * 255.0) as u32 });
            match page {
                // the house hops up a little
                Page::Home => c.fill_fn(icx - 12.0, icy - 12.0 - 2.5 * k, 24.0, 24.0, house, col),
                // the gear spins while hovered (speeding up and slowing down) and turns a quarter when Settings opens
                Page::Settings => {
                    let mut sp = self.spin.borrow_mut();
                    let dt = (self.time - sp.1).clamp(0.0, 0.1);
                    sp.0 += dt * 3.5 * k;
                    sp.1 = self.time;
                    sp.2 = k;
                    let turn = self.tween("gear open", if self.page == page { 1.0 } else { 0.0 }, 0.6) * std::f32::consts::FRAC_PI_2;
                    c.fill_rot(icx - 12.0, icy - 12.0, 24.0, 24.0, gear, sp.0 + turn, col);
                }
                // the badge grows a little
                Page::Game(slot) => c.badge(icx, icy, 12.0 * (1.0 + 0.12 * k), self.status(slot)),
            }
            // the label leans right
            let r = Rect { x: r.x + 3.0 * k, ..r };
            let (label, sub) = match page {
                Page::Home => (tx.home().to_string(), None),
                Page::Settings => (tx.settings().to_string(), self.update_badge(tx)),
                Page::Game(slot) => (slot.name().to_string(), Some(self.state_line(tx, slot))),
            };
            match sub {
                // the full name takes two lines
                Some((s, col)) if page == Page::Game(Slot::Warmonger) => {
                    self.text(c, "State of War:", r.x + 50.0, r.y + 2.0, 14.0, TEXT, true);
                    self.text(c, "Warmonger", r.x + 50.0, r.y + 18.0, 14.0, TEXT, true);
                    let s = self.fit(&s, 11.5, r.w - 56.0);
                    self.text(c, &s, r.x + 50.0, r.y + 36.0, 11.5, col, false);
                }
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
        self.text(c, "OpenSOW", x - 2.0, 98.0, 50.0, TEXT, true);
        let card = self.games_card();
        c.rrect_a(card.x, card.y, card.w, card.h, 12.0, 0x10131a, 200);
        self.text(c, tx.your_games(), card.x + 18.0, card.y + 14.0, 14.0, DIM, true);
        for slot in [Slot::Original, Slot::Warmonger] {
            let r = self.game_row(slot);
            let k = self.hovered(Action::Nav(Page::Game(slot)), r, true);
            c.badge(r.x + 24.0, r.y + r.h / 2.0, 13.0 * (1.0 + 0.1 * k), self.status(slot));
            let title = if slot == Slot::Warmonger { "State of War: Warmonger" } else { "State of War" };
            self.text(c, title, r.x + 50.0, r.y + 7.0, 16.0, TEXT, true);
            let (s, col) = self.state_line(tx, slot);
            let s = self.fit(&s, 12.5, r.w - 80.0);
            self.text(c, &s, r.x + 50.0, r.y + 29.0, 12.5, col, false);
            // the arrow slides towards the page it opens
            let ax = r.x + r.w - 22.0 + 5.0 * k;
            c.stroke(&[(ax, r.y + 19.0), (ax + 7.0, r.y + 26.0), (ax, r.y + 33.0)], 2.0, mix(DIM, TEXT, (k * 255.0) as u32));
        }
        if !self.playable(Slot::Original) {
            let p = self.play_rect();
            let s = self.fit(tx.locate_first(), 13.5, p.w + 120.0);
            let w = self.width(&s, 13.5, false);
            self.text(c, &s, p.x + p.w - w, p.y - 26.0, 13.5, 0xc9ced8, false);
        }
        self.draw_notes(c, tx);
    }

    /// The home page's "What's new" card: this version's notes, or the new release's with its state while one waits.
    fn draw_notes(&self, c: &mut Canvas, tx: Tx) {
        let Some(n) = self.notes_card() else { return };
        c.rrect_a(n.x, n.y, n.w, n.h, 12.0, 0x10131a, 200);
        let (head, col) = match &self.update {
            UpdateState::Available(r) => (tx.update_ready(&r.version_text()), ACCENT),
            UpdateState::Downloading(r, got) => (tx.downloading(&r.version_text(), download_pct(r, *got)), TEXT),
            UpdateState::Installed(r) => (tx.installed(&r.version_text()), GREEN),
            _ => (tx.whats_new(crate::update::VERSION), DIM),
        };
        let mut hx = n.x + 18.0;
        if self.animating() {
            self.pulse_ring(c, hx + 4.0, n.y + 22.0, 4.0);
            c.rrect(hx, n.y + 18.0, 8.0, 8.0, 4.0, ACCENT);
            hx += 16.0;
        }
        let room = n.x + n.w - 18.0 - hx;
        let size = self.font_fit(&head, 14.0, room).min(14.0);
        self.text(c, &self.fit(&head, size, room), hx, n.y + 14.0, size, col, true);
        let notes = self.card_notes();
        let fy = n.y + n.h - 42.0;
        let fit = ((fy - 6.0 - (n.y + 44.0)) / 22.0).floor().max(1.0) as usize;
        let tag_w = [false, true].map(|f| self.width(tx.note_tag(f), 9.5, true)).into_iter().fold(0.0, f32::max) + 12.0;
        for (i, note) in notes.iter().take(fit).enumerate() {
            let y = n.y + 44.0 + 22.0 * i as f32;
            let (bg, fg) = if note.fixed { (GREEN, GREEN) } else { (BLUE, BLUE) };
            c.rrect_a(n.x + 18.0, y + 1.0, tag_w, 16.0, 4.0, bg, 40);
            let tag = tx.note_tag(note.fixed);
            let w = self.width(tag, 9.5, true);
            self.text(c, tag, n.x + 18.0 + (tag_w - w) / 2.0, y + 3.5, 9.5, fg, true);
            let tx0 = n.x + 18.0 + tag_w + 10.0;
            self.text(c, &self.fit(&note.title, 13.0, n.x + n.w - 18.0 - tx0), tx0, y + 1.0, 13.0, TEXT, false);
        }
        if notes.is_empty() {
            // a release without notes in the expected form: its title
            if let Some(r) = self.new_release() {
                self.text(c, &self.fit(&r.name, 13.0, n.w - 36.0), n.x + 18.0, n.y + 45.0, 13.0, TEXT, false);
            }
        }
        // "+N more" when no button sits in the footer's left corner
        let more = notes.len().saturating_sub(fit);
        let has_button = self.buttons().iter().any(|b| matches!(b.action, Action::InstallUpdate | Action::Restart));
        if more > 0 && !has_button {
            self.text(c, &tx.more(more), n.x + 18.0, fy + 8.0, 12.5, DIM, false);
        }
    }

    /// How much the button at `r` is hovered, easing in and out (0..1).
    fn hovered(&self, action: Action, r: Rect, enabled: bool) -> f32 {
        let hover = enabled && r.contains(self.cursor.0, self.cursor.1);
        self.tween(&format!("hover {action:?} {} {}", r.x as i32, r.y as i32), if hover { 1.0 } else { 0.0 }, HOVER_S)
    }

    /// The pulse around the update badge: a ring that grows from `r` and fades, once every [`PULSE`] seconds.
    fn pulse_ring(&self, c: &mut Canvas, cx: f32, cy: f32, r: f32) {
        let p = (self.time / PULSE).fract();
        if p < 0.7 {
            let k = p / 0.7;
            c.ring(cx, cy, r * (1.0 + 1.3 * k), 2.0, ACCENT, ((1.0 - k) * 230.0) as u32);
        }
    }

    /// The badge's small hop (page units, up is negative), after each ring has started.
    fn hop(&self) -> f32 {
        let p = (self.time / PULSE).fract();
        if (0.6..0.76).contains(&p) {
            let t = (p - 0.6) / 0.16;
            -3.0 * (t * std::f32::consts::TAU).sin().abs() * (1.0 - t)
        } else {
            0.0
        }
    }

    fn draw_game(&self, c: &mut Canvas, tx: Tx, slot: Slot) {
        let x = SIDE + 40.0;
        let maxw = self.view.pw - x - 40.0;
        let col = accent(self.page);
        let title = if slot == Slot::Warmonger { "State of War: Warmonger" } else { "State of War" };
        let size = self.bold.as_ref().map_or(48.0, |f| fit_size(f, title, 48.0, maxw));
        self.text(c, title, x, PAGE_H - 236.0 + (48.0 - size) * 0.7, size, col, true);
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
                    let (col, row) = (k / Self::EXTRA_ROWS, k % Self::EXTRA_ROWS);
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
        let k = self.hovered(b.action, r, b.enabled);
        let ka = (k * 255.0) as u32;
        match &b.style {
            Style::Nav => {
                // the selected entry's highlight is drawn by `draw` (it slides)
                if !b.selected && k > 0.0 {
                    c.rrect_a(r.x, r.y, r.w, r.h, 8.0, SIDE_HOVER, ka);
                }
            }
            Style::Row => {
                if k > 0.0 {
                    c.rrect_a(r.x, r.y, r.w, r.h, 8.0, 0xffffff, (18.0 * k) as u32);
                }
            }
            Style::Header => {
                if k > 0.0 {
                    c.rrect_a(r.x, r.y, r.w, r.h, 8.0, SIDE_HOVER, ka);
                }
            }
            Style::SideLink => {
                if k > 0.0 {
                    c.rrect_a(r.x, r.y, r.w, r.h, 8.0, SIDE_HOVER, ka);
                }
                // a code sign `</>`, the label and an arrow pointing out of the launcher; they lean right on hover
                let col = mix(DIM, TEXT, ka);
                let (cx, cy) = (r.x + 26.0 + 2.0 * k, r.y + r.h / 2.0);
                c.stroke(&[(cx - 4.5, cy - 5.0), (cx - 9.5, cy), (cx - 4.5, cy + 5.0)], 1.8, col);
                c.stroke(&[(cx + 4.5, cy - 5.0), (cx + 9.5, cy), (cx + 4.5, cy + 5.0)], 1.8, col);
                c.stroke(&[(cx + 1.8, cy - 6.0), (cx - 1.8, cy + 6.0)], 1.6, col);
                let lx = r.x + 50.0 + 3.0 * k;
                self.text(c, &b.label, lx, r.y + (r.h - 13.5) / 2.0 - 1.0, 13.5, col, false);
                let ax = lx + self.width(&b.label, 13.5, false) + 7.0 + 2.0 * k;
                let ay = cy - 2.0 * k;
                c.stroke(&[(ax, ay + 3.5), (ax + 6.0, ay - 2.5)], 1.5, col);
                c.stroke(&[(ax + 1.5, ay - 2.5), (ax + 6.0, ay - 2.5), (ax + 6.0, ay + 2.0)], 1.5, col);
            }
            Style::Link => {
                let s = self.fit(&b.label, 12.5, r.w);
                let tw = self.width(&s, 12.5, false);
                self.text(c, &s, r.x + r.w - tw + 3.0 * k, r.y + (r.h - 12.5) / 2.0 - 1.0, 12.5, mix(TEXT, ACCENT, ka), false);
            }
            Style::Primary(col) => {
                let bg = if !b.enabled { DISABLED } else { mix(*col, 0xffffff, (40.0 * k) as u32) };
                // the button rises a little on hover, its shadow deepening
                let lift = if b.enabled { 1.5 * k } else { 0.0 };
                let r = Rect { y: r.y - lift, ..r };
                c.rrect_a(r.x + 2.0, r.y + 4.0 + 2.0 * lift, r.w, r.h, 10.0, 0x000000, (110.0 + 40.0 * k) as u32);
                c.rrect(r.x, r.y, r.w, r.h, 10.0, bg);
                // a band of light sweeps across once when the pointer arrives
                let key = format!("shine {:?}", b.action);
                let sweep = if hover { self.tween(&key, 1.0, 0.7) } else { self.tween(&key, 0.0, 0.0) };
                if sweep > 0.0 && sweep < 1.0 {
                    // back from the tween's ease-out to even time, then a gentle ease in and out
                    let lin = 1.0 - (1.0 - sweep).cbrt();
                    c.shine(r, 10.0, smooth(0.0, 1.0, lin));
                }
                let fg = if b.enabled { 0x101215 } else { 0x7b818c };
                let size = self.font_fit(&b.label, (r.h * 0.37).min(22.0), r.w - 24.0);
                let tw = self.width(&b.label, size, true);
                self.text(c, &b.label, r.x + (r.w - tw) / 2.0, r.y + (r.h - size) / 2.0 - 1.0, size, fg, true);
            }
            Style::Secondary => {
                let r = Rect { y: r.y - 1.0 * k, ..r };
                c.rrect_a(r.x, r.y, r.w, r.h, 8.0, mix(0x0c0f14, BTN_HOVER, ka), (190.0 + 45.0 * k) as u32);
                let size = self.font_fit(&b.label, 15.0, r.w - 16.0);
                let tw = self.width(&b.label, size, false);
                self.text(c, &b.label, r.x + (r.w - tw) / 2.0, r.y + (r.h - size) / 2.0 - 1.0, size, if b.enabled { TEXT } else { 0x5b6272 }, false);
            }
            Style::Tab => {
                let col = if b.selected { TEXT } else { mix(DIM, 0xc9ced8, ka) };
                let tw = self.width(&b.label, 16.0, b.selected);
                // the underline is drawn by `draw` (it slides)
                self.text(c, &b.label, r.x + (r.w - tw) / 2.0, r.y + 6.0, 16.0, col, b.selected);
            }
            Style::Chip => {
                let sel = self.tween(&format!("chip {:?}", b.action), if b.selected { 1.0 } else { 0.0 }, HOVER_S);
                let bg = mix(mix(BTN, BTN_HOVER, ka), ACCENT, (sel * 255.0) as u32);
                c.rrect(r.x, r.y, r.w, r.h, 8.0, bg);
                let fg = mix(TEXT, 0x16110a, (sel * 255.0) as u32);
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
                // the knob slides and the track's colour follows it
                let p = self.tween(&format!("switch {:?}", b.action), if *on { 1.0 } else { 0.0 }, HOVER_S + 0.04);
                let track = mix(0x3a414d, ACCENT, (p * 255.0) as u32);
                c.rrect(r.x, r.y, r.w, r.h, r.h / 2.0, mix(track, 0xffffff, (30.0 * k) as u32));
                let d = r.h - 6.0;
                let kx = r.x + 3.0 + (r.w - 6.0 - d) * p;
                c.rrect(kx, r.y + 3.0, d, d, d / 2.0, 0xf6f7f9);
            }
            Style::Arrow(left) => {
                let col = if !b.enabled { 0x3d434e } else { mix(0xb9bfca, TEXT, ka) };
                c.rrect(r.x, r.y, r.w, r.h, 6.0, mix(BTN, BTN_HOVER, ka));
                let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                let d = if *left { 3.0 } else { -3.0 };
                // the chevron nudges the way it steps
                let cx = cx - d / 3.0 * 1.5 * k;
                c.stroke(&[(cx + d, cy - 6.0), (cx - d, cy), (cx + d, cy + 6.0)], 2.0, col);
            }
        }
    }

    /// The last action's note, bottom left of the page.
    fn draw_message(&self, c: &mut Canvas, tx: Tx) {
        if self.message == Message::None {
            return;
        }
        // a new message slides up and fades in
        let appeared = {
            let mut shown = self.shown_msg.borrow_mut();
            if shown.0 != self.message {
                *shown = (self.message.clone(), self.time);
            }
            shown.1
        };
        let k = ((self.time - appeared) / SLIDE_S).clamp(0.0, 1.0);
        let k = 1.0 - (1.0 - k).powi(3);
        let x = SIDE + 24.0;
        let room = self.view.pw - x - 24.0 - if matches!(self.page, Page::Settings) { 0.0 } else { 260.0 };
        // up to two lines; a path too long even for that is cut at its start
        let mut lines = self.wrap(&tx.message(&self.message), 13.0, room - 24.0);
        if lines.len() > 2 || lines.iter().any(|l| self.width(l, 13.0, false) > room - 24.0) {
            lines = vec![self.fit_left(&tx.message(&self.message), 13.0, room - 24.0)];
        }
        let w = lines.iter().map(|l| self.width(l, 13.0, false)).fold(0.0, f32::max);
        let y = PAGE_H - 50.0 - 18.0 * (lines.len() as f32 - 1.0) + 10.0 * (1.0 - k);
        c.rrect_a(x, y, w + 24.0, 32.0 + 18.0 * (lines.len() as f32 - 1.0), 8.0, 0x000000, (190.0 * k) as u32);
        for (i, l) in lines.iter().enumerate() {
            self.text(c, l, x + 12.0, y + 8.0 + 18.0 * i as f32, 13.0, mix(0x000000, TEXT, (k * 255.0) as u32), false);
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

/// 0..1 to a scale that overshoots a little before settling (the badge's pop).
fn back_out(k: f32) -> f32 {
    let c = 1.7;
    let t = k - 1.0;
    1.0 + (c + 1.0) * t * t * t + c * t * t
}

/// How much of the release's archive is downloaded, in percent (none when its size is unknown).
fn download_pct(r: &Release, got: u64) -> Option<u32> {
    let size = r.archive.as_ref().map_or(0, |a| a.size);
    (size > 0).then(|| ((got * 100) / size).min(100) as u32)
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

    /// [`Canvas::fill_fn`] turned by `angle` radians around the box's centre.
    #[allow(clippy::too_many_arguments)]
    fn fill_rot(&mut self, x: f32, y: f32, w: f32, h: f32, inside: fn(f32, f32) -> bool, angle: f32, color: u32) {
        let (x0, y0, s) = (self.ox + x * self.scale, self.oy + y * self.scale, self.scale);
        let (sin, cos) = angle.sin_cos();
        let (px0, py0, px1, py1) = self.span(x, y, w, h);
        for py in py0..py1 {
            for px in px0..px1 {
                let n = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
                    .iter()
                    .filter(|(sx, sy)| {
                        let (u, v) = ((px as f32 + sx - x0) / (w * s) - 0.5, (py as f32 + sy - y0) / (h * s) - 0.5);
                        inside(0.5 + u * cos + v * sin, 0.5 - u * sin + v * cos)
                    })
                    .count() as u32;
                if n > 0 {
                    let i = py * self.w + px;
                    self.buf[i] = mix(self.buf[i], color, n * 255 / 4);
                }
            }
        }
    }

    /// A slanted band of light across the rounded box `r`, at `pos` 0..1 of its way from left to right.
    fn shine(&mut self, r: Rect, radius: f32, pos: f32) {
        let s = self.scale;
        let (x0, y0, x1, y1) = (self.ox + r.x * s, self.oy + r.y * s, self.ox + (r.x + r.w) * s, self.oy + (r.y + r.h) * s);
        let rad = (radius * s).min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
        let band = 30.0 * s;
        let centre = x0 - band * 2.0 + (x1 - x0 + band * 4.0) * pos;
        let (px0, py0, px1, py1) = self.span(r.x, r.y, r.w, r.h);
        for py in py0..py1 {
            for px in px0..px1 {
                let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
                let qx = (cx - (x0 + x1) / 2.0).abs() - ((x1 - x0) / 2.0 - rad);
                let qy = (cy - (y0 + y1) / 2.0).abs() - ((y1 - y0) / 2.0 - rad);
                let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - rad;
                let cover = (0.5 - d).clamp(0.0, 1.0);
                // slanted: the band leans right going up
                let dist = (cx + (cy - y0) * 0.45 - centre).abs() / band;
                let a = cover * (1.0 - dist).max(0.0).powi(2) * 0.6;
                if a > 0.0 {
                    let i = py * self.w + px;
                    self.buf[i] = mix(self.buf[i], 0xffffff, (a * 255.0) as u32);
                }
            }
        }
    }

    /// A circle outline of `radius` and `width` (page units) at opacity `alpha`.
    #[allow(clippy::too_many_arguments)]
    fn ring(&mut self, cx: f32, cy: f32, radius: f32, width: f32, color: u32, alpha: u32) {
        let (pcx, pcy, pr, hw) = (self.ox + cx * self.scale, self.oy + cy * self.scale, radius * self.scale, width * self.scale / 2.0);
        let e = radius + width;
        let (px0, py0, px1, py1) = self.span(cx - e, cy - e, 2.0 * e, 2.0 * e);
        for py in py0..py1 {
            for px in px0..px1 {
                let d = ((px as f32 + 0.5 - pcx).powi(2) + (py as f32 + 0.5 - pcy).powi(2)).sqrt();
                let a = (hw + 0.5 - (d - pr).abs()).clamp(0.0, 1.0);
                if a > 0.0 {
                    let i = py * self.w + px;
                    self.buf[i] = mix(self.buf[i], color, (a * alpha as f32) as u32);
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
            notes: Default::default(),
            translations: vec![],
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
    fn the_notes_card_and_update_badge_fit_the_home_page() {
        let mut a = app();
        a.page = Page::Home;
        let asset = |n: &str| crate::update::Asset { name: n.into(), browser_download_url: String::new(), size: 10 };
        let notes = (0..9).map(|i| Note { fixed: i > 1, title: format!("A rather long note title number {i} that will not fit") }).collect();
        let r = Release {
            tag: "v9.10.11".into(),
            version: (9, 10, 11),
            name: "N".into(),
            url: String::new(),
            archive: Some(asset("a.zip")),
            checksum: Some(asset("a.zip.sha256")),
            notes: [notes, vec![], vec![], vec![], vec![]],
            translations: vec![],
        };
        let states = [UpdateState::Idle, UpdateState::Available(r.clone()), UpdateState::Downloading(r.clone(), 5), UpdateState::Installed(r.clone())];
        for lang in std::iter::once(None).chain(Language::SUPPORTED.into_iter().map(Some)) {
            a.settings.language = lang;
            for (w, h) in [(640, 480), (1000, 620), (1920, 1080), (2560, 1080)] {
                a.resize(w, h);
                for st in &states {
                    a.update = st.clone();
                    let b = a.buttons();
                    for (i, x) in b.iter().enumerate() {
                        assert!(x.rect.x + x.rect.w <= a.view().pw && x.rect.y + x.rect.h <= PAGE_H, "{st:?} {:?}", x.action);
                        assert!(b[i + 1..].iter().all(|y| !x.rect.overlaps(&y.rect)), "{lang:?} {w}x{h} {st:?}: {:?} overlaps", x.action);
                    }
                    // no card only before this version's notes are written (docs/releases)
                    let Some(n) = a.notes_card() else {
                        assert!(a.own_notes[0].is_empty() && *st == UpdateState::Idle);
                        continue;
                    };
                    assert!(!n.overlaps(&a.games_card()) && !n.overlaps(&a.play_rect()) && n.w > 200.0, "{w}x{h}");
                    // the card's buttons sit inside it
                    for x in b.iter().filter(|x| matches!(x.action, Action::ReleaseNotes | Action::InstallUpdate | Action::Restart)) {
                        let r = x.rect;
                        assert!(r.x >= n.x && r.y >= n.y && r.x + r.w <= n.x + n.w && r.y + r.h <= n.y + n.h, "{st:?} {:?}", x.action);
                    }
                    let has = |act: Action| b.iter().any(|x| x.action == act);
                    assert_eq!(has(Action::Updates), *st != UpdateState::Idle, "the header opens the updates while a release waits");
                    assert_eq!(has(Action::InstallUpdate), matches!(st, UpdateState::Available(_)));
                    assert_eq!(a.animating(), matches!(st, UpdateState::Available(_) | UpdateState::Downloading(..)));
                }
            }
        }
        // the pulse: a ring early in each period, a hop after it, still in between
        a.time = 0.6 * PULSE + 0.04 * PULSE;
        assert!(a.hop() < -0.5);
        a.time = 0.9 * PULSE;
        assert_eq!(a.hop(), 0.0);
    }

    #[test]
    fn animations_run_only_while_something_moves() {
        let mut a = app();
        let (w, h) = (800, 500);
        a.resize(w, h);
        let mut buf = vec![0u32; w * h];
        a.draw(&mut buf, w, h);
        assert_eq!(a.frame_interval(), None, "at rest the launcher sleeps");
        // a page change fades and the sidebar highlight slides
        a.time = 1.0;
        a.begin_fade(FadeKind::Page);
        a.page = Page::Settings;
        a.draw(&mut buf, w, h);
        assert_eq!(a.frame_interval(), Some(1.0 / 60.0));
        let y0 = a.anims.borrow()["nav"].value(a.time);
        a.time = 1.1;
        a.draw(&mut buf, w, h);
        let y1 = a.anims.borrow()["nav"].value(a.time);
        assert!(y0 < y1 && y1 < App::nav_rect(3).y, "{y0} {y1}");
        a.time = 2.0;
        a.draw(&mut buf, w, h);
        let rest = buf.clone();
        assert!(a.fade.borrow().is_none() && a.frame_interval().is_none());
        // the finished frame equals one drawn without any animation
        let mut b = app();
        b.resize(w, h);
        b.page = Page::Settings;
        let mut fresh = vec![0u32; w * h];
        b.draw(&mut fresh, w, h);
        assert!(rest == fresh, "after the animations the page looks as if drawn at once");
        // hover eases in
        let r = App::nav_rect(0);
        a.cursor = (r.x + 5.0, r.y + 5.0);
        a.time = 3.0;
        a.draw(&mut buf, w, h);
        a.time = 3.0 + HOVER_S / 2.0;
        a.draw(&mut buf, w, h);
        let k = a.anims.borrow().iter().find(|(k, _)| k.starts_with("hover Nav(Home)")).map(|(_, t)| t.value(a.time)).unwrap();
        assert!(k > 0.3 && k < 1.0, "{k}");
        // the update badge pops in from nothing
        a.update = UpdateState::Available(Release {
            tag: "v9.0.0".into(),
            version: (9, 0, 0),
            name: "N".into(),
            url: String::new(),
            archive: None,
            checksum: None,
            notes: Default::default(),
            translations: vec![],
        });
        a.time = 4.0;
        a.draw(&mut buf, w, h);
        assert!(a.anims.borrow()["badge"].value(a.time) < 0.05);
        a.time = 4.5;
        a.draw(&mut buf, w, h);
        assert_eq!(a.anims.borrow()["badge"].value(a.time), 1.0);
        assert_eq!(a.frame_interval(), Some(1.0 / 30.0), "only the pulse keeps going");
        assert!((back_out(0.0)).abs() < 1e-6 && (back_out(1.0) - 1.0).abs() < 1e-6 && back_out(0.7) > 1.0);
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
        assert_eq!(a.buttons().iter().filter(|b| matches!(b.action, Action::Toggle(Field::Extra(_)))).count(), 9);
        for b in a.buttons() {
            assert!(b.rect.y + b.rect.h <= PAGE_H, "{:?} below the page", b.action);
        }
    }

    #[test]
    fn the_icon_is_a_cut_out() {
        let px = icon_rgba(64);
        let alpha = |x: usize, y: usize| px[(y * 64 + x) * 4 + 3];
        assert_eq!((alpha(0, 0), alpha(63, 0), alpha(0, 63), alpha(63, 63)), (0, 0, 0, 0), "transparent corners");
        assert_eq!(alpha(32, 32), 255, "opaque in the middle");
        assert_eq!(ICON_RGBA.len(), ICON_SIDE * ICON_SIDE * 4);
    }

    #[test]
    fn the_pictures_decode() {
        let a = app();
        assert!(a.art.iter().all(|p| p.as_ref().is_some_and(|p| p.w > 1000 && p.h > 600)));
    }
}
