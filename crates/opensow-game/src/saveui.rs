//! The load and save pages (`CMenuLoadSave`, object `0x7104d0`): ten numbered rows with title and date, a 160x120 screenshot,
//! a LOAD / SAVE button and CANCEL. Notes: docs/saveload.md.
//!
//! One class serves both pages (mode byte `[+0x10]`: 1 = load, 0 = save). Rows are slots 0..=9; the file container is the
//! original's (`savegame_<stem>.data` / `.info`, `opensow_formats::save`), but the `.data` stream only carries the level
//! (the object records of the real state are not decoded far enough to write), so loading restarts that mission.

use crate::assets::{load_backdrop, Assets};
use crate::ui::{self, Rect};
use opensow_formats::lang::{self, LangFile};
use opensow_formats::save::{self, SaveInfo, StateHead};
use opensow_formats::sprite::{self, Frame, Kind};
use opensow_viewer::render::pack;
use std::path::PathBuf;

pub const W: usize = 640;
pub const H: usize = 480;
const ROWS: usize = save::LIST_ROWS;
/// Widest title in pixels (`0x444a47`: the typed text must stay under 0xc3).
const MAX_TITLE_PX: i32 = 0xc3;
/// Text colours (`0x46c346` normal, `0x5afc5a` hovered/selected, COLORREF = 0xBBGGRR) and the button label colours.
const GREEN: (u8, u8, u8) = (0x46, 0xc3, 0x46);
const GREEN_HI: (u8, u8, u8) = (0x5a, 0xfc, 0x5a);
const ORANGE: (u8, u8, u8) = (0xde, 0x71, 0x4a);
const ORANGE_DIM: (u8, u8, u8) = (0xa6, 0x54, 0x37);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Load,
    Save,
}

/// Where the savegame files live. The original uses `<install>\Savegame`; we never write into the install, so saves go to
/// `write`, and `also` (the install's own folder) is only read.
#[derive(Debug, Clone)]
pub struct Store {
    pub write: PathBuf,
    pub also: Option<PathBuf>,
}

impl Store {
    /// `$OPENSOW_SAVE_DIR`, else the per-user data folder; the install's `Savegame` is read as a fallback.
    pub fn locate(game: &opensow_formats::GameDir) -> Store {
        let write = std::env::var_os("OPENSOW_SAVE_DIR").map(PathBuf::from).unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            if cfg!(target_os = "macos") {
                home.join("Library/Application Support/OpenSOW/Savegame")
            } else {
                home.join(".local/share/opensow/Savegame")
            }
        });
        Store { write, also: game.resolve("Savegame").filter(|p| p.is_dir()) }
    }

    fn name(slot: usize, ext: &str) -> String {
        format!("savegame_{}.{ext}", save::slot_stem(slot))
    }

    fn read(&self, slot: usize, ext: &str) -> Option<Vec<u8>> {
        let n = Self::name(slot, ext);
        std::fs::read(self.write.join(&n)).ok().or_else(|| {
            let dir = self.also.as_ref()?;
            let want = n.to_ascii_lowercase();
            let hit = std::fs::read_dir(dir).ok()?.flatten().find(|e| e.file_name().to_string_lossy().to_ascii_lowercase() == want)?;
            std::fs::read(hit.path()).ok()
        })
    }

    /// What the list shows for a slot (`0x443ca0`).
    pub fn slot(&self, slot: usize, text: &Texts) -> Slot {
        let mut s = Slot { valid: false, title: text.empty.clone(), time: text.no_time.clone(), date: text.no_date.clone() };
        let Some(info_bytes) = self.read(slot, "info") else { return s };
        let sum = self.read(slot, "data").map_or(0, |d| save::data_checksum(&d));
        match SaveInfo::parse(&info_bytes) {
            Ok(info) => {
                s.time = info.time.clone();
                s.date = info.date.clone();
                if info.stamp == sum {
                    s.valid = true;
                    s.title = info.name;
                } else {
                    s.title = text.damaged.clone();
                }
            }
            Err(_) => s.title = text.damaged.clone(),
        }
        s
    }

    /// The screenshot of a slot (`0x4440d0`), RGB565; `None` when the `.info` cannot be read.
    pub fn thumbnail(&self, slot: usize) -> Option<Vec<u16>> {
        let info = SaveInfo::parse(&self.read(slot, "info")?).ok()?;
        Some(info.thumbnail.iter().map(|&p| save::thumb_to_565(p, info.extra)).collect())
    }

    /// The level of a slot and the game snapshot it carries (empty for a save of the original game or an old one of ours: the
    /// level restarts then); `None` when the file is missing or not a savegame.
    pub fn level_of(&self, slot: usize) -> Option<(String, Vec<u8>)> {
        let data = save::huff_decode(&self.read(slot, "data")?).ok()?;
        Some((level_from_state(&data)?, snapshot_from_state(&data)))
    }

    /// Writes `.data` then `.info` (the exe's order, `0x428bb0`).
    pub fn write(&self, slot: usize, title: &str, level: &str, snapshot: &[u8], thumb: &[u16]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.write)?;
        let data = save::huff_encode(&state_bytes(level, snapshot));
        std::fs::write(self.write.join(Self::name(slot, "data")), &data)?;
        let (time, date, weekday) = clock_strings(std::time::SystemTime::now());
        let info = SaveInfo { stamp: save::data_checksum(&data), name: title.to_owned(), time, date, weekday, extra: save::PIXEL_565, thumbnail: thumb.to_vec() };
        std::fs::write(self.write.join(Self::name(slot, "info")), info.to_bytes())
    }
}

/// The decompressed state stream we write: the real head (`0x42a400`: version words and the level number, the other scalars 0)
/// followed by our own marker and the level's name, which the head cannot hold (skirmish maps have no number).
pub fn state_bytes(level: &str, snapshot: &[u8]) -> Vec<u8> {
    let number = level.trim_start_matches(|c: char| !c.is_ascii_digit()).parse::<u32>().unwrap_or(0);
    let head = StateHead { level: number, rng_index: 0, v_5baafc: 0, flag_49a339: false, v_5baad8: 0, v_5baadc: 0, v_5baac4: 0, v_5baac8: 0, flag_5baa99: false, v_89cf50: 0, len: 0 };
    let mut o = head.to_bytes();
    save::put_mfc_string(&mut o, if snapshot.is_empty() { "OpenSOW1" } else { "OpenSOW2" });
    save::put_mfc_string(&mut o, level);
    if !snapshot.is_empty() {
        o.extend_from_slice(&(snapshot.len() as u32).to_le_bytes());
        o.extend_from_slice(snapshot);
    }
    o
}

/// The game snapshot after the level name of one of our states (`OpenSOW2`); empty for anything else.
pub fn snapshot_from_state(data: &[u8]) -> Vec<u8> {
    let Ok(head) = StateHead::parse(data) else { return Vec::new() };
    let Some(rest) = data.get(head.len..) else { return Vec::new() };
    let mut r = opensow_formats::Reader::new(rest);
    let (Ok(tag), Ok(_)) = (opensow_formats::saveobj::mfc_string(&mut r, "tag"), opensow_formats::saveobj::mfc_string(&mut r, "level")) else { return Vec::new() };
    if tag != "OpenSOW2" {
        return Vec::new();
    }
    let at = rest.len() - r.remaining();
    match rest.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize) {
        Some(n) => rest.get(at + 4..at + 4 + n).map(<[u8]>::to_vec).unwrap_or_default(),
        None => Vec::new(),
    }
}

/// The level name out of a state stream: ours has it after the marker; a genuine one only has the number (`M01`...).
pub fn level_from_state(data: &[u8]) -> Option<String> {
    let head = StateHead::parse(data).ok()?;
    let mut r = opensow_formats::Reader::new(data.get(head.len..)?);
    if let (Ok(tag), Ok(name)) = (opensow_formats::saveobj::mfc_string(&mut r, "tag"), opensow_formats::saveobj::mfc_string(&mut r, "level")) {
        if (tag == "OpenSOW1" || tag == "OpenSOW2") && !name.is_empty() {
            return Some(name);
        }
    }
    (head.level > 0).then(|| format!("M{:02}", head.level))
}

/// `"%X"`, `"%b %d, %Y"` and `"%a %H:%M"` of the exe's info writer (`0x42be20`), in UTC (the exe uses the local time zone).
pub fn clock_strings(t: std::time::SystemTime) -> (String, String, String) {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    civil(secs)
}

fn civil(secs: i64) -> (String, String, String) {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    // days since 1970-01-01 -> civil date (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let wd = WEEKDAYS[days.rem_euclid(7) as usize];
    (format!("{h:02}:{m:02}:{s:02}"), format!("{} {day:02}, {year}", MONTHS[month as usize - 1]), format!("{wd} {h:02}:{m:02}"))
}

/// The language strings the page uses (exe index = file index + 1).
#[derive(Debug, Clone, Default)]
pub struct Texts {
    pub load_title: String,
    pub save_title: String,
    pub cancel: String,
    pub load: String,
    pub save: String,
    pub quick_save: String,
    pub empty: String,
    pub no_time: String,
    pub no_date: String,
    pub damaged: String,
}

impl Texts {
    fn from(strings: Option<&LangFile>) -> Texts {
        let get = |i: usize, fallback: &str| strings.and_then(|s| s.get(i)).map_or(fallback, |s| s).to_owned();
        Texts {
            load_title: get(1, "Load Game"),
            save_title: get(363, "Save Game"),
            cancel: get(22, "Cancel"),
            load: get(359, "LOAD"),
            save: get(360, "SAVE"),
            quick_save: get(351, "Quick Save"),
            empty: get(269, "Empty Savegame..."),
            no_time: get(270, "No Time"),
            no_date: get(271, "No Date"),
            damaged: get(272, "savegame is damaged!!!"),
        }
    }
}

/// One list row: `valid` means the files exist and the checksum matches (only those can be loaded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub valid: bool,
    pub title: String,
    pub time: String,
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    None,
    /// CANCEL or Esc: the in-game page resumes the mission (state 7), the one from the main menu goes back (state 0x1c).
    Cancel,
    /// LOAD on a valid row: this level, with the saved game snapshot (empty = start the level over).
    Load(String, Vec<u8>),
    /// LOAD where the state cannot be read: the exe returns to the main menu.
    LoadFailed(usize),
    /// SAVE: written; the game resumes and says "01-game-saved".
    Saved,
    SaveFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Row(usize),
    Main,
    Cancel,
}

pub struct SaveUi {
    pub mode: Mode,
    pub store: Store,
    pub text: Texts,
    pub slots: Vec<Slot>,
    /// The row under the pointer (`[+4]`) and the clicked row (`[+0xc]`).
    pub hover: Option<usize>,
    pub selected: Option<usize>,
    hover_btn: Option<Hit>,
    /// 160x120 RGB565: the screenshot taken when the page opened (save) or the selected slot's (load).
    pub thumb: Vec<u16>,
    level: String,
    /// The running game (`World::snapshot` + camera) a save stores; set by the game when it opens the page.
    pub snapshot: Vec<u8>,
    needs_capture: bool,
    bg: Option<crate::ui::Image>,
    bg_full: Option<crate::ui::Image>,
    frames: Vec<Frame>,
    fonts: std::rc::Rc<(opensow_formats::font::Font, opensow_formats::font::Font)>,
    big: std::rc::Rc<(opensow_formats::font::Font, opensow_formats::font::Font)>,
    /// Ticks (60 per second) for the caret blink.
    pub tick: u32,
    canvas: Vec<u32>,
    /// GDI stand-in for the row text (sharp at any zoom); `None` falls back to the bitmap font.
    gdi: Option<crate::gdi::Gdi>,
    /// Row texts `(text, x, y, colour)` of the last render, drawn by `draw` on the scaled page.
    texts: std::cell::RefCell<Vec<(String, i32, i32, (u8, u8, u8))>>,
    /// Effect ids for the game to play: 30 hover, 38 LOAD/SAVE, 39 CANCEL, 32 typing, 40 screenshot.
    pub sounds: Vec<u8>,
}

/// Natural rows kept on screen when the window is bigger than the page: the 480-row page plus a margin (a window of height `h`
/// is zoomed by `h / 560`, like the campaign map).
const ZOOM_ROWS: f32 = 560.0;

/// Placement of the 640x480 page on a `w`x`h` screen: the exe draws it unscaled in the centre (`0x443500`); bigger windows zoom it
/// uniformly (not in the original) so it stays readable, a screen smaller than 640x480 scales it down to fit.
fn fit(w: usize, h: usize) -> (f32, f32, f32) {
    let s = (h as f32 / ZOOM_ROWS).max(1.0).min(w as f32 / W as f32).min(h as f32 / H as f32);
    (((w as f32 - W as f32 * s) / 2.0).floor(), ((h as f32 - H as f32 * s) / 2.0).floor(), s)
}

impl SaveUi {
    /// `level` is the running mission (what a save stores); `tick` is not needed to open.
    pub fn new(assets: &mut Assets, mode: Mode, level: &str, store: Store) -> SaveUi {
        let language = assets.game.resolve("Data\\lang.dat").and_then(|p| std::fs::read(p).ok()).and_then(|d| lang::read_lang_dat(&d)).unwrap_or(lang::Language::English);
        let strings = assets.game.resolve(&format!("Data\\Language\\{}", language.file_name())).and_then(|p| std::fs::read(p).ok()).and_then(|d| LangFile::parse(&d).ok());
        let frames = assets.game.resolve("Data\\Sprites2\\Menu\\loadsave_b.tsp").and_then(|p| std::fs::read(p).ok()).and_then(|d| sprite::parse(&d, Kind::Color).ok()).unwrap_or_default();
        let mut ui = SaveUi::with_parts(mode, level, store, Texts::from(strings.as_ref()), frames, assets.font("medium"), assets.font("big"));
        ui.gdi = crate::gdi::Gdi::load();
        ui.bg = load_backdrop(&assets.game, "Load");
        ui.bg_full = load_backdrop(&assets.game, "Load2");
        ui
    }

    pub fn with_parts(mode: Mode, level: &str, store: Store, text: Texts, frames: Vec<Frame>, fonts: std::rc::Rc<(opensow_formats::font::Font, opensow_formats::font::Font)>, big: std::rc::Rc<(opensow_formats::font::Font, opensow_formats::font::Font)>) -> SaveUi {
        let slots = (0..ROWS).map(|i| store.slot(i, &text)).collect();
        SaveUi { mode, store, text, slots, hover: None, selected: None, hover_btn: None, thumb: vec![0; save::THUMB_W * save::THUMB_H], level: level.to_owned(), snapshot: Vec::new(), needs_capture: mode == Mode::Save, bg: None, bg_full: None, frames, fonts, big, tick: 0, canvas: vec![0; W * H], gdi: None, texts: Default::default(), sounds: Vec::new() }
    }

    /// The level a save stores (headless screenshots have no running mission).
    pub fn set_level(&mut self, level: &str) {
        self.level = level.to_owned();
    }

    /// The save page grabs the frozen game once (`0x445070`); the game calls this with its drawn frame.
    pub fn needs_capture(&self) -> bool {
        self.needs_capture
    }

    pub fn capture(&mut self, pixels: &[u32], w: usize, h: usize) {
        self.thumb = save::grab_thumbnail(pixels, w, h);
        self.needs_capture = false;
    }

    // ---- layout (design space 640x480; centre 320,240) ----

    fn row_rect(i: usize) -> Rect {
        // hit area: x in (20, 424), y in (y0+4+32i, y0+28+32i) with y0 = 80
        Rect { x: 21, y: 85 + 32 * i as i32, w: 403, h: 23 }
    }

    fn frame_rect(&self, frame: usize, centre: (i32, i32)) -> Option<Rect> {
        let f = self.frames.get(frame)?;
        Some(Rect { x: centre.0 - f.hot_x as i32, y: centre.1 - f.hot_y as i32, w: f.width as i32, h: f.height as i32 })
    }

    fn cancel_rect(&self) -> Rect {
        self.frame_rect(0, (320, 240)).unwrap_or(Rect { x: 318, y: 430, w: 155, h: 39 })
    }

    fn main_rect(&self) -> Rect {
        let f = if self.mode == Mode::Load { 1 } else { 3 };
        self.frame_rect(f, (320, 240)).unwrap_or(Rect { x: 151, y: 430, w: 113, h: 47 })
    }

    pub fn hit_at(&self, p: (i32, i32)) -> Option<Hit> {
        if self.cancel_rect().contains(p.0, p.1) {
            return Some(Hit::Cancel);
        }
        if self.main_rect().contains(p.0, p.1) {
            return Some(Hit::Main);
        }
        (0..ROWS).find(|&i| Self::row_rect(i).contains(p.0, p.1)).map(Hit::Row)
    }

    /// Centre of a control in design space (for scripted clicks and tests).
    pub fn centre(&self, hit: Hit) -> (i32, i32) {
        let r = match hit {
            Hit::Cancel => self.cancel_rect(),
            Hit::Main => self.main_rect(),
            Hit::Row(i) => Self::row_rect(i),
        };
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    /// Screen position of a design-space point (for scripted pointer moves).
    pub fn to_screen(p: (i32, i32), w: usize, h: usize) -> (i32, i32) {
        let (ox, oy, s) = fit(w, h);
        ((p.0 as f32 * s + ox) as i32, (p.1 as f32 * s + oy) as i32)
    }

    fn to_design(x: i32, y: i32, w: usize, h: usize) -> (i32, i32) {
        let (ox, oy, s) = fit(w, h);
        (((x as f32 - ox) / s) as i32, ((y as f32 - oy) / s) as i32)
    }

    // ---- input ----

    pub fn mouse_moved(&mut self, x: i32, y: i32, w: usize, h: usize) {
        let hit = self.hit_at(Self::to_design(x, y, w, h));
        let row = if let Some(Hit::Row(i)) = hit { Some(i) } else { None };
        if row != self.hover {
            self.hover = row;
            if row.is_some() {
                self.sounds.push(30); // mainmenu-move
            }
        }
        let btn = hit.filter(|h| matches!(h, Hit::Main | Hit::Cancel));
        if btn.is_some() && btn != self.hover_btn {
            self.sounds.push(30);
        }
        self.hover_btn = btn;
    }

    pub fn click(&mut self, x: i32, y: i32, w: usize, h: usize) -> Event {
        match self.hit_at(Self::to_design(x, y, w, h)) {
            Some(Hit::Cancel) => self.cancel(),
            Some(Hit::Main) => self.confirm(),
            Some(Hit::Row(i)) => {
                self.select(i);
                Event::None
            }
            None => Event::None,
        }
    }

    /// Esc and CANCEL: sound 39 (`0x4447ac`).
    pub fn cancel(&mut self) -> Event {
        self.sounds.push(39);
        Event::Cancel
    }

    /// Enter, numpad Enter and the LOAD / SAVE button (`0x444827`): sound 38, then act on the selected row.
    pub fn confirm(&mut self) -> Event {
        let Some(row) = self.selected else { return Event::None };
        match self.mode {
            Mode::Load => {
                if !self.slots[row].valid {
                    return Event::None;
                }
                self.sounds.push(38);
                match self.store.level_of(row) {
                    Some((level, snapshot)) => Event::Load(level, snapshot),
                    None => Event::LoadFailed(row),
                }
            }
            Mode::Save => {
                self.sounds.push(38);
                let title = self.slots[row].title.clone();
                match self.store.write(row, &title, &self.level, &self.snapshot, &self.thumb) {
                    Ok(()) => Event::Saved,
                    Err(e) => Event::SaveFailed(e.to_string()),
                }
            }
        }
    }

    /// A click on a row (`0x4448f3`): the previous row's edited text is reloaded from its files (save page), the new row gets
    /// its default text, or its screenshot is read (load page, sound 40).
    pub fn select(&mut self, row: usize) {
        if self.selected == Some(row) {
            return;
        }
        match self.mode {
            Mode::Load => {
                if let Some(t) = self.store.thumbnail(row) {
                    self.thumb = t;
                    self.sounds.push(40); // loadsave-pic
                }
            }
            Mode::Save => {
                if let Some(prev) = self.selected {
                    self.slots[prev] = self.store.slot(prev, &self.text);
                }
                if !self.slots[row].valid {
                    self.slots[row].title = if row == 0 { self.text.quick_save.clone() } else { String::new() };
                }
            }
        }
        self.selected = Some(row);
    }

    /// Typed characters (save page, any row but 0): appended while the title stays narrower than 195 px (`0x444a04`).
    pub fn type_text(&mut self, s: &str) {
        let (Mode::Save, Some(row)) = (self.mode, self.selected) else { return };
        if row == 0 {
            return;
        }
        let typed: String = s.chars().filter(|c| (' '..='}').contains(c)).collect();
        if typed.is_empty() {
            return;
        }
        let joined = format!("{}{typed}", self.slots[row].title);
        let width = self.gdi.as_ref().map_or_else(|| ui::text_width(&self.fonts, &joined, 1.0), |g| g.width(&joined));
        if width < MAX_TITLE_PX {
            self.slots[row].title = joined;
            self.sounds.push(32); // save-type
        }
    }

    /// Backspace removes the last character (`0x444b1c`), with the typing sound; the key repeat does the rest.
    pub fn backspace(&mut self) {
        let (Mode::Save, Some(row)) = (self.mode, self.selected) else { return };
        if row != 0 && self.slots[row].title.pop().is_some() {
            self.sounds.push(32);
        }
    }

    pub fn update(&mut self, ticks: u32) {
        self.tick = self.tick.wrapping_add(ticks);
    }

    // ---- drawing ----

    fn text(&self, canvas: &mut [u32], s: &str, x: i32, y: i32, fg: (u8, u8, u8)) {
        // the exe draws GDI text with a black shadow two pixels down and right
        ui::text(canvas, W, H, &self.fonts, s, x + 2, y + 2, 1.0, (0, 0, 0), (0, 0, 0));
        ui::text(canvas, W, H, &self.fonts, s, x, y, 1.0, fg, (0, 30, 0));
    }

    /// Row text: queued for the GDI face when there is one (drawn sharp after the page is scaled), else the bitmap font.
    fn row_text(&self, canvas: &mut [u32], s: &str, x: i32, y: i32, fg: (u8, u8, u8)) {
        if self.gdi.is_some() {
            if !s.is_empty() {
                self.texts.borrow_mut().push((s.to_owned(), x, y, fg));
            }
        } else {
            self.text(canvas, s, x, y, fg);
        }
    }

    fn render(&mut self) {
        self.texts.borrow_mut().clear();
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
        // the selected row's green bar (frame 5, 399x26 at x = 23)
        if let (Some(row), Some(f)) = (self.selected, self.frames.get(5)) {
            ui::sprite_add(&mut canvas, W, H, f, 23, 84 + 32 * row as i32, 1.0, false);
        }
        let blink = self.mode == Mode::Save && self.tick % 30 < 15;
        for i in 0..ROWS {
            let bright = self.hover == Some(i) || self.selected == Some(i);
            let _ = bright;
            let colour = if self.hover == Some(i) { GREEN_HI } else { GREEN };
            let y = 87 + 32 * i as i32;
            self.row_text(&mut canvas, &i.to_string(), 29, y, colour);
            let mut title = self.slots[i].title.clone();
            if blink && self.selected == Some(i) && i != 0 {
                title.push('|');
            }
            self.row_text(&mut canvas, &title, 58, y, colour);
            self.row_text(&mut canvas, &self.slots[i].date, 284, y, colour);
        }
        // screenshot box
        let show = match self.mode {
            Mode::Save => true,
            Mode::Load => self.selected.is_some_and(|r| self.slots[r].valid),
        };
        if show {
            for ty in 0..save::THUMB_H {
                for tx in 0..save::THUMB_W {
                    let (r, g, b) = save::rgb_of_565(self.thumb[ty * save::THUMB_W + tx]);
                    canvas[(258 + ty) * W + 450 + tx] = pack(r, g, b);
                }
            }
        }
        // title and buttons
        let title = if self.mode == Mode::Load { &self.text.load_title } else { &self.text.save_title };
        ui::text(&mut canvas, W, H, &self.big, title, 213 + 2, 4 + 2, 1.0, (0, 0, 0), (0, 0, 0));
        ui::text(&mut canvas, W, H, &self.big, title, 213, 4, 1.0, ORANGE, (0, 30, 0));
        let lit = |h: Hit| if self.hover_btn == Some(h) { ORANGE } else { ORANGE_DIM };
        let (cancel, main) = (self.text.cancel.clone(), if self.mode == Mode::Load { self.text.load.clone() } else { self.text.save.clone() });
        self.text(&mut canvas, &cancel, 324, 437, lit(Hit::Cancel));
        self.text(&mut canvas, &main, 160, 437, lit(Hit::Main));
        self.canvas = canvas;
    }

    pub fn draw(&mut self, buf: &mut [u32], w: usize, h: usize) {
        self.render();
        buf.fill(0);
        let (ox, oy, s) = fit(w, h);
        // On a screen wider than 640 the 1920x1080 picture is cropped around the centre (`0x443500`, no scaling up to
        // 1920x1080; a bigger window stretches it) and the 640x480 page is laid over its middle.
        if let (Some(bg), true) = (&self.bg_full, w > W) {
            let crop = w <= bg.w && h <= bg.h;
            let (x0, y0) = if crop { ((bg.w - w) / 2, (bg.h - h) / 2) } else { (0, 0) };
            for y in 0..h {
                let sy = if crop { y0 + y } else { (y * bg.h / h).min(bg.h - 1) };
                for x in 0..w {
                    let sx = if crop { x0 + x } else { (x * bg.w / w).min(bg.w - 1) };
                    let i = (sy * bg.w + sx) * 3;
                    buf[y * w + x] = pack(bg.rgb[i], bg.rgb[i + 1], bg.rgb[i + 2]);
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
        if let Some(g) = &self.gdi {
            for (text, x, y, rgb) in self.texts.borrow().iter() {
                g.draw_rgb(buf, w, h, text, ox + *x as f32 * s, oy + *y as f32 * s, s, *rgb);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensow_formats::font::Font;

    fn fonts() -> std::rc::Rc<(Font, Font)> {
        std::rc::Rc::new((Font::new(Default::default()), Font::new(Default::default())))
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("opensow-saveui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn texts() -> Texts {
        Texts::from(None)
    }

    fn ui(mode: Mode, dir: &PathBuf) -> SaveUi {
        SaveUi::with_parts(mode, "M03", Store { write: dir.clone(), also: None }, texts(), Vec::new(), fonts(), fonts())
    }

    #[test]
    fn clock_strings_match_strftime() {
        let (t, d, w) = civil(0);
        assert_eq!((t.as_str(), d.as_str(), w.as_str()), ("00:00:00", "Jan 01, 1970", "Thu 00:00"));
        // 2026-11-04 13:05:09 UTC was a Wednesday
        let (t, d, w) = civil(1_793_797_509);
        assert_eq!((t.as_str(), d.as_str(), w.as_str()), ("13:05:09", "Nov 04, 2026", "Wed 13:05"));
    }

    #[test]
    fn state_stream_carries_the_level() {
        let s = state_bytes("M03", &[]);
        assert_eq!(StateHead::parse(&s).unwrap().level, 3);
        assert_eq!(level_from_state(&s).as_deref(), Some("M03"));
        // a genuine stream has no marker: the number gives the level
        let head = StateHead { level: 7, rng_index: 0, v_5baafc: 0, flag_49a339: false, v_5baad8: 0, v_5baadc: 0, v_5baac4: 0, v_5baac8: 0, flag_5baa99: false, v_89cf50: 0, len: 0 };
        assert_eq!(level_from_state(&head.to_bytes()).as_deref(), Some("M07"));
        assert_eq!(level_from_state(&[1, 2, 3]), None);
    }

    #[test]
    fn empty_store_lists_defaults_and_load_needs_a_valid_row() {
        let dir = temp("empty");
        let mut u = ui(Mode::Load, &dir);
        assert!(u.slots.iter().all(|s| !s.valid && s.title == "Empty Savegame..." && s.date == "No Date" && s.time == "No Time"));
        u.select(2);
        assert_eq!(u.confirm(), Event::None, "an empty row cannot be loaded");
        assert!(u.sounds.is_empty());
        assert_eq!(u.cancel(), Event::Cancel);
        assert_eq!(u.sounds, vec![39]);
    }

    #[test]
    fn save_then_load_round_trip_with_typing() {
        let dir = temp("roundtrip");
        let mut u = ui(Mode::Save, &dir);
        assert!(u.needs_capture());
        let px = vec![0x00ff00u32; 640 * 480];
        u.capture(&px, 640, 480);
        assert!(!u.needs_capture());
        u.select(0);
        assert_eq!(u.slots[0].title, "Quick Save", "row 0 starts as Quick Save");
        u.type_text("x");
        assert_eq!(u.slots[0].title, "Quick Save", "row 0 is not editable");
        u.select(4);
        assert_eq!(u.slots[0].title, "Empty Savegame...", "leaving a row restores it from the files");
        assert_eq!(u.slots[4].title, "");
        // an empty font has no glyphs: width 0, so everything fits
        u.type_text("Hi");
        u.type_text("\u{7f}\u{1}");
        assert_eq!(u.slots[4].title, "Hi");
        u.backspace();
        u.type_text("!");
        assert_eq!(u.slots[4].title, "H!");
        assert_eq!(u.sounds.iter().filter(|&&s| s == 32).count(), 3);
        assert_eq!(u.confirm(), Event::Saved);
        assert!(dir.join("savegame_052.data").exists() && dir.join("savegame_052.info").exists());

        let mut l = ui(Mode::Load, &dir);
        assert!(l.slots[4].valid);
        assert_eq!(l.slots[4].title, "H!");
        assert!(!l.slots[3].valid);
        l.select(4);
        assert_eq!(l.sounds, vec![40]);
        assert_eq!(l.thumb[0], save::rgb565(0, 255, 0));
        assert_eq!(l.confirm(), Event::Load("M03".into(), Vec::new()));
        // corrupt the data: the row turns into "damaged" and cannot be loaded
        let data = dir.join("savegame_052.data");
        let mut bytes = std::fs::read(&data).unwrap();
        *bytes.last_mut().unwrap() ^= 0x55;
        std::fs::write(&data, bytes).unwrap();
        let d = ui(Mode::Load, &dir);
        assert!(!d.slots[4].valid);
        assert_eq!(d.slots[4].title, "savegame is damaged!!!");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rows_hit_and_hover_sound() {
        let dir = temp("hit");
        let mut u = ui(Mode::Load, &dir);
        assert_eq!(u.hit_at((100, 84)), None, "above the first row");
        assert_eq!(u.hit_at((100, 90)), Some(Hit::Row(0)));
        assert_eq!(u.hit_at((100, 90 + 32 * 9)), Some(Hit::Row(9)));
        assert_eq!(u.hit_at((100, 90 + 32 * 10)), None);
        assert_eq!(u.hit_at((430, 90)), None, "right of the table");
        u.mouse_moved(100, 90, 640, 480);
        u.mouse_moved(120, 92, 640, 480);
        assert_eq!(u.sounds, vec![30], "one hover sound per row entered");
        u.mouse_moved(100, 122, 640, 480);
        assert_eq!(u.sounds, vec![30, 30]);
        let c = u.centre(Hit::Row(3));
        assert_eq!(u.hit_at(c), Some(Hit::Row(3)));
    }

    #[test]
    fn title_width_limit() {
        let dir = temp("wide");
        let mut u = ui(Mode::Save, &dir);
        u.select(1);
        u.slots[1].title = "a".repeat(10);
        // with no glyphs the width is 0 and typing is never refused; the limit itself is a constant of the exe
        assert_eq!(MAX_TITLE_PX, 195);
        u.type_text("b");
        assert_eq!(u.slots[1].title.len(), 11);
    }
}
