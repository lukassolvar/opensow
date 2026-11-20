//! Custom skirmish maps made with the in-game map editor (docs/map-editor-ingame.md; not in the original game).
//!
//! A file is a complete level script (`.net` layout, [`Level::to_bytes`]) followed by the port's own block:
//!
//! ```text
//! "OSOW", u16 version (1), u16 record count
//! records: u16 tag, u32 length, payload
//!   1 NAME     the map's name as typed (UTF-8)
//!   2 TERRAIN  u8 edition (0 State of War, 1 Warmonger); the mission number is header word 1
//!   3 OBJECT   u32 object index, i32 x5 upgrade points per production line (0 = the game's own), u32 flags (bit 0: no Advancer)
//!   4 MISSION  u8 win rule (0 no enemy headquarters, 1 nothing of the enemy left, 2 a pod of the convoy reaches the goal)
//!   5 REINFORCEMENT  i32 credits, i32 research, u8 x5 air forces (header order), u16 count, count x (u16 cell x, u16 cell y, u8 squad)
//!   6 ESCORT   u8 goal set, u16 goal cell x, u16 goal cell y, u16 count, count x (u16 cell x, u16 cell y) waypoints in order
//!   7 LAYOUT   a composed ground ([`Layout`], docs/generated-maps.md): u16 patch count, count x (u16 x, u16 y) ground patch
//!              origins in the base mission's picture, row by row; u16 feature count, count x (u8 source edition, u8 source
//!              mission, u8 x, u8 y, u8 w, u8 h (source cells), u8 dest x, u8 dest y, ceil(w*h/8) bytes of cell bits, row by row,
//!              lowest bit first); u16 trail count, count x (u16 x, u16 y, u16 source x, u16 source y, u8 radius) trail
//!              stamps in pixels
//! ```
//!
//! The ground is the base mission's, or the generated layout built on the base mission's ground on every load
//! (docs/generated-maps.md).
//!
//! The mission countdown is original data: level header words 65 / 66 / 67 (enabled, minutes, end mode; [`Timer`]).
//!
//! The original's loader stops at the level footer (`Level::parse_prefix`), so the level part alone is still a valid `.net`.
//! Unknown record tags are skipped (newer files stay readable).

use crate::kinds;
use crate::level::{Level, Marker, HEADER_WORDS, MARKER_ADVANCER};
use crate::{Error, Reader, Result};

/// File extension of custom maps (the game's own `.map` holds terrain grids).
pub const EXTENSION: &str = "sowmap";
const MAGIC: &[u8; 4] = b"OSOW";
const VERSION: u16 = 1;
const TAG_NAME: u16 = 1;
const TAG_TERRAIN: u16 = 2;
const TAG_OBJECT: u16 = 3;
const TAG_MISSION: u16 = 4;
const TAG_REINFORCEMENT: u16 = 5;
const TAG_ESCORT: u16 = 6;
const TAG_LAYOUT: u16 = 7;
/// Most features a generated layout keeps.
pub const MAX_FEATURES: usize = 512;
/// Most ground patches a generated layout keeps (a 128 x 128 map of 88 px steps needs 47 x 47).
pub const MAX_PATCHES: usize = 4096;
/// Most waypoints of an escort route.
pub const MAX_WAYPOINTS: usize = 32;
/// The convoy's unit: the Rogon pod of the original's missions 3 and 8.
pub const POD: i32 = crate::kinds::POD;
/// Most carryall landings of a reinforcement, and the most credits / research points / aircraft of a kind it brings.
pub const MAX_DROPS: usize = 6;
/// Most credits or research points a reinforcement can bring.
pub const MAX_REINFORCEMENT: i32 = 20_000;
/// Most aircraft of one kind a reinforcement can bring.
pub const MAX_REINFORCEMENT_AIR: u8 = 20;
/// Carryall squads a landing point can bring: `aircraft::squad(0..SQUADS)` (the exe's eighteen, the last one two walkers).
pub const SQUADS: u8 = 18;

/// The cells a carryall landing at `cell` fills (`aircraft::squad`: columns x-1..=x+2, rows y..=y+1).
pub fn landing_block(cell: (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    (0..2).flat_map(move |dy| (-1..3).map(move |dx| (cell.0 + dx, cell.1 + dy)))
}

/// Whether two landing points' blocks share a cell.
pub fn landings_overlap(a: (i32, i32), b: (i32, i32)) -> bool {
    (a.0 - b.0).abs() < 4 && (a.1 - b.1).abs() < 2
}
/// Longest countdown the editor and the reader keep (minutes; the HUD clock shows two digits).
pub const MAX_MINUTES: i32 = 99;
/// Longest map name kept (characters) and longest file stem.
pub const MAX_NAME: usize = 32;
/// The Advancer's strength on every custom map (level header words 31/32, both sides): the shipped maps' usual value (they use
/// 6..15). The editor does not offer it; a file is read and written with it.
pub const ADVANCER_POWER: i32 = 10;
/// Production lines of a building (`maptweak::MAX_LINES`).
pub const LINES: usize = 5;

/// What a custom map adds to one object (index into `level.objects`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ObjectExt {
    /// Upgrade points the Advancer must bring to unlock production line `k` (0 = the game's own: 2000 ground, 4000 air).
    pub unlock: [i32; LINES],
    /// A headquarters that never gets an Advancer (neither at the start nor as a replacement).
    pub no_advancer: bool,
}

impl ObjectExt {
    /// Whether the object has no custom settings (nothing needs to be stored for it).
    pub fn is_default(&self) -> bool {
        *self == ObjectExt::default()
    }
}

/// How a countdown ends (level header word 67, seen from the human's side: `0x42ebd0`, docs/ai.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerEnd {
    /// Hold out until 0:00: victory.
    Victory,
    /// Win before 0:00, or lose.
    Defeat,
    /// Nothing ends at 0:00: reinforcements arrive.
    Reinforcements,
}

/// The mission countdown (level header words 65-67).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timer {
    /// Length of the countdown in minutes.
    pub minutes: i32,
    /// What happens when the countdown reaches 0:00.
    pub end: TimerEnd,
}

impl Timer {
    /// The countdown stored in the level header, `None` when the level has none.
    pub fn read(level: &Level) -> Option<Timer> {
        let h = &level.header;
        let end = match h[67] {
            0 => TimerEnd::Victory,
            1 => TimerEnd::Defeat,
            _ => TimerEnd::Reinforcements,
        };
        (h[65] != 0).then(|| Timer { minutes: h[66].clamp(1, MAX_MINUTES), end })
    }

    /// Writes the countdown (`None`: no countdown, all three words 0).
    pub fn write(timer: Option<Timer>, level: &mut Level) {
        let h = &mut level.header;
        (h[65], h[66], h[67]) = match timer {
            None => (0, 0, 0),
            Some(t) => (
                1,
                t.minutes.clamp(1, MAX_MINUTES),
                match t.end {
                    TimerEnd::Victory => 0,
                    TimerEnd::Defeat => 1,
                    TimerEnd::Reinforcements => 2,
                },
            ),
        };
    }
}

/// When the human has won (apart from a countdown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WinRule {
    /// The enemy has no headquarters left (the original's usual rule).
    #[default]
    Headquarters,
    /// The enemy has no units and no buildings left (the original's missions 4, 8, 13, 16, 21 and 22).
    Everything,
    /// Escort: one of the human's pods reaches the goal (3x3 cells around [`Escort::goal`]), as in the original's mission 3;
    /// losing every pod loses.
    Escort,
}

/// An escort mission's way: the waypoints the pods walk through in order, then the goal (the middle of a 3x3 zone).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Escort {
    /// The goal cell (the middle of the 3x3 goal zone), `None` until one is set.
    pub goal: Option<(i32, i32)>,
    /// The waypoint cells the pods walk through, in order, before the goal.
    pub route: Vec<(i32, i32)>,
}

impl Escort {
    /// Whether neither a goal nor a waypoint is set.
    pub fn is_empty(&self) -> bool {
        *self == Escort::default()
    }

    /// Whether `cell` lies in the goal zone (the goal cell and its eight neighbours).
    pub fn in_goal(&self, cell: (i32, i32)) -> bool {
        self.goal.is_some_and(|g| (g.0 - cell.0).abs() <= 1 && (g.1 - cell.1).abs() <= 1)
    }
}

/// How many pods (the convoy) `side` starts with.
pub fn pods(level: &Level, side: usize) -> usize {
    level.markers.iter().filter(|m| m.kind == POD && m.player == side as i32).count()
}

/// A custom map's mission: what wins (the human always plays blue on a custom map). The default is a plain skirmish map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mission {
    /// What the human must do to win.
    pub win: WinRule,
}

/// What the human gets when a [`TimerEnd::Reinforcements`] countdown ends (the original pays missions 7, 12 and 17 from its
/// code, `0x42ebd0`; a custom map carries its own).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reinforcement {
    /// Credits added.
    pub credits: i32,
    /// Research points added.
    pub research: i32,
    /// Aircraft added to the stock, in [`crate::air::HEADER_ORDER`] order (bomber, asteroid, carryall, tripler, fighter).
    pub air: [u8; 5],
    /// Where carryalls land, at most [`MAX_DROPS`].
    pub drops: Vec<Landing>,
}

/// One carryall of a reinforcement: the cell it aims at and the squad it brings (`aircraft::squad` index, below [`SQUADS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Landing {
    /// The cell the carryall aims at.
    pub cell: (i32, i32),
    /// The squad it brings: an index for `aircraft::squad`, below [`SQUADS`].
    pub squad: u8,
}

impl Reinforcement {
    /// Whether the reinforcement brings nothing.
    pub fn is_empty(&self) -> bool {
        *self == Reinforcement::default()
    }
}

/// Where the ground comes from: mission `M<mission>` of the base game or of Warmonger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Terrain {
    /// True = the ground of the Warmonger mission, false = of the base game's.
    pub warmonger: bool,
    /// The mission number whose ground is used (`M<mission>`).
    pub mission: i32,
}

/// One piece of a generated layout: the cells `cells` of a rectangle of a mission's ground (a cliff, a lake with its shore, a
/// forest, a building painted into the picture) and where the rectangle lands (docs/generated-maps.md; not in the exe).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Feature {
    /// The mission the feature is cut from.
    pub source: Terrain,
    /// Top-left source cell of the rectangle.
    pub x: u8,
    /// Top-left source cell of the rectangle.
    pub y: u8,
    /// Width of the rectangle in cells.
    pub w: u8,
    /// Height of the rectangle in cells.
    pub h: u8,
    /// Top-left destination cell.
    pub dx: u8,
    /// Top-left destination cell.
    pub dy: u8,
    /// The feature's own cells in the rectangle, row by row (`w * h`).
    pub cells: Vec<bool>,
}

/// A composed ground (docs/generated-maps.md; not in the exe): open ground quilted from patches of the base mission's picture,
/// then whole features cut from missions. The map keeps the base mission's size.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct Layout {
    /// Top-left pixel of each ground patch in the base mission's picture, row by row.
    pub patches: Vec<(u16, u16)>,
    /// Pasted over the ground in order.
    pub features: Vec<Feature>,
    /// Trail texture laid over the open ground before the features, in order (worn tracks and base plazas).
    pub trails: Vec<TrailStamp>,
}

/// A round dab of trail texture (docs/generated-maps.md): the source pixel of the base mission's picture taken to `at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrailStamp {
    /// Centre on the map (pixels).
    pub at: (u16, u16),
    /// Centre in the base mission's picture (pixels), inside one of its trails.
    pub from: (u16, u16),
    /// Radius (pixels).
    pub radius: u8,
}

/// Most trail stamps a layout keeps.
pub const MAX_TRAILS: usize = 8192;

/// Everything the map's ground is built from: the base mission, or a layout on its ground (docs/generated-maps.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ground {
    /// The base mission's ground.
    pub terrain: Terrain,
    /// A composed ground replacing the base mission's own.
    pub layout: Option<Layout>,
}

fn read_layout(b: &mut Reader) -> Result<Layout> {
    let mut l = Layout::default();
    for _ in 0..b.u16("patches")? {
        let p = (b.u16("patch x")?, b.u16("patch y")?);
        if l.patches.len() < MAX_PATCHES {
            l.patches.push(p);
        }
    }
    for _ in 0..b.u16("features")? {
        let source = Terrain { warmonger: b.u8("feature edition")? != 0, mission: i32::from(b.u8("feature mission")?) };
        let mut v = [0u8; 6];
        for x in &mut v {
            *x = b.u8("feature cells")?;
        }
        let [x, y, w, h, dx, dy] = v;
        let n = usize::from(w) * usize::from(h);
        let bits = b.bytes(n.div_ceil(8), "feature bits")?;
        let cells = (0..n).map(|i| bits[i / 8] >> (i % 8) & 1 != 0).collect();
        if w > 0 && h > 0 && l.features.len() < MAX_FEATURES {
            l.features.push(Feature { source, x, y, w, h, dx, dy, cells });
        }
    }
    // trail stamps follow (absent in older files)
    if b.remaining() >= 2 {
        for _ in 0..b.u16("trails")? {
            let at = (b.u16("trail x")?, b.u16("trail y")?);
            let from = (b.u16("trail source x")?, b.u16("trail source y")?);
            let radius = b.u8("trail radius")?;
            if l.trails.len() < MAX_TRAILS {
                l.trails.push(TrailStamp { at, from, radius });
            }
        }
    }
    Ok(l)
}

fn write_layout(l: &Layout) -> Vec<u8> {
    let patches = &l.patches[..l.patches.len().min(MAX_PATCHES)];
    let mut p = (patches.len() as u16).to_le_bytes().to_vec();
    for &(x, y) in patches {
        p.extend(x.to_le_bytes());
        p.extend(y.to_le_bytes());
    }
    let features = &l.features[..l.features.len().min(MAX_FEATURES)];
    p.extend((features.len() as u16).to_le_bytes());
    for f in features {
        p.extend([u8::from(f.source.warmonger), f.source.mission.clamp(0, 255) as u8, f.x, f.y, f.w, f.h, f.dx, f.dy]);
        let n = usize::from(f.w) * usize::from(f.h);
        let mut bits = vec![0u8; n.div_ceil(8)];
        for (i, _) in f.cells.iter().take(n).enumerate().filter(|(_, &c)| c) {
            bits[i / 8] |= 1 << (i % 8);
        }
        p.extend(bits);
    }
    let trails = &l.trails[..l.trails.len().min(MAX_TRAILS)];
    p.extend((trails.len() as u16).to_le_bytes());
    for t in trails {
        for v in [t.at.0, t.at.1, t.from.0, t.from.1] {
            p.extend(v.to_le_bytes());
        }
        p.push(t.radius);
    }
    p
}

/// A custom skirmish map (`*.sowmap`): a plain `.net` level followed by the `OSOW` block with everything the original format has no place for.
#[derive(Debug, Clone)]
pub struct CustomMap {
    /// The map's name as shown to the player.
    pub name: String,
    /// Whether the ground comes from a Warmonger mission (see [`Terrain`]).
    pub warmonger: bool,
    /// The level in the original format: header, objects and unit markers.
    pub level: Level,
    /// One entry per object of `level` (missing ones are the default).
    pub ext: Vec<ObjectExt>,
    /// The win rule.
    pub mission: Mission,
    /// Used when the countdown ends with reinforcements; kept in the file whenever it is not empty.
    pub reinforcement: Reinforcement,
    /// The convoy's goal and route (used when the win rule is [`WinRule::Escort`]; kept whenever it is not empty).
    pub escort: Escort,
    /// A composed ground replacing the base mission's own.
    pub layout: Option<Layout>,
}

impl CustomMap {
    /// An empty map on mission `mission`'s ground: both sides with the same starting resources, no objects.
    pub fn new(name: &str, terrain: Terrain) -> CustomMap {
        let mut header = [0i32; HEADER_WORDS];
        header[0] = 4 | i32::from(crate::level::MAGIC) << 16;
        header[1] = terrain.mission;
        let sides = Sides { credits: 3000, power: ADVANCER_POWER, turrets: [true; 5], ..Default::default() };
        let mut level = Level { version: 4, header, objects: Vec::new(), markers: Vec::new() };
        for side in 0..2 {
            sides.write(&mut level, side);
        }
        CustomMap {
            name: clean_name(name),
            warmonger: terrain.warmonger,
            level,
            ext: Vec::new(),
            mission: Mission::default(),
            reinforcement: Reinforcement::default(),
            escort: Escort::default(),
            layout: None,
        }
    }

    /// Which mission's ground the map uses.
    pub fn terrain(&self) -> Terrain {
        Terrain { warmonger: self.warmonger, mission: self.level.level_id() }
    }

    /// What the map's ground is built from.
    pub fn ground(&self) -> Ground {
        Ground { terrain: self.terrain(), layout: self.layout.clone() }
    }

    /// The mission countdown, if the map has one.
    pub fn timer(&self) -> Option<Timer> {
        Timer::read(&self.level)
    }

    /// Sets or removes (`None`) the mission countdown.
    pub fn set_timer(&mut self, timer: Option<Timer>) {
        Timer::write(timer, &mut self.level);
    }

    /// The object's extension (default when none is stored).
    pub fn ext_of(&self, i: usize) -> ObjectExt {
        self.ext.get(i).copied().unwrap_or_default()
    }

    /// Parses a `.sowmap` file: the level, then the `OSOW` block when there is one.
    pub fn parse(data: &[u8]) -> Result<CustomMap> {
        let (mut level, used) = Level::parse_prefix(data)?;
        level.header[31] = ADVANCER_POWER;
        level.header[32] = ADVANCER_POWER;
        let mut map = CustomMap {
            name: String::new(),
            warmonger: false,
            ext: vec![ObjectExt::default(); level.objects.len()],
            mission: Mission::default(),
            reinforcement: Reinforcement::default(),
            escort: Escort::default(),
            layout: None,
            level,
        };
        let mut r = Reader::new(&data[used..]);
        if r.remaining() == 0 {
            return Ok(map); // a plain `.net`
        }
        if r.bytes(4, "custom map magic")? != MAGIC {
            return Err(Error::Invalid("not a custom map: unknown data after the level".into()));
        }
        let _version = r.u16("custom map version")?;
        let count = r.u16("custom map records")?;
        for _ in 0..count {
            let tag = r.u16("record tag")?;
            let len = r.u32("record length")? as usize;
            let body = r.bytes(len, "record")?;
            let mut b = Reader::new(body);
            match tag {
                TAG_NAME => map.name = clean_name(&String::from_utf8_lossy(body)),
                TAG_TERRAIN => map.warmonger = b.u8("edition")? != 0,
                TAG_OBJECT => {
                    let i = b.u32("object index")? as usize;
                    let mut e = ObjectExt::default();
                    for u in &mut e.unlock {
                        *u = b.i32("unlock points")?.max(0);
                    }
                    e.no_advancer = b.u32("object flags")? & 1 != 0;
                    let slot = map.ext.get_mut(i).ok_or_else(|| Error::Invalid(format!("custom map: object {i} does not exist")))?;
                    *slot = e;
                }
                TAG_REINFORCEMENT => {
                    let mut f = Reinforcement {
                        credits: b.i32("credits")?.clamp(0, MAX_REINFORCEMENT),
                        research: b.i32("research")?.clamp(0, MAX_REINFORCEMENT),
                        ..Reinforcement::default()
                    };
                    for a in &mut f.air {
                        *a = b.u8("air force")?.min(MAX_REINFORCEMENT_AIR);
                    }
                    for _ in 0..b.u16("landings")? {
                        let cell = (i32::from(b.u16("cell x")?), i32::from(b.u16("cell y")?));
                        let squad = b.u8("squad")?.min(SQUADS - 1);
                        if f.drops.len() < MAX_DROPS {
                            f.drops.push(Landing { cell, squad });
                        }
                    }
                    map.reinforcement = f;
                }
                TAG_ESCORT => {
                    let set = b.u8("goal set")? != 0;
                    let goal = (i32::from(b.u16("goal x")?), i32::from(b.u16("goal y")?));
                    let mut e = Escort { goal: set.then_some(goal), route: Vec::new() };
                    for _ in 0..b.u16("waypoints")? {
                        let c = (i32::from(b.u16("waypoint x")?), i32::from(b.u16("waypoint y")?));
                        if e.route.len() < MAX_WAYPOINTS {
                            e.route.push(c);
                        }
                    }
                    map.escort = e;
                }
                TAG_LAYOUT => map.layout = Some(read_layout(&mut b)?),
                TAG_MISSION => {
                    map.mission.win = match b.u8("win rule")? {
                        1 => WinRule::Everything,
                        2 => WinRule::Escort,
                        _ => WinRule::Headquarters,
                    };
                }
                _ => {}
            }
        }
        Ok(map)
    }

    /// The file: the level (with the Advancer markers and flags of [`sync_advancers`]) and the block.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut level = self.level.clone();
        sync_advancers(&mut level, &self.ext);
        level.header[31] = ADVANCER_POWER;
        level.header[32] = ADVANCER_POWER;
        let mut records: Vec<(u16, Vec<u8>)> = vec![(TAG_NAME, self.name.as_bytes().to_vec()), (TAG_TERRAIN, vec![u8::from(self.warmonger)])];
        for (i, e) in self.ext.iter().enumerate().take(level.objects.len()).filter(|(_, e)| !e.is_default()) {
            let mut p = (i as u32).to_le_bytes().to_vec();
            for u in e.unlock {
                p.extend(u.to_le_bytes());
            }
            p.extend(u32::from(e.no_advancer).to_le_bytes());
            records.push((TAG_OBJECT, p));
        }
        if self.mission != Mission::default() {
            let win = match self.mission.win {
                WinRule::Headquarters => 0,
                WinRule::Everything => 1,
                WinRule::Escort => 2,
            };
            records.push((TAG_MISSION, vec![win]));
        }
        if !self.reinforcement.is_empty() {
            let f = &self.reinforcement;
            let mut p = f.credits.to_le_bytes().to_vec();
            p.extend(f.research.to_le_bytes());
            p.extend(f.air);
            let drops = &f.drops[..f.drops.len().min(MAX_DROPS)];
            p.extend((drops.len() as u16).to_le_bytes());
            for d in drops {
                p.extend((d.cell.0.clamp(0, 0xffff) as u16).to_le_bytes());
                p.extend((d.cell.1.clamp(0, 0xffff) as u16).to_le_bytes());
                p.push(d.squad.min(SQUADS - 1));
            }
            records.push((TAG_REINFORCEMENT, p));
        }
        if !self.escort.is_empty() {
            let e = &self.escort;
            let cell = |c: (i32, i32)| [(c.0.clamp(0, 0xffff) as u16).to_le_bytes(), (c.1.clamp(0, 0xffff) as u16).to_le_bytes()].concat();
            let mut p = vec![u8::from(e.goal.is_some())];
            p.extend(cell(e.goal.unwrap_or_default()));
            let route = &e.route[..e.route.len().min(MAX_WAYPOINTS)];
            p.extend((route.len() as u16).to_le_bytes());
            for &c in route {
                p.extend(cell(c));
            }
            records.push((TAG_ESCORT, p));
        }
        if let Some(l) = &self.layout {
            records.push((TAG_LAYOUT, write_layout(l)));
        }
        let mut d = level.to_bytes();
        d.extend(MAGIC);
        d.extend(VERSION.to_le_bytes());
        d.extend((records.len() as u16).to_le_bytes());
        for (tag, p) in records {
            d.extend(tag.to_le_bytes());
            d.extend((p.len() as u32).to_le_bytes());
            d.extend(p);
        }
        d
    }

    /// What is wrong with the map for a match: each side needs a headquarters (a side without one has lost, docs/ai.md); on
    /// an escort map blue may do without one (its convoy is the game, like the original's mission 3) but needs pods and a goal.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        let escort = self.mission.win == WinRule::Escort;
        for side in 0..2 {
            let hqs = self.level.objects.iter().filter(|o| o.kind == kinds::HEADQUARTERS && o.raw[14] == side).count();
            if hqs == 0 {
                if !(escort && side == 0) {
                    out.push(Problem::NoHeadquarters(side as usize));
                }
            } else if advancers(&self.level, &self.ext, side as usize) == 0 {
                out.push(Problem::NoAdvancer(side as usize));
            }
        }
        if escort {
            if pods(&self.level, 0) == 0 {
                out.push(Problem::NoPods);
            }
            if self.escort.goal.is_none() {
                out.push(Problem::NoGoal);
            }
        }
        out
    }
}

/// What [`CustomMap::problems`] finds. Only [`Problem::NoHeadquarters`] prevents a match; a side without an Advancer can still play
/// (it cannot capture).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// This side (0 or 1) has no headquarters.
    NoHeadquarters(usize),
    /// This side (0 or 1) has no Advancer.
    NoAdvancer(usize),
    /// An escort map without a blue pod.
    NoPods,
    /// An escort map without a goal.
    NoGoal,
}

impl Problem {
    /// Whether the map cannot be played until this is fixed.
    pub fn blocks_play(self) -> bool {
        !matches!(self, Problem::NoAdvancer(_))
    }
}

/// Headquarters of `side` (indices into `level.objects`) in placement order.
pub fn headquarters(level: &Level, side: usize) -> Vec<usize> {
    level.objects.iter().enumerate().filter(|(_, o)| o.kind == kinds::HEADQUARTERS && o.raw[14] == side as i32).map(|(i, _)| i).collect()
}

/// How many of `side`'s headquarters have an Advancer.
pub fn advancers(level: &Level, ext: &[ObjectExt], side: usize) -> usize {
    headquarters(level, side).into_iter().filter(|&i| !ext.get(i).is_some_and(|e| e.no_advancer)).count()
}

/// Turns the Advancers of `side` on for its first `n` headquarters (placement order) and off for the rest.
pub fn set_advancers(level: &Level, ext: &mut Vec<ObjectExt>, side: usize, n: usize) {
    ext.resize(level.objects.len().max(ext.len()), ObjectExt::default());
    for (k, i) in headquarters(level, side).into_iter().enumerate() {
        ext[i].no_advancer = k >= n;
    }
}

/// Writes the Advancers the way the original reads them: one Advancer marker per headquarters that has one, at HQ cell + (1,1)
/// where the exe also places a replacement (`0x42e38d`), and header word 34 / 35 ("no replacement Advancer") set for a side
/// none of whose headquarters has one. A side with Advancers on only some of its headquarters needs the port's flags (the
/// original would give every headquarters one).
pub fn sync_advancers(level: &mut Level, ext: &[ObjectExt]) {
    level.markers.retain(|m| m.kind != MARKER_ADVANCER);
    for (i, o) in level.objects.iter().enumerate() {
        let side = o.raw[14];
        if o.kind != kinds::HEADQUARTERS || !(0..2).contains(&side) || ext.get(i).is_some_and(|e| e.no_advancer) {
            continue;
        }
        level.markers.push(Marker { kind: MARKER_ADVANCER, player: side, x: (o.cell_x + 1) * 32 + 16, y: (o.cell_y + 1) * 32 + 16 });
    }
    for side in 0..2 {
        let has = advancers(level, ext, side) > 0;
        level.header[34 + side] = i32::from(!has);
    }
}

/// One side's starting resources as the level header holds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sides {
    /// Starting credits (header word 2 / 3).
    pub credits: i32,
    /// Starting research points (header word 37 / 38).
    pub research: i32,
    /// Air forces in [`crate::air::HEADER_ORDER`] order (bomber, asteroid, carryall, tripler, fighter).
    pub air: [i32; 5],
    /// Turret types 1-5 the side may build (header bytes 84.. / 94..).
    pub turrets: [bool; 5],
    /// The Advancer's strength (header word 31 / 32: hit points and upgrade points per beam shot, docs/advancer-actions.md).
    pub power: i32,
}

impl Sides {
    /// Reads `side`'s (0 or 1) starting resources from the level header.
    pub fn read(level: &Level, side: usize) -> Sides {
        let h = &level.header;
        let byte = |b: usize| (h[b / 4 + 2] >> (8 * (b % 4))) & 0xff != 0;
        Sides {
            credits: h[2 + side],
            research: h[37 + side],
            air: std::array::from_fn(|g| h[5 + 3 * g + side]),
            turrets: std::array::from_fn(|t| byte(84 + 10 * side + t)),
            power: h[31 + side],
        }
    }

    /// Writes them into the level header for `side` (0 or 1).
    pub fn write(&self, level: &mut Level, side: usize) {
        let h = &mut level.header;
        h[2 + side] = self.credits;
        h[37 + side] = self.research;
        for g in 0..5 {
            h[5 + 3 * g + side] = self.air[g];
        }
        for t in 0..5 {
            let b = 84 + 10 * side + t;
            let (word, shift) = (b / 4 + 2, 8 * (b % 4));
            h[word] = (h[word] & !(0xff << shift)) | (i32::from(self.turrets[t]) << shift);
        }
        h[31 + side] = self.power;
    }
}

/// The name as kept: control characters dropped, spaces collapsed, at most [`MAX_NAME`] characters.
pub fn clean_name(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| c.is_whitespace() || c.is_control()).filter(|w| !w.is_empty()).collect();
    words.join(" ").chars().take(MAX_NAME).collect::<String>().trim().to_string()
}

/// The file name stem for a map name: letters with diacritics as plain ones (`Žltá púšť` -> `zlta-pust`), any other character
/// that is not an ASCII letter or digit as `-`, runs of `-` collapsed and trimmed, lower case, at most [`MAX_NAME`] long. A name
/// with nothing left (Chinese, symbols only) gets `map-` and a short hash of the name, so different names keep different files.
pub fn file_stem(name: &str) -> String {
    let mut s = String::new();
    for c in clean_name(name).chars() {
        let plain = fold(c);
        for p in plain.chars() {
            if p.is_ascii_alphanumeric() {
                s.push(p.to_ascii_lowercase());
            } else if !s.ends_with('-') {
                s.push('-');
            }
        }
    }
    let s: String = s.trim_matches('-').chars().take(MAX_NAME).collect();
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() {
        let h = name.bytes().fold(0x811c9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x01000193));
        if name.trim().is_empty() {
            "map".into()
        } else {
            format!("map-{:06x}", h & 0xff_ffff)
        }
    } else {
        s
    }
}

/// A letter without its diacritics (Slovak, Czech and the other Latin letters the supported keyboards type).
fn fold(c: char) -> String {
    const PAIRS: [(&str, &str); 26] = [
        ("áàâäãåāą", "a"),
        ("ÁÀÂÄÃÅĀĄ", "A"),
        ("čćç", "c"),
        ("ČĆÇ", "C"),
        ("ďđ", "d"),
        ("ĎĐ", "D"),
        ("éèêëěēę", "e"),
        ("ÉÈÊËĚĒĘ", "E"),
        ("íìîïī", "i"),
        ("ÍÌÎÏĪ", "I"),
        ("ĺľł", "l"),
        ("ĹĽŁ", "L"),
        ("ňńñ", "n"),
        ("ŇŃÑ", "N"),
        ("óòôöõőø", "o"),
        ("ÓÒÔÖÕŐØ", "O"),
        ("ŕř", "r"),
        ("ŔŘ", "R"),
        ("šś", "s"),
        ("ŠŚ", "S"),
        ("ťţ", "t"),
        ("ŤŢ", "T"),
        ("úùûüůűū", "u"),
        ("ÚÙÛÜŮŰŪ", "U"),
        ("ýÿžźż", "yyzzz"),
        ("ÝŸŽŹŻ", "YYZZZ"),
    ];
    for (from, to) in PAIRS {
        if let Some(k) = from.chars().position(|f| f == c) {
            let to: Vec<char> = to.chars().collect();
            return to[k.min(to.len() - 1)].to_string();
        }
    }
    match c {
        'ß' => "ss".into(),
        'æ' => "ae".into(),
        'Æ' => "AE".into(),
        'œ' => "oe".into(),
        _ => c.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Object;

    fn obj(kind: i32, x: i32, y: i32, owner: i32) -> Object {
        let mut raw = [0i32; 19];
        (raw[0], raw[1], raw[14], raw[16], raw[17], raw[18]) = (123, kind, owner, x, y, 100);
        Object { kind, cell_x: x, cell_y: y, raw }
    }

    fn sample() -> CustomMap {
        let mut m = CustomMap::new("Twin  Rivers", Terrain { warmonger: true, mission: 7 });
        m.level.objects = vec![obj(100, 2, 2, 0), obj(100, 40, 40, 1), obj(100, 50, 2, 1), obj(102, 20, 20, 2)];
        m.ext = vec![ObjectExt::default(); 4];
        m.ext[2].no_advancer = true;
        m.ext[3].unlock = [0, 3000, 0, 0, 0];
        m
    }

    #[test]
    fn round_trips_and_the_level_part_stays_a_plain_level() {
        let m = sample();
        let d = m.to_bytes();
        let back = CustomMap::parse(&d).unwrap();
        assert_eq!((back.name.as_str(), back.warmonger, back.level.level_id()), ("Twin Rivers", true, 7));
        assert_eq!(back.ext, m.ext);
        assert_eq!(back.to_bytes(), d);
        let (plain, used) = Level::parse_prefix(&d).unwrap();
        assert!(used < d.len() && &d[used..used + 4] == b"OSOW");
        assert_eq!(plain.objects.len(), 4);
        // a plain `.net` reads as a custom map with defaults
        let only = CustomMap::parse(&d[..used]).unwrap();
        assert!(only.name.is_empty() && only.ext.iter().all(ObjectExt::is_default));
        assert!(CustomMap::parse(&[&d[..used], b"junk"].concat()).is_err());
    }

    #[test]
    fn the_advancer_power_is_always_the_same() {
        let mut m = sample();
        m.level.header[31] = 14;
        m.level.header[32] = 6;
        let d = m.to_bytes();
        let back = CustomMap::parse(&d).unwrap();
        assert_eq!((back.level.header[31], back.level.header[32]), (ADVANCER_POWER, ADVANCER_POWER));
        // a plain level copied into the folder reads with it too
        let (mut plain, used) = Level::parse_prefix(&d).unwrap();
        plain.header[31] = 15;
        assert_eq!(CustomMap::parse(&plain.to_bytes()).unwrap().level.header[31], ADVANCER_POWER);
        assert!(used > 0);
    }

    #[test]
    fn the_mission_and_its_countdown_round_trip() {
        let mut m = sample();
        assert_eq!((m.timer(), m.mission), (None, Mission::default()));
        let plain = m.to_bytes();
        m.mission = Mission { win: WinRule::Everything };
        m.set_timer(Some(Timer { minutes: 500, end: TimerEnd::Defeat }));
        let d = m.to_bytes();
        assert_eq!(d.len(), plain.len() + 6 + 1, "one record of one byte; the countdown lives in the level header");
        let back = CustomMap::parse(&d).unwrap();
        assert_eq!(back.mission, m.mission);
        assert_eq!(back.timer(), Some(Timer { minutes: MAX_MINUTES, end: TimerEnd::Defeat }));
        assert_eq!((back.level.header[65], back.level.header[66], back.level.header[67]), (1, MAX_MINUTES, 1));
        assert_eq!(back.to_bytes(), d);
        // the original's level part alone carries the countdown
        let (level, _) = Level::parse_prefix(&d).unwrap();
        assert_eq!(Timer::read(&level).map(|t| t.end), Some(TimerEnd::Defeat));
        m.set_timer(Some(Timer { minutes: 20, end: TimerEnd::Victory }));
        assert_eq!((m.level.header[66], m.level.header[67]), (20, 0));
        m.set_timer(None);
        m.mission = Mission::default();
        assert_eq!(m.to_bytes(), plain);
    }

    #[test]
    fn reinforcements_round_trip() {
        let mut m = sample();
        let plain = m.to_bytes();
        m.set_timer(Some(Timer { minutes: 15, end: TimerEnd::Reinforcements }));
        m.reinforcement = Reinforcement {
            credits: 5000,
            research: 2500,
            air: [3, 6, 0, 1, 8],
            drops: vec![Landing { cell: (70, 48), squad: 17 }, Landing { cell: (77, 36), squad: 0 }],
        };
        let d = m.to_bytes();
        assert_eq!(d.len(), plain.len() + 6 + 4 + 4 + 5 + 2 + 2 * 5);
        let back = CustomMap::parse(&d).unwrap();
        assert_eq!(back.reinforcement, m.reinforcement);
        assert_eq!(back.timer().map(|t| t.end), Some(TimerEnd::Reinforcements));
        assert_eq!(back.to_bytes(), d);
        // out-of-range values of a hand-made file are cut back
        m.reinforcement = Reinforcement { credits: 999_999, research: -5, air: [200; 5], drops: vec![Landing { cell: (1, 1), squad: 99 }; 9] };
        let back = CustomMap::parse(&m.to_bytes()).unwrap().reinforcement;
        assert_eq!((back.credits, back.research, back.air, back.drops.len()), (MAX_REINFORCEMENT, 0, [MAX_REINFORCEMENT_AIR; 5], MAX_DROPS));
        assert_eq!(back.drops[0].squad, SQUADS - 1);
    }

    #[test]
    fn a_landing_block_is_four_by_two_and_blocks_that_share_a_cell_overlap() {
        let cells: Vec<_> = landing_block((10, 5)).collect();
        assert_eq!(cells.len(), 8);
        assert!(cells.contains(&(9, 5)) && cells.contains(&(12, 6)) && !cells.contains(&(13, 5)) && !cells.contains(&(10, 7)));
        for (b, want) in [((13, 5), true), ((14, 5), false), ((7, 6), true), ((6, 6), false), ((10, 6), true), ((10, 7), false)] {
            assert_eq!(landings_overlap((10, 5), b), want, "{b:?}");
            assert_eq!(landing_block((10, 5)).any(|c| landing_block(b).any(|d| d == c)), want, "{b:?} by cells");
        }
    }

    #[test]
    fn an_escort_round_trips_and_needs_pods_and_a_goal_but_no_blue_headquarters() {
        let mut m = sample();
        m.mission.win = WinRule::Escort;
        m.level.objects.remove(0); // blue's headquarters: not needed for an escort
        m.ext.remove(0);
        assert_eq!(m.problems(), vec![Problem::NoPods, Problem::NoGoal]);
        assert!(Problem::NoPods.blocks_play() && Problem::NoGoal.blocks_play());
        m.level.markers.push(Marker { kind: POD, player: 0, x: 5 * 32 + 16, y: 5 * 32 + 16 });
        m.level.markers.push(Marker { kind: POD, player: 1, x: 9 * 32 + 16, y: 5 * 32 + 16 });
        assert_eq!((pods(&m.level, 0), pods(&m.level, 1)), (1, 1));
        m.escort = Escort { goal: Some((60, 70)), route: vec![(20, 5), (40, 30)] };
        assert_eq!(m.problems(), vec![]);
        let d = m.to_bytes();
        let back = CustomMap::parse(&d).unwrap();
        assert_eq!((back.mission.win, back.escort.clone()), (WinRule::Escort, m.escort.clone()));
        assert_eq!(back.to_bytes(), d);
        assert!(back.escort.in_goal((61, 69)) && back.escort.in_goal((60, 70)) && !back.escort.in_goal((62, 70)));
        // a long winding route round-trips whole
        m.escort.route = (0..32).map(|k| (3 + k % 2 * 3, 4 + k)).collect();
        let back = CustomMap::parse(&m.to_bytes()).unwrap();
        assert_eq!(back.escort, m.escort);
        // a route without a goal yet is kept; too many waypoints are cut
        m.escort = Escort { goal: None, route: vec![(1, 2); MAX_WAYPOINTS + 4] };
        let back = CustomMap::parse(&m.to_bytes()).unwrap();
        assert_eq!((back.escort.goal, back.escort.route.len()), (None, MAX_WAYPOINTS));
        assert_eq!(back.problems(), vec![Problem::NoGoal]);
        // the other rules still need blue's headquarters
        m.mission.win = WinRule::Everything;
        assert_eq!(m.problems()[0], Problem::NoHeadquarters(0));
    }

    #[test]
    fn unknown_records_are_skipped() {
        let mut d = sample().to_bytes();
        let (_, used) = Level::parse_prefix(&d).unwrap();
        let count = u16::from_le_bytes([d[used + 6], d[used + 7]]);
        d[used + 6..used + 8].copy_from_slice(&(count + 1).to_le_bytes());
        d.extend(99u16.to_le_bytes());
        d.extend(3u32.to_le_bytes());
        d.extend([1, 2, 3]);
        assert_eq!(CustomMap::parse(&d).unwrap().name, "Twin Rivers");
    }

    #[test]
    fn a_layout_round_trips_with_its_cell_bits() {
        let mut m = sample();
        let cells: Vec<bool> = (0..15).map(|i| i % 3 == 0 || i == 14).collect();
        let f = Feature { source: Terrain { warmonger: true, mission: 17 }, x: 2, y: 3, w: 5, h: 3, dx: 40, dy: 7, cells };
        let trails = vec![TrailStamp { at: (300, 400), from: (1000, 64), radius: 22 }];
        m.layout = Some(Layout { patches: vec![(0, 0), (88, 1200), (2900, 17)], features: vec![f.clone(), Feature { dx: 1, ..f }], trails });
        let d = m.to_bytes();
        assert_eq!(d.len(), sample().to_bytes().len() + 6 + 2 + 3 * 4 + 2 + 2 * (8 + 2) + 2 + 9, "one LAYOUT record, 15 cells in 2 bytes");
        assert_eq!(CustomMap::parse(&d).unwrap().layout, m.layout);
        assert_eq!(CustomMap::parse(&sample().to_bytes()).unwrap().layout, None);
    }

    #[test]
    fn advancers_follow_the_headquarters_flags() {
        let m = sample();
        let l = CustomMap::parse(&m.to_bytes()).unwrap().level;
        let adv: Vec<(i32, (i32, i32))> = l.markers.iter().filter(|m| m.is_advancer()).map(|m| (m.player, m.cell())).collect();
        assert_eq!(adv, vec![(0, (3, 3)), (1, (41, 41))], "one per headquarters with an Advancer, at its cell + (1,1)");
        assert_eq!((l.header[34], l.header[35]), (0, 0));
        let mut m = sample();
        set_advancers(&m.level, &mut m.ext, 1, 0);
        assert_eq!(advancers(&m.level, &m.ext, 1), 0);
        let l = CustomMap::parse(&m.to_bytes()).unwrap().level;
        assert_eq!(l.header[35], 1, "green gets no replacement Advancer either");
        assert_eq!(m.problems(), vec![Problem::NoAdvancer(1)]);
        set_advancers(&m.level, &mut m.ext, 1, 2);
        assert_eq!(advancers(&m.level, &m.ext, 1), 2);
        m.level.objects.remove(0);
        m.ext.remove(0);
        assert_eq!(m.problems(), vec![Problem::NoHeadquarters(0)]);
        assert!(m.problems()[0].blocks_play() && !Problem::NoAdvancer(0).blocks_play());
    }

    #[test]
    fn sides_read_back_what_they_write() {
        let mut m = sample();
        let s = Sides { credits: 4500, research: 300, air: [1, 2, 3, 4, 5], turrets: [true, false, true, false, false], power: 12 };
        s.write(&mut m.level, 1);
        assert_eq!(Sides::read(&m.level, 1), s);
        assert_eq!(Sides::read(&m.level, 0).credits, 3000, "the other side is untouched");
        assert_eq!((m.level.header[3], m.level.header[38], m.level.header[32], m.level.header[6 + 3 * 4]), (4500, 300, 12, 5));
        let byte = |b: usize| (m.level.header[b / 4 + 2] >> (8 * (b % 4))) & 0xff;
        assert_eq!([byte(94), byte(95), byte(96), byte(84)], [1, 0, 1, 1]);
    }

    #[test]
    fn names_become_plain_file_stems() {
        assert_eq!(file_stem("Žltá púšť"), "zlta-pust");
        assert_eq!(file_stem("  Twin Rivers!! "), "twin-rivers");
        assert_eq!(file_stem("Ďaleká Ťava 2"), "daleka-tava-2");
        assert_eq!(file_stem("a/b\\c:d*e?"), "a-b-c-d-e");
        assert_eq!(file_stem("Straße"), "strasse");
        let cjk = file_stem("双河");
        assert!(cjk.starts_with("map-") && cjk != file_stem("三河"));
        assert_eq!(file_stem("   "), "map");
        assert!(file_stem(&"x".repeat(80)).len() <= MAX_NAME);
        assert_eq!(clean_name("a\tb\n  c"), "a b c");
        assert_eq!(clean_name(&"é".repeat(50)).chars().count(), MAX_NAME);
    }
}
