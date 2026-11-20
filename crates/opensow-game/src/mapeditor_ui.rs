//! The in-game map editor (main menu MAP EDITOR, docs/map-editor-ingame.md; not in the original). Two pages in the skirmish
//! screen's look: the start page (the player's maps on the left, a new map's terrain on the right) and the editor (the map on the
//! left, a sidebar on the right: name and save, the palette to place buildings / turrets / units, both sides' starting resources,
//! and what the selected object can change). The editing state is the shared `opensow_viewer::editor::Editor`; files are
//! `formats::custommap` maps in the maps folder (`custom_maps.rs`).
//!
//! Every control is a widget with a rectangle: the same list is drawn and hit-tested, so the two never disagree.
//!
//! This file holds the editor's state and what changes it (opening, saving, the mission settings). The rest of
//! `MapEditor` is in submodules: `layout` (where each widget sits), `input` (mouse, keys and what a press does) and
//! `draw`.

mod draw;
mod input;
mod layout;
use crate::assets::{load_backdrop, Assets};
use crate::custom_maps::{self, Entry};
use crate::page::{
    self, capitalised, hud_icon, outline, rect, thing_picture, turret_picture, Ink, Picture, ACCENT, CHOSEN, EDGE, MUTED, NEUTRAL, SIDE, T, TEXT, WARN,
};
use crate::textinput::{self, TextField};
use crate::tr::Tr;
use crate::ui::{self, Image, Rect};
use opensow_formats::custommap::{self, CustomMap, ObjectExt, Sides, Terrain};
use opensow_formats::kinds;
use opensow_formats::lang::LangFile;
use opensow_formats::level::Level;
use opensow_formats::maptweak::{Production, Side, Thing};
use opensow_formats::GameDir;
use opensow_viewer::editor::{self, Editor, LineChange, Lines, Sel};
use opensow_viewer::render::{Overlays, View};
use opensow_viewer::scene::{Scene, Thumbnail};
use std::path::PathBuf;

/// The air-force pictures in the skirmish screen's order (fighter, bomber, tripler, carryall, asteroid) and each one's group in the
/// level header (`air::HEADER_ORDER`: bomber, asteroid, carryall, tripler, fighter).
const AIR_SPRITES: [&str; 5] = ["specials\\fighter", "specials\\bomber_r", "specials\\tripler", "specials\\carryAll", "specials\\meteor_medium"];
const AIR_GROUP: [usize; 5] = [4, 0, 3, 2, 1];
const MAX_AIR: i32 = crate::multiplayer::MAX_AIR;
const MAX_CREDITS: i32 = crate::multiplayer::MAX_CREDITS;
const MAX_RESEARCH: i32 = crate::multiplayer::MAX_RESEARCH;
/// Language-file strings (`LangFile` indices): CREDITS, RESEARCH, Advancer. Names: units 173-187, walkers 188-192, Advancer 193,
/// Rogon bomb 194, turrets 195-199, air forces 200-204, buildings 205-213, upgrade levels 219-227 (gold, energy, research).
const CREDITS: usize = 275;
const RESEARCH: usize = 276;
const ADVANCER: usize = 193;
const SATELLITE: usize = 145;
/// The countdown's minutes: the stepper's step and range, and what a new countdown starts with.
const MINUTE_STEP: i32 = 5;
const MAX_MINUTES: i32 = 95;
const DEFAULT_MINUTES: i32 = 20;
/// One wheel notch zooms by this factor; the change is eased in at `ZOOM_EASE` (per second, exponential); closest zoom.
const ZOOM_STEP: f32 = 1.08;
const ZOOM_EASE: f32 = 12.0;
const MAX_ZOOM: f32 = 2.0;
/// Terrain thumbnails are reduced to this size once.
const THUMB: usize = 240;

/// Every control of both pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    // start page
    Row(usize),
    OpenMap,
    /// Deletes the selected map; the first press only asks (the button then reads "sure?").
    DeleteMap,
    OpenFolder,
    Edition(bool),
    Terrain(usize),
    Create,
    Back,
    // editor: top
    Name,
    Save,
    Undo,
    Redo,
    /// Nothing selected and the sidebar too short for all cards: show the palette (0), the sides (1) or the mission (2).
    Content(usize),
    // editor: the mission (`custommap::Mission`, `Timer`)
    /// The countdown: none, hold out (0:00 = victory), time limit (0:00 = defeat), reinforcements (0:00 = they arrive).
    Countdown(usize),
    Minutes(bool),
    /// What wins: 0 the enemy's headquarters gone, 1 everything of the enemy gone, 2 the convoy at the goal.
    WinRule(usize),
    // editor: what a reinforcement countdown brings (`plus`)
    RCredits(bool),
    RResearch(bool),
    /// Air force (skirmish order), plus.
    RAir(usize, bool),
    /// Arms the map click that adds a carryall landing point; removes the last one.
    DropAdd,
    /// Landing point `k`: the previous / next squad, delete it.
    DropSquad(usize, bool),
    DropDelete(usize),
    // editor: the escort (`custommap::Escort`): arm the goal tool, the waypoint tool, remove the last waypoint
    GoalSet,
    WaypointAdd,
    WaypointRemove,
    /// Show the cells where nothing can be built (key B).
    BuildAreas,
    // editor: palette
    Owner(usize),
    Tab(usize),
    Palette(usize),
    // editor: sides (`side`, `plus`)
    Credits(usize, bool),
    Research(usize, bool),
    /// Air force row (skirmish order), side, plus.
    Air(usize, usize, bool),
    Turret(usize, usize),
    // editor: the selection
    Done,
    Delete,
    Faction(usize),
    Hp(bool),
    HpDefault,
    TurretType(i32),
    HqAdvancer(bool),
    /// Satellite protection of the selected building or turret (object word 15).
    Satellite(bool),
    LineKind(usize, bool),
    LineTiers(usize),
    LineLock(usize),
    LineUnlock(usize, bool),
    LineRemove(usize),
    AddLine,
}

/// What the editor asks the game to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    None,
    /// Back to the main menu.
    Leave,
    /// Back to the skirmish screen (the editor was opened from its Custom tab).
    Skirmish,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PicRef {
    Palette(usize, usize),
    Line(i32),
    Air(usize),
    Turret(usize),
    Gold,
    Bulb,
    Advancer,
    Thumb(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Glyph {
    Cross,
    Undo,
    Redo,
    Lock(bool),
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
enum Look {
    /// A framed panel: its heading, and in window pixels the heading strip's height, the heading's size and the inset.
    Panel(String, i32, i32, i32),
    Text(String, i32, (u8, u8, u8)),
    TextCentred(String, i32, (u8, u8, u8)),
    TextRight(String, i32, (u8, u8, u8)),
    Button(String, i32, bool),
    Chip(String, i32, bool, (u8, u8, u8)),
    Stepper(String, i32, (u8, u8, u8), Hit, Hit),
    Pic(PicRef, bool),
    Card(bool),
    Field,
    Glyph(Glyph, (u8, u8, u8)),
    Toggle(bool, (u8, u8, u8)),
    Row(bool),
    Scroll(i32, i32, i32),
}

#[derive(Debug, Clone)]
struct W {
    r: Rect,
    hit: Option<Hit>,
    look: Look,
}

/// While the left button is down on the map: panning the view, or moving the selection (`grab` = cursor cell - its cell).
#[derive(Debug, Clone, Copy)]
enum Grab {
    Pan(i32, i32),
    Move((i32, i32), (i32, i32)),
}

/// One map open in the editor.
struct Session {
    ed: Editor,
    scene: Scene,
    terrain: Terrain,
    source: GameDir,
    name: TextField,
    /// Who the human plays and what wins (`custommap::Mission`; the countdown is in the level header).
    mission: custommap::Mission,
    /// What a reinforcement countdown brings (`custommap::Reinforcement`), and whether a map click adds a landing point.
    reinforcement: custommap::Reinforcement,
    tool: Option<MapTool>,
    /// The convoy's goal and waypoints (`custommap::Escort`).
    escort: custommap::Escort,
    /// The file the map came from or was last saved to.
    path: Option<PathBuf>,
    /// The file's bytes as last saved or loaded (empty: never saved), to tell unsaved changes.
    saved: Vec<u8>,
    view: View,
    fitted: bool,
    grab: Option<Grab>,
    /// The wheel's zoom on its way: the zoom aimed at and the window point that stays put (`MapEditor::update` eases towards it).
    zoom_to: Option<(f32, (i32, i32))>,
    tab: usize,
    /// When the sidebar is too short for the palette and the sides together: which one shows.
    content: usize,
    /// Sidebar scroll (design pixels) when the selection's card is taller than the window.
    scroll: i32,
    message: Option<(String, (u8, u8, u8))>,
    leave_armed: bool,
    /// The blocked / no-build overlay on the map (also shown while a building or turret is armed).
    areas: bool,
    overwrite_armed: Option<PathBuf>,
    /// The generated layout the ground was built from (docs/generated-maps.md), kept for saving.
    layout: Option<custommap::Layout>,
}

impl Session {
    fn map(&self) -> CustomMap {
        CustomMap {
            name: custommap::clean_name(&self.name.text),
            warmonger: self.terrain.warmonger,
            level: self.ed.level().clone(),
            ext: self.ed.ext.clone(),
            mission: self.mission,
            reinforcement: self.reinforcement.clone(),
            escort: self.escort.clone(),
            layout: self.layout.clone(),
        }
    }

    fn dirty(&self) -> bool {
        self.saved.is_empty() || self.map().to_bytes() != self.saved
    }
}

/// The in-game map editor: the start page (the player's maps, a new map's terrain) and the editing session.
pub struct MapEditor {
    game: GameDir,
    tr: Tr,
    strings: Option<LangFile>,
    ink: Ink,
    bg: Option<Image>,
    /// Palette pictures per owner (neutral, blue, green), in `editor::palette()` order.
    pics: [Vec<Picture>; 3],
    /// `extras\\ikony` frames as pictures: the HUD's production-line icons, by line kind.
    lines: Vec<Picture>,
    air: Vec<Picture>,
    turrets: Vec<Picture>,
    icons: [Option<Picture>; 2],
    advancer: Picture,
    /// The maps folder (`custom_maps::dir`; tests use their own).
    folder: PathBuf,
    // start page
    /// The maps found in the maps folder.
    pub maps: Vec<Entry>,
    list_scroll: usize,
    list_sel: Option<usize>,
    /// The Delete button was pressed once for the selected map.
    delete_armed: bool,
    terrains: Vec<Terrain>,
    thumbs: Vec<Option<Thumbnail>>,
    /// Index of the terrain chosen for a new map.
    pub terrain: usize,
    warmonger_tab: bool,
    // editor
    s: Option<Session>,
    /// Opened from the skirmish screen's Custom tab: Back returns there.
    from_skirmish: bool,
    hover: Option<Hit>,
    cursor: (i32, i32),
    pressed: bool,
    ctrl: bool,
    /// Camera keys held: left, right, up, down (arrows or W A S D); `update` moves the view while they are down.
    held: [bool; 4],
    /// The window size of the last frame (the view is clamped to it while keys scroll).
    size: (usize, usize),
    canvas: Vec<u32>,
    map_buf: Vec<u32>,
    /// Effect ids for the game to play: 30 hover, 33 small controls, 38 create / save, 39 the other buttons.
    pub sounds: Vec<u8>,
}

/// A map click tool of the mission card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapTool {
    Landing,
    Goal,
    Waypoint,
}

/// Language-file string of the Rogon pod's name (the convoy's unit).
const POD_NAME: usize = 194;

fn win_index(w: custommap::WinRule) -> usize {
    match w {
        custommap::WinRule::Headquarters => 0,
        custommap::WinRule::Everything => 1,
        custommap::WinRule::Escort => 2,
    }
}

/// The legs of an escort the convoy cannot walk (`custom_maps::route_issues`); nothing without the escort rule.
fn escort_issues(s: &Session) -> Vec<Option<usize>> {
    if s.mission.win != custommap::WinRule::Escort {
        return Vec::new();
    }
    custom_maps::route_issues(&s.ed.walkable(), s.ed.grid_size(), s.ed.level(), &s.escort)
}

/// Refused: a drop under the pointer that may not go there, a route leg the convoy cannot walk.
const RED: (u8, u8, u8) = (230, 60, 50);

/// A dotted line from `a` to `b` (window pixels): 3x3 dots every 7 pixels.
fn dotted(buf: &mut [u32], w: usize, h: usize, a: (i32, i32), b: (i32, i32), col: (u8, u8, u8)) {
    let n = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) / 7).max(1);
    for k in 0..=n {
        let (x, y) = (a.0 + (b.0 - a.0) * k / n, a.1 + (b.1 - a.1) * k / n);
        ui::fill(buf, w, h, Rect { x: x - 1, y: y - 1, w: 3, h: 3 }, col, 255);
    }
}

/// Why a landing point is refused or flagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandingWhy {
    /// Some of the 4x2 cells are blocked ground, outside the map or under a building or turret.
    Ground,
    /// It shares cells with another landing point.
    Overlap,
}

/// Height of one landing point's row on the mission card (design pixels).
const DROP_ROW: i32 = 92;

/// Why no carryall can land a whole squad at `cell` (`skip`: the landing point being checked, left out of the overlap test).
fn landing_refusal(s: &Session, cell: (i32, i32), skip: Option<usize>) -> Option<LandingWhy> {
    if s.ed.can_land(cell).is_err() {
        return Some(LandingWhy::Ground);
    }
    s.reinforcement.drops.iter().enumerate().any(|(j, d)| Some(j) != skip && custommap::landings_overlap(d.cell, cell)).then_some(LandingWhy::Overlap)
}

/// The problem of landing point `k` (overlaps count against the earlier point only, so one of two is flagged).
fn landing_issue(s: &Session, k: usize) -> Option<LandingWhy> {
    let cell = s.reinforcement.drops.get(k)?.cell;
    if s.ed.can_land(cell).is_err() {
        return Some(LandingWhy::Ground);
    }
    s.reinforcement.drops[..k].iter().any(|d| custommap::landings_overlap(d.cell, cell)).then_some(LandingWhy::Overlap)
}

/// Every landing point's problem, when the countdown brings reinforcements.
fn landing_issues(s: &Session) -> Vec<(usize, LandingWhy)> {
    if countdown_mode(custommap::Timer::read(s.ed.level())) != 3 {
        return Vec::new();
    }
    (0..s.reinforcement.drops.len()).filter_map(|k| landing_issue(s, k).map(|w| (k, w))).collect()
}

/// The countdown chip that is on: 0 none, 1 hold out, 2 time limit, 3 reinforcements.
fn countdown_mode(timer: Option<custommap::Timer>) -> usize {
    match timer.map(|t| t.end) {
        None => 0,
        Some(custommap::TimerEnd::Victory) => 1,
        Some(custommap::TimerEnd::Defeat) => 2,
        Some(custommap::TimerEnd::Reinforcements) => 3,
    }
}

/// The palette without the Advancer (each headquarters' Advancer is a switch of the headquarters, `custommap::sync_advancers`).
fn items() -> Vec<Thing> {
    editor::palette().into_iter().filter(|&t| t != Thing::Unit(40)).collect()
}

/// Palette tab of a thing: 0 buildings, 1 turrets, 2 units.
fn tab_of(t: Thing) -> usize {
    match t {
        Thing::Object(k) if kinds::TURRETS.contains(&k) => 1,
        Thing::Object(_) => 0,
        Thing::Unit(_) => 2,
    }
}

fn side_index(s: Side) -> usize {
    match s {
        Side::Neutral => 0,
        Side::Blue => 1,
        Side::Green => 2,
    }
}

const SIDES3: [Side; 3] = [Side::Neutral, Side::Blue, Side::Green];

/// Design sizes of the two layouts: (design w, h, sidebar width, head / body / small text, pad, wide).
#[derive(Clone, Copy)]
struct Geo {
    d: (i32, i32),
    side: i32,
    head: i32,
    body: i32,
    small: i32,
    pad: i32,
    wide: bool,
}

impl Geo {
    fn for_window(w: usize, h: usize) -> Geo {
        if w >= 1200 && h >= 720 {
            Geo { d: (1920, 1080), side: 480, head: 24, body: 20, small: 16, pad: 14, wide: true }
        } else {
            Geo { d: (1024, 768), side: 360, head: 20, body: 18, small: 15, pad: 10, wide: false }
        }
    }
}

/// Rows the start page's map list shows: the list panel (wide 800 design px high with 50 px rows, compact 608 / 40) less its
/// heading and the Open / Open folder row.
fn start_rows(g: &Geo) -> usize {
    let (h, row_h, hh) = if g.wide { (800, 50, 58) } else { (608, 40, 44) };
    ((h - hh - (2 * row_h + g.pad) - g.pad / 2) / row_h).max(1) as usize
}

/// The sidebar's transform: design pixels scaled like the page, the panel at the window's right edge, top aligned.
fn side_t(g: &Geo, w: usize, h: usize) -> T {
    let s = T::new(g.d, w, h).s;
    T { s, ox: w as f32 - g.side as f32 * s, oy: 0.0 }
}

/// Width of the map's part of the editor page.
fn map_width(w: usize, h: usize) -> usize {
    let g = Geo::for_window(w, h);
    (w as f32 - g.side as f32 * side_t(&g, w, h).s).max(0.0) as usize
}

/// The zoom that shows the whole map in the map's part of the window: the start view and the farthest the wheel zooms out
/// (further out the map would only shrink inside an empty frame).
fn fit_zoom(scene: &Scene, mw: usize, h: usize) -> f32 {
    (mw as f32 / scene.bg_w.max(1) as f32).min(h as f32 / scene.bg_h.max(1) as f32).min(MAX_ZOOM)
}

/// Builds the sidebar top-down in design pixels.
struct L<'a> {
    out: Vec<W>,
    t: &'a T,
    g: Geo,
    x0: i32,
    inner: i32,
    y: i32,
}

impl L<'_> {
    fn r(&self, x: i32, y: i32, w: i32, h: i32) -> Rect {
        self.t.r(rect(x, y, w, h))
    }
    fn push(&mut self, r: Rect, hit: Option<Hit>, look: Look) {
        self.out.push(W { r, hit, look });
    }
    fn px(&self, v: i32) -> i32 {
        self.t.px(v)
    }
    /// A small grey label on its own line.
    fn label(&mut self, s: &str) {
        let r = self.r(self.x0, self.y, self.inner, self.g.small + 6);
        self.push(r, None, Look::Text(s.to_string(), self.px(self.g.small), MUTED));
        self.y += self.g.small + 8;
    }
    /// `n` equal cells across the content width.
    fn cells(&self, n: i32, y: i32, h: i32, gap: i32) -> Vec<Rect> {
        let cw = (self.inner - (n - 1) * gap) / n;
        (0..n).map(|i| self.r(self.x0 + i * (cw + gap), y, cw, h)).collect()
    }
    /// A stepper: its look and its two arrows' hits.
    fn stepper(&mut self, r: Rect, value: String, col: (u8, u8, u8), minus: Hit, plus: Hit) {
        let (a, b) = page::stepper_arrows(r);
        let px = self.px(self.g.body);
        self.push(r, None, Look::Stepper(value, px, col, minus, plus));
        self.push(a, Some(minus), Look::Card(false));
        self.push(b, Some(plus), Look::Card(false));
    }
}

impl MapEditor {
    /// Opens the editor on its start page.
    pub fn new(assets: &mut Assets) -> MapEditor {
        let language = assets.game.language();
        let strings = assets
            .game
            .resolve(&format!("Data\\Language\\{}", language.file_name()))
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| LangFile::parse(&d).ok())
            .map(|f| f.with_supplement(language));
        let pics = SIDES3.map(|side| items().into_iter().map(|t| thing_picture(&mut assets.art, t, side)).collect());
        let hud = assets.art.load("extras\\ikony.ps6");
        let lines = hud.as_deref().map_or(Vec::new(), |set| set.iter().map(|f| Picture::of(vec![(f.clone(), 0, 0)])).collect());
        let icons = [18, 19].map(|k| hud.as_deref().and_then(|set| hud_icon(set, k)).map(|f| Picture::of(vec![(f, 0, 0)])));
        let air = AIR_SPRITES
            .iter()
            .map(|n| Picture::of(assets.art.load(&format!("{n}.ps6")).and_then(|set| set.first().cloned()).map(|f| (f, 0, 0)).into_iter().collect()))
            .collect();
        let turrets = (25..=29).map(|k| turret_picture(&mut assets.art, k)).collect();
        let advancer = thing_picture(&mut assets.art, Thing::Unit(40), Side::Blue);
        let terrains = custom_maps::terrains(&assets.game);
        let mut m = MapEditor {
            tr: Tr::new(language),
            strings,
            ink: Ink::new(assets.font("medium")),
            bg: load_backdrop(&assets.game, "NetworkBackground"),
            pics,
            lines,
            air,
            turrets,
            icons,
            advancer,
            folder: custom_maps::dir(),
            maps: Vec::new(),
            list_scroll: 0,
            list_sel: None,
            delete_armed: false,
            thumbs: (0..terrains.len()).map(|_| None).collect(),
            terrains,
            terrain: 0,
            warmonger_tab: false,
            s: None,
            from_skirmish: false,
            hover: None,
            cursor: (-1, -1),
            pressed: false,
            ctrl: false,
            held: [false; 4],
            size: (0, 0),
            canvas: Vec::new(),
            map_buf: Vec::new(),
            sounds: Vec::new(),
            game: assets.game.clone(),
        };
        m.rescan();
        m
    }

    fn string(&self, i: usize) -> String {
        self.strings.as_ref().and_then(|s| s.get(i)).map(|s| s.trim().to_owned()).unwrap_or_default()
    }

    /// The name the language file gives a building, turret or unit (the English editor name without one).
    fn thing_name(&self, t: Thing) -> String {
        let i = match t {
            Thing::Object(k @ 100..=108) => Some([205, 206, 207, 208, 210, 211, 212, 213, 209][(k - 100) as usize]),
            Thing::Object(k @ 25..=29) => Some(195 + (k - 25) as usize),
            Thing::Unit(k @ 1..=15) => Some(172 + k as usize),
            Thing::Unit(40) => Some(ADVANCER),
            Thing::Unit(k @ 41..=45) => Some(147 + k as usize),
            Thing::Unit(46) => Some(194),
            _ => None,
        };
        i.map(|i| self.string(i)).filter(|s| !s.is_empty()).unwrap_or_else(|| editor::display_name(t))
    }

    /// The name of a production line's kind (unit, upgrade level or air force) from the language file.
    fn line_name(&self, kind: i32) -> String {
        let i = match kind {
            1..=15 => Some(172 + kind as usize),
            16..=18 => Some(219 + (kind - 16) as usize),
            19..=21 => Some(225 + (kind - 19) as usize),
            22..=24 => Some(222 + (kind - 22) as usize),
            30..=34 => Some([200, 202, 204, 203, 201][(kind - 30) as usize]),
            41..=45 => Some(147 + kind as usize),
            46 => Some(194),
            _ => None,
        };
        i.map(|i| self.string(i)).filter(|s| !s.is_empty()).unwrap_or_else(|| editor::line_label(kind, 0))
    }

    /// Reads the maps folder again.
    pub fn rescan(&mut self) {
        self.maps = custom_maps::list_in(&self.folder);
        self.list_sel = self.list_sel.filter(|&i| i < self.maps.len());
    }

    #[cfg(test)]
    pub fn editing(&self) -> bool {
        self.s.is_some()
    }

    /// The map name field has the keyboard (the game then sends typed text, `text_input`).
    pub fn wants_text(&self) -> bool {
        self.s.as_ref().is_some_and(|s| s.name.focused)
    }

    /// Loads the terrain thumbnails not loaded yet; `budget` = how many at most (the start page loads a few per frame).
    pub fn load_thumbs(&mut self, budget: usize) {
        let mut n = 0;
        for i in 0..self.terrains.len() {
            if self.thumbs[i].is_some() {
                continue;
            }
            if n == budget {
                return;
            }
            let (g, m) = custom_maps::terrain_source(&self.game, self.terrains[i]);
            self.thumbs[i] = opensow_viewer::scene::thumbnail(&g, &m, THUMB, THUMB);
            n += 1;
        }
    }

    /// The terrains of the edition tab shown (indices into `terrains`).
    fn shown_terrains(&self) -> Vec<usize> {
        (0..self.terrains.len()).filter(|&i| self.terrains[i].warmonger == self.warmonger_tab).collect()
    }

    fn has_warmonger(&self) -> bool {
        self.terrains.iter().any(|t| t.warmonger)
    }

    /// Opens a new map on terrain `i` of `terrains`.
    pub fn create(&mut self, i: usize) -> Result<(), String> {
        let t = *self.terrains.get(i).ok_or("no such terrain")?;
        let map = CustomMap::new(self.tr.untitled(), t);
        self.open(map, None)
    }

    /// Opens a map file of the folder.
    pub fn open_file(&mut self, path: &std::path::Path) -> Result<(), String> {
        let map = custom_maps::load(path)?;
        self.open(map, Some(path.to_path_buf()))
    }

    fn open(&mut self, mut map: CustomMap, path: Option<PathBuf>) -> Result<(), String> {
        let terrain = map.terrain();
        let (source, mission) = custom_maps::terrain_source(&self.game, terrain);
        // the Advancers belong to the headquarters' switches; the file's markers are derived from them
        map.level.markers.retain(|m| !m.is_advancer());
        map.ext.resize(map.level.objects.len(), ObjectExt::default());
        let saved = if path.is_some() { map.to_bytes() } else { Vec::new() };
        let mut scene = Scene::load_with(&source, &mission, 0, None, Some(map.level.clone()))?;
        if map.layout.is_some() {
            let built = custom_maps::base_ground(&self.game, &map.ground(), scene.ground_copy(), &mut custom_maps::Sources::default());
            scene.set_ground(built);
        }
        let ed = Editor::custom(&map.name, map.level.clone(), map.ext.clone(), &scene.map);
        scene.set_level(&source, shown_level(&ed));
        let first = items().iter().position(|&t| t == Thing::Object(100)).unwrap_or(0);
        let mut ed = ed;
        ed.palette = editor::palette().iter().position(|&t| t == items()[first]).unwrap_or(0);
        ed.side = Side::Blue;
        let name = if map.name.is_empty() { self.tr.untitled().to_string() } else { map.name.clone() };
        self.s = Some(Session {
            ed,
            scene,
            terrain,
            source,
            name: TextField::new(&name, custommap::MAX_NAME),
            mission: map.mission,
            reinforcement: map.reinforcement.clone(),
            tool: None,
            escort: map.escort.clone(),
            path,
            saved,
            view: View { x: 0.0, y: 0.0, zoom: 1.0 },
            fitted: false,
            grab: None,
            zoom_to: None,
            tab: 0,
            content: 0,
            scroll: 0,
            message: None,
            leave_armed: false,
            areas: false,
            overwrite_armed: None,
            layout: map.layout.clone(),
        });
        Ok(())
    }

    /// Opened from the skirmish screen: Back goes there.
    pub fn open_from_skirmish(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.from_skirmish = true;
        self.open_file(path)
    }

    /// The map shown after an edit (the scene shows the derived Advancers too).
    fn refresh(&mut self) {
        if let Some(s) = self.s.as_mut() {
            s.scene.set_level(&s.source, shown_level(&s.ed));
            s.leave_armed = false;
            s.overwrite_armed = None;
            s.message = None;
        }
    }

    /// `dt` seconds passed; `key_speed` = the Options' KEYS SCROLL in screen pixels per second (`options::scroll_px_per_second`):
    /// held camera keys move the view smoothly, like the game's map (`Play::update`).
    pub fn update(&mut self, dt: f32, key_speed: f32) {
        let Some(s) = self.s.as_mut() else {
            self.load_thumbs(2);
            return;
        };
        // the wheel's zoom: a share of the way each frame (about 90 % in a fifth of a second), the point under the pointer kept
        if let Some((target, (ax, ay))) = s.zoom_to {
            let (mx, my) = (s.view.x + ax as f32 / s.view.zoom, s.view.y + ay as f32 / s.view.zoom);
            let k = 1.0 - (-dt * ZOOM_EASE).exp();
            let z = s.view.zoom + (target - s.view.zoom) * k;
            s.view.zoom = if (target - z).abs() < 0.0005 { target } else { z };
            if s.view.zoom == target {
                s.zoom_to = None;
            }
            s.view.x = mx - ax as f32 / s.view.zoom;
            s.view.y = my - ay as f32 / s.view.zoom;
            let (w, h) = self.size;
            if w > 0 {
                s.view.clamp(&s.scene, map_width(w, h), h);
            }
        }
        if s.name.focused {
            self.held = [false; 4];
            return;
        }
        let d = key_speed * dt / s.view.zoom;
        let dx = (i32::from(self.held[1]) - i32::from(self.held[0])) as f32 * d;
        let dy = (i32::from(self.held[3]) - i32::from(self.held[2])) as f32 * d;
        if dx != 0.0 || dy != 0.0 {
            s.view.x += dx;
            s.view.y += dy;
            let (w, h) = self.size;
            if w > 0 {
                s.view.clamp(&s.scene, map_width(w, h), h);
            }
        }
    }

    // ---- headless helpers ----

    /// Selects what stands on `cell` (headless screenshots).
    pub fn select_cell(&mut self, cell: (i32, i32)) {
        if let Some(s) = self.s.as_mut() {
            s.ed.sel = s.ed.pick((cell.0 as f32 + 0.5) * 32.0, (cell.1 as f32 + 0.5) * 32.0);
        }
    }

    /// Arms the palette item named like the internal editor's (`mine`, `turret2`, `unit cannon1`), headless.
    pub fn arm_tool(&mut self, name: &str) -> bool {
        let Some(s) = self.s.as_mut() else { return false };
        let Some(i) = editor::palette().iter().position(|&p| editor::thing_name(p).eq_ignore_ascii_case(name)) else { return false };
        s.ed.palette = i;
        s.ed.placing = true;
        s.tab = tab_of(editor::palette()[i]);
        true
    }

    /// Adds the armed palette item at `cell` for `side` (headless setup of demo maps).
    pub fn place_at(&mut self, cell: (i32, i32), side: Side) -> bool {
        let Some(s) = self.s.as_mut() else { return false };
        s.ed.side = side;
        let at = Editor::add_cell(editor::palette()[s.ed.palette], cell);
        let ok = s.ed.add(at);
        self.refresh();
        ok
    }

    /// Generates the map of `plan` (docs/generated-maps.md; the headless `--plan`, not offered on the start page) and opens it in
    /// the editor, not saved: terrain only, the players place the buildings. Returns what the generator measured.
    pub fn generate(&mut self, plan: &opensow_viewer::layout::Plan) -> Result<opensow_viewer::layout::Generated, String> {
        let load = |t: Terrain| {
            let (g, name) = custom_maps::terrain_source(&self.game, t);
            opensow_viewer::scene::load_ground(&g, &name)
        };
        let base = load(plan.base)?;
        let loaded: Vec<(Terrain, opensow_viewer::scene::Ground)> = plan.missions().into_iter().map(|t| load(t).map(|g| (t, g))).collect::<Result<_, _>>()?;
        let refs: Vec<(Terrain, &opensow_viewer::scene::Ground)> = loaded.iter().map(|(t, g)| (*t, g)).collect();
        // a wall's gaps depend on the seed: the next seeds are tried until every pocket keeps one entrance (at most 40)
        let mut made = opensow_viewer::layout::generate(&base, &refs, plan)?;
        for k in 1..40u64 {
            if made.sealed.iter().all(|s| s.unwrap_or(true)) {
                break;
            }
            let next = opensow_viewer::layout::Plan { seed: plan.seed + k, ..plan.clone() };
            made = opensow_viewer::layout::generate(&base, &refs, &next)?;
            made.seed = next.seed;
        }
        let mut map = CustomMap::new(&plan.name, plan.base);
        map.layout = Some(made.layout.clone());
        self.open(map, None)?;
        Ok(made)
    }

    /// Headless demos and tests: both sides' headquarters in opposite corners, neutral buildings between them, a turret and units.
    pub fn demo(&mut self) -> bool {
        let Some(s) = self.s.as_ref() else { return false };
        let (w, h) = (s.scene.map.width as i32, s.scene.map.height as i32);
        // cells where a 5x5 block fits, in reading order
        let free: Vec<(i32, i32)> = (4..h - 6)
            .step_by(3)
            .flat_map(|y| (4..w - 6).step_by(3).map(move |x| (x, y)))
            .filter(|&c| s.ed.can_place(Thing::Object(102), (c.0 - 2, c.1 - 2), None).is_ok())
            .collect();
        if free.len() < 8 {
            return false;
        }
        // the free cells nearest to a point first
        let near = |fx: i32, fy: i32| {
            let mut v = free.clone();
            v.sort_by_key(|c| (c.0 - fx).pow(2) + (c.1 - fy).pow(2));
            v
        };
        let plan: [(&str, (i32, i32), Side); 9] = [
            ("hq", (0, 0), Side::Blue),
            ("hq", (w, h), Side::Green),
            ("mine", (w / 2, h / 2), Side::Neutral),
            ("medium", (w / 4, h * 3 / 4), Side::Neutral),
            ("lab", (w * 3 / 4, h / 4), Side::Neutral),
            ("wind", (w / 2, h / 5), Side::Neutral),
            ("air", (w / 2, h * 4 / 5), Side::Neutral),
            ("turret2", (w / 5, h / 5), Side::Blue),
            ("turret2", (w * 4 / 5, h * 4 / 5), Side::Green),
        ];
        let mut placed = 0;
        for (tool, at, side) in plan {
            if self.arm_tool(tool) && near(at.0, at.1).into_iter().take(40).any(|c| self.place_at(c, side)) {
                placed += 1;
            }
        }
        for (k, (fx, fy, side)) in [(w / 6, h / 3, Side::Blue), (w * 5 / 6, h * 2 / 3, Side::Green)].into_iter().enumerate() {
            if self.arm_tool(if k == 0 { "unit cannon1" } else { "unit flamer2" }) {
                if let Some(c) = near(fx, fy).into_iter().take(40).find(|&c| self.place_at(c, side)) {
                    for d in 1..3 {
                        self.place_at((c.0 + d, c.1), side);
                    }
                }
            }
        }
        if let Some(s) = self.s.as_mut() {
            s.ed.placing = false;
            s.ed.sel = None;
            s.message = None;
        }
        placed == 9
    }

    /// Headless: the map's countdown and win rule (any minutes, also ones the card's stepper does not offer).
    pub fn set_mission(&mut self, timer: Option<custommap::Timer>, mission: custommap::Mission) {
        if let Some(s) = self.s.as_mut() {
            let mut level = s.ed.level().clone();
            custommap::Timer::write(timer, &mut level);
            s.ed.set_header(level.header);
            s.mission = mission;
        }
    }

    /// Headless: the convoy's goal and waypoints, and blue pods added at `pods` (cells a unit may stand on); false when one
    /// of the pods does not fit.
    pub fn set_escort(&mut self, escort: custommap::Escort, pods: &[(i32, i32)]) -> bool {
        let Some(s) = self.s.as_mut() else { return false };
        s.escort = escort;
        let (side, palette) = (s.ed.side, s.ed.palette);
        s.ed.side = Side::Blue;
        s.ed.palette = editor::palette().iter().position(|&t| t == Thing::Unit(custommap::POD)).unwrap_or(0);
        let ok = pods.iter().all(|&c| s.ed.add(Editor::add_cell(Thing::Unit(custommap::POD), c)));
        (s.ed.side, s.ed.palette, s.ed.sel) = (side, palette, None);
        self.refresh();
        ok
    }

    /// Headless: what a reinforcement countdown brings.
    pub fn set_reinforcement(&mut self, r: custommap::Reinforcement) {
        if let Some(s) = self.s.as_mut() {
            s.reinforcement = r;
        }
    }

    /// Headless: the name field's text (and focus).
    pub fn set_name(&mut self, name: &str, focus: bool) {
        if let Some(s) = self.s.as_mut() {
            s.name = TextField::new(name, custommap::MAX_NAME);
            s.name.focused = focus;
        }
    }

    /// Headless: the pointer at the centre of `cell` (shows the placement ghost / hover frame).
    pub fn point_at_cell(&mut self, cell: (i32, i32), w: usize, h: usize) {
        let Some(s) = self.s.as_mut() else { return };
        if !s.fitted {
            let mw = map_width(w, h);
            s.view = View { x: 0.0, y: 0.0, zoom: fit_zoom(&s.scene, mw, h) };
            s.view.clamp(&s.scene, mw, h);
            s.fitted = true;
        }
        let (x, y) = (((cell.0 as f32 + 0.5) * 32.0 - s.view.x) * s.view.zoom, ((cell.1 as f32 + 0.5) * 32.0 - s.view.y) * s.view.zoom);
        self.cursor = (x as i32, y as i32);
    }

    /// Headless: the build-areas overlay on or off.
    pub fn show_build_areas(&mut self, on: bool) {
        if let Some(s) = self.s.as_mut() {
            s.areas = on;
        }
    }

    /// Headless: the start page with map `i` of the list chosen.
    pub fn choose_row(&mut self, i: usize) {
        if i < self.maps.len() {
            self.list_sel = Some(i);
        }
    }

    /// Saves the open map (headless and the Save button).
    pub fn save(&mut self) -> Result<PathBuf, String> {
        self.save_in(&custom_maps::dir())
    }

    fn save_in(&mut self, folder: &std::path::Path) -> Result<PathBuf, String> {
        let tr = self.tr;
        let Some(s) = self.s.as_mut() else { return Err("no map".into()) };
        let map = s.map();
        if map.name.is_empty() {
            s.message = Some((tr.enter_name().into(), WARN));
            return Err(tr.enter_name().into());
        }
        let path = custom_maps::path_for(folder, &map.name);
        let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        if path.exists() && s.path.as_deref() != Some(path.as_path()) && s.overwrite_armed.as_deref() != Some(path.as_path()) {
            s.overwrite_armed = Some(path.clone());
            s.message = Some((tr.overwrite(&file), WARN));
            return Err(tr.overwrite(&file));
        }
        match custom_maps::save_in(folder, &map) {
            Ok(p) => {
                s.saved = map.to_bytes();
                s.path = Some(p.clone());
                s.overwrite_armed = None;
                s.leave_armed = false;
                s.message = Some((tr.saved_to_folder().into(), ACCENT));
                Ok(p)
            }
            Err(e) => {
                s.message = Some((tr.cannot_save(&e), WARN));
                Err(e)
            }
        }
    }

    // ---- layout ----

    // ---- input ----

    // ---- drawing ----
}

/// The level the map shows: the editor's, with each headquarters' Advancer where the file will have it.
fn shown_level(ed: &Editor) -> Level {
    let mut l = ed.level().clone();
    custommap::sync_advancers(&mut l, &ed.ext);
    l
}

/// Tints the cells of `map` where no building or turret may stand: impassable ground red, the no-build layer orange (the same
/// rules as `Editor::can_place`); open ground stays as it is.
fn build_areas(buf: &mut [u32], w: usize, h: usize, map: &opensow_formats::map::Map, view: &View) {
    let (x0, y0) = (((view.x / 32.0).floor() as i32).max(0), ((view.y / 32.0).floor() as i32).max(0));
    let (x1, y1) = (((view.x + w as f32 / view.zoom) / 32.0).ceil() as i32, ((view.y + h as f32 / view.zoom) / 32.0).ceil() as i32);
    for cy in y0..y1.min(map.height as i32) {
        for cx in x0..x1.min(map.width as i32) {
            let Some(c) = map.cell(cx as u32, cy as u32) else { continue };
            let col = if c.blocked {
                (220, 40, 30)
            } else if c.no_build {
                (250, 160, 40)
            } else {
                continue;
            };
            let (sx, sy) = view.to_screen(cx as f32 * 32.0, cy as f32 * 32.0);
            let (ex, ey) = view.to_screen((cx + 1) as f32 * 32.0, (cy + 1) as f32 * 32.0);
            ui::fill(buf, w, h, Rect { x: sx, y: sy, w: ex - sx, h: ey - sy }, col, 110);
        }
    }
}

/// A padlock, open or closed.
fn lock(c: &mut [u32], w: usize, h: usize, r: Rect, locked: bool, col: (u8, u8, u8)) {
    let s = (r.h / 2).max(6);
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2 + s / 6);
    let body = Rect { x: cx - s / 2, y: cy - s / 6, w: s, h: s * 2 / 3 };
    ui::fill(c, w, h, body, col, 255);
    // the shackle: closed over the body, or swung up to the right
    let (sx, top) = if locked { (cx - s / 3, cy - s / 6 - s / 2) } else { (cx, cy - s / 6 - s / 2 - s / 5) };
    let sw = s * 2 / 3;
    ui::fill(c, w, h, Rect { x: sx, y: top, w: sw, h: 2 }, col, 255);
    ui::fill(c, w, h, Rect { x: sx, y: top, w: 2, h: s / 2 + if locked { 0 } else { s / 5 } }, col, 255);
    ui::fill(c, w, h, Rect { x: sx + sw - 2, y: top, w: 2, h: s / 2 }, col, 255);
}

#[cfg(test)]
pub(crate) mod tests;
