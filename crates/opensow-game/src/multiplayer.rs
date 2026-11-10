//! The multiplayer screens (`CMenuNetwork`, object `0x71a298`, main state `0x15`): connection list, session list and the lobby
//! where both sides pick the map, credits, air forces and turrets, with a chat. UI only: the transport (DirectPlay in the original,
//! docs/network.md) is stubbed, the flow runs offline. Notes: docs/multiplayer-menus.md.

use crate::assets::{load_backdrop, Assets};
use crate::ui::{self, Image};
use opensow_formats::lang::{self, LangFile};
use opensow_formats::level::Level;
use opensow_formats::sprite::{self, Frame, Kind};
use opensow_viewer::render::pack;

/// Design resolution; frames are placed relative to its centre (top-left = centre - hotspot).
pub const W: usize = 640;
pub const H: usize = 480;

/// The session name the host creates (`0x44e500`, string at `0x48fc44`) and the two player names (`0x48fc78`, `0x48fc98`).
pub const SESSION_NAME: &str = "State of War session";
pub const HOST_NAME: &str = "Player1";
pub const GUEST_NAME: &str = "Player2";

/// The DirectPlay service providers a Windows install lists (names as DirectPlay 4 reports them); the stub offers these.
pub const STUB_CONNECTIONS: [&str; 4] = ["Internet TCP/IP Connection For DirectPlay", "IPX Connection For DirectPlay", "Modem Connection For DirectPlay", "Serial Connection For DirectPlay"];

/// Which connection the exe picks by itself (`0x44f270`): the first whose name is longer than 35 characters and has `TCP/I` at
/// characters 9..14; none matching gives 0.
pub fn preferred_connection(names: &[String]) -> usize {
    names.iter().position(|n| n.len() > 0x23 && n.as_bytes().get(9..14) == Some(b"TCP/I")).unwrap_or(0)
}

/// Rows of the air-force arrows, top to bottom (icons: fighter, bomber, tripler, carryall, asteroid).
#[allow(dead_code)]
pub const AIR_ROWS: [&str; 5] = ["Fighter", "Bomber", "Tripler", "Carryall", "Asteroid"];
/// Index into the level header's air groups (`level::HEADER_ORDER`: bomber, asteroid, carryall, tripler, fighter) of each row.
const ROW_GROUP: [usize; 5] = [4, 0, 3, 2, 1];
/// The carryall's group in the level header (`air::HEADER_ORDER`: bomber, asteroid, carryall, tripler, fighter).
const CARRYALL_GROUP: usize = 2;
pub const MAX_AIR: i32 = 20;
pub const MAX_CREDITS: i32 = 19000;
pub const MAX_RESEARCH: i32 = 2000;

/// What one side brings to the match (the exe keeps both in the table at `0x71cac0`, 4 bytes per side per field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideSetup {
    pub credits: i32,
    pub research: i32,
    /// Starting air forces in [`AIR_ROWS`] order.
    pub air: [i32; 5],
    /// Turret types 1..=5 available (bytes `0x71cb14..18` blue, `0x71cb1e..22` green).
    pub turrets: [bool; 5],
}

impl SideSetup {
    pub fn new() -> SideSetup {
        SideSetup { credits: 0, research: 0, air: [0; 5], turrets: [true; 5] }
    }
}

impl Default for SideSetup {
    fn default() -> Self {
        SideSetup::new()
    }
}

/// Both sides' values; the defaults come from the selected map (`0x41c440` fills them from the `.net` header).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Setup {
    pub sides: [SideSetup; 2],
}

impl Setup {
    /// Credits from header words 2 and 3, research from words 37 and 38, air forces from the groups' slot 1 (blue) and slot 2
    /// (green) counts (docs/air.md; which slot belongs to whom was not traced), turrets all available.
    pub fn from_level(level: &Level) -> Setup {
        let mut s = Setup::default();
        for side in 0..2 {
            let h = &level.header;
            s.sides[side].credits = h[2 + side];
            s.sides[side].research = h[37 + side];
            for (row, g) in ROW_GROUP.iter().enumerate() {
                s.sides[side].air[row] = h[4 + 3 * g + 1 + side].clamp(0, MAX_AIR);
            }
        }
        s
    }

    /// The default of a skirmish: [`Setup::from_level`], and the turret tiers the map's own header allows (First Spark offers tiers 1 and 2
    /// only; the sim reads those bytes, `World::from_level`) instead of the lobby's all-available assumption.
    pub fn skirmish_default(level: &Level) -> Setup {
        let mut s = Setup::from_level(level);
        for side in 0..2 {
            for t in 0..5 {
                let b = 84 + 10 * side + t;
                s.sides[side].turrets[t] = (level.header[b / 4 + 2] >> (8 * (b % 4))) & 0xff != 0;
            }
            // skirmishes start without carryalls (a map's own stock, e.g. 3 each on Ice Blizzard, let the computer drop
            // squads into the player's base at once); air bases still build them and the lobby can add some
            if let Some(row) = ROW_GROUP.iter().position(|&g| g == CARRYALL_GROUP) {
                s.sides[side].air[row] = 0;
            }
        }
        s
    }

    /// The inverse of [`Setup::from_level`]: writes both sides' credits, research, air forces and turret switches into the level's
    /// header words (the turret switches are bytes `84..89` / `94..99` of the header block, `header[i]` is block word `i - 2`).
    pub fn apply(&self, level: &mut Level) {
        for side in 0..2 {
            let (s, h) = (&self.sides[side], &mut level.header);
            h[2 + side] = s.credits;
            h[37 + side] = s.research;
            for (row, g) in ROW_GROUP.iter().enumerate() {
                h[4 + 3 * g + 1 + side] = s.air[row];
            }
            for t in 0..5 {
                let b = 84 + 10 * side + t;
                let (word, shift) = (b / 4 + 2, 8 * (b % 4));
                h[word] = (h[word] & !(0xff << shift)) | (i32::from(s.turrets[t]) << shift);
            }
        }
    }

    /// An arrow of the lobby (`0x448a0b` jump table, 28 of them: 14 per side, "-" then "+" for each of fighter, bomber, tripler,
    /// carryall, asteroid, credits, research). Only the local side's own arrows act. Credits step 500 up to 19000 and research 100
    /// up to 2000; the exe lets credits go below zero (`> 0` is checked before subtracting), which is kept.
    pub fn arrow(&mut self, k: usize, local: usize) -> bool {
        let (side, j) = (k / 14, k % 14);
        if side != local || side > 1 {
            return false;
        }
        let up = j % 2 == 1;
        let s = &mut self.sides[side];
        match j / 2 {
            r @ 0..=4 => {
                let v = &mut s.air[r];
                if up && *v < MAX_AIR {
                    *v += 1;
                } else if !up && *v > 0 {
                    *v -= 1;
                }
            }
            5 => {
                if up && s.credits < MAX_CREDITS {
                    s.credits += 500;
                } else if !up && s.credits > 0 {
                    s.credits -= 500;
                }
            }
            _ => {
                if up && s.research < MAX_RESEARCH {
                    s.research += 100;
                } else if !up && s.research > 0 {
                    s.research -= 100;
                }
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// Which kind of connection (only visible when no TCP/IP provider exists, see [`preferred_connection`]).
    Connections,
    /// Sessions on the network; CREATE SESSION / SHOW ALL SESSIONS / JOIN SESSION.
    Sessions,
    Lobby,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Continue,
    Create,
    ShowAll,
    Join,
    Accept,
    Back,
    /// The BLUE SIDE / GREEN SIDE selectors.
    Side(usize),
    Arrow(usize),
    Turret(usize),
    Map(usize),
    Session(usize),
    Connection(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    None,
    /// Back to the main menu (state `0x1c`).
    Leave,
    /// ACCEPT by the host: both machines start `map` (the original sends the setup with the start flag and calls state 7).
    Start { map: String, local: usize, setup: Setup },
}

pub struct NetMenu {
    pub page: Page,
    connections: Vec<String>,
    /// Selected connection (`[+0x10]`), -1 when none.
    conn_sel: i32,
    sessions: Vec<String>,
    /// Selected session (`[+0x14]`), -1 when none.
    sess_sel: i32,
    /// Level names (`data\levels\*.net` without the extension).
    pub maps: Vec<String>,
    pub map: usize,
    pub setup: Setup,
    /// This machine's side: 0 blue (host), 1 green.
    pub local: usize,
    pub chat: Vec<String>,
    pub input: String,
    /// Another player is in the session.
    pub peer: bool,
    game: opensow_formats::GameDir,
    bg: [Option<Image>; 2],
    bg_full: [Option<Image>; 2],
    join: Vec<Frame>,
    others: Vec<Frame>,
    arrows: Vec<Frame>,
    turrets: Vec<Frame>,
    digits: Vec<Frame>,
    strings: Option<LangFile>,
    fonts: std::rc::Rc<(opensow_formats::font::Font, opensow_formats::font::Font)>,
    /// Unicode text for languages other than English (see `options.rs`).
    ugdi: Option<crate::gdi::Gdi>,
    hover: Option<Hit>,
    cursor: (i32, i32),
    canvas: Vec<u32>,
    /// Effect ids for the game to play: 30 hover, 33 arrows and turrets, 38 ACCEPT, 39 the other buttons, 29 a player joining.
    pub sounds: Vec<u8>,
}

/// The names the skirmish list shows. The base game's maps keep their names (`01-First Spark`); Warmonger's, which are numbered from
/// 00 in their own files and in lower case, continue the numbering (`09-The First Step`), in the order of the list.
pub fn display_names(maps: &[String], game: &opensow_formats::GameDir) -> Vec<String> {
    let from_warmonger = |m: &str| match (game.resolve(&format!("Data\\Levels\\{m}.net")), &game.warmonger) {
        (Some(p), Some(w)) => p.starts_with(w),
        _ => false,
    };
    let base = maps.iter().filter(|m| !from_warmonger(m)).count();
    let mut next = base;
    maps.iter()
        .map(|m| {
            if !from_warmonger(m) {
                return m.clone();
            }
            next += 1;
            let title = m.split_once('-').map_or(m.as_str(), |(_, t)| t);
            let title: Vec<String> = title.split_whitespace().map(|w| {
                let mut c = w.chars();
                c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
            }).collect();
            format!("{next:02}-{}", title.join(" "))
        })
        .collect()
}

fn load(assets: &Assets, name: &str) -> Vec<Frame> {
    assets.game.resolve(&format!("Data\\Sprites2\\Menu\\{name}.tsp")).and_then(|p| std::fs::read(p).ok()).and_then(|d| sprite::parse(&d, Kind::Color).ok()).unwrap_or_default()
}

fn fit(w: usize, h: usize) -> (f32, f32, f32) {
    let s = (w as f32 / W as f32).min(h as f32 / H as f32);
    ((w as f32 - W as f32 * s) / 2.0, (h as f32 - H as f32 * s) / 2.0, s)
}

fn frame_rect(f: &Frame) -> ui::Rect {
    ui::Rect { x: W as i32 / 2 - f.hot_x as i32, y: H as i32 / 2 - f.hot_y as i32, w: f.width as i32, h: f.height as i32 }
}

/// Frames of `net_join`: caption, then normal / highlighted pairs.
const J_CAPTION: usize = 0;
const J_CONTINUE: usize = 1;
const J_CREATE: usize = 3;
const J_SHOW: usize = 5;
const J_JOIN: usize = 7;
/// Frames of `net_others`.
const O_ACCEPT: usize = 0;
const O_BACK: usize = 1;
const O_BLUE: usize = 2;
const O_GREEN: usize = 4;
const O_CROSS: usize = 6;
/// A green side's controls sit this far right of the blue side's.
const SIDE_DX: i32 = 210;
/// Chat lines shown (`0x447ce0`: 7).
pub const CHAT_LINES: usize = 7;
const MAP_ROWS_X: i32 = 12;
const MAP_ROWS_Y: i32 = 16;
const MAP_ROW_H: i32 = 15;

impl NetMenu {
    /// Opens the menu: the connection list is the stub's; with a TCP/IP provider the connection page is skipped at once (as the exe does).
    pub fn new(assets: &mut Assets) -> NetMenu {
        let names = STUB_CONNECTIONS.iter().map(|s| s.to_string()).collect();
        NetMenu::with_connections(assets, names)
    }

    pub fn with_connections(assets: &mut Assets, connections: Vec<String>) -> NetMenu {
        let language = assets.game.language();
        let strings = assets.game.resolve(&format!("Data\\Language\\{}", language.file_name())).and_then(|p| std::fs::read(p).ok()).and_then(|d| LangFile::parse(&d).ok()).map(|f| f.with_supplement(language));
        let mut maps: Vec<String> = assets
            .game
            .level_files()
            .into_iter()
            .filter_map(|n| {
                let (stem, ext) = n.rsplit_once('.')?;
                ext.eq_ignore_ascii_case("net").then(|| stem.to_owned())
            })
            .collect();
        // the base game's maps first, Warmonger's own (docs/warmonger.md) after them
        let from_warmonger = |m: &str| match (assets.game.resolve(&format!("Data\\Levels\\{m}.net")), &assets.game.warmonger) {
            (Some(p), Some(w)) => p.starts_with(w),
            _ => false,
        };
        maps.sort_by_key(|m| (from_warmonger(m), m.to_lowercase()));
        let mut m = NetMenu {
            page: Page::Connections,
            conn_sel: -1,
            connections,
            sessions: Vec::new(),
            sess_sel: -1,
            maps,
            map: 0,
            setup: Setup::default(),
            local: 0,
            chat: Vec::new(),
            input: String::new(),
            peer: false,
            game: assets.game.clone(),
            bg: [load_backdrop(&assets.game, "Network"), load_backdrop(&assets.game, "Network2")],
            bg_full: [load_backdrop(&assets.game, "NetworkBackground"), load_backdrop(&assets.game, "NetworkBackground2")],
            join: load(assets, "net_join"),
            others: load(assets, "net_others"),
            arrows: load(assets, "net_arrows"),
            turrets: load(assets, "net_turrets"),
            digits: load(assets, "options_numbers"),
            strings,
            fonts: assets.font("medium"),
            ugdi: crate::gdi::Gdi::for_language(language, 17.0),
            hover: None,
            cursor: (-1, -1),
            canvas: vec![0; W * H],
            sounds: Vec::new(),
        };
        m.enter_connections();
        m
    }

    /// The text of the exe's string index (the exe counts one higher than `LangFile`, docs/text.md).
    fn string(&self, exe_index: usize) -> &str {
        self.strings.as_ref().and_then(|s| s.get(exe_index.wrapping_sub(1))).unwrap_or("")
    }

    /// Page 0 on entry (`0x44842d`): the list is selected at its first entry, and as the selection is not `-1` the exe continues
    /// with [`preferred_connection`] at once; with an empty list the page stays.
    fn enter_connections(&mut self) {
        self.page = Page::Connections;
        self.conn_sel = if self.connections.is_empty() { -1 } else { 0 };
        if self.conn_sel != -1 {
            self.conn_sel = preferred_connection(&self.connections) as i32;
            self.enter_sessions();
        }
    }

    fn enter_sessions(&mut self) {
        self.page = Page::Sessions;
        self.sess_sel = -1;
    }

    /// Sessions another machine hosts (what "SHOW ALL SESSIONS" lists); the stub has none unless a test or demo sets them.
    pub fn set_sessions(&mut self, names: Vec<String>) {
        self.sess_sel = if names.is_empty() { -1 } else { 0 };
        self.sessions = names;
    }

    /// The lobby with the given role (the host owns the map list; a guest only sees the chosen map).
    pub fn open_lobby(&mut self, local: usize) {
        self.page = Page::Lobby;
        self.local = local;
        self.chat.clear();
        self.select_map(0);
    }

    /// Chooses a map and loads its defaults (`0x4499d0`).
    pub fn select_map(&mut self, i: usize) {
        if i >= self.maps.len() {
            return;
        }
        self.map = i;
        if let Some(level) = self.game.resolve(&format!("Data\\Levels\\{}.net", self.maps[i])).and_then(|p| std::fs::read(p).ok()).and_then(|d| Level::parse(&d).ok()) {
            self.setup = Setup::from_level(&level);
        }
    }

    /// A second player arrives (a message from the network in the original): the chat says so and sound 29 plays.
    pub fn peer_joined(&mut self) {
        self.peer = true;
        let text = self.string(266).to_owned();
        self.chat.push(text);
        self.sounds.push(29);
    }

    #[allow(dead_code)]
    pub fn peer_left(&mut self) {
        self.peer = false;
        let text = self.string(267).to_owned();
        self.chat.push(text);
        self.sounds.push(29);
    }

    pub fn typed(&mut self, c: char) {
        if self.page == Page::Lobby && !c.is_control() && self.input.len() < 60 {
            self.input.push(c);
        }
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    /// Enter: the line goes into the chat, starting with the side digit (`1` blue, `2` green) that colours it.
    pub fn enter(&mut self) {
        if self.page == Page::Lobby && !self.input.trim().is_empty() {
            let name = if self.local == 0 { HOST_NAME } else { GUEST_NAME };
            self.chat.push(format!("{}{}: {}", self.local + 1, name, self.input.trim()));
            self.input.clear();
        }
    }

    // ---- geometry ----

    fn join_rect(&self, frame: usize) -> Option<ui::Rect> {
        self.join.get(frame).map(frame_rect)
    }

    fn arrow_frame(&self, k: usize) -> Option<&Frame> {
        self.arrows.get(k)
    }

    fn session_row(&self, p: (i32, i32)) -> Option<usize> {
        // `0x4483a0`: x 350..=622 (raw), row = (y - 100) / 30 as long as it is a valid index
        if !(0x15f..0x15e + 0x10e).contains(&p.0) || p.1 < 100 {
            return None;
        }
        let r = ((p.1 - 100) / 30) as usize;
        (r < self.sessions.len()).then_some(r)
    }

    fn connection_row(&self, p: (i32, i32)) -> Option<usize> {
        let r = (p.1 - (H as i32 / 2 - 0x99)) / 32;
        (p.1 >= H as i32 / 2 - 0x99 && (r as usize) < self.connections.len() && (0..W as i32).contains(&p.0)).then_some(r as usize)
    }

    fn map_row(&self, p: (i32, i32)) -> Option<usize> {
        if self.local != 0 || p.0 < MAP_ROWS_X || p.0 > 210 || p.1 < MAP_ROWS_Y {
            return None;
        }
        let r = ((p.1 - MAP_ROWS_Y) / MAP_ROW_H) as usize;
        (r < self.maps.len()).then_some(r)
    }

    /// The control under a design-space point.
    pub fn hit_at(&self, p: (i32, i32)) -> Option<Hit> {
        let inside = |r: Option<ui::Rect>| r.is_some_and(|r| r.contains(p.0, p.1));
        match self.page {
            Page::Connections => {
                if self.conn_sel >= 0 && inside(self.join_rect(J_CONTINUE)) {
                    return Some(Hit::Continue);
                }
                self.connection_row(p).map(Hit::Connection)
            }
            Page::Sessions => {
                if inside(self.join_rect(J_CREATE)) {
                    return Some(Hit::Create);
                }
                if inside(self.join_rect(J_SHOW)) {
                    return Some(Hit::ShowAll);
                }
                if self.sess_sel >= 0 && inside(self.join_rect(J_JOIN)) {
                    return Some(Hit::Join);
                }
                self.session_row(p).map(Hit::Session)
            }
            Page::Lobby => {
                if self.local == 0 && inside(self.others.get(O_ACCEPT).map(frame_rect)) {
                    return Some(Hit::Accept);
                }
                if inside(self.others.get(O_BACK).map(frame_rect)) {
                    return Some(Hit::Back);
                }
                for (s, f) in [(0, O_BLUE), (1, O_GREEN)] {
                    if inside(self.others.get(f).map(frame_rect)) {
                        return Some(Hit::Side(s));
                    }
                }
                if let Some(k) = (0..28).find(|&k| inside(self.arrow_frame(k).map(frame_rect))) {
                    return Some(Hit::Arrow(k));
                }
                if let Some(t) = (0..10).find(|&t| inside(self.turrets.get(t).map(frame_rect))) {
                    return Some(Hit::Turret(t));
                }
                self.map_row(p).map(Hit::Map)
            }
        }
    }

    /// Design-space centre of a control (for scripted clicks and tests).
    pub fn centre(&self, hit: Hit) -> (i32, i32) {
        let c = |r: Option<ui::Rect>| r.map_or((0, 0), |r| (r.x + r.w / 2, r.y + r.h / 2));
        match hit {
            Hit::Continue => c(self.join_rect(J_CONTINUE)),
            Hit::Create => c(self.join_rect(J_CREATE)),
            Hit::ShowAll => c(self.join_rect(J_SHOW)),
            Hit::Join => c(self.join_rect(J_JOIN)),
            Hit::Accept => c(self.others.get(O_ACCEPT).map(frame_rect)),
            Hit::Back => c(self.others.get(O_BACK).map(frame_rect)),
            Hit::Side(s) => c(self.others.get(if s == 0 { O_BLUE } else { O_GREEN }).map(frame_rect)),
            Hit::Arrow(k) => c(self.arrow_frame(k).map(frame_rect)),
            Hit::Turret(t) => c(self.turrets.get(t).map(frame_rect)),
            Hit::Map(i) => (MAP_ROWS_X + 40, MAP_ROWS_Y + i as i32 * MAP_ROW_H + MAP_ROW_H / 2),
            Hit::Session(i) => (486, 100 + i as i32 * 30 + 10),
            Hit::Connection(i) => (W as i32 / 2, H as i32 / 2 - 0x99 + i as i32 * 32 + 10),
        }
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

    #[allow(dead_code)]
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

    /// Escape: back to the main menu from every page (`0x4482a0` checks the key first).
    pub fn escape(&mut self) -> Event {
        Event::Leave
    }

    /// What a click does. Sounds: hover 30, ACCEPT 38, arrows and turrets 33, every other button 39 (button setup `0x44a3e9..0x44a8ac`).
    pub fn press(&mut self, hit: Hit) -> Event {
        match hit {
            Hit::Connection(i) => {
                self.conn_sel = i as i32;
            }
            Hit::Continue => {
                self.sounds.push(39);
                if self.conn_sel >= 0 {
                    self.enter_sessions();
                }
            }
            Hit::ShowAll => {
                self.sounds.push(39);
                let keep = self.sessions.clone();
                self.set_sessions(keep);
            }
            Hit::Session(i) => self.sess_sel = i as i32,
            Hit::Create => {
                self.sounds.push(39);
                self.open_lobby(0);
            }
            Hit::Join => {
                self.sounds.push(39);
                if self.sess_sel >= 0 {
                    self.open_lobby(1);
                    self.peer = true;
                }
            }
            Hit::Back => {
                self.sounds.push(39);
                return Event::Leave;
            }
            Hit::Accept => {
                self.sounds.push(38);
                if self.local == 0 {
                    return Event::Start { map: self.maps.get(self.map).cloned().unwrap_or_default(), local: self.local, setup: self.setup.clone() };
                }
            }
            Hit::Side(s) => {
                self.sounds.push(39);
                self.local = s;
            }
            Hit::Arrow(k) => {
                self.sounds.push(33);
                self.setup.arrow(k, self.local);
            }
            Hit::Turret(t) => {
                self.sounds.push(33);
                // buttons 0..5 are the blue side's (host only), 5..10 the green side's (guest only)
                if t / 5 == self.local {
                    let v = &mut self.setup.sides[t / 5].turrets[t % 5];
                    *v = !*v;
                }
            }
            Hit::Map(i) => {
                self.sounds.push(33);
                self.select_map(i);
            }
        }
        Event::None
    }

    // ---- drawing ----

    fn text(&self, canvas: &mut [u32], s: &str, x: i32, y: i32, fg: (u8, u8, u8)) {
        if let Some(g) = &self.ugdi {
            g.draw_shadowed(canvas, W, H, s, x as f32, y as f32 - 3.0, 1.0, fg, (0, 0, 0));
            return;
        }
        ui::text(canvas, W, H, &self.fonts, s, x, y, 1.0, fg, (0, 0, 0));
    }

    fn centred(&self, canvas: &mut [u32], s: &str, cx: i32, y: i32, fg: (u8, u8, u8)) {
        let w = self.ugdi.as_ref().map_or_else(|| ui::text_width(&self.fonts, s, 1.0), |g| g.width(s));
        self.text(canvas, s, cx - w / 2, y, fg);
    }

    /// `0x4467b0`: up to five digits without leading zeros, 12 px apart, centred on x.
    fn number(&self, canvas: &mut [u32], x: i32, y: i32, value: i32) {
        let v = value.clamp(0, 99999).to_string();
        let n = v.len() as i32;
        for (i, ch) in v.chars().enumerate() {
            let d = ch as usize - '0' as usize;
            if let Some(f) = self.digits.get(d) {
                let dx = (i as i32 * 2 - (n - 1)) * 6;
                ui::sprite(canvas, W, H, f, x + dx - f.hot_x as i32, y - f.hot_y as i32, 1.0);
            }
        }
    }

    fn put(&self, canvas: &mut [u32], f: Option<&Frame>) {
        if let Some(f) = f {
            let r = frame_rect(f);
            ui::sprite(canvas, W, H, f, r.x, r.y, 1.0);
        }
    }

    /// Non-English: the art buttons carry English words. Paints over the word (a gradient between the pixels left and right of it, so the
    /// oval/strip keeps its look) and writes the language's string (`0x467060`, Times New Roman bold) centred on the button.
    fn relabel(&self, canvas: &mut [u32], f: Option<&Frame>, text: &str, fg: (u8, u8, u8), lit: bool) {
        let (Some(g), Some(f)) = (&self.ugdi, f) else { return };
        let r = frame_rect(f);
        // strips (the long session / side buttons) have letters out to the border, the green ovals keep their rim
        let wide = r.w > r.h * 3;
        let (x0, x1) = if wide { (r.x, r.x + r.w) } else { (r.x + r.w / 8, r.x + r.w * 7 / 8) };
        let (y0, y1) = if wide { (r.y, r.y + r.h) } else { (r.y + r.h / 5, r.y + r.h * 4 / 5) };
        // colours to blend between: just outside a strip, the rim of an oval
        let (ea, eb) = if wide { (-1, r.w) } else { (r.w / 10, r.w - 1 - r.w / 10) };
        for y in y0..y1 {
            let (a, b) = (canvas[(y * W as i32 + r.x + ea) as usize], canvas[(y * W as i32 + r.x + eb) as usize]);
            for x in x0..x1 {
                let t = (x - x0) as u32 * 256 / (x1 - x0).max(1) as u32;
                let mix = |sh: u32| ((a >> sh & 255) * (256 - t) + (b >> sh & 255) * t) >> 8;
                canvas[(y * W as i32 + x) as usize] = (mix(16) << 16) | (mix(8) << 8) | mix(0);
            }
        }
        let tw = g.width(text);
        let c = |v: u8| if lit { v } else { (v as u32 * 4 / 5) as u8 };
        g.draw_shadowed(canvas, W, H, text, (r.x + (r.w - tw) / 2) as f32, (r.y + r.h / 2 - 11) as f32, 1.0, (c(fg.0), c(fg.1), c(fg.2)), (0, 0, 0));
    }

    /// A `net_join` button: frame `normal`, or the highlighted one after it under the pointer.
    fn pair(&self, canvas: &mut [u32], normal: usize, hit: Hit) {
        let f = if self.hover == Some(hit) { normal + 1 } else { normal };
        self.put(canvas, self.join.get(f));
    }

    fn render(&mut self) {
        let mut canvas = std::mem::take(&mut self.canvas);
        canvas.fill(0);
        let page = if self.page == Page::Lobby { 1 } else { 0 };
        if let Some(bg) = &self.bg[page] {
            for y in 0..H {
                for x in 0..W {
                    let (sx, sy) = (x * bg.w / W, y * bg.h / H);
                    let i = (sy * bg.w + sx) * 3;
                    canvas[y * W + x] = pack(bg.rgb[i], bg.rgb[i + 1], bg.rgb[i + 2]);
                }
            }
        }
        let white = (255, 255, 255);
        match self.page {
            Page::Connections => {
                self.put(&mut canvas, self.join.get(J_CAPTION));
                for (i, name) in self.connections.iter().enumerate() {
                    let colour = if self.conn_sel == i as i32 { (255, 220, 0) } else { white };
                    self.centred(&mut canvas, name, W as i32 / 2, H as i32 / 2 - 0x8c + 32 * i as i32, colour);
                }
                if self.conn_sel >= 0 {
                    self.pair(&mut canvas, J_CONTINUE, Hit::Continue);
                }
            }
            Page::Sessions => {
                for (f, hit, idx) in [(J_CREATE, Hit::Create, 263), (J_SHOW, Hit::ShowAll, 264)] {
                    self.pair(&mut canvas, f, hit);
                    let fr = self.join.get(if self.hover == Some(hit) { f + 1 } else { f });
                    self.relabel(&mut canvas, fr, self.string(idx + 1), (60, 220, 70), self.hover == Some(hit));
                }
                for (i, name) in self.sessions.iter().enumerate() {
                    // selected: red, others grey (`0x446ca0`); the exe's x is `W - width / 2`, the list is drawn centred on its click column instead
                    let colour = if self.sess_sel == i as i32 { (255, 0, 0) } else { (192, 192, 192) };
                    self.centred(&mut canvas, name, 486, H as i32 / 2 - 0x8c + 30 * i as i32, colour);
                }
                if self.sess_sel >= 0 {
                    self.pair(&mut canvas, J_JOIN, Hit::Join);
                }
            }
            Page::Lobby => self.render_lobby(&mut canvas),
        }
        self.canvas = canvas;
    }

    fn render_lobby(&self, canvas: &mut [u32]) {
        let white = (255, 255, 255);
        let hot = |h: Hit| self.hover == Some(h);
        // turret plates: lit when the side offers the turret, an X over those switched off (`0x4476b8`)
        for t in 0..10 {
            self.put(canvas, self.turrets.get(t));
            if !self.setup.sides[t / 5].turrets[t % 5] {
                if let (Some(cross), Some(f)) = (self.others.get(O_CROSS), self.turrets.get(t)) {
                    let r = frame_rect(f);
                    ui::sprite(canvas, W, H, cross, r.x + (r.w - cross.width as i32) / 2, r.y + (r.h - cross.height as i32) / 2, 1.0);
                }
            }
            if hot(Hit::Turret(t)) {
                if let Some(f) = self.turrets.get(t) {
                    let r = frame_rect(f);
                    ui::fill(canvas, W, H, r, (255, 255, 255), 40);
                }
            }
        }
        for k in 0..28 {
            let f = if hot(Hit::Arrow(k)) { self.arrows.get(k + 28) } else { self.arrows.get(k) };
            self.put(canvas, f);
        }
        for side in 0..2 {
            let dx = side as i32 * SIDE_DX;
            let s = &self.setup.sides[side];
            for (row, v) in s.air.iter().enumerate() {
                if let Some(f) = self.arrows.get(side * 14 + row * 2) {
                    self.number(canvas, 265 + dx, frame_rect(f).y + 10, *v);
                }
            }
            for (row, v) in [(5, s.credits), (6, s.research)] {
                if let Some(f) = self.arrows.get(side * 14 + row * 2) {
                    self.number(canvas, 365 + dx, frame_rect(f).y + 10, v);
                }
            }
            self.text(canvas, self.string(276), 332 + dx, 43, (247, 239, 99));
            self.text(canvas, self.string(277), 323 + dx + side as i32 * 4, 100, white);
        }
        for (s, f) in [(0, O_BLUE), (1, O_GREEN)] {
            // the local side's selector stays lit
            let lit = self.local == s || hot(Hit::Side(s));
            let f = if lit { f + 1 } else { f };
            self.put(canvas, self.others.get(f));
            self.relabel(canvas, self.others.get(f), self.string(274 + s), if s == 0 { (120, 150, 255) } else { (110, 230, 110) }, lit);
        }
        if self.local == 0 {
            self.put(canvas, self.others.get(O_ACCEPT));
            self.relabel(canvas, self.others.get(O_ACCEPT), self.string(353), white, hot(Hit::Accept));
        }
        self.put(canvas, self.others.get(O_BACK));
        self.relabel(canvas, self.others.get(O_BACK), self.string(354), white, hot(Hit::Back));
        // the map: the host picks from the list, a guest sees "Selected level:" and the name
        if self.local == 0 {
            for (i, m) in self.maps.iter().enumerate() {
                let colour = if hot(Hit::Map(i)) {
                    (255, 0, 0)
                } else if i == self.map {
                    (255, 220, 0)
                } else {
                    white
                };
                let y = MAP_ROWS_Y + i as i32 * MAP_ROW_H;
                self.text(canvas, m, MAP_ROWS_X, y, colour);
            }
        } else {
            self.text(canvas, self.string(263), MAP_ROWS_X, MAP_ROWS_Y, (0xeb, 0xeb, 0xeb));
            if let Some(m) = self.maps.get(self.map) {
                self.text(canvas, m, MAP_ROWS_X, MAP_ROWS_Y + 20, white);
            }
        }
        // chat: the last lines, blue (leading `1`) or green (`2`) with the digit dropped
        let start = self.chat.len().saturating_sub(CHAT_LINES);
        for (i, line) in self.chat[start..].iter().enumerate() {
            let (colour, body) = match line.chars().next() {
                Some('1') => ((0x80, 0x80, 0xff), &line[1..]),
                Some('2') => ((0, 0xff, 0), &line[1..]),
                _ => (white, &line[..]),
            };
            self.text(canvas, body, 232, 345 + 15 * i as i32, colour);
        }
        if !self.input.is_empty() {
            self.text(canvas, &format!("{}_", self.input), 232, 452, white);
        }
    }

    pub fn draw(&mut self, buf: &mut [u32], w: usize, h: usize) {
        opensow_viewer::record::cpu(|| self.render());
        crate::ui::clear(buf, w, h, 0);
        // the picture behind the 640x480 screen: `NetworkBackground(2).srf`, centre-cropped to the window in the exe (`0x446ca0`), stretched here
        if let Some(bg) = &self.bg_full[usize::from(self.page == Page::Lobby)] {
            crate::ui::stretch(buf, w, h, bg, false);
        }
        let (ox, oy, s) = fit(w, h);
        crate::ui::page(buf, w, h, &self.canvas, W, H, ox as usize, oy as usize, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_tcp_ip_provider_is_found_by_its_name() {
        assert_eq!(preferred_connection(&names(&STUB_CONNECTIONS)), 0);
        let reordered = names(&["IPX Connection For DirectPlay", "Modem Connection For DirectPlay", "Internet TCP/IP Connection For DirectPlay"]);
        assert_eq!(preferred_connection(&reordered), 2);
        assert_eq!(preferred_connection(&names(&["Serial Connection For DirectPlay"])), 0);
        // shorter than 36 characters: not taken even with TCP/I in place
        assert_eq!(preferred_connection(&names(&["xxxxxxxxxTCP/Ixxx", "yyyyyyyyyTCP/Iyyyyyyyyyyyyyyyyyyyyyyyyyyy"])), 1);
    }

    #[test]
    fn arrows_follow_the_jump_table() {
        let mut s = Setup::default();
        s.sides[0].air = [5; 5];
        for k in 0..28 {
            assert!(s.arrow(k, 0) == (k < 14), "only the local side's arrows act (arrow {k})");
        }
        // every pair: "-" then "+" for a count row leaves it unchanged
        assert_eq!(s.sides[0].air, [5; 5]);
        assert_eq!(s.sides[0].credits, 500, "-500 is refused at 0, +500 follows");
        assert_eq!(s.sides[0].research, 100, "-100 is refused at 0, +100 follows");
        assert_eq!(s.sides[1], SideSetup::default());
        // the green side's arrows act for the green player
        s.arrow(15, 1);
        assert_eq!(s.sides[1].air[0], 1);
        s.arrow(14, 1);
        s.arrow(14, 1);
        assert_eq!(s.sides[1].air[0], 0);
        s.arrow(25, 1);
        assert_eq!(s.sides[1].credits, 500);
        s.arrow(27, 1);
        assert_eq!(s.sides[1].research, 100);
    }

    #[test]
    fn values_clamp_at_the_exes_limits() {
        let mut s = Setup::default();
        for _ in 0..40 {
            s.arrow(1, 0);
            s.arrow(11, 0);
            s.arrow(13, 0);
        }
        assert_eq!(s.sides[0].air[0], 20);
        assert_eq!(s.sides[0].credits, 19000);
        assert_eq!(s.sides[0].research, 2000);
        s.sides[0].credits = 800;
        s.arrow(10, 0);
        assert_eq!(s.sides[0].credits, 300);
        s.arrow(10, 0);
        assert_eq!(s.sides[0].credits, -200, "`> 0` is tested before subtracting, so this goes below zero like in the exe");
        s.arrow(10, 0);
        assert_eq!(s.sides[0].credits, -200);
    }

    #[test]
    fn rows_map_to_the_headers_air_groups() {
        // fighter = group 4 (last), bomber = group 0, tripler = group 3, carryall = group 2, asteroid = group 1
        assert_eq!(ROW_GROUP, [4, 0, 3, 2, 1]);
        assert!(Level::parse(&[]).is_err(), "an empty file is no level");
    }

    #[test]
    fn digits_are_centred_twelve_apart() {
        // the layout `0x4467b0`: 5 digits at -24, -12, 0, 12, 24; 4 at -18, -6, 6, 18; 2 at -6, 6
        for (n, expect) in [(1, vec![0]), (2, vec![-6, 6]), (3, vec![-12, 0, 12]), (4, vec![-18, -6, 6, 18]), (5, vec![-24, -12, 0, 12, 24])] {
            let got: Vec<i32> = (0..n).map(|i| (i * 2 - (n - 1)) * 6).collect();
            assert_eq!(got, expect);
        }
    }

    fn menu() -> Option<NetMenu> {
        let mut assets = Assets::load(opensow_formats::GameDir::locate()?).ok()?;
        Some(NetMenu::new(&mut assets))
    }

    #[test]
    fn the_connection_page_is_skipped_with_a_tcp_ip_provider() {
        let Some(mut m) = menu() else { return };
        assert_eq!(m.page, Page::Sessions);
        // without any connection the page stays and CONTINUE is not offered
        let mut assets = Assets::load(opensow_formats::GameDir::locate().unwrap()).unwrap();
        let empty = NetMenu::with_connections(&mut assets, Vec::new());
        assert_eq!(empty.page, Page::Connections);
        assert_eq!(empty.hit_at(empty.centre(Hit::Continue)), None);
        m.press(Hit::Create);
        assert_eq!(m.page, Page::Lobby);
    }

    #[test]
    fn host_flow_through_the_lobby() {
        let Some(mut m) = menu() else { return };
        assert_eq!(m.hit_at(m.centre(Hit::Join)), None, "JOIN needs a selected session");
        assert_eq!(m.press(Hit::Create), Event::None);
        assert_eq!((m.page, m.local), (Page::Lobby, 0));
        assert!(m.maps.len() >= 8 && m.maps[0].starts_with("01"), "{:?}", m.maps);
        // the map's header gives the defaults (First Spark: 800 credits each)
        assert_eq!((m.setup.sides[0].credits, m.setup.sides[1].credits), (800, 800));
        m.sounds.clear();
        for hit in [Hit::Arrow(1), Hit::Arrow(11), Hit::Arrow(11), Hit::Arrow(15)] {
            let c = m.centre(hit);
            assert_eq!(m.hit_at(c), Some(hit));
            m.press(hit);
        }
        assert_eq!(m.setup.sides[0].air[0], 1);
        assert_eq!(m.setup.sides[0].credits, 1800);
        assert_eq!(m.setup.sides[1].air[0], 0, "the green arrows are not the host's");
        assert_eq!(m.sounds, vec![33; 4]);
        // a turret toggle of the own side only
        m.press(Hit::Turret(2));
        m.press(Hit::Turret(7));
        assert_eq!(m.setup.sides[0].turrets, [true, true, false, true, true]);
        assert_eq!(m.setup.sides[1].turrets, [true; 5]);
        // picking another map reloads the defaults
        m.press(Hit::Map(7));
        assert_eq!(m.maps[m.map], "08-Island of Fate");
        assert!(m.setup.sides[0].air.iter().sum::<i32>() > 0, "Island of Fate starts with fighters");
        assert_eq!(m.setup.sides[0].turrets, [true; 5]);
        // chat
        m.peer_joined();
        for c in "gl".chars() {
            m.typed(c);
        }
        m.enter();
        assert_eq!(m.chat.len(), 2);
        assert!(m.chat[1].starts_with("1Player1: gl"));
        assert!(m.sounds.contains(&29));
        // ACCEPT starts the chosen map; BACK leaves
        let Event::Start { map, local, setup } = m.press(Hit::Accept) else { panic!() };
        assert_eq!((map.as_str(), local), ("08-Island of Fate", 0));
        assert_eq!(setup, m.setup);
        assert_eq!(m.sounds.last(), Some(&38));
        assert_eq!(m.press(Hit::Back), Event::Leave);
        assert_eq!(m.sounds.last(), Some(&39));
    }

    #[test]
    fn a_guest_sees_the_map_and_edits_only_the_green_side() {
        let Some(mut m) = menu() else { return };
        m.set_sessions(vec![SESSION_NAME.into()]);
        let c = m.centre(Hit::Session(0));
        assert_eq!(m.hit_at(c), Some(Hit::Session(0)));
        m.press(Hit::Session(0));
        assert_eq!(m.hit_at(m.centre(Hit::Join)), Some(Hit::Join));
        m.press(Hit::Join);
        assert_eq!((m.page, m.local), (Page::Lobby, 1));
        assert!(!matches!(m.hit_at(m.centre(Hit::Accept)), Some(Hit::Accept)), "only the host has ACCEPT");
        assert_eq!(m.press(Hit::Accept), Event::None);
        let before = m.setup.clone();
        m.press(Hit::Arrow(1));
        m.press(Hit::Turret(0));
        assert_eq!(m.setup, before, "the blue side belongs to the host");
        m.press(Hit::Arrow(15));
        assert_eq!(m.setup.sides[1].air[0], before.sides[1].air[0] + 1);
        m.press(Hit::Turret(5));
        assert!(!m.setup.sides[1].turrets[0]);
        assert_eq!(m.hit_at(m.centre(Hit::Map(0))), None, "no map list for a guest");
    }

    #[test]
    fn escape_leaves_from_every_page() {
        let Some(mut m) = menu() else { return };
        for page in [Page::Sessions, Page::Lobby] {
            m.page = page;
            assert_eq!(m.escape(), Event::Leave);
        }
    }
}
