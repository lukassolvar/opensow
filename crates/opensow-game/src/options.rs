//! The options menu (`CMenuOptions`, `0x44c590`): the original art, controls and sounds. Notes: docs/options.md.

use crate::assets::{load_backdrop, Assets};
use crate::tr::Tr;
use crate::ui::{self, Image};
use opensow_formats::config::{Audio, Options, SettingsFile};
use opensow_formats::effects::Blit;
use opensow_formats::lang::LangFile;
use opensow_formats::sprite::{self, Frame, Kind};
use opensow_viewer::render::pack;

/// Design resolution; frames are placed relative to its centre.
pub const W: usize = 640;
/// Height of the design space.
pub const H: usize = 480;

/// The screen sizes the resolution arrows step through (`0x49ce68`).
/// The exe's table ends at 1920x1080; 2560x1440 (2K), 3840x2160 (4K) and [`NATIVE`] are additions of this port (the game
/// scales to any size).
pub const RESOLUTIONS: [(u32, u32); 8] = [(640, 480), (800, 600), (1024, 768), (1600, 900), (1920, 1080), (2560, 1440), (3840, 2160), NATIVE];

/// The size stored for NATIVE: the game draws at the window's own pixel size (the whole screen when fullscreen). Our
/// first-run default; the exe never writes 0 x 0 (it reads only its own table's sizes).
pub const NATIVE: (u32, u32) = (0, 0);

/// Options of this port (page 2 of the menu). They are not in the exe's settings file (its layout is fixed), so they live in
/// their own small text file next to it: `opensow.cfg`, one `name=0|1` per line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extra {
    /// Run-time texture enhancement of the zoomed-in world (docs/enhance.md).
    pub enhance: bool,
    /// Blurred shadow edges (docs/fx.md).
    pub soft_shadows: bool,
    /// Dark halo where buildings and units meet the ground (docs/fx.md).
    pub contact_shadows: bool,
    /// Explosions and muzzle flashes light the ground (docs/fx.md).
    pub glow: bool,
    /// Every turret casts a shadow, not only the A.A. turret (docs/fx.md); off by default (the original has none for the others).
    pub turret_shadows: bool,
    /// Run-time sound enhancement: sinc resampling, short fades, a limiter on the effects mix (`hifi`, docs/audio.md).
    pub sound: bool,
    /// While a turret is being placed, the cells its gun reaches are tinted (not in the original; docs/hotkeys.md).
    pub turret_range: bool,
    /// The mouse wheel zooms the mission map (not in the original, which has one fixed scale; docs/game.md).
    pub zoom: bool,
}

impl Default for Extra {
    fn default() -> Self {
        Extra { enhance: true, soft_shadows: true, contact_shadows: true, glow: true, turret_shadows: false, sound: true, turret_range: true, zoom: true }
    }
}

/// The port's own settings file (current directory, like the exe's).
pub const EXTRA_FILE: &str = "opensow.cfg";

impl Extra {
    /// Reads this port's extra settings from the text of their file.
    pub fn parse(text: &str) -> Extra {
        let mut e = Extra::default();
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let on = v.trim() != "0";
                match k.trim() {
                    "enhance_textures" => e.enhance = on,
                    "soft_shadows" => e.soft_shadows = on,
                    "contact_shadows" => e.contact_shadows = on,
                    "explosion_glow" => e.glow = on,
                    "turret_shadows" => e.turret_shadows = on,
                    "enhance_sound" => e.sound = on,
                    "turret_range" => e.turret_range = on,
                    "map_zoom" => e.zoom = on,
                    _ => {}
                }
            }
        }
        e
    }

    /// The extra settings as the text of their file.
    pub fn write(&self) -> String {
        format!(
            "enhance_textures={}\nsoft_shadows={}\ncontact_shadows={}\nexplosion_glow={}\nturret_shadows={}\nenhance_sound={}\nturret_range={}\nmap_zoom={}\n",
            self.enhance as u8,
            self.soft_shadows as u8,
            self.contact_shadows as u8,
            self.glow as u8,
            self.turret_shadows as u8,
            self.sound as u8,
            self.turret_range as u8,
            self.zoom as u8
        )
    }
}

/// Everything the menu edits: the settings file (`COptions`) and the audio object's volumes and switches, plus this port's [`Extra`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameOptions {
    /// The original's settings record.
    pub cfg: Options,
    /// This port's own settings.
    pub extra: Extra,
    /// Music volume, 0..=100.
    pub music_volume: i32,
    /// Speech volume, 0..=100.
    pub voice_volume: i32,
    /// Sound effect volume, 0..=100.
    pub fx_volume: i32,
    /// Music switch.
    pub music_on: bool,
    /// Speech switch.
    pub voice_on: bool,
    /// Sound effect switch.
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
    /// The options as the settings file holds them (this port's extras start at their defaults).
    pub fn from_file(f: &SettingsFile) -> GameOptions {
        let a = &f.audio;
        GameOptions {
            cfg: f.options.clone(),
            extra: Extra::default(),
            music_volume: a.music_volume as i32,
            voice_volume: a.voice_volume as i32,
            fx_volume: a.fx_volume as i32,
            music_on: a.music_on,
            voice_on: a.voice_on,
            fx_on: a.fx_on,
        }
    }

    /// The settings file to write for these options.
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

    /// `0x452c30`: the file's values, or the first-run ones when it is missing or unreadable, except that the resolution
    /// starts at [`NATIVE`] (the exe's 1024 x 768 would leave most of a modern screen to the scaler). Tests never touch the disk.
    pub fn load() -> GameOptions {
        if cfg!(test) {
            return GameOptions::default();
        }
        let mut o = std::fs::read(FILE).ok().and_then(|d| SettingsFile::parse(&d).ok()).map_or_else(GameOptions::first_run, |f| GameOptions::from_file(&f));
        o.extra = std::fs::read_to_string(EXTRA_FILE).map(|t| Extra::parse(&t)).unwrap_or_default();
        o
    }

    /// The exe's first-run values with the resolution at [`NATIVE`].
    pub fn first_run() -> GameOptions {
        let mut o = GameOptions::default();
        (o.cfg.width, o.cfg.height) = NATIVE;
        o
    }

    /// The stored resolution is [`NATIVE`].
    pub fn native(&self) -> bool {
        (self.cfg.width, self.cfg.height) == NATIVE
    }

    /// `0x453310` (O.K. in the options menu): writes the file; a write error is ignored like a read-only folder would be.
    pub fn save(&self) {
        if !cfg!(test) {
            let _ = std::fs::write(FILE, self.to_file().write());
            let _ = std::fs::write(EXTRA_FILE, self.extra.write());
        }
    }
}

/// Map scroll in pixels per second for a KEYS / MOUSE SCROLL setting (0..=10): the logic step scrolls `ticks * speed`
/// and `0x457f80` doubles it, 60 ticks per second (`0x415ef1`, `0x416b8b`).
pub fn scroll_px_per_second(speed: u32) -> f32 {
    2.0 * 60.0 * speed as f32 * SCROLL_BOOST
}

/// Our own addition: the camera moves this much faster than the exe's formula (the original's defaults, KEYS 5 and MOUSE 3, feel
/// slow on today's large windows). The Options values keep their meaning (0..=10); 1.0 is the original speed.
pub const SCROLL_BOOST: f32 = 1.5;

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
    /// The arrow that switches to page `n` (0 = the exe's page, 1 = this port's options).
    Page(usize),
    /// Row `k` of page 2 ([`OWN_ROWS`]): its label (clicking it toggles, like the exe's labels).
    Extra(usize),
    /// The green arrow left (`false`) or right (`true`) of row `k`'s value on page 2; either one toggles the switch.
    ExtraArrow(usize, bool),
}

/// Pages of the menu: the exe's own, then the port's (`Hit::Page`).
pub const PAGES: usize = 2;

/// Rows of page 2: (label, one-line explanation), English ([`Tr::option_rows`] has the other languages). Row `k`'s label sits at
/// [`own_rect`]; its value (On / Off) between two green arrows starting at [`OWN_ARROWS_X`], like the exe's arrow rows.
pub const OWN_ROWS: [(&str, &str); 8] = [
    ("Enhanced textures", "Smooths sprites and the ground when you zoom in"),
    ("Soft shadows", "Blurs the edges of shadows"),
    ("Contact shadows", "Dark halo where buildings and units meet the ground"),
    ("Explosion glow", "Explosions and muzzle flashes light the ground"),
    ("Turret shadows", "Every turret casts a shadow (original: only A.A.)"),
    ("Enhanced sound", "Cleaner, smoother effects, speech and music"),
    ("Turret range", "Shows how far a turret reaches while you place it"),
    ("Map zoom", "Zoom with the mouse wheel (original: fixed scale)"),
];
/// Left edge of the left arrow of every page-2 row (design space).
const OWN_ARROWS_X: i32 = 236;
/// Width of the value slot between the two arrows.
const OWN_VALUE_W: i32 = 60;

/// Click area of row `k`'s label on page 2 (design space).
fn own_rect(k: usize) -> ui::Rect {
    ui::Rect { x: 62, y: 118 + 36 * k as i32, w: OWN_ARROWS_X - 62 - 6, h: 20 }
}

/// Click area of the page arrow that leads to page `to` (a right arrow at the right edge going forward, a left one at the left edge back).
fn page_rect(to: usize) -> ui::Rect {
    if to == 1 {
        ui::Rect { x: 596, y: 227, w: 32, h: 28 }
    } else {
        ui::Rect { x: 12, y: 227, w: 32, h: 28 }
    }
}

/// What a click on the menu asks the game to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    None,
    /// O.K.: keep these options.
    Done(GameOptions),
    /// CANCEL: the settings file is reloaded (`0x452c30` reads the audio object from it too), so everything is back.
    Cancel(GameOptions),
}

/// The options screen: the values being edited, the art and the turning cogs.
pub struct OptionsMenu {
    /// The values as edited so far.
    pub opts: GameOptions,
    /// What the menu was opened with (CANCEL goes back to the file's values; the audio object is part of the file).
    saved: GameOptions,
    bg: Option<Image>,
    bg_full: Option<Image>,
    frames: Vec<Frame>,
    digits: Vec<Frame>,
    /// `extras\kruh`, the cog frames.
    gear_frames: Vec<Frame>,
    /// The state of the turning cogs.
    pub gears: Gears,
    /// Real time of the last drawn frame (the cogs advance 60 ticks per second).
    clock: Option<std::time::Instant>,
    tick_rest: f64,
    strings: Option<LangFile>,
    fonts: std::rc::Rc<(opensow_formats::font::Font, opensow_formats::font::Font)>,
    /// Unicode text for languages other than English (the bitmap fonts only have ASCII, the exe draws GDI Times New Roman Bold).
    ugdi: Option<crate::gdi::Gdi>,
    ubig: Option<crate::gdi::Gdi>,
    hover: Option<Hit>,
    /// This port's own texts (page 2).
    tr: Tr,
    /// Which page is shown (0 = the exe's controls, 1 = this port's).
    pub page: usize,
    /// Mouse position in the 640x480 design space.
    cursor: (i32, i32),
    canvas: Vec<u32>,
    /// Effect ids for the game to play: 30 hover, 33 arrow, 34 switch, 38 O.K., 39 CANCEL.
    pub sounds: Vec<u8>,
}

fn load(assets: &Assets, name: &str) -> Vec<Frame> {
    assets
        .game
        .resolve(&format!("Data\\Sprites2\\Menu\\{name}.tsp"))
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|d| sprite::parse(&d, Kind::Color).ok())
        .unwrap_or_default()
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
    /// Opens the menu on the given options.
    pub fn new(assets: &mut Assets, opts: GameOptions) -> OptionsMenu {
        let language = assets.game.language();
        let strings = assets
            .game
            .resolve(&format!("Data\\Language\\{}", language.file_name()))
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| LangFile::parse(&d).ok())
            .map(|f| f.with_supplement(language));
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
            ugdi: crate::gdi::Gdi::for_language(language, 17.0),
            ubig: crate::gdi::Gdi::for_language(language, 30.0),
            hover: None,
            tr: Tr::new(language),
            page: 0,
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

    /// Click area of a page-2 arrow: the exe's arrow art (frames 13 / 14, left / right) placed on row `k`.
    fn own_arrow_rect(&self, k: usize, right: bool) -> ui::Rect {
        let size = |f: usize| self.frames.get(ARROW0 + f).map_or((22, 20), |f| (f.width as i32, f.height as i32));
        let (lw, _) = size(0);
        let (w, h) = size(right as usize);
        let row = own_rect(k);
        let x = if right { OWN_ARROWS_X + lw + OWN_VALUE_W } else { OWN_ARROWS_X };
        ui::Rect { x, y: row.y + (row.h - h) / 2, w, h }
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
        let to = if self.page == 0 { 1 } else { 0 };
        if page_rect(to).contains(p.0, p.1) {
            return Some(Hit::Page(to));
        }
        if self.page == 1 {
            let arrow = (0..OWN_ROWS.len())
                .flat_map(|k| [false, true].map(|right| (k, right)))
                .find(|&(k, right)| self.own_arrow_rect(k, right).contains(p.0, p.1));
            if let Some((k, right)) = arrow {
                return Some(Hit::ExtraArrow(k, right));
            }
            return (0..OWN_ROWS.len()).find(|&k| own_rect(k).contains(p.0, p.1)).map(Hit::Extra);
        }
        if let Some(k) = (0..14).find(|&k| inside(ARROW0 + k)) {
            return Some(Hit::Arrow(k));
        }
        (0..11).find(|&k| inside(LABEL0 + k)).map(Hit::Label)
    }

    /// Design-space point at the centre of a control (for scripted clicks).
    pub fn centre(&self, hit: Hit) -> (i32, i32) {
        let own = match hit {
            Hit::Page(to) => Some(page_rect(to)),
            Hit::Extra(k) => Some(own_rect(k)),
            Hit::ExtraArrow(k, right) => Some(self.own_arrow_rect(k, right)),
            _ => None,
        };
        if let Some(r) = own {
            return (r.x + r.w / 2, r.y + r.h / 2);
        }
        let frame = match hit {
            Hit::Ok => 1,
            Hit::Cancel => 0,
            Hit::Arrow(k) => ARROW0 + k,
            Hit::Label(k) => LABEL0 + k,
            Hit::Page(_) | Hit::Extra(_) | Hit::ExtraArrow(..) => unreachable!(),
        };
        self.rect(frame).map_or((0, 0), |r| (r.x + r.w / 2, r.y + r.h / 2))
    }

    /// Updates what the pointer is over.
    pub fn mouse_moved(&mut self, x: i32, y: i32, w: usize, h: usize) {
        let (ox, oy, s) = fit(w, h);
        self.cursor = (((x as f32 - ox) / s) as i32, ((y as f32 - oy) / s) as i32);
        let hover = self.hit_at(self.cursor);
        if hover.is_some() && hover != self.hover {
            self.sounds.push(30); // mainmenu-move
        }
        self.hover = hover;
    }

    /// A click at a window position.
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
            Hit::Page(to) => {
                self.sounds.push(33); // options-arrow
                self.page = to.min(PAGES - 1);
            }
            Hit::Extra(k) | Hit::ExtraArrow(k, _) => {
                // the arrows sound like the exe's arrows, the label like its labels
                self.sounds.push(if matches!(hit, Hit::Extra(_)) { 34 } else { 33 });
                match k {
                    0 => o.extra.enhance ^= true,
                    1 => o.extra.soft_shadows ^= true,
                    2 => o.extra.contact_shadows ^= true,
                    3 => o.extra.glow ^= true,
                    4 => o.extra.turret_shadows ^= true,
                    5 => o.extra.sound ^= true,
                    6 => o.extra.turret_range ^= true,
                    7 => o.extra.zoom ^= true,
                    _ => {}
                }
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
                            let j = if up { (i + 1).min(RESOLUTIONS.len() - 1) } else { i.saturating_sub(1) };
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
        if let Some(g) = &self.ugdi {
            g.draw_shadowed(canvas, W, H, s, x as f32, y as f32 - 3.0, 1.0, fg, (0, 0, 0));
            return;
        }
        ui::text(canvas, W, H, &self.fonts, s, x, y, 1.0, fg, (0, 30, 0));
    }

    fn text_w(&self, s: &str) -> i32 {
        self.ugdi.as_ref().map_or_else(|| ui::text_width(&self.fonts, s, 1.0), |g| g.width(s))
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

    /// The picture has the exe's arrow slots baked in; page 2 has no such arrows, so each slot's rectangle is filled from its
    /// surroundings (bilinear between the pixels just outside it).
    fn wipe_arrow_slots(&self, canvas: &mut [u32]) {
        let at = |c: &[u32], x: i32, y: i32| c[(y.clamp(0, H as i32 - 1) as usize) * W + x.clamp(0, W as i32 - 1) as usize];
        for k in 0..14 {
            let Some(r) = self.rect(ARROW0 + k) else { continue };
            let (x0, y0, x1, y1) = (r.x - 2, r.y - 2, r.x + r.w + 2, r.y + r.h + 2);
            let copy = canvas.to_vec();
            for y in y0.max(0)..y1.min(H as i32) {
                for x in x0.max(0)..x1.min(W as i32) {
                    let (fy, fx) = ((y - y0) as f32 / (y1 - y0) as f32, (x - x0) as f32 / (x1 - x0) as f32);
                    let lerp = |a: u32, b: u32, t: f32, sh: u32| (((a >> sh & 255) as f32 * (1.0 - t) + (b >> sh & 255) as f32 * t) as u32) << sh;
                    let (top, bottom) = (at(&copy, x, y0 - 1), at(&copy, x, y1));
                    let (left, right) = (at(&copy, x0 - 1, y), at(&copy, x1, y));
                    let mix = |sh: u32| ((lerp(top, bottom, fy, sh) + lerp(left, right, fx, sh)) / 2) & (255 << sh);
                    canvas[y as usize * W + x as usize] = mix(16) | mix(8) | mix(0);
                }
            }
        }
    }

    /// Page 2: this port's options (the same picture and cogs). Each [`OWN_ROWS`] row: the label, then its value (On / Off)
    /// between the exe's green left / right arrows, like the page-1 rows with arrows; a dimmer explanation under it.
    fn render_extra(&self, canvas: &mut [u32]) {
        let t = self.string(4).trim().to_owned();
        let t = if t.is_empty() { "Options".to_owned() } else { t };
        let tw = self.text_w(&t);
        self.text(canvas, &t, W as i32 / 2 - tw / 2 - 0x2a, 10, true);
        self.text(canvas, self.tr.enhancements(), 21, 62, true);
        let lh = ui::text_height(&self.fonts, 1.0);
        let e = &self.opts.extra;
        let values = [e.enhance, e.soft_shadows, e.contact_shadows, e.glow, e.turret_shadows, e.sound, e.turret_range, e.zoom];
        for (k, (label, hint)) in self.tr.option_rows().iter().enumerate() {
            let r = own_rect(k);
            let hot = matches!(self.hover, Some(Hit::Extra(j) | Hit::ExtraArrow(j, _)) if j == k);
            self.text(canvas, label, r.x, r.y + (r.h - lh) / 2, hot);
            for right in [false, true] {
                let lit = self.hover == Some(Hit::ExtraArrow(k, right));
                let frame = ARROW0 + right as usize + if lit { 14 } else { 0 };
                if let Some(f) = self.frames.get(frame) {
                    let a = self.own_arrow_rect(k, right);
                    ui::sprite(canvas, W, H, f, a.x, a.y, 1.0);
                }
            }
            let word = self.string(if values[k] { 267 } else { 268 }).trim().to_owned();
            let word = if word.is_empty() { (if values[k] { "On" } else { "Off" }).to_owned() } else { word };
            let left = self.own_arrow_rect(k, false);
            let x = left.x + left.w + (OWN_VALUE_W - self.text_w(&word)) / 2;
            self.text(canvas, &word, x, r.y + (r.h - lh) / 2, hot);
            self.hint(canvas, hint, r.x, r.y + r.h + 1);
        }
    }

    /// A row's explanation: darker than the labels so the rows stand apart.
    fn hint(&self, canvas: &mut [u32], s: &str, x: i32, y: i32) {
        let fg = (120, 140, 120);
        if let Some(g) = &self.ugdi {
            g.draw_shadowed(canvas, W, H, s, x as f32, y as f32 - 3.0, 1.0, fg, (0, 0, 0));
            return;
        }
        ui::text(canvas, W, H, &self.fonts, s, x, y, 1.0, fg, (0, 30, 0));
    }

    /// The arrow that leads to the other page, and the page number at the top right.
    fn render_page_arrow(&self, canvas: &mut [u32]) {
        let to = if self.page == 0 { 1 } else { 0 };
        let r = page_rect(to);
        // a green triangle in the exe's arrow colours, drawn here (the exe's arrow art comes with its own slot)
        let hot = self.hover == Some(Hit::Page(to));
        let (fill, edge) = if hot { ((140, 255, 150), (20, 120, 40)) } else { ((40, 220, 70), (10, 80, 25)) };
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        let dir = if to == 1 { 1.0f32 } else { -1.0 };
        for dy in -12i32..=12 {
            for dx in -12i32..=12 {
                // distance from the base line towards the apex, 0..=20; half-height shrinks linearly to 0 at the apex
                let t = (dx as f32 * dir + 10.0) / 20.0;
                let half = 11.0 * (1.0 - t);
                let inside = |shrink: f32| (0.0..=1.0).contains(&t) && (dy as f32).abs() <= half - shrink && t >= shrink / 20.0 && t <= 1.0 - shrink / 11.0;
                if inside(0.0) {
                    let c = if inside(2.0) { fill } else { edge };
                    let (x, y) = (cx + dx, cy + dy);
                    if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
                        canvas[y as usize * W + x as usize] = pack(c.0, c.1, c.2);
                    }
                }
            }
        }
        self.text(canvas, &format!("{}/{}", self.page + 1, PAGES), W as i32 - 56, 10, false);
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
        if self.page == 1 {
            self.wipe_arrow_slots(&mut canvas);
        }
        // the two turning cogs: both shadows, then both cogs (`0x44afea..0x44b093`), under every button
        let (f1, f2) = self.gears.frames();
        let (cx, cy) = (W as i32 / 2, H as i32 / 2);
        let place = |frame: Option<&Frame>, (dx, dy): (i32, i32)| frame.map(|f| (f.clone(), cx + dx - f.hot_x as i32, cy + dy - f.hot_y as i32));
        let cogs: Vec<(Frame, i32, i32, Blit)> =
            [(f1, GEAR1.0, Blit::Shade), (f2, GEAR2.0, Blit::Shade), (f1, GEAR1.1, Blit::Additive), (f2, GEAR2.1, Blit::Additive)]
                .into_iter()
                .filter_map(|(k, at, mode)| place(self.gear_frames.get(k), at).map(|(f, x, y)| (f, x, y, mode)))
                .collect();
        for (f, x, y, mode) in &cogs {
            blit(&mut canvas, f, *x, *y, *mode);
        }
        if self.page == 1 {
            self.render_extra(&mut canvas);
        } else {
            let on = |k: usize| self.hover == Some(Hit::Label(k));
            // title and headings
            let t = self.string(4).to_owned();
            let tw = self.text_w(&t);
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
                let cap = if o.native() { self.tr.native().to_owned() } else { format!("{}x{}", o.cfg.width, o.cfg.height) };
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
            for (x, k, v) in [
                (255, 0, o.cfg.detail as i32),
                (550, 2, o.fx_volume),
                (550, 4, o.voice_volume),
                (550, 6, o.music_volume),
                (284, 8, o.cfg.scroll_b as i32),
                (284, 10, o.cfg.scroll_a as i32),
            ] {
                self.number(&mut canvas, x, row_y(k), v);
            }
        }
        self.render_page_arrow(&mut canvas);
        for (i, hit) in [Hit::Cancel, Hit::Ok].into_iter().enumerate() {
            if let Some(f) = self.frames.get(i) {
                let (x, y) = Self::top_left(f);
                // the art carries "O.K." / "CANCEL"; other languages get their string (file 21 / 22) in the same place and colour
                match &self.ubig {
                    Some(g) => {
                        let t = self.string(if hit == Hit::Ok { 21 } else { 22 }).to_uppercase();
                        let (tx, ty) = (x + (f.width as i32 - g.width(&t)) / 2, y + (f.height as i32 - 30) / 2);
                        g.draw_shadowed(&mut canvas, W, H, &t, tx as f32, ty as f32, 1.0, (232, 96, 56), (30, 8, 0));
                    }
                    None => ui::sprite(&mut canvas, W, H, f, x, y, 1.0),
                }
            }
        }
        self.canvas = canvas;
    }

    /// Draws the menu scaled into the `w` x `h` frame buffer.
    pub fn draw(&mut self, buf: &mut [u32], w: usize, h: usize) {
        let now = std::time::Instant::now();
        if let Some(last) = self.clock.replace(now) {
            self.tick_rest += now.duration_since(last).as_secs_f64().min(0.25) * 60.0;
            let ticks = self.tick_rest as u32;
            self.tick_rest -= ticks as f64;
            self.gears.step(ticks);
        }
        opensow_viewer::record::cpu(|| self.render());
        ui::clear(buf, w, h, 0);
        let (ox, oy, s) = fit(w, h);
        // the larger background fills what the 640x480 picture does not cover (`0x44ade0`)
        if let Some(bg) = &self.bg_full {
            ui::stretch(buf, w, h, bg, true);
        }
        crate::ui::page(buf, w, h, &self.canvas, W, H, ox as usize, oy as usize, s);
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
            ugdi: None,
            ubig: None,
            hover: None,
            tr: Tr::new(opensow_formats::lang::Language::English),
            page: 0,
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
        for _ in 0..4 {
            m.press(Hit::Arrow(13));
        }
        assert_eq!((m.opts.cfg.width, m.opts.cfg.height), (3840, 2160));
        m.press(Hit::Arrow(13));
        assert!(m.opts.native(), "NATIVE comes after 4K");
        m.press(Hit::Arrow(13));
        assert!(m.opts.native(), "last entry stays");
        m.press(Hit::Arrow(12));
        m.press(Hit::Arrow(12));
        assert_eq!((m.opts.cfg.width, m.opts.cfg.height), (2560, 1440));
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
    fn page_arrow_switches_pages_and_row_toggles_enhancement() {
        let mut m = menu(GameOptions::default());
        assert_eq!(m.page, 0);
        assert!(m.opts.extra.enhance, "enhanced textures start on");
        // page 1: the arrow on the right leads on, and its area is found by the pointer
        let c = m.centre(Hit::Page(1));
        assert_eq!(m.hit_at(c), Some(Hit::Page(1)));
        assert_eq!(m.hit_at(m.centre(Hit::Extra(0))), None, "page 2 rows are not clickable on page 1");
        m.press(Hit::Page(1));
        assert_eq!(m.page, 1);
        assert_eq!(m.hit_at(m.centre(Hit::Extra(0))), Some(Hit::Extra(0)));
        assert_eq!(m.hit_at(m.centre(Hit::Page(0))), Some(Hit::Page(0)), "the way back is on the left");
        m.press(Hit::Extra(0));
        assert!(!m.opts.extra.enhance);
        // the green arrows either side of the value toggle it too, with the arrow sound
        for right in [false, true] {
            let c = m.centre(Hit::ExtraArrow(4, right));
            assert_eq!(m.hit_at(c), Some(Hit::ExtraArrow(4, right)));
        }
        let shadows = m.opts.extra.turret_shadows;
        m.sounds.clear();
        m.press(Hit::ExtraArrow(4, true));
        assert_eq!(m.opts.extra.turret_shadows, !shadows);
        m.press(Hit::ExtraArrow(4, false));
        assert_eq!(m.opts.extra.turret_shadows, shadows);
        assert_eq!(m.sounds, [33, 33]);
        // CANCEL puts the file's values back, this port's too
        let Event::Cancel(c) = m.press(Hit::Cancel) else { panic!() };
        assert!(c.extra.enhance);
        m.press(Hit::Page(0));
        assert_eq!(m.page, 0);
    }

    #[test]
    fn own_settings_file_round_trips() {
        for on in [true, false] {
            let e = Extra { enhance: on, soft_shadows: !on, contact_shadows: on, glow: !on, turret_shadows: on, sound: !on, turret_range: on, zoom: !on };
            assert_eq!(Extra::parse(&e.write()), e);
        }
        assert_eq!(Extra::parse(""), Extra::default(), "a missing file keeps the defaults");
        assert_eq!(Extra::parse("enhance_textures=0\nfuture=1\n"), Extra { enhance: false, ..Extra::default() });
        assert_eq!(Extra::parse("enhance_sound=0\n"), Extra { sound: false, ..Extra::default() });
        assert!(Extra::default().sound, "enhanced sound starts on");
        assert!(Extra::default().turret_range, "the turret range starts on");
        assert_eq!(Extra::parse("turret_range=0\n"), Extra { turret_range: false, ..Extra::default() });
        assert!(Extra::default().zoom, "map zoom starts on");
        assert_eq!(Extra::parse("map_zoom=0\n"), Extra { zoom: false, ..Extra::default() });
    }

    #[test]
    fn scroll_speed_is_two_pixels_per_tick_and_setting() {
        // the exe's speed (2 px per tick and setting, 60 ticks/s) times our SCROLL_BOOST
        assert_eq!(scroll_px_per_second(0), 0.0);
        assert_eq!(scroll_px_per_second(5), 600.0 * SCROLL_BOOST, "keys default");
        assert_eq!(scroll_px_per_second(3), 360.0 * SCROLL_BOOST, "mouse default");
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
        assert_eq!(g.frames(), (8, 15));
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
