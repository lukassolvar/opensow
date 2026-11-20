//! The skirmish setup screen (main menu SKIRMISH, docs/skirmish.md): a full-window page of the port's own, not in the original.
//! Everything on it comes from the install: the `NetworkBackground` picture, the map's ground for the preview, the aircraft as the
//! game flies them and the turrets as they stand on the map (base + gun top), the HUD's gold bars and light bulb, and the
//! language file's names; panels, arrows,
//! toggles and stars are drawn flat. It is laid out in design pixels (1920x1080, or 1024x768 for small windows where three columns
//! would not fit) scaled to the window, and drawn with the main menu's face in every language.

use crate::assets::{load_backdrop, Assets};
use crate::multiplayer::{display_names, Setup};
use crate::page::{
    arrow, capitalised, hud_icon, mark, outline, rect, ring, star_shape, stepper_arrows, turret_picture, Ink, Picture, ACCENT, BLUE, GOLD, GREEN, MUTED, SIDE,
    T, TEXT, WARN,
};
use crate::sim::Difficulty;
use crate::tr::Tr;
use crate::ui::{self, Image, Rect};
use opensow_formats::lang::LangFile;
use opensow_formats::level::{Level, ObjectKind};

/// The in-game sprites of the air-force rows in `multiplayer::AIR_ROWS` order (fighter, bomber, tripler, carryall, asteroid; first
/// frame, the bomber flying right, docs/air.md); the cards carry no names, the pictures say it.
const AIR_SPRITES: [&str; 5] = ["specials\\fighter", "specials\\bomber_r", "specials\\tripler", "specials\\carryAll", "specials\\meteor_medium"];
/// `extras\\ikony` frames (the HUD's icons, docs/hud.md): three gold bars, one light bulb.
const GOLD_ICON: usize = 18;
const BULB_ICON: usize = 19;
/// Language-file strings: CREDITS, RESEARCH.
const CREDITS: usize = 275;
const RESEARCH: usize = 276;
/// Picture size the map preview is reduced to once (then scaled to the window).
const THUMB: usize = 720;

/// Every control of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Map(usize),
    MapPrev,
    MapNext,
    Swap,
    LevelDown,
    LevelUp,
    /// One of the 14 arrows of a side (`Setup::arrow` numbering within the side: 2 per row, "-" then "+"; rows 0-4 air forces,
    /// 5 credits, 6 research).
    Step(usize, usize),
    Turret(usize, usize),
    Back,
    Reset,
    Start,
    /// The map list's tabs: the shipped maps, the players' own (docs/map-editor-ingame.md).
    Tab(Tab),
    /// Custom tab: show the maps folder, edit the selected map.
    OpenFolder,
    Edit,
}

/// Which maps the list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Original,
    Custom,
}

/// What a press on the screen asks the game to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    None,
    Leave,
    Start,
    /// Open this custom map in the map editor.
    Edit(std::path::PathBuf),
}

/// The selected map's picture and script.
struct Preview {
    map: usize,
    thumb: opensow_viewer::scene::Thumbnail,
    level: Level,
}

/// Panel and control rectangles in design pixels.
struct Geo {
    d: (i32, i32),
    top: i32,
    title_px: i32,
    sub_px: i32,
    head_px: i32,
    body_px: i32,
    small_px: i32,
    pad: i32,
    /// Height of a panel's heading strip.
    hh: i32,
    badge: Rect,
    /// The map list (wide) or the map selector line (compact).
    list: Option<Rect>,
    /// Compact layout: the row of the list's tabs (and the Custom tab's folder / edit buttons) above the selector.
    tab_row: Option<Rect>,
    row_h: i32,
    selector: Option<Rect>,
    preview_panel: Rect,
    preview: Rect,
    facts_y: i32,
    facts_dy: i32,
    players: Rect,
    resources: Rect,
    air: Rect,
    turrets: Rect,
    bottom: i32,
    back: Rect,
    reset: Rect,
    start: Rect,
    note_x: i32,
}

impl Geo {
    fn wide() -> Geo {
        Geo {
            d: (1920, 1080),
            top: 96,
            title_px: 50,
            sub_px: 24,
            head_px: 24,
            body_px: 24,
            small_px: 18,
            pad: 20,
            hh: 58,
            badge: rect(1520, 24, 340, 48),
            list: Some(rect(60, 128, 420, 800)),
            tab_row: None,
            row_h: 50,
            selector: None,
            preview_panel: rect(510, 128, 560, 800),
            preview: rect(530, 190, 520, 520),
            facts_y: 730,
            facts_dy: 46,
            players: rect(1100, 128, 760, 206),
            resources: rect(1100, 350, 760, 170),
            air: rect(1100, 536, 760, 196),
            turrets: rect(1100, 748, 760, 180),
            bottom: 958,
            back: rect(60, 982, 220, 64),
            reset: rect(1240, 982, 260, 64),
            start: rect(1540, 976, 320, 76),
            note_x: 330,
        }
    }

    fn compact() -> Geo {
        Geo {
            d: (1024, 768),
            top: 60,
            title_px: 34,
            sub_px: 18,
            head_px: 20,
            body_px: 22,
            small_px: 17,
            pad: 12,
            hh: 40,
            badge: rect(744, 12, 264, 36),
            list: None,
            tab_row: Some(rect(16, 70, 420, 38)),
            row_h: 0,
            selector: Some(rect(16, 114, 420, 42)),
            preview_panel: rect(16, 164, 420, 516),
            preview: rect(54, 176, 344, 344),
            facts_y: 534,
            facts_dy: 34,
            players: rect(452, 72, 556, 132),
            resources: rect(452, 212, 556, 120),
            air: rect(452, 340, 556, 168),
            turrets: rect(452, 516, 556, 164),
            bottom: 692,
            back: rect(16, 704, 150, 50),
            reset: rect(640, 704, 172, 50),
            start: rect(824, 700, 184, 58),
            note_x: 182,
        }
    }

    /// Wide when three columns fit legibly.
    fn for_window(w: usize, h: usize) -> Geo {
        if w >= 1200 && h >= 720 {
            Geo::wide()
        } else {
            Geo::compact()
        }
    }

    /// The two player rows.
    fn player_rows(&self) -> [Rect; 2] {
        let p = self.players;
        let rh = (p.h - self.hh - self.pad) / 2 - 3;
        [0, 1].map(|i| rect(p.x + self.pad, p.y + self.hh + i * (rh + 6), p.w - 2 * self.pad, rh))
    }

    /// Credits and research rows.
    fn resource_rows(&self) -> [Rect; 2] {
        let p = self.resources;
        let rh = (p.h - self.hh - self.pad / 2) / 2;
        [0, 1].map(|i| rect(p.x + self.pad, p.y + self.hh + i * rh, p.w - 2 * self.pad, rh))
    }

    /// The x centre of a side's value column in the resources panel.
    fn side_col(&self, side: usize) -> i32 {
        self.resources.x + self.resources.w * [58, 84][side] / 100
    }

    /// Card `i` of the air forces or turrets panel.
    fn card(&self, panel: Rect, i: i32) -> Rect {
        let gap = self.pad / 2 + 2;
        let cw = (panel.w - 2 * self.pad - 4 * gap) / 5;
        rect(panel.x + self.pad + i * (cw + gap), panel.y + self.hh, cw, panel.h - self.hh - self.pad / 2 - 2)
    }

    /// The strip at the bottom of a card that holds its controls, split per side.
    fn card_controls(&self, card: Rect) -> [Rect; 2] {
        let ch = self.small_px * 3 / 2;
        let y = card.y + card.h - ch - 4;
        let half = (card.w - 12) / 2;
        [rect(card.x + 4, y, half, ch), rect(card.x + 8 + half, y, half, ch)]
    }

    /// A stepper's "-" and "+" areas at its two ends.
    fn stepper(r: Rect) -> (Rect, Rect) {
        stepper_arrows(r)
    }

    /// The difficulty stepper on the computer's row.
    fn level_stepper(&self) -> Rect {
        let row = self.player_rows()[1];
        // the compact rows are narrow: a shorter stepper further right leaves the side's name its room
        let (at, wide) = if self.list.is_some() { (50, 30) } else { (57, 24) };
        rect(row.x + row.w * at / 100, row.y + 4, row.w * wide / 100, row.h - 8)
    }

    fn swap_button(&self) -> Rect {
        let row = self.player_rows()[0];
        let w = row.w * 28 / 100;
        rect(row.x + row.w - w - 8, row.y + row.h / 6, w, row.h * 2 / 3)
    }

    /// A resource stepper of a side on row `r`.
    fn resource_stepper(&self, r: usize, side: usize) -> Rect {
        let row = self.resource_rows()[r];
        let w = self.resources.w * 22 / 100;
        rect(self.side_col(side) - w / 2, row.y + 4, w, row.h - 8)
    }

    /// Visible map rows of the list (the Custom tab keeps a row for its buttons).
    fn visible_rows_of(&self, tab: Tab) -> usize {
        let foot = if tab == Tab::Custom { self.row_h + self.pad / 2 } else { 0 };
        self.list.map_or(0, |l| ((l.h - self.hh - self.pad / 2 - foot) / self.row_h).max(1) as usize)
    }

    #[cfg(test)]
    fn visible_rows(&self) -> usize {
        self.visible_rows_of(Tab::Original)
    }

    /// The two tabs: in the list's heading (wide) or at the left of the tab row (compact).
    fn tabs(&self) -> [Rect; 2] {
        match (self.list, self.tab_row) {
            (Some(l), _) => {
                let tw = (l.w - 2 * self.pad - 8) / 2;
                [0, 1].map(|i| rect(l.x + self.pad + i * (tw + 8), l.y + 10, tw, self.hh - 18))
            }
            (None, Some(r)) => [0, 1].map(|i| rect(r.x + i * 128, r.y, 124, r.h)),
            _ => [rect(0, 0, 0, 0); 2],
        }
    }

    /// The Custom tab's folder and edit buttons: under the list (wide) or at the right of the tab row (compact).
    fn custom_buttons(&self) -> [Rect; 2] {
        match (self.list, self.tab_row) {
            (Some(l), _) => {
                let bw = (l.w - 2 * self.pad - 8) / 2;
                let y = l.y + l.h - self.pad / 2 - self.row_h;
                [0, 1].map(|i| rect(l.x + self.pad + i * (bw + 8), y, bw, self.row_h - 6))
            }
            (None, Some(r)) => [rect(r.x + 262, r.y, 76, r.h), rect(r.x + 344, r.y, 76, r.h)],
            _ => [rect(0, 0, 0, 0); 2],
        }
    }
}

/// The skirmish setup screen: map list, preview, both sides' resources, side and difficulty.
pub struct SkirmishMenu {
    /// The list's maps: level names (`data\levels\*.net` without the extension) on the Original tab, the stars' keys
    /// (`custom/<file stem>`) on the Custom tab; and what the list shows for each.
    pub maps: Vec<String>,
    /// What the list shows for each entry of `maps`.
    pub names: Vec<String>,
    /// The tab shown: the shipped maps or the player's custom maps.
    pub tab: Tab,
    /// The shipped maps' `maps` / `names` while the Custom tab shows.
    shipped: (Vec<String>, Vec<String>),
    /// The maps folder (`custom_maps.rs`), read when the Custom tab opens.
    pub custom: Vec<crate::custom_maps::Entry>,
    /// The maps folder (`custom_maps::dir`; tests use their own).
    custom_dir: std::path::PathBuf,
    /// The Custom tab's escort maps checked so far: whether the convoy cannot walk its route (`custom_maps::convoy_blocked`,
    /// which loads the terrain, so it runs when a map is chosen).
    convoy: std::collections::HashMap<usize, bool>,
    /// Index of the chosen map in `maps`.
    pub map: usize,
    /// Both sides' starting values as edited.
    pub setup: Setup,
    /// The selected map's own setup (`Setup::skirmish_default`); a win earns a star only while `setup` equals it.
    defaults: Setup,
    /// The human's side: 0 blue, 1 green.
    pub local: usize,
    /// The computer's strength.
    pub difficulty: Difficulty,
    stars: crate::stars::Stars,
    preview: Option<Preview>,
    /// First map row shown in the list.
    scroll: usize,
    game: opensow_formats::GameDir,
    tr: Tr,
    strings: Option<LangFile>,
    bg: Option<Image>,
    /// The card pictures: air forces, turrets.
    air: Vec<Picture>,
    turrets: Vec<Picture>,
    /// Credits and research: the HUD's gold bars and light bulb cut off their tile (`hud_icon`).
    icons: [Option<Picture>; 2],
    ink: Ink,
    hover: Option<Hit>,
    cursor: (i32, i32),
    /// Ticks shown so far (the hovered card's picture turns).
    tick: u32,
    canvas: Vec<u32>,
    /// Effect ids for the game to play: 30 hover, 33 arrows and toggles, 38 START, 39 the other buttons.
    pub sounds: Vec<u8>,
}

impl SkirmishMenu {
    /// Builds the screen; `stars` are the maps the player has already won.
    pub fn new(assets: &mut Assets, stars: &crate::stars::Stars) -> SkirmishMenu {
        let language = assets.game.language();
        let strings = assets
            .game
            .resolve(&format!("Data\\Language\\{}", language.file_name()))
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| LangFile::parse(&d).ok())
            .map(|f| f.with_supplement(language));
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
        let names = display_names(&maps, &assets.game);
        let air = AIR_SPRITES
            .iter()
            .map(|n| Picture::of(assets.art.load(&format!("{n}.ps6")).and_then(|set| set.first().cloned()).map(|f| (f, 0, 0)).into_iter().collect()))
            .collect();
        let turrets = (25..=29).map(|kind| turret_picture(&mut assets.art, kind)).collect();
        let hud = assets.art.load("extras\\ikony.ps6");
        let icons = [GOLD_ICON, BULB_ICON].map(|k| hud.as_deref().and_then(|set| hud_icon(set, k)).map(|f| Picture::of(vec![(f, 0, 0)])));
        let mut m = SkirmishMenu {
            shipped: (maps.clone(), names.clone()),
            maps,
            names,
            tab: Tab::Original,
            custom: Vec::new(),
            custom_dir: crate::custom_maps::dir(),
            convoy: std::collections::HashMap::new(),
            map: 0,
            setup: Setup::default(),
            defaults: Setup::default(),
            local: 0,
            difficulty: Difficulty::Normal,
            stars: stars.clone(),
            preview: None,
            scroll: 0,
            tr: Tr::new(language),
            strings,
            bg: load_backdrop(&assets.game, "NetworkBackground"),
            air,
            turrets,
            icons,
            ink: Ink::new(assets.font("medium")),
            hover: None,
            cursor: (-1, -1),
            tick: 0,
            canvas: Vec::new(),
            sounds: Vec::new(),
            game: assets.game.clone(),
        };
        m.select_map(0);
        m
    }

    /// Shows the shipped maps or the players' own (the folder is read again each time the Custom tab opens).
    pub fn set_tab(&mut self, tab: Tab) {
        if tab == Tab::Custom {
            self.custom = crate::custom_maps::list_in(&self.custom_dir);
            self.convoy.clear();
            self.maps = self.custom.iter().map(|e| e.star_key()).collect();
            self.names = self.custom.iter().map(|e| e.name.clone()).collect();
        } else {
            (self.maps, self.names) = self.shipped.clone();
        }
        self.tab = tab;
        self.scroll = 0;
        self.preview = None;
        self.map = 0;
        self.select_map(0);
    }

    /// The selected custom map's file, when the Custom tab shows one.
    pub fn custom_path(&self) -> Option<std::path::PathBuf> {
        (self.tab == Tab::Custom).then(|| self.custom.get(self.map).map(|e| e.path.clone())).flatten()
    }

    /// The selected custom map's mission in words (countdown, destroy-everything rule); `None` for a plain skirmish.
    pub fn objective(&self) -> Option<String> {
        let map = (self.tab == Tab::Custom).then(|| self.custom.get(self.map).and_then(|e| e.map.as_ref().ok())).flatten()?;
        self.preview.as_ref().filter(|p| p.map == self.map)?;
        self.tr.objective(map.timer(), map.mission.win)
    }

    /// Why the selected custom map cannot be played (`None` on the Original tab).
    pub fn blocker(&self) -> Option<crate::custom_maps::Blocker> {
        (self.tab == Tab::Custom).then(|| self.custom_blocker(self.map)).flatten()
    }

    /// Why custom map `i` cannot be played: its file, or (once it was chosen) its convoy's route.
    fn custom_blocker(&self, i: usize) -> Option<crate::custom_maps::Blocker> {
        let e = self.custom.get(i)?;
        e.blocker().or_else(|| self.convoy.get(&i).is_some_and(|&b| b).then_some(crate::custom_maps::Blocker::ConvoyPath))
    }

    /// Chooses a map, loads its defaults and its picture.
    pub fn select_map(&mut self, i: usize) {
        if i >= self.maps.len() {
            return;
        }
        self.map = i;
        self.preview = None;
        let (level, thumb) = match self.tab {
            Tab::Original => {
                let Some(level) = crate::skirmish::load_map(&self.game, &self.maps[i]) else { return };
                self.setup = Setup::skirmish_default(&level);
                let (game, terrain) = self.game.net_terrain(&self.maps[i], level.level_id());
                (level, opensow_viewer::scene::thumbnail(&game, &terrain, THUMB, THUMB))
            }
            Tab::Custom => {
                let Some(map) = self.custom.get(i).and_then(|e| e.map.as_ref().ok()).filter(|_| self.custom[i].blocker().is_none()) else { return };
                if *self.convoy.entry(i).or_insert_with(|| crate::custom_maps::convoy_blocked(&self.game, map)) {
                    return;
                }
                // a custom map's own resources are its default (no carryall rule: its maker chose them)
                self.setup = Setup::of_map(&map.level);
                // the human plays blue on every custom map (no Swap sides on that tab)
                self.local = 0;
                // the ground, a generated layout included (docs/generated-maps.md)
                (map.level.clone(), crate::custom_maps::thumbnail(&self.game, &map.ground(), THUMB, THUMB))
            }
        };
        self.defaults = self.setup.clone();
        if let Some(thumb) = thumb {
            self.preview = Some(Preview { map: i, thumb, level });
        }
        // keep the chosen row in the list's view
        let rows = Geo::wide().visible_rows_of(self.tab).max(1);
        if i < self.scroll {
            self.scroll = i;
        } else if i >= self.scroll + rows {
            self.scroll = i + 1 - rows;
        }
    }

    /// Nothing was changed from the map's own setup (a side choice or the difficulty does not count).
    pub fn is_default(&self) -> bool {
        self.setup == self.defaults
    }

    /// The selected map's script (blue = side 0), for the start.
    pub fn map_level(&self) -> Option<Level> {
        self.preview.as_ref().filter(|p| p.map == self.map).map(|p| p.level.clone())
    }

    /// Headless demos: gives map `i` a star of the level.
    pub fn demo_star(&mut self, i: usize, d: Difficulty) {
        if let Some(m) = self.maps.get(i) {
            self.stars.record(m, d);
        }
    }

    fn string(&self, i: usize) -> String {
        self.strings.as_ref().and_then(|s| s.get(i)).map(|s| s.trim().to_owned()).unwrap_or_default()
    }

    /// Advances the screen's animations by `ticks` game ticks.
    pub fn update(&mut self, ticks: u32) {
        self.tick = self.tick.wrapping_add(ticks);
    }

    // ---- input ----

    /// Every control with its window rectangle.
    fn controls(&self, w: usize, h: usize) -> Vec<(Rect, Hit)> {
        let g = Geo::for_window(w, h);
        let t = T::new(g.d, w, h);
        let mut out = Vec::new();
        for (r, tab) in g.tabs().into_iter().zip([Tab::Original, Tab::Custom]) {
            out.push((t.r(r), Hit::Tab(tab)));
        }
        if self.tab == Tab::Custom {
            let [folder, edit] = g.custom_buttons();
            out.push((t.r(folder), Hit::OpenFolder));
            if self.custom.get(self.map).is_some_and(|e| e.map.is_ok()) {
                out.push((t.r(edit), Hit::Edit));
            }
        }
        if let Some(l) = g.list {
            for row in 0..g.visible_rows_of(self.tab) {
                let i = self.scroll + row;
                if i >= self.maps.len() {
                    break;
                }
                out.push((t.r(rect(l.x + 12, l.y + g.hh + row as i32 * g.row_h, l.w - 24, g.row_h - 4)), Hit::Map(i)));
            }
        }
        if let Some(s) = g.selector {
            out.push((t.r(rect(s.x, s.y, s.h + 10, s.h)), Hit::MapPrev));
            out.push((t.r(rect(s.x + s.w - s.h - 10, s.y, s.h + 10, s.h)), Hit::MapNext));
        }
        // on a custom map the human always plays blue: no Swap sides there
        if self.tab == Tab::Original {
            out.push((t.r(g.swap_button()), Hit::Swap));
        }
        let (down, up) = Geo::stepper(g.level_stepper());
        out.push((t.r(down), Hit::LevelDown));
        out.push((t.r(up), Hit::LevelUp));
        for (r, row) in [(0, 5), (1, 6)] {
            for side in 0..2 {
                let (minus, plus) = Geo::stepper(g.resource_stepper(r, side));
                out.push((t.r(minus), Hit::Step(side, row * 2)));
                out.push((t.r(plus), Hit::Step(side, row * 2 + 1)));
            }
        }
        for i in 0..5 {
            let card = g.card(g.air, i);
            for (side, c) in g.card_controls(card).into_iter().enumerate() {
                let (minus, plus) = Geo::stepper(c);
                out.push((t.r(minus), Hit::Step(side, i as usize * 2)));
                out.push((t.r(plus), Hit::Step(side, i as usize * 2 + 1)));
            }
            let card = g.card(g.turrets, i);
            for (side, c) in g.card_controls(card).into_iter().enumerate() {
                out.push((t.r(c), Hit::Turret(side, i as usize)));
            }
        }
        out.push((t.r(g.back), Hit::Back));
        out.push((t.r(g.reset), Hit::Reset));
        out.push((t.r(g.start), Hit::Start));
        out
    }

    fn hit(&self, x: i32, y: i32, w: usize, h: usize) -> Option<Hit> {
        self.controls(w, h).into_iter().find(|(r, _)| r.contains(x, y)).map(|(_, hit)| hit)
    }

    /// Window rectangle of a control (tests, headless hover).
    pub fn centre(&self, hit: Hit, w: usize, h: usize) -> Option<(i32, i32)> {
        self.controls(w, h).into_iter().find(|(_, k)| *k == hit).map(|(r, _)| (r.x + r.w / 2, r.y + r.h / 2))
    }

    /// Updates what the pointer is over.
    pub fn mouse_moved(&mut self, x: i32, y: i32, w: usize, h: usize) {
        self.cursor = (x, y);
        let hit = self.hit(x, y, w, h);
        if hit.is_some() && hit != self.hover && !matches!(hit, Some(Hit::Map(_))) {
            self.sounds.push(30);
        }
        self.hover = hit;
    }

    /// A click at a window position.
    pub fn click(&mut self, x: i32, y: i32, w: usize, h: usize) -> Event {
        match self.hit(x, y, w, h) {
            Some(hit) => self.press(hit),
            None => Event::None,
        }
    }

    /// Does what a widget does when pressed.
    pub fn press(&mut self, hit: Hit) -> Event {
        match hit {
            Hit::Map(i) => {
                if i != self.map {
                    self.select_map(i);
                    self.sounds.push(39);
                }
            }
            Hit::MapPrev | Hit::MapNext => {
                if self.maps.is_empty() {
                    return Event::None;
                }
                let n = self.maps.len().max(1);
                let i = if hit == Hit::MapNext { (self.map + 1) % n } else { (self.map + n - 1) % n };
                self.select_map(i);
                self.sounds.push(33);
            }
            Hit::Swap => {
                self.local ^= 1;
                self.sounds.push(39);
            }
            Hit::LevelDown | Hit::LevelUp => {
                self.difficulty = if hit == Hit::LevelUp { self.difficulty.next() } else { self.difficulty.prev() };
                self.sounds.push(33);
            }
            Hit::Step(side, k) => {
                self.setup.arrow(side * 14 + k, side);
                self.sounds.push(33);
            }
            Hit::Turret(side, t) => {
                let v = &mut self.setup.sides[side].turrets[t];
                *v = !*v;
                self.sounds.push(33);
            }
            Hit::Reset => {
                self.setup = self.defaults.clone();
                self.sounds.push(39);
            }
            Hit::Back => {
                self.sounds.push(39);
                return Event::Leave;
            }
            Hit::Start => {
                if self.map_level().is_none() {
                    return Event::None; // nothing playable selected (an empty or broken custom map)
                }
                self.sounds.push(38);
                return Event::Start;
            }
            Hit::Tab(tab) => {
                if tab != self.tab {
                    self.set_tab(tab);
                    self.sounds.push(39);
                }
            }
            Hit::OpenFolder => {
                self.sounds.push(39);
                if !cfg!(test) {
                    crate::custom_maps::open_folder();
                }
            }
            Hit::Edit => {
                if let Some(p) = self.custom.get(self.map).filter(|e| e.map.is_ok()).map(|e| e.path.clone()) {
                    self.sounds.push(39);
                    return Event::Edit(p);
                }
            }
        }
        Event::None
    }

    /// Escape: the same as the Back button.
    pub fn escape(&mut self) -> Event {
        self.press(Hit::Back)
    }

    /// Enter: the same as the Start button.
    pub fn enter(&mut self) -> Event {
        self.press(Hit::Start)
    }

    /// Up / down arrow keys walk the map list.
    pub fn step_map(&mut self, down: bool) {
        self.press(if down { Hit::MapNext } else { Hit::MapPrev });
    }

    /// The mouse wheel scrolls the list (wide layout) or changes the map (compact).
    pub fn wheel(&mut self, dy: f32, w: usize, h: usize) {
        let g = Geo::for_window(w, h);
        if g.list.is_none() {
            if dy.abs() >= 0.5 {
                self.step_map(dy < 0.0);
            }
            return;
        }
        let max = self.maps.len().saturating_sub(g.visible_rows_of(self.tab));
        let steps = dy.round() as i32;
        self.scroll = (self.scroll as i32 - steps).clamp(0, max as i32) as usize;
    }

    // ---- drawing ----

    /// Draws the screen into the `w` x `h` frame buffer.
    pub fn draw(&mut self, buf: &mut [u32], w: usize, h: usize) {
        let mut canvas = std::mem::take(&mut self.canvas);
        canvas.resize(w * h, 0);
        opensow_viewer::record::cpu(|| self.render(&mut canvas, w, h));
        ui::page(buf, w, h, &canvas, w, h, 0, 0, 1.0);
        self.canvas = canvas;
    }

    fn render(&self, c: &mut [u32], w: usize, h: usize) {
        let g = Geo::for_window(w, h);
        let t = T::new(g.d, w, h);
        let px = |v: i32| t.len(v).max(9);
        let tr = self.tr;
        // backdrop, dimmed
        match &self.bg {
            Some(img) => ui::backdrop(c, w, h, img),
            None => ui::clear(c, w, h, 0x00081008),
        }
        ui::dim(c, w, h, Rect { x: 0, y: 0, w: w as i32, h: h as i32 }, 165);
        // top and bottom bars across the whole window
        let (top, bottom) = (t.y(g.top), t.y(g.bottom));
        ui::fill(c, w, h, Rect { x: 0, y: 0, w: w as i32, h: top }, (0, 0, 0), 170);
        ui::fill(c, w, h, Rect { x: 0, y: top, w: w as i32, h: 2 }, (90, 200, 100), 255);
        ui::fill(c, w, h, Rect { x: 0, y: bottom, w: w as i32, h: h as i32 - bottom }, (0, 0, 0), 180);
        ui::fill(c, w, h, Rect { x: 0, y: bottom, w: w as i32, h: 2 }, (90, 200, 100), 255);
        // title, subtitle, star badge
        let title_x = t.x(g.list.or(g.selector).map_or(16, |r| r.x));
        let title = tr.title();
        self.ink.text(c, w, h, title, title_x, t.y((g.top - g.title_px) / 2 - 4), px(g.title_px), TEXT);
        let sub_x = title_x + self.ink.width(title, px(g.title_px)) + t.len(36);
        if g.list.is_some() {
            self.ink.text(c, w, h, tr.subtitle(), sub_x, t.y((g.top - g.sub_px) / 2 + 2), px(g.sub_px), MUTED);
        }
        // stars belong to the shipped maps only: the Custom tab has no badge, no row stars and no star note
        let stars = self.tab == Tab::Original;
        let star = self.maps.get(self.map).filter(|_| stars).and_then(|m| self.stars.get(m));
        let badge = t.r(g.badge);
        if stars {
            let bc = star.map_or(GOLD, crate::stars::colour);
            ui::fill(c, w, h, badge, (30, 30, 10), 220);
            outline(c, w, h, badge, bc, 2);
            let r = badge.h / 4;
            star_shape(c, w, h, badge.x + badge.h / 2 + 4, badge.y + badge.h / 2, r.max(4), bc);
            let label = star.map_or_else(|| tr.star_none().to_owned(), |d| tr.star_line(d));
            let lx = badge.x + badge.h + 4;
            self.ink.fit(c, w, h, &label, lx, badge.y + (badge.h - px(g.small_px + 2)) / 2 - 2, px(g.small_px + 2), bc, badge.x + badge.w - lx - 8);
        }
        // maps: the tabs in the list's heading (wide) or in their own row (compact)
        if let Some(l) = g.list {
            self.panel(c, w, h, &g, &t, l, "");
            for row in 0..g.visible_rows_of(self.tab) {
                let i = self.scroll + row;
                let Some(name) = self.names.get(i) else { break };
                let r = t.r(rect(l.x + 12, l.y + g.hh + row as i32 * g.row_h, l.w - 24, g.row_h - 4));
                let chosen = i == self.map;
                if chosen {
                    ui::fill(c, w, h, r, (40, 110, 50), 230);
                } else if self.hover == Some(Hit::Map(i)) {
                    ui::fill(c, w, h, r, (40, 110, 50), 90);
                }
                let tp = px(g.body_px - 2);
                let broken = self.tab == Tab::Custom && self.custom_blocker(i).is_some();
                let colour = if broken {
                    WARN
                } else if chosen {
                    (255, 255, 255)
                } else {
                    TEXT
                };
                self.ink.fit(c, w, h, name, r.x + t.len(16), r.y + (r.h - tp) / 2 - 2, tp, colour, r.w - t.len(56));
                if broken {
                    self.ink.centred(c, w, h, "!", Rect { x: r.x + r.w - t.len(40), y: r.y, w: t.len(36), h: r.h }, tp, WARN);
                } else if let Some(d) = self.stars.get(&self.maps[i]).filter(|_| stars) {
                    star_shape(c, w, h, r.x + r.w - t.len(22), r.y + r.h / 2, t.len(11).max(4), crate::stars::colour(d));
                }
            }
            // a scroll bar when not all maps fit
            let rows = g.visible_rows_of(self.tab);
            if self.maps.len() > rows {
                let track = t.r(rect(l.x + l.w - 8, l.y + g.hh, 4, rows as i32 * g.row_h));
                ui::fill(c, w, h, track, (60, 120, 70), 140);
                let n = self.maps.len() as i32;
                let thumb = Rect { x: track.x, y: track.y + track.h * self.scroll as i32 / n, w: track.w, h: (track.h * rows as i32 / n).max(8) };
                ui::fill(c, w, h, thumb, ACCENT, 255);
            }
            if self.tab == Tab::Custom && self.maps.is_empty() {
                let tp = px(g.body_px - 4);
                for (k, line) in self.ink.wrap(tr.no_custom_maps(), tp, t.len(l.w - 2 * g.pad)).iter().enumerate() {
                    self.ink.text(c, w, h, line, t.x(l.x + g.pad), t.y(l.y + g.hh + 8) + k as i32 * (tp + tp / 3), tp, MUTED);
                }
            }
        }
        for (r, tab) in g.tabs().into_iter().zip([Tab::Original, Tab::Custom]) {
            let label = if tab == Tab::Original { tr.tab_original() } else { tr.tab_custom() };
            self.ink.chip(c, w, h, t.r(r), label, px(g.small_px + 2), self.tab == tab, self.hover == Some(Hit::Tab(tab)), ACCENT);
        }
        if self.tab == Tab::Custom {
            let [folder, edit] = g.custom_buttons();
            let fr = t.r(folder);
            if g.list.is_some() {
                self.ink.button(c, w, h, fr, tr.open_folder(), px(g.small_px), self.hover == Some(Hit::OpenFolder), false);
            } else {
                // the compact row has no room for the label: a folder
                let hot = self.hover == Some(Hit::OpenFolder);
                self.ink.button(c, w, h, fr, "", px(g.small_px), hot, false);
                crate::page::folder(c, w, h, fr, if hot { (255, 255, 255) } else { ACCENT });
            }
            if self.custom.get(self.map).is_some_and(|e| e.map.is_ok()) {
                self.ink.button(c, w, h, t.r(edit), tr.edit_map(), px(g.small_px), self.hover == Some(Hit::Edit), false);
            }
        }
        if let Some(s) = g.selector {
            let r = t.r(s);
            ui::fill(c, w, h, r, (4, 14, 8), 200);
            outline(c, w, h, r, (70, 150, 80), 2);
            let a = t.len(10);
            for (hit, left) in [(Hit::MapPrev, true), (Hit::MapNext, false)] {
                let cx = if left { r.x + r.h / 2 + 4 } else { r.x + r.w - r.h / 2 - 4 };
                arrow(c, w, h, cx, r.y + r.h / 2, a, left, if self.hover == Some(hit) { (255, 255, 255) } else { ACCENT });
            }
            let inner = Rect { x: r.x + r.h + 8, y: r.y, w: r.w - 2 * r.h - 16, h: r.h };
            match self.names.get(self.map) {
                Some(name) => self.ink.centred(c, w, h, name, inner, px(g.body_px), (255, 255, 255)),
                None => self.ink.centred(c, w, h, tr.no_custom_short(), inner, px(g.small_px), MUTED),
            }
        }
        // the map picture with its facts
        let title = self.names.get(self.map).map_or(String::new(), |n| n.to_uppercase());
        self.panel(c, w, h, &g, &t, g.preview_panel, if g.list.is_some() { &title } else { "" });
        let pr = t.r(if g.list.is_some() { g.preview } else { rect(g.preview.x, g.preview_panel.y + g.pad, g.preview.w, g.preview.h) });
        ui::fill(c, w, h, pr, (0, 0, 0), 255);
        if let Some(p) = self.preview.as_ref().filter(|p| p.map == self.map) {
            self.draw_preview(c, w, h, p, pr);
        } else if let Some(b) = self.blocker() {
            let tp = px(g.body_px - 4);
            let lines = self.ink.wrap(&tr.blocker(&b), tp, pr.w - t.len(40));
            let y0 = pr.y + pr.h / 2 - lines.len() as i32 * (tp + 4) / 2;
            for (k, line) in lines.iter().enumerate() {
                self.ink.centred(c, w, h, line, Rect { x: pr.x, y: y0 + k as i32 * (tp + 4), w: pr.w, h: tp + 4 }, tp, WARN);
            }
        }
        // a custom map's mission: a strip over the picture's lower edge
        if let Some(line) = self.objective() {
            let tp = px(g.body_px - 3);
            let strip = Rect { x: pr.x, y: pr.y + pr.h - tp - t.len(16), w: pr.w, h: tp + t.len(16) };
            ui::fill(c, w, h, strip, (0, 0, 0), 190);
            self.ink.centred(c, w, h, &line, Rect { x: strip.x, y: strip.y + t.len(6), w: strip.w, h: tp + 4 }, tp, GOLD);
        }
        outline(c, w, h, pr, (70, 150, 80), 2);
        if let Some(p) = self.preview.as_ref().filter(|p| p.map == self.map) {
            let lv = &p.level;
            let buildings = lv.objects.iter().filter(|o| o.object_kind().is_building());
            let all = buildings.clone().count();
            let neutral = buildings.filter(|o| o.raw[14] == 2).count();
            let mines = lv.objects.iter().filter(|o| matches!(o.object_kind(), ObjectKind::GoldMine)).count();
            let values =
                [format!("{} × {}", p.thumb.full_w / 32, p.thumb.full_h / 32), tr.buildings(all, neutral), mines.to_string(), tr.mission(lv.level_id())];
            let fy = if g.list.is_some() { g.facts_y } else { g.preview_panel.y + g.pad + g.preview.h + 14 };
            for (i, (k, v)) in tr.fact_labels().iter().zip(values.iter()).enumerate() {
                let y = t.y(fy + i as i32 * g.facts_dy);
                let x = g.preview.x + 10;
                self.ink.text(c, w, h, k, t.x(x), y, px(g.body_px - 3), MUTED);
                self.ink.text(c, w, h, v, t.x(x + g.preview.w * 42 / 100), y, px(g.body_px - 3), TEXT);
            }
        }
        // players
        self.panel(c, w, h, &g, &t, g.players, tr.players());
        for (i, row) in g.player_rows().into_iter().enumerate() {
            let side = if i == 0 { self.local } else { self.local ^ 1 };
            let r = t.r(row);
            ui::fill(c, w, h, r, (10, 30, 16), 230);
            ui::fill(c, w, h, Rect { x: r.x, y: r.y, w: t.len(10).max(3), h: r.h }, SIDE[side], 255);
            let bp = px(g.body_px);
            let ty = r.y + (r.h - bp) / 2 - 2;
            let sx = r.x + t.len(if g.list.is_some() { 28 + 170 } else { 28 + 128 });
            self.ink.fit(c, w, h, if i == 0 { tr.you() } else { tr.computer() }, r.x + t.len(28), ty, bp, TEXT, sx - r.x - t.len(28 + 10));
            // the side's name ends before the swap button / the level stepper of its row
            let end = if i == 0 { t.r(g.swap_button()).x } else { t.r(g.level_stepper()).x };
            self.ink.fit(c, w, h, tr.side(side == 1), sx, ty + t.len(2), px(g.body_px - 3), SIDE[side], (end - sx - t.len(10)).min(t.len(170)));
        }
        if self.tab == Tab::Original {
            let sw = t.r(g.swap_button());
            self.ink.button(c, w, h, sw, tr.swap(), px(g.small_px), self.hover == Some(Hit::Swap), false);
        }
        let ls = t.r(g.level_stepper());
        self.stepper(
            c,
            w,
            h,
            ls,
            tr.difficulty(self.difficulty),
            px(if g.list.is_some() { g.body_px } else { g.small_px }),
            ACCENT,
            self.hover,
            Hit::LevelDown,
            Hit::LevelUp,
        );
        let hc = self.difficulty.handicap();
        let row = t.r(g.player_rows()[1]);
        // income and production on two lines, so neither has to be squeezed
        let np = px(g.small_px - 2);
        let nx = ls.x + ls.w + t.len(12);
        let gap = np / 6;
        let y0 = row.y + (row.h - 2 * np - gap) / 2 - np / 8;
        for (k, line) in tr.income(hc.income_pct, 100 + 40 * hc.free_windtrap_levels).iter().enumerate() {
            self.ink.fit(c, w, h, line, nx, y0 + k as i32 * (np + gap), np, MUTED, row.x + row.w - nx - 4);
        }
        // resources
        self.panel(c, w, h, &g, &t, g.resources, tr.resources());
        for side in 0..2 {
            let head = tr.side_short(side == 1);
            let hp = px(g.small_px);
            self.ink.text(c, w, h, head, t.x(g.side_col(side)) - self.ink.width(head, hp) / 2, t.y(g.resources.y + (g.hh - g.small_px) / 2), hp, SIDE[side]);
        }
        for (r, row) in g.resource_rows().into_iter().enumerate() {
            let rr = t.r(row);
            if let Some(pic) = &self.icons[r] {
                pic.draw(c, w, h, Rect { x: rr.x, y: rr.y + rr.h / 10, w: t.len(44), h: rr.h * 8 / 10 }, 2.0 * t.s);
            }
            // the Slovak / Czech strings are lower case ("kredity"): a capital first letter like the other labels
            let label = capitalised(&self.string([CREDITS, RESEARCH][r]));
            self.ink.fit(
                c,
                w,
                h,
                &label,
                rr.x + t.len(56),
                rr.y + (rr.h - px(g.body_px)) / 2 - 2,
                px(g.body_px),
                TEXT,
                t.x(g.side_col(0)) - t.len(110) - rr.x - t.len(56),
            );
            for side in 0..2 {
                let s = &self.setup.sides[side];
                let v = if r == 0 { s.credits } else { s.research };
                let k = (5 + r) * 2;
                self.stepper(
                    c,
                    w,
                    h,
                    t.r(g.resource_stepper(r, side)),
                    &v.to_string(),
                    px(g.body_px + 2),
                    SIDE[side],
                    self.hover,
                    Hit::Step(side, k),
                    Hit::Step(side, k + 1),
                );
            }
        }
        // air forces and turrets
        self.panel(c, w, h, &g, &t, g.air, tr.air_forces());
        self.panel(c, w, h, &g, &t, g.turrets, tr.turrets());
        for i in 0..5 {
            for (panel, turret) in [(g.air, false), (g.turrets, true)] {
                let card = g.card(panel, i);
                let cr = t.r(card);
                let hot = self.hover.is_some_and(|hit| match hit {
                    Hit::Step(_, k) => !turret && k / 2 == i as usize,
                    Hit::Turret(_, k) => turret && k == i as usize,
                    _ => false,
                }) || cr.contains(self.cursor.0, self.cursor.1);
                ui::fill(c, w, h, cr, (10, 30, 16), 230);
                outline(c, w, h, cr, if hot { ACCENT } else { (50, 110, 60) }, 1);
                // the picture above the controls (no name: the picture says it)
                let pic = if turret { &self.turrets[i as usize] } else { &self.air[i as usize] };
                let ctl = t.r(g.card_controls(card)[0]);
                let top = cr.y + t.len(6);
                pic.draw(c, w, h, Rect { x: cr.x + 6, y: top, w: cr.w - 12, h: ctl.y - top - 4 }, 3.0 * t.s);
                for (side, ctl) in g.card_controls(card).into_iter().enumerate() {
                    let r = t.r(ctl);
                    if turret {
                        let on = self.setup.sides[side].turrets[i as usize];
                        let hot = self.hover == Some(Hit::Turret(side, i as usize));
                        ui::fill(c, w, h, r, if on { SIDE[side] } else { (34, 38, 36) }, 255);
                        outline(c, w, h, r, if hot { (255, 255, 255) } else { SIDE[side] }, 1);
                        mark(c, w, h, r, on);
                    } else {
                        let v = self.setup.sides[side].air[i as usize];
                        let k = i as usize * 2;
                        self.stepper(c, w, h, r, &v.to_string(), px(g.small_px), SIDE[side], self.hover, Hit::Step(side, k), Hit::Step(side, k + 1));
                    }
                }
            }
        }
        // bottom bar
        self.ink.button(c, w, h, t.r(g.back), tr.back(), px(g.body_px + 2), self.hover == Some(Hit::Back), false);
        self.ink.button(c, w, h, t.r(g.reset), tr.reset(), px(g.body_px - 2), self.hover == Some(Hit::Reset), false);
        self.ink.button(c, w, h, t.r(g.start), tr.start(), px(g.title_px * 7 / 10), self.hover == Some(Hit::Start), true);
        if stars {
            let (nc, note) = if self.is_default() { ((140, 220, 140), tr.setup_default()) } else { ((230, 150, 90), tr.setup_changed()) };
            let nx = t.x(g.note_x);
            let room = t.x(g.reset.x) - nx - t.len(16);
            self.ink.fit(c, w, h, note, nx, t.y(g.back.y + g.back.h / 2) - px(g.body_px - 2) / 2 - 2, px(g.body_px - 2), nc, room);
        }
    }

    fn panel(&self, c: &mut [u32], w: usize, h: usize, g: &Geo, t: &T, r: Rect, title: &str) {
        self.ink.panel(c, w, h, t.r(r), title, t.len(g.head_px).max(9), t.len(g.pad), t.len(g.hh));
    }

    /// Arrow, value, arrow (`minus` / `plus` = the arrows' controls, highlighted under the mouse).
    fn stepper(&self, c: &mut [u32], w: usize, h: usize, r: Rect, value: &str, px: i32, colour: (u8, u8, u8), hover: Option<Hit>, minus: Hit, plus: Hit) {
        let hot = if hover == Some(minus) {
            Some(false)
        } else if hover == Some(plus) {
            Some(true)
        } else {
            None
        };
        self.ink.stepper(c, w, h, r, value, px, colour, hot);
    }

    /// The map picture scaled into `r`, with the buildings and start positions marked.
    fn draw_preview(&self, c: &mut [u32], w: usize, h: usize, p: &Preview, r: Rect) {
        let th = &p.thumb;
        let s = (r.w as f32 / th.w as f32).min(r.h as f32 / th.h as f32);
        let (dw, dh) = ((th.w as f32 * s) as i32, (th.h as f32 * s) as i32);
        let (ox, oy) = (r.x + (r.w - dw) / 2, r.y + (r.h - dh) / 2);
        for y in 0..dh {
            let (sy, dy) = (((y as f32 / s) as usize).min(th.h - 1), oy + y);
            if dy < 0 || dy >= h as i32 {
                continue;
            }
            for x in 0..dw {
                let dx = ox + x;
                if dx >= 0 && dx < w as i32 {
                    c[dy as usize * w + dx as usize] = th.pixels[sy * th.w + ((x as f32 / s) as usize).min(th.w - 1)];
                }
            }
        }
        // one cell of the full picture in window pixels
        let k = dw as f32 / th.full_w as f32 * 32.0;
        let colour = |side: i32| match side {
            0 => BLUE,
            1 => GREEN,
            _ => (215, 215, 215),
        };
        let dot = |c: &mut [u32], x: f32, y: f32, n: i32, col: (u8, u8, u8)| {
            let (x0, y0) = (ox + (x * k) as i32 - n / 2, oy + (y * k) as i32 - n / 2);
            ui::fill(c, w, h, Rect { x: x0 - 1, y: y0 - 1, w: n + 2, h: n + 2 }, (0, 0, 0), 255);
            ui::fill(c, w, h, Rect { x: x0, y: y0, w: n, h: n }, col, 255);
        };
        let unit = (dw / 90).max(3);
        for o in &p.level.objects {
            let kind = o.object_kind();
            if !kind.is_building() && !matches!(kind, ObjectKind::Turret(_)) {
                continue;
            }
            let hq = matches!(kind, ObjectKind::Headquarters);
            let (cx, cy) = (o.cell_x as f32 + 0.5, o.cell_y as f32 + 0.5);
            if hq {
                // the start position: a ring around the headquarters
                let (x, y) = (ox + (cx * k) as i32, oy + (cy * k) as i32);
                ring(c, w, h, x, y, unit * 3, unit / 2 + 1, colour(o.raw[14]));
            }
            dot(
                c,
                cx,
                cy,
                if hq {
                    unit * 2
                } else if kind.is_building() {
                    unit + 1
                } else {
                    unit - 1
                },
                colour(o.raw[14]),
            );
        }
        for m in p.level.markers.iter().filter(|m| m.is_advancer()) {
            dot(c, m.x as f32 / 32.0, m.y as f32 / 32.0, unit - 1, colour(m.player));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_layouts_fit_their_design_area_and_keep_controls_apart() {
        for g in [Geo::wide(), Geo::compact()] {
            let inside = |r: Rect| r.x >= 0 && r.y >= 0 && r.x + r.w <= g.d.0 && r.y + r.h <= g.d.1;
            for r in [g.badge, g.preview_panel, g.players, g.resources, g.air, g.turrets, g.back, g.reset, g.start] {
                assert!(inside(r), "{r:?}");
            }
            assert!(g.players.y + g.players.h <= g.resources.y && g.resources.y + g.resources.h <= g.air.y && g.air.y + g.air.h <= g.turrets.y);
            assert!(g.turrets.y + g.turrets.h <= g.bottom, "the panels end above the bottom bar");
            let last = g.card(g.air, 4);
            assert!(last.x + last.w <= g.air.x + g.air.w, "five cards fit");
            let [a, b] = g.card_controls(last);
            assert!(a.x + a.w <= b.x && b.x + b.w <= last.x + last.w);
            let (minus, plus) = Geo::stepper(g.resource_stepper(0, 1));
            assert!(minus.x + minus.w <= plus.x);
            assert!(g.resource_stepper(0, 0).x + g.resource_stepper(0, 0).w <= g.resource_stepper(0, 1).x, "the two columns do not overlap");
        }
        assert!(Geo::wide().visible_rows() >= 14, "the base game's maps fit without scrolling");
        assert!(Geo::for_window(640, 480).list.is_none() && Geo::for_window(1920, 1080).list.is_some());
    }

    fn menu() -> Option<(SkirmishMenu, Assets)> {
        let mut assets = opensow_formats::GameDir::locate().and_then(|g| Assets::load(g).ok())?;
        Some((SkirmishMenu::new(&mut assets, &crate::stars::Stars::default()), assets))
    }

    #[test]
    fn every_control_is_reachable_by_the_pointer_in_both_layouts() {
        let Some((m, _)) = menu() else { return };
        for (w, h) in [(640, 480), (1024, 768), (1280, 720), (1920, 1080), (2560, 1080)] {
            let controls = m.controls(w, h);
            for (r, hit) in &controls {
                assert!(r.w > 0 && r.h > 0 && r.x >= 0 && r.y >= 0 && r.x + r.w <= w as i32 && r.y + r.h <= h as i32, "{hit:?} at {w}x{h}: {r:?}");
                let (x, y) = m.centre(*hit, w, h).unwrap();
                assert_eq!(m.hit(x, y, w, h), Some(*hit), "{hit:?} at {w}x{h} is covered by another control");
            }
            let n = controls.len();
            assert!(n >= 3 + 2 + 8 + 20 + 10 + 3, "{n} controls at {w}x{h}");
        }
    }

    #[test]
    fn both_sides_are_editable_and_the_star_rule_follows_the_maps_defaults() {
        let Some((mut m, _)) = menu() else { return };
        assert!(m.is_default() && m.map_level().is_some());
        // First Spark offers turret tiers 1 and 2 only (header bytes 84.. / 94..)
        assert_eq!(m.setup.sides[0].turrets, [true, true, false, false, false]);
        assert_eq!(m.setup.sides[1].turrets, [true, true, false, false, false]);
        let green = m.setup.sides[1].credits;
        m.press(Hit::Step(1, 11)); // green credits +500: the computer's side is editable too
        assert_eq!(m.setup.sides[1].credits, green + 500);
        assert!(!m.is_default());
        m.press(Hit::Step(1, 10));
        assert!(m.is_default(), "undoing a change restores the default");
        m.press(Hit::Step(0, 3)); // blue bombers +1
        assert_eq!(m.setup.sides[0].air[1], 1);
        m.press(Hit::Turret(1, 2));
        assert!(m.setup.sides[1].turrets[2] && !m.is_default());
        m.press(Hit::Reset);
        assert!(m.is_default());
        // swapping sides and the difficulty do not count as changes
        m.press(Hit::Swap);
        assert_eq!(m.local, 1);
        assert!(m.is_default());
        assert_eq!(m.difficulty, Difficulty::Normal);
        m.press(Hit::LevelUp);
        m.press(Hit::LevelUp);
        assert_eq!(m.difficulty, Difficulty::Brutal);
        m.press(Hit::LevelUp);
        assert_eq!(m.difficulty, Difficulty::Easy, "the level wraps");
        m.press(Hit::LevelDown);
        assert_eq!(m.difficulty, Difficulty::Brutal);
        // another map has its own defaults; the picture and script follow
        m.press(Hit::Step(0, 11));
        m.press(Hit::Map(2));
        assert_eq!(m.map, 2);
        assert!(m.is_default() && m.map_level().is_some());
        m.press(Hit::MapPrev);
        assert_eq!(m.map, 1);
        assert_eq!(m.press(Hit::Start), Event::Start);
        assert_eq!(m.escape(), Event::Leave);
    }

    #[test]
    fn the_list_scrolls_with_the_wheel_and_follows_the_selection() {
        let Some((mut m, _)) = menu() else { return };
        let rows = Geo::wide().visible_rows();
        m.wheel(-3.0, 1920, 1080);
        assert_eq!(m.scroll, 3.min(m.maps.len().saturating_sub(rows)));
        m.wheel(10.0, 1920, 1080);
        assert_eq!(m.scroll, 0);
        let last = m.maps.len() - 1;
        m.select_map(last);
        assert!(last < m.scroll + rows, "the chosen map is in view");
        // the compact layout has no list: the wheel walks the maps
        m.select_map(0);
        m.wheel(-1.0, 640, 480);
        assert_eq!(m.map, 1);
    }

    #[test]
    fn a_custom_map_is_played_from_the_blue_side_and_offers_no_swap() {
        let Some((mut m, _)) = menu() else { return };
        let dir = crate::mapeditor_ui::tests::many_maps("skirmish-side", 2);
        // the second map is a mission
        let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        let mut map = crate::custom_maps::load(&files[1]).unwrap();
        map.mission.win = opensow_formats::custommap::WinRule::Everything;
        map.set_timer(Some(opensow_formats::custommap::Timer { minutes: 20, end: opensow_formats::custommap::TimerEnd::Victory }));
        std::fs::write(&files[1], map.to_bytes()).unwrap();
        m.custom_dir = dir.clone();
        let swap = |m: &SkirmishMenu| [(1920, 1080), (640, 480)].iter().all(|&(w, h)| m.controls(w, h).iter().any(|(_, h)| *h == Hit::Swap));
        assert!(swap(&m), "the shipped maps let the player choose");
        m.press(Hit::Swap);
        assert_eq!(m.local, 1);
        m.set_tab(Tab::Custom);
        assert!([(1920, 1080), (640, 480)].iter().all(|&(w, h)| !m.controls(w, h).iter().any(|(_, h)| *h == Hit::Swap)));
        assert_eq!(m.local, 0);
        m.select_map(1);
        // the mission in words on the map's picture (English in the tests' install or not: both parts are there)
        if m.map_level().is_some() {
            let line = m.objective().expect("a mission line");
            assert!(line.contains("20") && line.contains('·'), "{line}");
        }
        m.select_map(0);
        assert_eq!(m.local, 0);
        assert_eq!(m.objective(), None, "a plain skirmish map says nothing");
        m.set_tab(Tab::Original);
        assert!(swap(&m));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_escort_map_whose_convoy_cannot_walk_its_route_is_not_played() {
        use crate::custom_maps::{self, Blocker};
        use opensow_formats::custommap::{Escort, WinRule, POD};
        let Some((mut m, assets)) = menu() else { return };
        let dir = crate::mapeditor_ui::tests::many_maps("skirmish-convoy", 1);
        let file = std::fs::read_dir(&dir).unwrap().flatten().next().unwrap().path();
        let mut map = custom_maps::load(&file).unwrap();
        let Some((grid, size)) = custom_maps::walkable(&assets.game, &map) else { return };
        let open = |c: (i32, i32)| grid[(c.1 * size.0 + c.0) as usize];
        let cells: Vec<(i32, i32)> = (0..size.1).flat_map(|y| (0..size.0).map(move |x| (x, y))).collect();
        let start = *cells.iter().find(|&&c| open(c) && c.1 > 8).unwrap();
        let reach = *cells.iter().rev().find(|&&c| c != start && open(c) && custom_maps::walks(&grid, size, start, c)).unwrap();
        let blocked = *cells.iter().find(|&&c| !open(c)).unwrap();
        map.mission.win = WinRule::Escort;
        map.level.markers.push(opensow_formats::level::Marker { kind: POD, player: 0, x: start.0 * 32 + 16, y: start.1 * 32 + 16 });
        m.custom_dir = dir.clone();
        for (goal, playable) in [(blocked, false), (reach, true)] {
            map.escort = Escort { goal: Some(goal), route: Vec::new() };
            std::fs::write(&file, map.to_bytes()).unwrap();
            m.set_tab(Tab::Custom);
            m.select_map(0);
            assert_eq!(m.map_level().is_some(), playable, "goal {goal:?}");
            assert_eq!(m.blocker() == Some(Blocker::ConvoyPath), !playable, "goal {goal:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_custom_list_scrolls_through_many_maps_in_both_layouts() {
        let Some((mut m, _)) = menu() else { return };
        let dir = crate::mapeditor_ui::tests::many_maps("skirmish", 30);
        m.custom_dir = dir.clone();
        m.set_tab(Tab::Custom);
        assert_eq!(m.maps.len(), 30);
        let rows = Geo::wide().visible_rows_of(Tab::Custom);
        assert!(rows < 30 && rows < Geo::wide().visible_rows(), "the Custom tab keeps a row for its buttons");
        let shown = |m: &SkirmishMenu, w, h| m.controls(w, h).iter().filter_map(|(_, h)| if let Hit::Map(i) = h { Some(*i) } else { None }).collect::<Vec<_>>();
        assert_eq!(shown(&m, 1920, 1080), (0..rows).collect::<Vec<_>>());
        // the wheel scrolls to the end and no further, the rows stay inside the list and clear of the buttons
        m.wheel(-100.0, 1920, 1080);
        assert_eq!(m.scroll, 30 - rows);
        assert_eq!(shown(&m, 1920, 1080), (30 - rows..30).collect::<Vec<_>>());
        let controls = m.controls(1920, 1080);
        let folder = controls.iter().find(|(_, h)| *h == Hit::OpenFolder).unwrap().0;
        for (r, hit) in controls.iter().filter(|(_, h)| matches!(h, Hit::Map(_))) {
            assert!(r.y + r.h <= folder.y, "{hit:?} {r:?} runs into the buttons at {folder:?}");
        }
        // clicking the last row picks it, and it loads
        let (x, y) = m.centre(Hit::Map(29), 1920, 1080).unwrap();
        m.click(x, y, 1920, 1080);
        assert_eq!(m.map, 29);
        assert!(m.map_level().is_some(), "the 30th map loads");
        // the keys walk the list and keep the choice in view
        m.select_map(0);
        assert_eq!(m.scroll, 0);
        for _ in 0..20 {
            m.step_map(true);
        }
        assert_eq!(m.map, 20);
        assert!(m.scroll <= 20 && 20 < m.scroll + rows, "row 20 in view (scroll {})", m.scroll);
        m.step_map(false);
        assert_eq!(m.map, 19);
        // compact: the selector walks all of them, the wheel too
        m.select_map(0);
        m.wheel(-1.0, 640, 480);
        assert_eq!(m.map, 1);
        for _ in 0..29 {
            m.press(Hit::MapNext);
        }
        assert_eq!(m.map, 0, "the selector wraps round");
        for (r, hit) in m.controls(640, 480) {
            assert!(r.x >= 0 && r.y >= 0 && r.x + r.w <= 640 && r.y + r.h <= 480, "{hit:?} at 640x480");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn warmonger_maps_continue_the_base_games_numbering() {
        let Some((m, _)) = menu() else { return };
        assert_eq!(m.names.len(), m.maps.len());
        if m.game.warmonger.is_some() && m.maps.len() > 8 {
            assert_eq!(m.names[8], "09-The First Step");
            assert!(m.names[..8].iter().zip(&m.maps).all(|(a, b)| a == b), "the base game's names are untouched");
        } else {
            assert_eq!(m.names, m.maps);
        }
    }

    #[test]
    fn draws_at_the_extreme_sizes() {
        let Some((mut m, _)) = menu() else { return };
        for (w, h) in [(640, 480), (1920, 1080)] {
            let mut buf = vec![0u32; w * h];
            m.draw(&mut buf, w, h);
            assert!(buf.iter().filter(|&&p| p != 0).count() > w * h / 2);
        }
    }
}
