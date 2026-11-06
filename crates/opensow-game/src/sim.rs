//! The skirmish simulation: units walk, weapon tops aim and fire, bullets fly and explode, structures take damage.
//!
//! Everything here is pure (no I/O, no rendering) and built from the decoded game rules: chassis animation
//! (`anim`), weapon tops (`aim`), target search over range rings (`rings`), bullet paths and area damage
//! (`projectile`). Values the exe analysis has not produced are placeholders and marked `PLACEHOLDER`:
//! movement speed, the weapons of static turrets and the enemy's behaviour.

use opensow_formats::cursor::Pointer;
use opensow_formats::wave::{find_targets, Seen};
mod tutorial;
pub mod savestate;
mod building_defaults;
pub use tutorial::{text_index as tutorial_text_index, TutEvent, Tutorial};
use opensow_formats::advancer::{action_for_pointer, pointer_over_building, building_max_hp, capture_winner, hp_per_event, rate as advancer_rate, removed_when_destroyed, upgrade_per_event, fragment, fragment_count, Action, BODKY_B, BODKY_R, BODKY_Y, HEAL_PER_TICK, BeamTimer};
use opensow_formats::aim::{aim_params, TopAim};
use opensow_formats::buildings::{blocked_cells, can_place, footprint};
use opensow_formats::air::{AirForce, HEADER_ORDER};
use opensow_formats::aircraft::{launch_counter, squad, MODE1_SQUAD, Aircraft, Event as AirEvent};
use opensow_formats::anim::{walk_params, ChassisAnim, HOVER_ALTITUDE, HOVER_PERIOD, HOVER_PHASES, HOVER_TABLE};
use opensow_formats::effects::{flame_trail, gun_trail, spark_trail, trail as shell_trail, blast, impact_anims, impact_leaves_decal, impact_smoke, smoke_puff, UNIT_DEATH_PUFFS, AnimSpec, BulletAnim, AIR_DESTROYED, BUILDING_DESTROYED, TURRET_DESTROYED, UNIT_CLOUD, UNIT_WRECK, debris, debris_variant, DEBRIS_STRENGTH, destruction_bursts, BURST_GAP, BURST_JITTER, DESTRUCTION_PASSES, UNIT_DEATH_CLOUDS, CORPSE_LIFE, CORPSE_BLAST_BELOW, corpse_blasts, WRECK_SMOKE_COLUMNS, WRECK_SMOKE_FIRST, WRECK_SMOKE_PERIOD, WRECK_SMOKE_TYPE, SATLASER_STRENGTH, SATLASER_START_DY, SATLASER_END_DY};
use opensow_formats::facing::{facing, heading_degrees, turn_step};
use opensow_formats::messages as msg;
use opensow_formats::level::{Level, ObjectKind, Owner, UnitKind};
use opensow_formats::production::{factory_exit, FactoryExit, NO_SPACE_TICKS, income_per_tick, next_tier, rate_row, slot_target, slot_unlock_cost, turret_price, Slots, CREDITS_CAP, RESEARCH_CAP};
use opensow_formats::projectile::{TRIPLER_BLAST_STRENGTH, TRIPLER_RING, TRIPLER_RING_USED, build_path, bullet_type, splash_victims, steps_at, weapon_shot, Path, UnitRef};
use opensow_formats::freecell::free_cell_near;
use opensow_formats::rings::Rings;
use opensow_formats::wave::{approach, approach_air, Wave};
use opensow_formats::rng::GameRng;
use opensow_formats::curves::{CurveTable, RandomBullet};
use std::collections::HashMap;

pub const CELL: i32 = 32;
/// Game ticks per second: the exe's frame accumulator (0x42e890) emits one tick per 1666 hundredths of a millisecond = 16.66 ms.
pub const TICKS_PER_SECOND: u32 = 60;
/// Ticks per facing step of the always-spinning Advancer (3 ticks = 50 ms, measured from a recording).
pub const ADVANCER_SPIN_TICKS: u64 = 3;
/// `tt-fire`: the sound the constructor of the Advancer gun's bullet (type 31) plays at its start point.
pub const GUN_SOUND: u8 = 56;

/// Ticks between Advancer beam shots: the beam state machine (`0x47c6b0`) counts a length of 10 (`[beam+0x20]`) down once per tick.
pub const ADVANCER_BEAM_TICKS: u32 = opensow_formats::advancer::BEAM_PERIOD;
/// Ticks between satellite checks of a protected building (`[bld+0x11c]` reloads to 240).
pub const SATELLITE_TICKS: i32 = 240;
/// Anti-air weapons never fire at ground targets (they use the air search functions of the exe).
const ANTI_AIR: [i32; 5] = [1, 2, 3, 42, 110];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Target {
    Unit(u32),
    Structure(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Outcome {
    Victory,
    Defeat,
}

/// Screen shake state (`0x6bf980` camera object: `+0x54` ticks left, `+0x60/+0x6c` epicentre, step list).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Quake {
    pub ticks: i32,
    pub at: (i32, i32),
    pub stack: Vec<(i32, i32)>,
}

impl Quake {
    /// `0x458620(duration, x, y)`: the shake lasts `duration / 2` ticks; the pending undo list is dropped (the camera keeps its offset).
    pub fn start(&mut self, duration: i32, x: i32, y: i32) {
        self.ticks = duration / 2;
        self.at = (x, y);
        self.stack.clear();
    }
}

/// Shake durations passed to `0x458620` by `0x409130`: 0x1e for turrets (kinds 25-29), 0x57 for other buildings.
/// Ticks the local player's air icons stay disabled after a launch (`0x467f03`: `[player+0x40] = 0xb4`).
pub const STRIKE_LOCK_TICKS: i32 = 0xb4;
pub const QUAKE_TURRET: i32 = 0x1e;
pub const QUAKE_BUILDING: i32 = 0x57;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Unit {
    pub id: u32,
    pub kind: i32,
    /// 0 = the human player (blue), anything else is an enemy (green).
    pub player: i32,
    pub x: f32,
    pub y: f32,
    pub hp: i32,
    pub max_hp: i32,
    pub chassis: Option<ChassisAnim>,
    pub aim: Option<TopAim>,
    pub path: Vec<(f32, f32)>,
    pub target: Option<Target>,
    pub selected: bool,
    moved: bool,
    line: Option<Line>,
    /// The pixel the unit is walking to right now: the centre of its step cell `[unit+0x6c/0x70]`, chosen by the local
    /// search (`0x473190`). `path` is the *global* route (`[unit+0x88]`): the exe only drops its first cell per planned step and
    /// does not follow the local detours, so `path[0]` stands for the segment in flight and is removed when it is done.
    #[serde(default)]
    step: Option<(f32, f32)>,
    /// The order's destination pixel (`[chassis+0x5c/0x60]`): when the route is used up elsewhere the unit searches again.
    #[serde(default)]
    dest: Option<(i32, i32)>,
    /// Planning rounds until the global route is searched again (`[chassis+0x80]`: reset to 6, searched below zero).
    #[serde(default)]
    cool: i32,
    /// A type-2 order (`0x470480`) is running: the unit walks the straight line in `path` without path finding or avoidance and
    /// then, as the queued type-1 order, plans a route to this cell (factory exits, `0x408740`).
    straight: Option<(i32, i32)>,
    /// The third order a factory gives a new unit (`0x46e510(1, rally)` queued after the exit orders): a type-1 order to this
    /// map pixel that starts when the previous one is done. Any order given to the unit afterwards replaces the list.
    queued: Option<(i32, i32)>,
    /// Hover bob phase of the Advancer (`[obj+0x84]`).
    pub hover_phase: u32,
    /// The Advancer's building order (destroy / repair / upgrade) and the structure it targets (docs/advancer-actions.md).
    pub action: Option<(Action, usize)>,
    /// The Advancer's beam cooldown member (`[top+0x104]`, `advancer::BeamTimer`): ticks every tick, a shot every 10.
    pub beam: BeamTimer,
    /// Beam shots so far; the beam sound plays on every third.
    pub beam_shots: u32,
    /// Target the unit chases (`[unit+0x7c]`, set by the AI order `0x469fe0`).
    pub chase: Option<Target>,
    /// Advancer idle counter (`[top+0x150]`): at 420 the player hears "Advancer disengaged" and it restarts from -840.
    pub idle: i32,
    /// Ticks until an order-less unit turns its top to a random facing (`[unit+0x8c]`, 240 + random(900); starts at 0).
    pub look: i32,
}

/// Bresenham state of the path segment a unit is walking.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
struct Line {
    goal: (i32, i32),
    major: i32,
    minor: i32,
    err: i32,
}

impl Unit {
    pub fn px(&self) -> (i32, i32) {
        (self.x.round() as i32, self.y.round() as i32)
    }

    /// Screen-space y of the sprite: the Advancer flies 64 px above its ground point and bobs up and down.
    pub fn sprite_y(&self) -> f32 {
        if self.kind == 40 {
            self.y + (HOVER_ALTITUDE + HOVER_TABLE[self.hover_phase as usize % HOVER_PHASES as usize]) as f32
        } else {
            self.y
        }
    }

    /// The two points the exe derives from a flying unit's hover offset `h = sprite y - y` (`0x4797c0`, flyer case `0x4798ac`):
    /// `(aim point for the facing, point a shot flies to)` = `(x, y + h/2)` (truncated) and `(x, y + h)`: a shot meets the sprite.
    pub fn flyer_points(&self) -> ((i32, i32), (i32, i32)) {
        let (x, y) = self.px();
        let h = self.sprite_y().round() as i32 - y;
        ((x, y + h / 2), (x, y + h))
    }

    pub fn cell(&self) -> (i32, i32) {
        (self.x.floor() as i32 / CELL, self.y.floor() as i32 / CELL)
    }

    /// The cell of the next path point: the step cell `[unit+0x6c/0x70]` the exe keeps stamped while the unit walks to it.
    pub fn heading_cell(&self) -> Option<(i32, i32)> {
        self.step.map(|(x, y)| ((x.floor() as i32).div_euclid(CELL), (y.floor() as i32).div_euclid(CELL)))
    }

    /// The order is dropped (a stop, a failed search): the unit finishes the segment in flight and stays there.
    fn halt(&mut self) {
        self.path.clear();
        self.dest = None;
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Structure {
    pub object: usize,
    pub kind: ObjectKind,
    pub owner: Owner,
    pub cell: (i32, i32),
    pub hp: i32,
    pub max_hp: i32,
    pub aim: Option<TopAim>,
    pub weapon: i32,
    pub target: Option<Target>,
    /// Production slots of a factory (docs/production.md); only the first `active` run.
    pub slots: Vec<SlotState>,
    pub active: usize,
    /// Raw object kind id (25-29 turrets, 100-108 buildings).
    pub kind_id: i32,
    /// Damage dealt by player 0/1/2 (`[bld+0x7c + p*4]`); the largest foreign tally takes the building when it is destroyed.
    pub tally: [i32; 3],
    /// Hit points still to be healed in (`[bld+0x6c]`), paid out at `HEAL_PER_TICK` per tick.
    pub heal: i32,
    /// Upgrade points towards the next production slot (`[bld+0x148]`).
    pub upgrade: i32,
    /// Satellite protection (object word 15 = `[bld+0x114]`; every HQ): an Advancer attacking this building is destroyed.
    pub protected: bool,
    /// An Advancer shot hit this building since the last satellite check (`[bld+0x118]`).
    pub attacked: bool,
    /// Ticks until the satellite may strike again (`[bld+0x11c]`).
    pub sat_cd: i32,
    /// Headquarters: the id of its Advancer (`[bld+0xfc]`); a new one is built at the HQ when it is gone.
    pub advancer: Option<u32>,
    /// Rally point in map pixels (`[bld+0x2c0]`, `[bld+0x2c4]`; -1 = none): set by the player on an own factory, units leaving
    /// the factory walk on to it (docs/rally.md). A captured building is rebuilt by the exe and starts without one.
    pub rally: Option<(i32, i32)>,
    /// The destruction animation (`0x409b60`, state `[b+0x104] == 2`): `(animation counter [b+0x10c], passes until the next burst
    /// [b+0x110])`. The dead building stays on the map until it ends; then it is captured or removed (`0x43ab40`).
    #[serde(default)]
    pub dying: Option<(i32, i32)>,
}

/// One production slot: the unit kind, progress and the target the progress must reach.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct SlotState {
    pub kind: i32,
    pub progress: u32,
    pub target: u32,
    /// Tech-levels bought so far (`[slot+0x20]`) and the most that can be bought (`[slot+0x24]`, level word 8-12).
    pub level: u32,
    pub max_level: u32,
    /// Ticks the finished unit has waited for a free factory exit (`[slot+0x1c]`).
    pub waiting: u32,
}

/// Unit slots per side (`0x5b62c0`, 1000 per player): the unit creator `0x43a420` fails at `cmp ecx, 0x3e8` when none is free, and a
/// factory whose unit cannot be created keeps it waiting. A destroyed unit holds its slot for [`CORPSE_LIFE`] steps.
const UNIT_SLOTS: usize = 1000;

impl Structure {
    pub fn alive(&self) -> bool {
        self.hp > 0
    }

    /// Still in the exe's object table: alive, or dying (the win/lose scan `0x4212f0` only looks at the table).
    pub fn present(&self) -> bool {
        self.alive() || self.dying.is_some()
    }

    fn clone_footprint(&self) -> Footprint {
        Footprint { cell: self.cell, size: self.size() }
    }

    /// Footprint in cells (`0x409760`); unknown kinds take one cell.
    pub fn size(&self) -> (i32, i32) {
        footprint(self.kind_id).map_or((1, 1), |(w, h, _)| (w, h))
    }

    pub fn covers(&self, c: (i32, i32)) -> bool {
        let (w, h) = self.size();
        c.0 >= self.cell.0 && c.0 < self.cell.0 + w && c.1 >= self.cell.1 && c.1 < self.cell.1 + h
    }

    /// Footprint plus the two cells above it, where the building's art rises over the cells behind it (for clicking).
    pub fn covers_art(&self, c: (i32, i32)) -> bool {
        let (w, h) = self.size();
        c.0 >= self.cell.0 && c.0 < self.cell.0 + w && c.1 >= self.cell.1 - 2 && c.1 < self.cell.1 + h
    }

    /// The cell in the middle of the footprint, where the Advancer hovers.
    pub fn center_cell(&self) -> (i32, i32) {
        let (w, h) = self.size();
        (self.cell.0 + w / 2, self.cell.1 + h / 2)
    }

    /// The cell the Advancer hovers over to work on this building (`0x409ab0` / `0x409ac0`).
    pub fn action_cell(&self) -> (i32, i32) {
        let (dx, dy) = opensow_formats::buildings::advancer_offset(self.kind_id);
        (self.cell.0 + dx, self.cell.1 + dy)
    }

    pub fn center(&self) -> (i32, i32) {
        let (w, h) = self.size();
        (self.cell.0 * CELL + w * CELL / 2, self.cell.1 * CELL + h * CELL / 2)
    }

    /// The cells this building blocks (layout mask `0x486b20`).
    pub fn blocked_cells(&self) -> Vec<(i32, i32)> {
        let v: Vec<_> = blocked_cells(self.kind_id).into_iter().map(|(dx, dy)| (self.cell.0 + dx, self.cell.1 + dy)).collect();
        if v.is_empty() { vec![self.cell] } else { v }
    }
}#[derive(serde::Serialize, serde::Deserialize)]


/// A Rogon bomb explosion (event type 35, `0x411220`): after an initial burst of 19 steps it advances one step per tick along the
/// range-ring offset list, each step an area damage of `370 - k` at the cell `k` words into the list (read as overlapping
/// `(w[k], w[k+1])` pairs, a quirk of the exe), until `NUKE_STEPS` words are used (the list up to ring 7).
pub struct Nuke {
    pub x: i32,
    pub y: i32,
    pub owner: i32,
    ex: opensow_formats::rogon::Explosion,
}

pub struct Footprint {
    cell: (i32, i32),
    size: (i32, i32),
}

impl Footprint {
    fn covers(&self, c: (i32, i32)) -> bool {
        c.0 >= self.cell.0 && c.0 < self.cell.0 + self.size.0 && c.1 >= self.cell.1 && c.1 < self.cell.1 + self.size.1
    }
}

/// Key offset of the `.po2` reference-point tables in `World::turret_muzzles` (`po1` tables sit at the weapon kind itself).
pub const REFERENCE_KEY: i32 = 1000;

fn bullet_type_interval(t: u8) -> i32 {
    bullet_type(t).and_then(|t| t.interval).unwrap_or(0) as i32
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Bullet {
    pub bullet_type: u8,
    pub strength: i32,
    pub owner: i32,
    pub path: Path,
    pub index: usize,
    /// Sprite frame state (`effects::BulletAnim`), for the types with a timer animation or progress-driven frame.
    pub anim: Option<BulletAnim>,
    /// Facing of the flight direction in 32 steps (the missile sprite).
    pub facing: u8,
    /// Steps until the next flame puff (flamer bullets, `[+0x264c]`, `effects::flame_trail`).
    pub trail: i32,
    /// `[+0x2680]`: the top sprite index of a flying piece (types 15/16, `effects::debris_variant`), else 0.
    pub variant: u32,
    /// The shooter's target handle `[+0x2688]`: bullet types 1, 10 and 31 take their strength off this target when they land
    /// (`0x4108bc..0x4109b8`) instead of making area damage.
    pub target: Option<HitTarget>,
    /// `[+0x2664]`: path steps per 60 ticks. The constructor's value, which the bomber's bomb (type 18) raises every tick by the
    /// steps it just took (`0x41146d..0x411472`): it falls faster and faster.
    #[serde(default)]
    pub speed: u32,
}

/// What a targeted bullet (types 1, 10, 31) hits: a unit by id, an aircraft by `Aircraft::id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HitTarget {
    Unit(u32),
    Aircraft(u32),
}

/// A burning wreck (`effects::WRECK_SMOKE_COLUMNS`): throws a smoke column every 20 steps.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Wreck {
    pub x: i32,
    pub y: i32,
    pub left: i32,
    pub timer: i32,
}

/// A destroyed unit still in its slot (`unit+0x90..0x98`, `0x46b600`): blocks its cell and ends in a few blasts.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Corpse {
    pub x: i32,
    pub y: i32,
    pub left: i32,
    pub variant: u32,
    pub air: bool,
    pub blasted: bool,
    /// Whose slot it holds.
    #[serde(default)]
    pub player: i32,
    /// The dead unit's kind: its slot still counts for the computer's turret threat (`0x431120`).
    #[serde(default)]
    pub kind: i32,
}

impl Bullet {
    pub fn new(bullet_type: u8, strength: i32, owner: i32, path: Path) -> Bullet {
        let index = path.first_step;
        let anim = crate::sim::bullet_type(bullet_type).and_then(|t| t.anim).map(BulletAnim::new);
        let (a, b) = (path.points[0], *path.points.last().unwrap());
        // the exe's own value for the missile (start to end, `Path::facing`); other types only need a heading for the viewer
        let facing = path.facing.unwrap_or_else(|| facing(a.0, a.1, b.0, b.1, 32) as u8);
        let trail = bullet_type_interval(bullet_type);
        let speed = crate::sim::bullet_type(bullet_type).and_then(|t| t.speed).unwrap_or(60);
        Bullet { bullet_type, strength, owner, path, index, anim, facing, trail, variant: 0, target: None, speed }
    }

    pub fn position(&self) -> (i32, i32) {
        self.path.points[self.index.min(self.path.points.len() - 1)]
    }
}

/// A scorch mark on the ground (`effects::impact_leaves_decal`): frame 0-4 of the decal sprite, one per cell.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Decal {
    pub x: i32,
    pub y: i32,
    pub cell: (i32, i32),
    pub frame: u8,
}

/// The flash of a building that was just given as a target (`0x4086a0` sets `[b+0x14c]=1`, `[+0x150]=5`, `[+0x154]=1`,
/// `[+0x158]=8`; `0x40de8a..0x40dec5` toggles `[+0x154]` every 5 ticks eight times; `0x409467` draws the building's own
/// sprite again, half-additive, while it is on).
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Blink {
    pub timer: i32,
    pub on: bool,
    pub left: i32,
}

impl Blink {
    pub fn new() -> Blink {
        Blink { timer: 5, on: true, left: 8 }
    }
}

/// An Advancer shot (bullet type 19 / 20 / 21, docs/advancer-actions.md) on its way to a building.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BeamShot {
    pub action: Action,
    pub target: usize,
    pub side: usize,
    pub rate: i32,
    pub path: Path,
    pub index: usize,
    /// Steps until the upgrade bullet drops its next sparks.
    pub trail: i32,
    /// Spark offsets and sprite frames (`advancer::fragment`).
    pub frags: Vec<(i32, i32, u16)>,
}

/// An animation sprite playing at a map position (explosions, smoke, wrecks; `effects::AnimSpec`).
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Effect {
    pub x: i32,
    pub y: i32,
    pub spec: AnimSpec,
    pub age: u32,
}

/// The grids of the exe's local avoidance search (docs/walking.md).
/// The tutorial-like hint box of missions 1 and 6 (`0x4303e0`, `0x430770`, docs/ai.md): a darkened box around the screen
/// centre with a line of text. Sizes are in the original's pixels: the box is `half` wide on each side of the screen
/// middle and spans middle-15 .. middle+10 vertically, the text starts `text_dx` left of the middle at middle-7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Hint {
    /// Index of the string in the language file (281 / 282).
    pub text: usize,
    pub half: i32,
    pub text_dx: i32,
}

/// The occupancy layers of one tick. The exe keeps two sets of grids (docs/pathfinding.md): the ground set for every unit
/// and a second set for units whose type has the air flag `[type+0x4b]` (only the Advancer), with its own terrain: the
/// map's border layer instead of the blocked layer, so Advancers fly over water, mountains and buildings.
pub struct Occupancy {
    ground: Wave,
    /// The Advancers' unit grid (`0x62ac38`).
    air: Wave,
    stat: Vec<u32>,
    /// The air terrain (`0x60ac38`), also the static grid of the Advancers' local search.
    air_stat: Vec<u32>,
    /// `[0x49a360]`, the base of the static grid.
    stat_base: u32,
}

/// Developer cheats (`devtools.rs`); not in the original, never saved, and all off by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cheats {
    /// The player's units never lose hit points.
    pub invulnerable: bool,
    /// Credits and research of the player are kept at their caps.
    pub rich: bool,
    /// The enemy's computer player, units, turrets and factories do nothing.
    pub freeze_enemy: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct World {
    #[serde(skip)]
    pub cheats: Cheats,
    pub cells: (i32, i32),
    blocked: Vec<bool>,
    /// The terrain Advancers cannot enter: the `.map` border layer (the outer ring and, on a few maps, no-fly areas).
    air_blocked: Vec<bool>,
    /// The map's "no marks" layer (`+0x2c0cc`, `map::Cell::no_marks`): no decal, flying piece or smoke puff lands on these cells.
    /// Empty (= none) for worlds built without a map and for older snapshots.
    #[serde(default)]
    no_marks: Vec<bool>,
    /// The map's "no build" layer (`+0x300cc`, `map::Cell::no_build`): the placement test `0x408cfc` refuses these cells. Empty = none.
    no_build: Vec<bool>,
    pub units: Vec<Unit>,
    pub structures: Vec<Structure>,
    pub bullets: Vec<Bullet>,
    /// The Advancers' beam bullets in flight.
    pub beams: Vec<BeamShot>,
    /// Buildings flashing after the player ordered units onto them (`0x4086a0`), by structure index.
    pub blinks: HashMap<usize, Blink>,
    /// Idle look-around counters of the turret tops (`[unit+0x8c]`, a turret's top is a unit, so it looks around like one), by structure index.
    pub turret_look: HashMap<usize, i32>,
    /// The aircraft an anti-air top aims at (`[unit+0x80]`, kept by `0x47b3c0`), by shooter: `(false, unit id)` or
    /// `(true, structure index)` for the anti-air turret.
    #[serde(default)]
    pub air_targets: HashMap<(bool, usize), u32>,
    /// `.po1` muzzle tables of the turret tops, by weapon (109-113); filled by the game from the install, empty in pure tests.
    #[serde(skip)]
    pub turret_muzzles: HashMap<i32, opensow_formats::fire::FireTables>,
    pub effects: Vec<Effect>,
    /// Sound effects raised this tick (effect id, map x, map y, pitch jitter); the game drains them (`sfx.rs`).
    pub sounds: Vec<(u8, i32, i32, bool)>,
    pub decals: Vec<Decal>,
    /// The exe's visual generator (`0x437cd0`), kept apart from the lockstep one.
    fx_rng: u32,
    /// The ambient clouds (`CCloud` list `[0x49a3fc]`, docs/clouds.md); purely visual, rebuilt from the CLOUDS option after a load.
    #[serde(skip)]
    pub sky: opensow_formats::clouds::Sky,
    /// Built lazily once per tick.
    #[serde(skip)]
    occ: Option<Occupancy>,
    pub tick: u64,
    pub outcome: Option<Outcome>,
    /// The statistics screen's counters per side (player, enemy, neutral), `[0x6bf788 + side * 0x80 + 4 + 4 * k]`: buildings
    /// captured, buildings lost, units produced, units lost, turrets built, turrets lost, air forces sent, air forces lost
    /// (`formats::endscreen`, docs/results.md).
    pub stats: [[u32; 8]; 3],
    next_id: u32,
    rng: u32,
    #[serde(skip)]
    game_rng: Option<GameRng>,
    /// The 200 random curves of the building debris (types 13/14), drawn from the random table right after it is installed
    /// (`0x422c13` -> `0x40f2c0`); `None` without `generic.file`.
    #[serde(skip)]
    curves: Option<CurveTable>,
    /// The exe's building handle tables (`0x5bab00`, 300 per player): structure indices by slot. A new building takes the
    /// first free slot (`0x439e30`), a removed one leaves a hole; the computer's turret routine walks its table in slot order.
    #[serde(default)]
    slot_tables: [Vec<Option<usize>>; 3],
    /// The random table's position, copied in by [`World::snapshot`].
    rng_index: usize,
    #[serde(skip, default = "Rings::generate")]
    rings: Rings,
    has_enemy_hq: bool,
    /// Advancer strength per side `[0x5baaac + p*4]` (level header words 31/32, times 4); neutral never acts.
    pub rates: [i32; 3],
    /// RMB-toggled mode of the Advancer on its own buildings: repair instead of upgrade (tutorial string).
    pub repair_mode: bool,
    /// Credits and research points of the player (0) and the enemy (1) (`0x517af8` / `0x517b04`).
    pub credits: [i32; 2],
    pub research: [i32; 2],
    /// Tutorial step (`[0x70f2cc]`, written by the tutorial script `0x432d6e`; -1 = not in the tutorial). Outside the tutorial slot
    /// speed is `30 + 12 * windtrap levels`; inside it `90` (x4 on steps 15-17).
    pub tutorial_step: Option<u32>,
    /// The tutorial script's state (`sim/tutorial.rs`); `None` outside M00.
    pub tutorial: Option<Tutorial>,
    /// A view jump requested by a script (`0x4588c0`), in map pixels.
    pub camera_request: Option<(i32, i32)>,
    /// The computer plays the enemy (every mission except the tutorial; `0x42dc22`).
    pub ai: bool,
    /// Air-force stock of the player (0) and the enemy (1) in `HEADER_ORDER` (bomber, asteroid, carryall, tripler, fighter).
    pub air_stock: [[u32; 5]; 2],
    /// Turret tiers 1-5 each side may build: level file bytes 84..88 / 94..98 of the header block (`0x41d7f2`: copied to
    /// `[player+0x70baf5..f9]`, read by the HUD tabs `0x454e60` and the credit-crossing message `0x42d88c`).
    pub turret_flags: [[bool; 5]; 2],
    pub aircraft: Vec<Aircraft>,
    /// The exe's global carryall counter `[0x89cf50]` (0..9): never reset by the exe between missions, so the game hands it on.
    pub carry_counter: usize,
    /// `[player+0x40]` of the local player: set to 180 by every launch (`0x467f03`) and counted down by the HUD tick (`0x454da9`);
    /// while it is above 0 the air pills are disabled (widget state 2, `0x454e4e`): no hover, no click.
    pub strike_lock: i32,
    next_air_id: u32,
    /// Detonating Rogon bombs (event type 35).
    pub nukes: Vec<Nuke>,
    /// Burning wrecks of destroyed units (`0x45b0a0`).
    pub wrecks: Vec<Wreck>,
    /// Destroyed units that keep their cell blocked for [`CORPSE_LIFE`] steps.
    #[serde(default)]
    pub corpses: Vec<Corpse>,
    /// Messages for the local player: (tick, event id of `formats::messages`).
    pub log: Vec<(u64, u8)>,
    /// Where the last message event with a position happened (`[0x6bf6b0 + 0x20*player + 4/8]`, kept when later events pass -1).
    pub last_event: Option<(i32, i32)>,
    /// Level header words 34 / 35: no replacement Advancer for the player / the enemy (`[0x5baab8 + p*4]`).
    pub no_advancer_respawn: [bool; 2],
    /// Level header word 64 (`[0x49a354]`, loaded at `0x41d7ed`): 2 = the radar picture is drawn plain instead of additive (`0x454697`).
    #[serde(default)]
    pub weather: i32,
    /// Screen shake of the local player (`0x458930`, started by `0x458620`).
    #[serde(default)]
    pub quake: Quake,
    /// Mission number (file `M##`), `None` for multiplayer maps; selects the win rule and the scripted events.
    pub mission_no: Option<u32>,
    /// Mission countdown `[minutes, seconds]` with its end mode (level header words 65-67) and whether it has run out.
    pub timer: Option<(i32, i32, i32)>,
    pub timer_expired: bool,
    ai_strategy: u32,
    ai_timer: i32,
    ai_strike: i32,
    /// Sum of the active levels of each side's windtrap (`[0x5baaa0 + p*4]`), refreshed every tick.
    wind: [u32; 2],
    /// Whether any enemy existed when the first tick ran (a world without enemies is a sandbox, never won).
    enemy_at_start: Option<bool>,
    /// `[0x71c0e8]`: phase of the mission-1 hint (0 start, 1 fighters handed out, 2 fighters used).
    pub hint_phase: u8,
    /// The hint box to draw this frame, if any.
    pub hint: Option<Hint>,
}

/// Weapon top of a turret building (`0x40e004`, jump table `0x40e3b4`): turrets 1-5 (kinds 25-29) own the tops 109-113.
fn turret_weapon(n: u32) -> i32 {
    108 + n as i32
}


impl World {
    /// The spawner's end-point scatter for one round (`0x47cfe1..0x47d03f`): per axis (x, then y) `random(2)` picks the sign
    /// (1 = plus) and `random(spread)` the size; `spread` is `aim::spread(kind)`. Single player only (remote games skip it).
    fn volley_scatter(&mut self, kind: i32) -> (i32, i32) {
        let spread = opensow_formats::aim::spread(kind);
        let mut axis = |w: &mut World| {
            let plus = w.random(2) == 1;
            let m = w.random(spread) as i32;
            if plus { m } else { -m }
        };
        let jx = axis(self);
        let jy = axis(self);
        (jx, jy)
    }

    pub fn new(cells: (i32, i32), blocked: Vec<bool>) -> World {
        assert_eq!(blocked.len(), (cells.0 * cells.1) as usize);
        // without a map the Advancers are only kept inside the outer ring
        let air_blocked = (0..cells.0 * cells.1).map(|i| { let (x, y) = (i % cells.0, i / cells.0); x == 0 || y == 0 || x == cells.0 - 1 || y == cells.1 - 1 }).collect();
        World {
            cheats: Cheats::default(),
            cells,
            blocked,
            air_blocked,
            no_marks: Vec::new(),
            no_build: Vec::new(),
            units: Vec::new(),
            structures: Vec::new(),
            bullets: Vec::new(),
            beams: Vec::new(),
            blinks: HashMap::new(),
            turret_look: HashMap::new(),
            air_targets: HashMap::new(),
            turret_muzzles: HashMap::new(),
            effects: Vec::new(),
            sounds: Vec::new(),
            decals: Vec::new(),
            fx_rng: 0x9E3779B9,
            sky: Default::default(),
            occ: None,
            tick: 0,
            outcome: None,
            stats: [[0; 8]; 3],
            next_id: 1,
            rng: 0x2545_f491,
            game_rng: None,
            curves: None,
            slot_tables: Default::default(),
            rng_index: 0,
            rings: Rings::generate(),
            has_enemy_hq: false,
            rates: [40; 3],
            repair_mode: false,
            credits: [0; 2],
            research: [0; 2],
            tutorial_step: None,
            tutorial: None,
            camera_request: None,
            ai: false,
            air_stock: [[0; 5]; 2],
            aircraft: Vec::new(),
            carry_counter: 0,
            strike_lock: 0,
            next_air_id: 1,
            nukes: Vec::new(),
            wrecks: Vec::new(),
            corpses: Vec::new(),
            log: Vec::new(),
            last_event: None,
            no_advancer_respawn: [false; 2],
            weather: 0,
            quake: Quake::default(),
            mission_no: None,
            timer: None,
            turret_flags: [[false; 5]; 2],
            timer_expired: false,
            ai_strategy: 0,
            ai_timer: 0,
            ai_strike: 600,
            wind: [0; 2],
            enemy_at_start: None,
            hint_phase: 0,
            hint: None,
        }
    }

    /// Builds the world of a loaded level: blocked cells from the map, structures from the objects, units from the markers.
    pub fn from_level(map: &opensow_formats::map::Map, level: &Level) -> World {
        let (w, h) = (map.width as i32, map.height as i32);
        let mut blocked = vec![false; (w * h) as usize];
        for c in map.cells.iter().filter(|c| c.blocked) {
            blocked[c.y as usize * w as usize + c.x as usize] = true;
        }
        let mut world = World::new((w, h), blocked);
        // the air layer: the map's second terrain grid (`+0x180cc`), which the .map file stores as the "border" layer
        for c in map.cells.iter() {
            world.air_blocked[c.y as usize * w as usize + c.x as usize] = c.border;
        }
        if map.cells.iter().any(|c| c.no_build) {
            world.no_build = vec![false; (w * h) as usize];
            for c in map.cells.iter().filter(|c| c.no_build) {
                world.no_build[c.y as usize * w as usize + c.x as usize] = true;
            }
        }
        if map.cells.iter().any(|c| c.no_marks) {
            world.no_marks = vec![false; (w * h) as usize];
            for c in map.cells.iter().filter(|c| c.no_marks) {
                world.no_marks[c.y as usize * w as usize + c.x as usize] = true;
            }
        }
        world.rates = [advancer_rate(level, 0), advancer_rate(level, 1), 40];
        world.ai = true;
        world.no_advancer_respawn = [level.header[34] != 0, level.header[35] != 0];
        world.weather = level.header[64] as i32;
        if level.header[65] != 0 {
            world.timer = Some((level.header[66], 0, level.header[67]));
        }
        for (g, st) in level.air_stock().iter().enumerate() {
            world.air_stock[0][g] = st.counts[0].max(0) as u32;
            world.air_stock[1][g] = st.counts[1].max(0) as u32;
        }
        // header words 2/3 = credits, 37/38 = research points of player/enemy (`0x41d9c5..0x41d9f5`)
        world.credits = [level.header[2], level.header[3]];
        // header block bytes: `header[i]` is block word `i - 2`
        let byte = |b: usize| (level.header[b / 4 + 2] >> (8 * (b % 4))) & 0xff != 0;
        for t in 0..5 {
            world.turret_flags[0][t] = byte(84 + t);
            world.turret_flags[1][t] = byte(94 + t);
        }
        world.research = [level.header[37], level.header[38]];
        for (i, o) in level.objects.iter().enumerate() {
            let kind = o.object_kind();
            if matches!(kind, ObjectKind::Unknown(_)) || o.start_hp(building_max_hp(o.kind)) <= 0 {
                continue;
            }
            let weapon = if let ObjectKind::Turret(n) = kind { turret_weapon(n as u32) } else { 0 };
            let aim = (weapon != 0).then(|| TopAim::new(weapon, 0)).flatten();
            let (mut slots, mut active) = (Vec::new(), 0);
            if matches!(kind, ObjectKind::LightFactory | ObjectKind::MediumFactory | ObjectKind::HeavyFactory | ObjectKind::WalkerFactory | ObjectKind::GoldMine | ObjectKind::ResearchLab | ObjectKind::Wind | ObjectKind::AirBase) {
                let rec = Slots::of(o);
                slots = rec.kinds.iter().zip(rec.values).filter_map(|(&k, v)| slot_target(k).filter(|_| k != 0).map(|target| SlotState { kind: k, progress: 0, target, level: 0, max_level: v.max(0) as u32, waiting: 0 })).collect();
                active = (rec.active as usize).min(slots.len());
            }
            world.add_structure(Structure { object: i, kind, owner: o.owner(), cell: (o.cell_x, o.cell_y), hp: o.start_hp(building_max_hp(o.kind)), max_hp: building_max_hp(o.kind), aim, weapon, target: None, slots, active, kind_id: o.kind, tally: [0; 3], heal: 0, upgrade: 0, protected: o.raw[15] == 1 || o.kind == 100, attacked: false, sat_cd: 0, advancer: None, rally: None, dying: None });
        }
        for m in &level.markers {
            if let UnitKind::Unknown(_) | UnitKind::RuntimeOnly(_) = m.unit_kind() {
                continue;
            }
            world.add_unit(m.kind, m.player, m.x, m.y);
        }
        // each headquarters owns the first free Advancer of its side (0x41e030..0x41e0a7)
        for k in 0..world.structures.len() {
            if world.structures[k].kind_id != 100 {
                continue;
            }
            let side = side_of(world.structures[k].owner);
            let taken: Vec<u32> = world.structures.iter().filter_map(|s| s.advancer).collect();
            let id = world.units.iter().find(|u| u.kind == 40 && Some(if u.player == 0 { 0 } else { 1 }) == side && !taken.contains(&u.id)).map(|u| u.id);
            world.structures[k].advancer = id;
        }
        world
    }

    /// The whole running state as bytes (JSON; the savegame container compresses it). Static tables (`rings`, muzzle tables,
    /// the random table, the occupancy grids) are not stored: `restore` takes them from a freshly built world of the same level.
    pub fn snapshot(&mut self) -> Vec<u8> {
        self.rng_index = self.game_rng.as_ref().map_or(0, |r| r.index());
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Replaces this (freshly built) world's state by a snapshot of the same level; `Err` when the bytes are not one or the map
    /// does not match.
    pub fn restore(&mut self, data: &[u8]) -> Result<(), String> {
        let mut saved: World = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        if saved.cells != self.cells || saved.blocked.len() != self.blocked.len() {
            return Err("the savegame is for another map".into());
        }
        saved.turret_muzzles = std::mem::take(&mut self.turret_muzzles);
        saved.game_rng = self.game_rng.take();
        saved.curves = self.curves.take();
        if let Some(r) = saved.game_rng.as_mut() {
            r.set_index(saved.rng_index);
        }
        for s in &mut saved.structures {
            if let Some(a) = s.aim.as_mut() {
                a.restore();
            }
        }
        for u in &mut saved.units {
            if let Some(a) = u.aim.as_mut() {
                a.restore();
            }
        }
        *self = saved;
        Ok(())
    }

    pub fn add_structure(&mut self, s: Structure) {
        for (cx, cy) in s.blocked_cells() {
            if let Some(b) = self.cell_index(cx, cy) {
                self.blocked[b] = true;
            }
        }
        if s.kind == ObjectKind::Headquarters && s.owner == Owner::Enemy {
            self.has_enemy_hq = true;
        }
        let owner = side(s.owner);
        self.structures.push(s);
        let i = self.structures.len() - 1;
        self.slot_alloc(owner, i);
    }

    fn slot_alloc(&mut self, owner: usize, i: usize) {
        let t = &mut self.slot_tables[owner];
        match t.iter().position(Option::is_none) {
            Some(p) => t[p] = Some(i),
            None => t.push(Some(i)),
        }
    }

    fn slot_free(&mut self, owner: usize, i: usize) {
        if let Some(e) = self.slot_tables[owner].iter_mut().find(|e| **e == Some(i)) {
            *e = None;
        }
    }

    /// Walk parameters of a kind in the running mission (the pods of missions 3 and 8 are slow, `anim::walk_params_for_mode`).
    fn walk_params_of(&self, kind: i32) -> Option<opensow_formats::anim::WalkParams> {
        opensow_formats::anim::walk_params_for_mode(kind, self.mission_no.unwrap_or(u32::MAX))
    }

    /// Sets the mission number and gives units that already exist the mission's walk parameters.
    pub fn set_mission(&mut self, n: Option<u32>) {
        self.mission_no = n;
        for i in 0..self.units.len() {
            let kind = self.units[i].kind;
            if kind == 46 {
                // spawned before the mission was known: rebuild the chassis with the mission's parameters
                self.units[i].chassis = self.walk_params_of(kind).map(|p| ChassisAnim::new(p, p.steps, 0));
            }
        }
    }

    pub fn add_unit(&mut self, kind: i32, player: i32, x: i32, y: i32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let params = self.walk_params_of(kind);
        let chassis = params.map(|p| ChassisAnim::new(p, p.steps, 0));
        let aim = TopAim::new(kind, 0);
        let hover_phase = self.random(20);
        let max_hp = UnitKind::max_hp(kind).unwrap_or(300);
        self.units.push(Unit { id, kind, player, x: x as f32, y: y as f32, hp: max_hp, max_hp, chassis, aim, path: Vec::new(), target: None, selected: false, moved: false, line: None, step: None, dest: None, cool: 0, straight: None, queued: None, hover_phase, action: None, beam: BeamTimer::default(), beam_shots: 0, chase: None, idle: 0, look: 0 });
        id
    }

    fn cell_index(&self, cx: i32, cy: i32) -> Option<usize> {
        (cx >= 0 && cy >= 0 && cx < self.cells.0 && cy < self.cells.1).then(|| (cy * self.cells.0 + cx) as usize)
    }

    /// The map's "no marks" layer: scorch decals, flying pieces and smoke puffs do not land here.
    pub fn is_no_marks(&self, cx: i32, cy: i32) -> bool {
        self.cell_index(cx, cy).map_or(false, |i| self.no_marks.get(i).copied().unwrap_or(false))
    }

    pub fn is_blocked(&self, cx: i32, cy: i32) -> bool {
        self.cell_index(cx, cy).map_or(true, |i| self.blocked[i])
    }

    /// Terrain of the air layer (`0x60ac38`): what an Advancer cannot enter.
    /// The map's no-build layer (shore and rock edges): buildings and turrets may not stand here.
    pub fn is_no_build(&self, cx: i32, cy: i32) -> bool {
        self.cell_index(cx, cy).map_or(false, |i| self.no_build.get(i).copied().unwrap_or(false))
    }

    pub fn is_air_blocked(&self, cx: i32, cy: i32) -> bool {
        self.cell_index(cx, cy).map_or(true, |i| self.air_blocked[i])
    }

    /// Installs the exe's random table (`Data/generic.file`); without it a xorshift generator is used.
    pub fn set_game_rng(&mut self, mut rng: GameRng) {
        // a new game builds the debris curves from the table at index 0, which leaves it at 600 (`curves.rs`)
        self.curves = Some(CurveTable::generate(&mut rng));
        self.game_rng = Some(rng);
    }

    /// One tick of the screen shake (`0x458930` for the local player); returns the camera step in the exe's units (the
    /// camera moves by twice this, `0x457f80`). `cam` is the view's top-left and `screen` its size in map pixels.
    /// Switched off (options QUAKE) it forgets the shake and consumes no random numbers.
    pub fn quake_step(&mut self, enabled: bool, cam: (i32, i32), screen: (i32, i32)) -> Option<(i32, i32)> {
        if !enabled {
            self.quake = Quake::default();
            return None;
        }
        let ticks = self.quake.ticks;
        if ticks <= 0 && self.quake.stack.is_empty() {
            return None;
        }
        self.quake.ticks = ticks - 1;
        if ticks > 0 {
            let (ex, ey) = self.quake.at;
            let (dx, dy) = (ex - (cam.0 + screen.0 / 2), ey - (cam.1 + screen.1 / 2));
            let dist = ((dx as i64 * dx as i64 + dy as i64 * dy as i64) as f64).sqrt() as i32;
            let m = ((9 - dist / 256).max(0)) / 2;
            if m == 0 {
                // deliberate deviation (user-requested): a building lost far from the view (distance >= 1792 px, where the exe's
                // magnitude term reaches 0) does not shake the screen; the exe would still move it by +-2 px
                return None;
            }
            let mut axis = |w: &mut World| {
                let positive = w.random(2) == 1;
                let r = w.random(m as u32) as i32;
                if positive { r + 2 } else { -2 - r }
            };
            let sx = axis(self);
            let sy = axis(self);
            self.quake.stack.push((sx, sy));
            Some((sx, sy))
        } else {
            // after the shake the steps are undone one per tick, in random order
            let k = self.random(self.quake.stack.len() as u32) as usize;
            let (sx, sy) = self.quake.stack.remove(k);
            Some((-sx, -sy))
        }
    }

    /// One tick of the ambient clouds for the options' CLOUDS `detail` (`0x453060` sizes the list, `0x42e290` moves it).
    pub fn sky_step(&mut self, detail: u32) {
        let (w, h) = self.cells;
        let target = opensow_formats::clouds::count_for_detail(detail);
        let mut rng = self.fx_rng;
        let mut rand = |n: u32| {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng % n.max(1)
        };
        if self.sky.clouds.len() != target {
            self.sky.resize(detail, w, h, &mut rand);
        }
        self.sky.step(&[(self.tick % 60) as u32], w, h, &mut rand);
        self.fx_rng = rng;
    }

    fn fx_random(&mut self, n: u32) -> u32 {
        self.fx_rng ^= self.fx_rng << 13;
        self.fx_rng ^= self.fx_rng >> 17;
        self.fx_rng ^= self.fx_rng << 5;
        self.fx_rng % n.max(1)
    }

    /// Scorch decal and smoke puffs of a landing bullet (the generic tail of the impact function, docs/effects.md). The
    /// terrain test (`ground` layer) gates everything, the "no marks" layer gates the decal and the single puff; the three
    /// puffs of a type 15/16 hit take their offsets from the lockstep generator (`0x4105c9..0x410689`), the others from the visual one.
    fn impact_ground_effects(&mut self, x: i32, y: i32, bt: u8) {
        let cell = (x.div_euclid(CELL), y.div_euclid(CELL));
        let inside = cell.0 > 0 && cell.0 < self.cells.0 && cell.1 > 0 && cell.1 < self.cells.1;
        let free = inside && !self.is_blocked(cell.0, cell.1);
        let marks_free = free && !self.is_no_marks(cell.0, cell.1);
        if matches!(bt, 17 | 18) {
            let (_, single) = impact_smoke(bt, 0, marks_free);
            for _ in 0..single {
                let (dy, dx) = (self.fx_random(20) as i32 - 10, self.fx_random(20) as i32 - 10);
                self.puff(x + dx, y + dy);
            }
            return;
        }
        if !impact_leaves_decal(bt) || !inside {
            return;
        }
        if marks_free && !self.decals.iter().any(|d| d.cell == cell) {
            let frame = (self.fx_random(9) / 2) as u8;
            self.decals.push(Decal { x, y, cell, frame });
        }
        let roll = self.random(3);
        if free {
            let (lockstep, single) = impact_smoke(bt, roll, marks_free);
            for _ in 0..lockstep {
                let (dy, dx) = (self.random(20) as i32 - 10, self.random(20) as i32 - 10);
                self.puff(x + dx, y + dy);
            }
            for _ in 0..single {
                let (dy, dx) = (self.fx_random(20) as i32 - 10, self.fx_random(20) as i32 - 10);
                self.puff(x + dx, y + dy);
            }
        }
    }

    fn puff(&mut self, x: i32, y: i32) {
        let (variant, extra) = (self.fx_random(2) as u8, self.fx_random(13) as u8);
        self.effects.push(Effect { x, y, spec: smoke_puff(variant, extra), age: 0 });
    }

    fn random(&mut self, n: u32) -> u32 {
        draw(&mut self.game_rng, &mut self.rng, n)
    }

    pub fn unit(&self, id: u32) -> Option<&Unit> {
        self.units.iter().find(|u| u.id == id)
    }

    pub fn player_units(&self) -> impl Iterator<Item = &Unit> {
        self.units.iter().filter(|u| u.player == 0)
    }

    // ---- orders -------------------------------------------------------------------------------------------------

    /// Orders the selected units to walk to `(x, y)` (map pixels); a group spreads over neighbouring free cells.
    pub fn order_move_selected(&mut self, x: i32, y: i32) {
        // the pods of missions 3 and 8 take no go-to orders either (`0x418a0c` / `0x419079`): the script drives them
        let scripted = matches!(self.mission_no, Some(3 | 8));
        if self.units.iter().any(|u| u.selected && u.player == 0 && u.kind == 40) {
            if let Some(t) = &mut self.tutorial {
                t.advancer_moved = true; // step 5: the selected Advancer got a go-to order
            }
        }
        for u in self.units.iter_mut().filter(|u| u.selected && u.player == 0 && !(scripted && u.kind == 46)) {
            u.chase = None;
            u.queued = None; // a go-to replaces the whole order list (`0x4706a0`)
        }
        let goal = (x.div_euclid(CELL), y.div_euclid(CELL));
        let mut slots = std::iter::once((0i16, 0i16)).chain(self.rings.search(0, 8).iter().copied().skip(0)).collect::<Vec<_>>();
        slots.dedup();
        let mut next = 0usize;
        let ids: Vec<u32> = self.units.iter().filter(|u| u.selected && u.player == 0 && !(scripted && u.kind == 46)).map(|u| u.id).collect();
        for id in ids {
            let mut cell = goal;
            while next < slots.len() {
                let c = (goal.0 + slots[next].0 as i32, goal.1 + slots[next].1 as i32);
                next += 1;
                if !self.is_blocked(c.0, c.1) {
                    cell = c;
                    break;
                }
            }
            self.order_move(id, cell, (cell == goal).then_some((x, y)));
        }
    }

    /// Orders the selected units to attack `target` (`CNet_UnitAttack`): they chase it and fight when it is in range.
    /// Starts the target flash of structure `t`.
    pub fn blink(&mut self, t: usize) {
        self.blinks.insert(t, Blink::new());
    }

    fn step_blinks(&mut self) {
        self.blinks.retain(|_, b| {
            b.timer -= 1;
            if b.timer <= 0 {
                b.timer = 5;
                b.on = !b.on;
                b.left -= 1;
                if b.left <= 0 {
                    return false;
                }
            }
            true
        });
    }

    pub fn order_attack_selected(&mut self, target: Target) {
        // the pods of missions 3 and 8 take no attack orders (0x46a10b: they are driven by the script)
        let scripted = matches!(self.mission_no, Some(3 | 8));
        for u in self.units.iter_mut().filter(|u| u.selected && u.player == 0 && u.kind != 40 && !(scripted && u.kind == 46)) {
            u.chase = Some(target);
            u.queued = None;
        }
    }

    /// The hostile unit or structure under map pixel `(x, y)` for the player, if any.
    pub fn enemy_at(&self, x: i32, y: i32) -> Option<Target> {
        let near = self.units.iter().filter(|u| u.hp > 0 && u.player != 0).find(|u| ((u.x - x as f32).powi(2) + (u.y - y as f32).powi(2)).sqrt() < 20.0).map(|u| Target::Unit(u.id));
        near.or_else(|| self.structure_at(x, y).filter(|&k| self.structures[k].owner != Owner::Player).map(Target::Structure))
    }

    /// Walks unit `id` to `cell`, ending at the pixel `exact` when given and reachable.
    pub fn order_move(&mut self, id: u32, cell: (i32, i32), exact: Option<(i32, i32)>) {
        if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
            u.straight = None;
        }
        let Some(u) = self.unit(id) else { return };
        // a unit in the middle of a segment finishes its pixel line first (a type-1 order does not touch it, `0x470480`); the
        // route is searched from the step cell it is walking to, which is where the exe's next search starts
        let (start, air, flying_to) = (u.heading_cell().unwrap_or(u.cell()), u.kind == 40, u.step);
        // a type-1 order (`0x470480`): the wave search of the unit's own layer (`0x4753a0` mode 0, or `0x474b30` for the Advancer)
        let mut path = match self.find_path_on(air, start, cell) {
            Some(p) => p,
            None => return,
        };
        if let (Some((ex, ey)), Some(last)) = (exact, path.last_mut()) {
            *last = (ex as f32, ey as f32);
        }
        if let Some(from) = flying_to {
            // `path[0]` stands for the segment in flight and is dropped when the unit gets there
            path.insert(0, from);
        }
        let dest = exact.unwrap_or_else(|| {
            let c = centre(cell);
            (c.0 as i32, c.1 as i32)
        });
        let u = self.units.iter_mut().find(|u| u.id == id).unwrap();
        u.path = path;
        u.dest = Some(dest);
        // the search just done counts as the first of the exe's every-7th-round searches (`[+0x80]` is cleared by the order)
        u.cool = 7;
        if flying_to.is_none() {
            // no pixel line yet: the next step plans it (0x473190), avoiding units
            u.line = None;
        }
    }

    /// A type-2 order (`0x470480`) for the Advancer: it flies the straight line to `cell` without path finding (the beam
    /// approach of `0x408890`).
    pub fn order_fly_straight(&mut self, id: u32, cell: (i32, i32)) {
        if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
            u.straight = None;
            u.line = None;
            // the pixel route is dropped for a straight line to the cell, which is also the step cell (`0x470480` type 2)
            u.step = Some(centre(cell));
            u.dest = None;
            u.path = vec![centre(cell)];
        }
    }

    pub fn stop(&mut self, id: u32) {
        if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
            u.halt();
        }
    }

    /// The exe's wave search (`opensow_formats::wave`, exact vs `0x4753a0`) over the cell grid. Returns the cell centres from
    /// the cell after `start` to `goal`. A blocked goal is replaced by the nearest free cell (the exe reaches into the
    /// blocked cell and relies on the walking code to stop short).
    pub fn find_path(&self, start: (i32, i32), goal: (i32, i32)) -> Option<Vec<(f32, f32)>> {
        self.find_path_on(false, start, goal)
    }

    /// `find_path` on the ground layer (`0x4753a0`, grid `0x5fac38`) or the air layer (`0x474b30`, grid `0x60ac38`, the same
    /// search over the border terrain).
    pub fn find_path_on(&self, air: bool, start: (i32, i32), goal: (i32, i32)) -> Option<Vec<(f32, f32)>> {
        let blocked = |x: i32, y: i32| if air { self.is_air_blocked(x, y) } else { self.is_blocked(x, y) };
        let goal = if blocked(goal.0, goal.1) { self.rings.search(0, 12).iter().map(|&(dx, dy)| (goal.0 + dx as i32, goal.1 + dy as i32)).find(|&(x, y)| !blocked(x, y))? } else { goal };
        if start == goal {
            return Some(vec![centre(goal)]);
        }
        let mut wave = Wave::new(blocked);
        // the exe searches from the destination to the unit (`[unit+0x5c]` is the destination, the target argument the
        // unit's own position), so the route comes back unit first
        let mut route = wave.find(goal, start, 0)?;
        route.remove(0);
        Some(route.into_iter().map(centre).collect())
    }

    /// Weapon range in cells a chasing unit tries to get within (`[chassis+0x154]`, set at `0x46c0ac`): the middle of the
    /// weapon's minimum and maximum range, 5 for anti-air weapons, 1 without a weapon.
    fn chase_range(&self, i: usize) -> i32 {
        let u = &self.units[i];
        let Some(p) = u.aim.as_ref().map(|a| &a.params) else { return 1 };
        if matches!(u.kind, 1 | 2 | 3 | 42) {
            return 5;
        }
        (((p.min_range + p.max_range) / 2) as i32).max(1)
    }

    /// `0x46eb40`: a unit with a chase target walks until it is within its range of the target (cell distance below
    /// `range`); farther away it takes the cell `range` steps before the target on the way there (`0x477630`).
    fn chase_step(&mut self, i: usize, t: Target, tcell: (i32, i32)) {
        let range = self.chase_range(i);
        let at = self.units[i].cell();
        let dist = (((at.0 - tcell.0).pow(2) + (at.1 - tcell.1).pow(2)) as f64).sqrt() as i32;
        if dist <= range - 1 {
            return;
        }
        let (handle, tcell) = match t {
            Target::Unit(_) => (0, tcell),
            Target::Structure(k) => (0x2000000 + k as u32, self.structures[k].center_cell()),
        };
        // a target of handle type 4 (an Advancer) is chased over the air layer (`0x478200`), every other target over the ground's
        let air_target = matches!(t, Target::Unit(uid) if self.unit(uid).is_some_and(|u| u.kind == 40));
        let dims = self.cells;
        let id = self.units[i].id;
        let dest = self.occupancy().and_then(|o| {
            if air_target {
                approach_air(&mut o.air_stat, &mut o.stat_base, dims, at, tcell, handle, range)
            } else {
                approach(&mut o.stat, &mut o.stat_base, dims, at, tcell, handle, range)
            }
        });
        match dest {
            Some(cell) if cell != at => self.order_move(id, cell, None),
            Some(_) => {}
            None => self.units[i].halt(),
        }
    }

    /// Unit-avoidance grids for this tick (`0x61ac38` / `0x62ac38`, static `0x5eac38`): built on first use.
    fn occupancy(&mut self) -> Option<&mut Occupancy> {
        if self.occ.is_none() && self.cells.0 <= 128 && self.cells.1 <= 128 {
            let mut stat = vec![0u32; 16384];
            for x in 0..128 {
                for y in 0..128 {
                    if self.is_blocked(x, y) {
                        stat[(x * 128 + y) as usize] = 0xFFFFFD;
                    }
                }
            }
            for (k, s) in self.structures.iter().enumerate().filter(|(_, s)| s.alive()) {
                for (x, y) in s.blocked_cells() {
                    if (0..128).contains(&x) && (0..128).contains(&y) {
                        stat[(x * 128 + y) as usize] = 0x2000000 + k as u32;
                    }
                }
            }
            let mut air_stat = vec![0u32; 16384];
            for x in 0..128 {
                for y in 0..128 {
                    if self.is_air_blocked(x, y) {
                        air_stat[(x * 128 + y) as usize] = 0xFFFFFD;
                    }
                }
            }
            let mut ground = Wave::new(|_, _| false);
            let mut air = Wave::new(|_, _| false);
            ground.cells = stat.clone();
            air.cells = air_stat.clone();
            for u in self.units.iter().filter(|u| u.hp > 0) {
                let (x, y) = u.cell();
                if (0..128).contains(&x) && (0..128).contains(&y) {
                    let grid = if u.kind == 40 { &mut air } else { &mut ground };
                    grid.cells[(x * 128 + y) as usize] = 0x3000000 + u.id;
                }
            }
            // a corpse re-stamps its cell with the terrain marker every step (0x46dfeb / 0x46e004)
            for c in &self.corpses {
                let (x, y) = (c.x.div_euclid(CELL), c.y.div_euclid(CELL));
                if (0..128).contains(&x) && (0..128).contains(&y) {
                    let grid = if c.air { &mut air } else { &mut ground };
                    grid.cells[(x * 128 + y) as usize] = 0xFFFFFD;
                }
            }
            // a walking unit also holds the cell it is heading to (0x473190 stamps `[unit+0x6c/0x70]`), so two units never
            // take the same next cell
            for u in self.units.iter().filter(|u| u.hp > 0) {
                if let Some((x, y)) = u.heading_cell().filter(|&c| c != u.cell()) {
                    if (0..128).contains(&x) && (0..128).contains(&y) {
                        let grid = if u.kind == 40 { &mut air } else { &mut ground };
                        let at = (x * 128 + y) as usize;
                        if grid.cells[at] == 0 {
                            grid.cells[at] = 0x3000000 + u.id;
                        }
                    }
                }
            }
            self.occ = Some(Occupancy { ground, air, stat, air_stat, stat_base: 1 });
        }
        self.occ.as_mut()
    }

    /// A unit's pixel step took it into another cell: the exe clears the old cell and stamps the new one in the same pass
    /// (`0x46f759` / `0x46f7ff` write the static grid at every pixel step), so units planning later in the same tick see the move.
    /// The unit grid holds the step cell, which is already stamped, so only the cell the unit left changes here.
    fn occupancy_moved(&mut self, i: usize, old: (i32, i32)) {
        let (own, air, new) = (0x3000000 + self.units[i].id, self.units[i].kind == 40, self.units[i].cell());
        let heading = self.units[i].heading_cell();
        let Some(occ) = self.occ.as_mut() else { return };
        let grid = if air { &mut occ.air } else { &mut occ.ground };
        let mut set = |cell: (i32, i32), from: u32, to: u32| {
            if (0..128).contains(&cell.0) && (0..128).contains(&cell.1) {
                let at = (cell.0 * 128 + cell.1) as usize;
                if grid.cells[at] == from {
                    grid.cells[at] = to;
                }
            }
        };
        if heading != Some(old) {
            set(old, own, 0);
        }
        set(new, 0, own);
    }

    /// The planning half of the exe's walking step (`0x46f550` when the pixel line is empty): arrival, the global route search
    /// every 7th round, then the local step (`0x473190`). Sets `step`; returns `true` when the unit has to wait.
    fn plan_step(&mut self, i: usize) -> bool {
        let u = &mut self.units[i];
        if u.step.is_some() {
            return false;
        }
        if u.straight.is_some() {
            // a type-2 order: a straight line without planning
            u.step = u.path.first().copied();
            return false;
        }
        let at = u.cell();
        let air = u.kind == 40;
        let Some(dest) = u.dest else {
            // no destination (an Advancer's straight flight, a stopped unit): follow what is left of the route
            if u.path.is_empty() {
                return false;
            }
            return self.avoid(i);
        };
        let dest_cell = (dest.0.div_euclid(CELL), dest.1.div_euclid(CELL));
        if u.path.is_empty() && at == dest_cell {
            // on the destination: the order is done (`0x46f5b1`)
            u.dest = None;
            return false;
        }
        // not there yet: the global route is searched when the cooldown `[+0x80]` runs out (every 7th round) or when it was used
        // up by detours (`[+0xa0] == 0` with the unit away from its destination)
        u.cool -= 1;
        if u.cool < 0 || u.path.is_empty() {
            u.cool = 6;
            match self.find_path_on(air, at, dest_cell) {
                Some(mut path) => {
                    if let Some(last) = path.last_mut().filter(|_| (dest.0 as f32, dest.1 as f32) != centre(dest_cell)) {
                        *last = (dest.0 as f32, dest.1 as f32);
                    }
                    self.units[i].path = path;
                }
                None => {
                    // a failed search clears the orders
                    self.units[i].halt();
                    return false;
                }
            }
        }
        self.avoid(i)
    }

    /// Re-plans the first stretch of unit `i`'s route around other units and buildings (`0x473190` -> `0x476870`): the cells
    /// up to six ahead are replaced by the route of a local wave over the occupancy grid.
    /// Returns `true` when no way around the units in front was found: the unit waits where it is.
    fn avoid(&mut self, i: usize) -> bool {
        let (at, own, air, cells) = {
            let u = &self.units[i];
            let cells: Vec<(i32, i32)> = u.path.iter().map(|&(x, y)| ((x.floor() as i32).div_euclid(CELL), (y.floor() as i32).div_euclid(CELL))).collect();
            (u.cell(), 0x3000000 + u.id, u.kind == 40, cells)
        };
        let mut route = vec![at];
        route.extend(&cells);
        let k = cells.len().min(6);
        let heading = self.units[i].heading_cell().filter(|&c| c != at);
        let Some(occ) = self.occupancy() else { return false };
        let Occupancy { ground, air: agrid, stat, air_stat, stat_base } = occ;
        // 0x473190 first clears the unit's old step cell
        if let Some((x, y)) = heading.filter(|&(x, y)| (0..128).contains(&x) && (0..128).contains(&y)) {
            let grid = if air { &mut *agrid } else { &mut *ground };
            if grid.cells[(x * 128 + y) as usize] == own {
                grid.cells[(x * 128 + y) as usize] = 0;
            }
        }
        let r = if air { agrid.local_step(air_stat, *stat_base, own, &route, at) } else { ground.local_step(stat, *stat_base, own, &route, at) };
        let u = &mut self.units[i];
        if r.stop {
            u.halt();
        } else if let Some(next) = r.next {
            // the step cell is the local search's first cell; the global route is not touched (the exe pops its head cell when the
            // segment is done and re-searches it every 7th round), so detours around units never rewrite the route
            u.step = Some(match u.path.first() {
                Some(&p) if (((p.0.floor() as i32).div_euclid(CELL), (p.1.floor() as i32).div_euclid(CELL))) == next => p,
                _ => centre(next),
            });
        } else {
            // no local route (0x476870 failed): the exe then looks for a free cell near the destination (0x4774c0) and keeps
            // its previous step cell; here the unit waits and tries again on the next tick
            let waiting = route.len() > 1 && route[k] != at;
            if waiting {
                self.retarget_to_free_cell(i);
            } else {
                // nothing to avoid (the route's last cell is the unit's own): walk straight to the first cell
                u.step = u.path.first().copied();
            }
            return waiting;
        }
        // the new step cell is stamped at once, so units planning later this tick see it taken
        if let Some((x, y)) = self.units[i].heading_cell().filter(|&c| c != at) {
            let air = self.units[i].kind == 40;
            let handle = 0x3000000 + self.units[i].id;
            if let Some(occ) = self.occupancy().filter(|_| (0..128).contains(&x) && (0..128).contains(&y)) {
                let grid = if air { &mut occ.air } else { &mut occ.ground };
                if grid.cells[(x * 128 + y) as usize] == 0 {
                    grid.cells[(x * 128 + y) as usize] = handle;
                }
            }
        }
        false
    }

    /// `0x4774c0`, the fallback of the planners when the local search fails: the destination moves to the free cell nearest to
    /// it that the unit can reach (ground: a flood of the unit grid from the unit's cell marks the reachable region and the
    /// range-ring list around the destination picks the first cell of it; Advancers take the first free ring cell). The exe
    /// keeps the old step cell and only searches a new global route a few steps later (cooldown `[+0x80]`); here the route is
    /// searched at once.
    fn retarget_to_free_cell(&mut self, i: usize) {
        let (air, own, at, at_px) = {
            let u = &self.units[i];
            (u.kind == 40, 0x3000000 + u.id, u.cell(), (u.x.round() as i32, u.y.round() as i32))
        };
        let Some(&(lx, ly)) = self.units[i].path.last() else { return };
        let dest = (lx.round() as i32, ly.round() as i32);
        let map = self.cells;
        let rings = std::mem::replace(&mut self.rings, Rings { offsets: Vec::new(), ends: Vec::new() });
        let found = self.occupancy().and_then(|occ| {
            let grid = if air { &mut occ.air } else { &mut occ.ground };
            free_cell_near(grid, &rings, own, dest, at_px, air, map).destination
        });
        self.rings = rings;
        let Some((px, py)) = found else { return };
        let goal = (px.div_euclid(CELL), py.div_euclid(CELL));
        if goal == (dest.0.div_euclid(CELL), dest.1.div_euclid(CELL)) {
            return;
        }
        if let Some(path) = self.find_path_on(air, at, goal) {
            self.units[i].path = path;
            self.units[i].dest = Some((px, py));
            self.units[i].line = None;
        }
    }

    /// Adds a unit on the free cell nearest to the pixel position (used for the starter squad).
    pub fn add_unit_near(&mut self, kind: i32, player: i32, x: i32, y: i32) -> Option<u32> {
        let cell = self.nearest_free((x.div_euclid(CELL), y.div_euclid(CELL)))?;
        let taken = |w: &World, c: (i32, i32)| w.units.iter().any(|u| u.cell() == c);
        let cell = std::iter::once(cell)
            .chain(self.rings.search(0, 12).iter().map(|&(dx, dy)| (x.div_euclid(CELL) + dx as i32, y.div_euclid(CELL) + dy as i32)))
            .find(|&c| !self.is_blocked(c.0, c.1) && !taken(self, c))?;
        Some(self.add_unit(kind, player, cell.0 * CELL + CELL / 2, cell.1 * CELL + CELL / 2))
    }

    fn nearest_free(&self, c: (i32, i32)) -> Option<(i32, i32)> {
        self.rings.search(0, 12).iter().map(|&(dx, dy)| (c.0 + dx as i32, c.1 + dy as i32)).find(|&(x, y)| !self.is_blocked(x, y))
    }

    // ---- selection ----------------------------------------------------------------------------------------------

    /// Advancers (40) and nuclear vehicles (46) are never part of a group: they are selected alone, by clicking them.
    pub fn is_single_select(kind: i32) -> bool {
        matches!(kind, 40 | 46)
    }

    pub fn clear_selection(&mut self) {
        self.units.iter_mut().for_each(|u| u.selected = false);
    }

    /// Selects the units inside the pixel rectangle; returns how many are selected afterwards. Like the exe (`0x417700..0x41776a` runs
    /// `0x46c820` over the unit lists of all three players, with no owner test) the box also takes enemy and neutral units: they
    /// get the local selection flag and so their health bar (`0x46a420`), but every order loop walks the player's own list only.
    pub fn select_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, additive: bool) -> usize {
        if !additive {
            self.clear_selection();
        }
        let (lx, hx, ly, hy) = (x0.min(x1) as f32, x0.max(x1) as f32, y0.min(y1) as f32, y0.max(y1) as f32);
        // a box never takes Advancers or nuclear vehicles, and drops one that was selected alone
        if self.units.iter().any(|u| u.selected && Self::is_single_select(u.kind)) {
            self.clear_selection();
        }
        for u in self.units.iter_mut().filter(|u| u.hp > 0 && !Self::is_single_select(u.kind)) {
            if u.x >= lx && u.x <= hx && u.sprite_y() >= ly && u.sprite_y() <= hy {
                u.selected = true;
            }
        }
        self.units.iter().filter(|u| u.selected).count()
    }

    /// Selects the player's unit nearest to `(x, y)` within `radius` pixels.
    pub fn select_at(&mut self, x: i32, y: i32, radius: f32, additive: bool) -> usize {
        if !additive {
            self.clear_selection();
        }
        let best = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.player == 0)
            .map(|(i, u)| (i, (u.x - x as f32).hypot(u.sprite_y() - y as f32)))
            .filter(|&(_, d)| d <= radius)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = best {
            // a single-select unit replaces the selection (even with shift); nothing joins one that is selected
            if Self::is_single_select(self.units[i].kind) || self.units.iter().any(|u| u.selected && Self::is_single_select(u.kind)) {
                self.clear_selection();
            }
            self.units[i].selected = true;
        }
        self.units.iter().filter(|u| u.selected).count()
    }

    pub fn select_all(&mut self) {
        self.units.iter_mut().for_each(|u| u.selected = u.player == 0 && !Self::is_single_select(u.kind));
    }

    // ---- simulation ---------------------------------------------------------------------------------------------

    /// Advances the world by one game tick.
    pub fn step(&mut self) {
        if self.outcome.is_some() {
            return;
        }
        self.tick += 1;
        self.strike_lock = (self.strike_lock - 1).max(0);
        self.occ = None;
        if self.enemy_at_start.is_none() {
            self.enemy_at_start = Some(self.units.iter().any(|u| u.player != 0) || self.structures.iter().any(|s| s.owner == Owner::Enemy));
        }
        let index = self.index();
        let frozen = self.cheats.freeze_enemy;
        for i in 0..self.units.len() {
            if frozen && self.units[i].player != 0 {
                continue;
            }
            self.step_unit(i, &index);
        }
        for i in 0..self.structures.len() {
            if frozen && self.structures[i].owner == Owner::Enemy {
                continue;
            }
            self.step_structure(i, &index);
        }
        for i in 0..self.structures.len() {
            if frozen && self.structures[i].owner == Owner::Enemy {
                continue;
            }
            self.step_production(i);
        }
        self.step_destruction();
        self.step_convoy();
        self.step_hq();
        self.step_economy();
        self.step_mission();
        self.step_tutorial();
        if self.ai && !frozen {
            self.step_ai(1);
        }
        self.step_aircraft();
        self.step_actions();
        self.step_beams();
        self.step_blinks();
        self.step_satellites();
        self.step_bullets();
        if self.tick % 30 == 0 && !self.decals.is_empty() {
            let blocked: Vec<(i32, i32)> = self.decals.iter().map(|d| d.cell).filter(|c| self.is_blocked(c.0, c.1)).collect();
            self.decals.retain(|d| !blocked.contains(&d.cell));
        }
        self.effects.iter_mut().for_each(|e| e.age += 1);
        self.effects.retain(|e| e.spec.frame_at(e.age).is_some());
        if self.cheats.invulnerable {
            self.units.iter_mut().filter(|u| u.player == 0).for_each(|u| u.hp = u.max_hp);
        }
        if self.cheats.rich {
            self.credits[0] = CREDITS_CAP;
            self.research[0] = RESEARCH_CAP;
        }
        // `0x42e412`: the cell of the unit * 32
        let dead_adv: Vec<(usize, i32, i32)> = self.units.iter().filter(|u| u.hp <= 0 && u.kind == 40).map(|u| (if u.player == 0 { 0 } else { 1 }, (u.x as i32).div_euclid(CELL) * CELL, (u.y as i32).div_euclid(CELL) * CELL)).collect();
        for (side, x, y) in dead_adv {
            self.say_at(side, msg::ADVANCER_DESTROYED, x, y);
        }
        for u in self.units.iter().filter(|u| u.hp <= 0 && u.kind == 46) {
            let (x, y) = u.px();
            self.nukes.push(Nuke { x, y, owner: u.player.min(1), ex: Default::default() });
        }
        // a pod's death handler (0x46a4f0) only creates the explosion event: no clouds, wreck, debris, puffs or death sound
        for u in self.units.iter().filter(|u| u.hp <= 0) {
            self.stats[u.player.clamp(0, 2) as usize][3] += 1; // 0x421fc1: a dead unit is counted as lost before it is removed
        }
        let dying: Vec<(i32, i32, i32, i32)> = self.units.iter().filter(|u| u.hp <= 0 && u.kind != 46).map(|u| (u.px().0, u.px().1, u.kind, u.player)).collect();
        for (x, y, kind, player) in dying {
            // 0x4077e0(-1): a random unit explosion (23, 24, 26 or 27)
            self.sound([23, 24, 26, 27][(x + y + self.tick as i32).rem_euclid(4) as usize], x, y, false);
            // the destruction code 0x46a600..0x46b0f0 (docs/effects.md): four clouds at (x - 15 + rand(30), y - 15 + rand(30)) with
            // the first draw for y, the wreck after the second one (not for the Advancer), the flying pieces and three puffs
            for n in 0..UNIT_DEATH_CLOUDS {
                let (dy, dx) = (self.random(30) as i32 - 15, self.random(30) as i32 - 15);
                self.effects.push(Effect { x: x + dx, y: y + dy, spec: UNIT_CLOUD, age: 0 });
                if n == 1 && kind != 40 {
                    self.effects.push(Effect { x, y: y + 7, spec: UNIT_WRECK, age: 0 });
                }
            }
            if let Some(d) = debris(kind) {
                // the four lockstep draws are made for every kind with a class; only tops with an index <= 14 leave a piece
                let off = |w: &mut World| (2 * w.random(2) as i32 - 1) * (w.random(d.spread as u32) as i32 + d.spread);
                let (sx, sy) = (off(self), off(self));
                let bt = if player == 0 { 15 } else { 16 };
                if let Some(variant) = debris_variant(kind) {
                    // delay 0, strength 40: the landing piece deals area damage like a shell (0x410881)
                    if let Some(path) = build_path(bt, x, y, x + sx, y + sy, 0) {
                        if !path.points.is_empty() {
                            let mut b = Bullet::new(bt, DEBRIS_STRENGTH, player.min(1), path);
                            b.variant = variant;
                            self.bullets.push(b);
                        }
                    }
                }
            }
            let variant = debris(kind).map_or(0, |d| d.wreck_param);
            self.corpses.push(Corpse { x, y, left: CORPSE_LIFE, variant, air: kind == 40, blasted: false, player, kind });
            if kind != 40 {
                self.wrecks.push(Wreck { x, y, left: WRECK_SMOKE_COLUMNS, timer: WRECK_SMOKE_FIRST });
            }
            if kind != 40 {
                for _ in 0..UNIT_DEATH_PUFFS {
                    let (dx, dy) = (self.random(20) as i32 - 10, self.random(20) as i32 - 10);
                    self.puff(x + dx, y + dy);
                }
            }
        }
        self.units.retain(|u| u.hp > 0);
        self.step_nukes();
        self.step_wrecks();
        self.step_corpses();
        if self.tick % 20 == 0 {
            self.check_outcome();
        }
    }

    /// Factory production: each active slot gains `steps_at(row, tick % 60)` per tick and spawns its unit at a free cell
    /// next to the building once full (0x40e640 + 0x408740). Neutral buildings do not produce.
    fn step_production(&mut self, i: usize) {
        let s = &self.structures[i];
        if !s.alive() || s.owner == Owner::Neutral || s.slots.is_empty() || !(s.kind.produces_units() || s.kind == ObjectKind::AirBase) || s.heal > 0 {
            return;
        }
        let (player, cell, active) = (if s.owner == Owner::Player { 0 } else { 1 }, s.cell, s.active.min(s.slots.len()));
        let gain = steps_at(rate_row(self.tutorial_step, self.wind[player as usize]), (self.tick % 60) as u32);
        for j in 0..active {
            let slot = &mut self.structures[i].slots[j];
            slot.progress = (slot.progress + gain).min(slot.target);
            if slot.progress < slot.target {
                continue;
            }
            let kind = slot.kind;
            if let Some(g) = AirForce::from_code(kind).and_then(|f| HEADER_ORDER.iter().position(|&h| h == f)) {
                // air bases add the force to the stock (0x453e80) and start over
                self.air_stock[player as usize][g] += 1;
                self.structures[i].slots[j].progress = 0;
                if let Some(id) = msg::force_ready(kind) {
                    self.say(player as usize, id); // `0x453ef4` passes -1, -1
                }
                continue;
            }
            let held = self.units.iter().filter(|u| u.player == player && u.hp > 0).count() + self.corpses.iter().filter(|c| c.player == player).count();
            if held >= UNIT_SLOTS {
                continue;
            }
            // `0x408740`: the unit appears at the spawn cell (inside the footprint) and
            // leaves through the primary exit, or the opposite one when that is occupied; with both occupied it waits
            let kind_id = self.structures[i].kind_id;
            let e = factory_exit(kind_id).unwrap_or(FactoryExit { spawn: (1, 1), primary: (3, 3), alternative: (-1, -1) });
            let at = |o: (i32, i32)| (cell.0 + o.0, cell.1 + o.1);
            // the exits are tested in the static grid `0x5eac38` (terrain and buildings only): units never block them
            let occupied = |w: &World, c: (i32, i32)| w.is_blocked(c.0, c.1);
            let spawn = at(e.spawn);
            let exit = if !occupied(self, at(e.primary)) {
                Some((at(e.primary), (0, 2)))
            } else if !occupied(self, at(e.alternative)) {
                Some((at(e.alternative), (0, -2)))
            } else {
                None
            };
            match exit {
                Some((walk, beyond)) => {
                    let id = self.add_unit(kind, player, spawn.0 * CELL + CELL / 2, spawn.1 * CELL + CELL / 2);
                    self.stats[player.clamp(0, 2) as usize][2] += 1; // 0x4087ed / 0x408a11
                    self.order_straight_then(id, walk, (walk.0 + beyond.0, walk.1 + beyond.1));
                    // the third order (`0x4089a6`): on to the rally point when it lies inside the map (player factories only: the
                    // exe's AI never sets one)
                    let rally = self.structures[i].rally.filter(|&p| player == 0 && opensow_formats::rally::usable(p, self.cells));
                    if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
                        u.queued = rally;
                    }
                    let slot = &mut self.structures[i].slots[j];
                    slot.progress = 0;
                    slot.waiting = 0;
                }
                None => {
                    let slot = &mut self.structures[i].slots[j];
                    slot.waiting += 1;
                    if slot.waiting == NO_SPACE_TICKS {
                        let (x, y) = self.structures[i].center(); // `0x40e323`
                        self.say_at(player as usize, msg::NO_SPACE, x, y);
                    }
                }
            }
        }
    }

    /// The factory exit orders (`0x408740`): a type-2 order walks the unit in a straight line to the centre of `first` (it comes
    /// out of the building through the wall), the queued type-1 order then plans a route to `then`.
    fn order_straight_then(&mut self, id: u32, first: (i32, i32), then: (i32, i32)) {
        if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
            u.path = vec![centre(first)];
            u.line = None;
            u.step = None;
            u.dest = None;
            u.straight = Some(then);
        }
    }

    fn index(&self) -> HashMap<(i32, i32), Vec<Target>> {
        let mut m: HashMap<(i32, i32), Vec<Target>> = HashMap::new();
        for u in self.units.iter().filter(|u| u.hp > 0) {
            m.entry(u.cell()).or_default().push(Target::Unit(u.id));
        }
        for (i, s) in self.structures.iter().enumerate().filter(|(_, s)| s.alive()) {
            m.entry(s.center_cell()).or_default().push(Target::Structure(i));
        }
        m
    }

    fn hostile(&self, player: i32, t: Target) -> bool {
        match t {
            Target::Unit(id) => self.unit(id).map_or(false, |u| u.hp > 0 && (u.player == 0) != (player == 0)),
            Target::Structure(i) => {
                // the exe's target search (0x47b140) takes every live building whose owner differs from the shooter's player,
                // so neutral buildings (owner 2) are valid targets for both sides
                let o = self.structures[i].owner;
                self.structures[i].alive() && if player == 0 { o != Owner::Player } else { o != Owner::Enemy }
            }
        }
    }

    fn target_pos(&self, t: Target) -> Option<(i32, i32)> {
        match t {
            Target::Unit(id) => self.unit(id).map(|u| u.px()),
            Target::Structure(i) => Some(self.structures[i].center()),
        }
    }

    /// Target choice of the exe (`0x47b140`): cells are scanned ring by ring between the dead zone and the range; a unit wins
    /// over a turret, a turret over any other building. `order` is the unit's own order target (`[unit+0x7c]`, set by an attack
    /// order): the scan treats it as a hostile candidate (`0x47b232`: the handle equal to it counts as an enemy), and when it
    /// is among the objects in range the weapon takes it whatever else is there (`0x47b27c` / `0x47b2a2` set `bl`, `0x47b2b3..0x47b2c1`
    /// store it as the weapon target). Only an order target out of range leaves the priority rule above in charge.
    pub fn find_target(&self, index: &HashMap<(i32, i32), Vec<Target>>, player: i32, cell: (i32, i32), min: u32, max: u32, order: Option<Target>) -> Option<Target> {
        let (mut unit, mut turret, mut building) = (None, None, None);
        for &(dx, dy) in self.rings.search(min as usize, (max as usize).min(63)) {
            let Some(list) = index.get(&(cell.0 + dx as i32, cell.1 + dy as i32)) else { continue };
            for &t in list {
                if order == Some(t) && self.hostile(player, t) && !matches!(t, Target::Unit(id) if self.unit(id).map_or(false, |u| u.kind == 40)) {
                    return Some(t);
                }
                if !self.hostile(player, t) {
                    continue;
                }
                match t {
                    // flyers (the Advancer, kind 40, `[unit+0x4b]`) are stamped into the air grid 0x60ac38, which only
                    // anti-air shooters search (0x47ad30); the ground search 0x47b140 reads grid 0x5eac38 and never sees them
                    Target::Unit(id) if self.unit(id).map_or(false, |u| u.kind == 40) => {}
                    Target::Unit(_) => {
                        unit = unit.or(Some(t));
                    }
                    Target::Structure(i) if matches!(self.structures[i].kind, ObjectKind::Turret(_)) => turret = turret.or(Some(t)),
                    Target::Structure(_) => building = building.or(Some(t)),
                }
            }
            // the scan covers all rings while there is an order target to look for; otherwise the nearest unit ends it
            if unit.is_some() && order.is_none() {
                break;
            }
        }
        unit.or(turret).or(building)
    }

    fn step_unit(&mut self, i: usize, index: &HashMap<(i32, i32), Vec<Target>>) {
        let (kind, player, cell) = (self.units[i].kind, self.units[i].player, self.units[i].cell());
        // idle look-around (`0x46e114`): a unit without orders turns its idle top to a random facing every 240..1139 ticks
        if self.units[i].kind != 40 && self.units[i].path.is_empty() && self.units[i].aim.is_some() {
            self.units[i].look -= 1;
            if self.units[i].look <= 0 {
                let wait = 240 + self.random(900) as i32;
                let steps = self.units[i].aim.as_ref().map_or(32, |a| a.params.steps);
                let face = self.random(steps);
                let u = &mut self.units[i];
                u.look = wait;
                if u.target.is_none() {
                    u.aim.as_mut().unwrap().set_target_facing(face);
                }
            }
        }
        // weapon top
        let mut spawn: Vec<((i32, i32), (i32, i32))> = Vec::new();
        if kind == 40 && self.units[i].aim.is_some() {
            // the Advancer's gun (top case 0x47a8af, bullet 31, strength 64): the flyer search 0x47aed0 finds ground anti-air
            // units (handle type 5: kinds 1, 2, 3, 42; not the anti-air turret), then 0x47ad30 enemy Advancers. It keeps
            // searching while the beam hovers over a building, but not while the Advancer is still flying to one (`[chassis+0xe0] == 2`).
            let (min, max, steps) = {
                let p = &self.units[i].aim.as_ref().unwrap().params;
                (p.min_range, p.max_range, p.steps)
            };
            let approaching = self.units[i].action.is_some() && !self.units[i].path.is_empty();
            let t = if approaching { None } else { self.advancer_target(index, player, cell, min, max) };
            let (ux, uy) = self.units[i].px();
            let uy = uy + opensow_formats::aim::top_dy(kind); // aim origin = position + [top+0x60] (0x4797c0)
            // a flying target is aimed at (x, y + h/2) but the shot flies to its sprite (x, y + h) (`0x4797c0`)
            let tpoints = t.and_then(|id| self.unit(id)).map(|u| if u.kind == 40 { u.flyer_points() } else { (u.px(), u.px()) });
            let face = tpoints.map(|(aim, _)| facing(ux, uy, aim.0, aim.1, steps));
            let tpos = tpoints.map(|(_, shot)| shot);
            let mut r = self.random(1 << 16);
            let ev = self.units[i].aim.as_mut().unwrap().tick(face, |n| {
                r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (r >> 8) % n.max(1)
            });
            if ev.shot.is_some() {
                if let (Some(id), Some(to), Some((bt, strength))) = (t, tpos, weapon_shot(40)) {
                    // a bullet of type 31 (invisible, speed 600); the strength comes off the unit by handle when it lands (0x4108bc..0x4109b8)
                    // the spawner starts at the shooter's sprite: unit position + hover offset `h` (0x479e0e..0x479e20, the shooter is a
                    // flyer) + the top's offset `[top+0x40]` (= `[top+0x60]`, -16, copied at 0x47abbc)
                    let (fx, fy) = self.units[i].flyer_points().1;
                    let from = (fx, fy + opensow_formats::aim::top_dy(kind));
                    // the constructor of a type 31 bullet plays `tt-fire` (id 56) at the start point (0x412240..0x412276)
                    self.sound(GUN_SOUND, from.0, from.1, true);
                    self.shoot_at(bt, strength as i32, player.min(1), from, to, HitTarget::Unit(id));
                }
            }
        } else if self.units[i].aim.is_some() && !ANTI_AIR.contains(&kind) {
            let (min, max, steps) = {
                let p = &self.units[i].aim.as_ref().unwrap().params;
                (p.min_range, p.max_range, p.steps)
            };
            let t = self.find_target(index, player, cell, min, max, self.units[i].chase);
            self.units[i].target = t;
            let (ux, uy) = self.units[i].px();
            let uy = uy + opensow_formats::aim::top_dy(kind);
            let tpos = t.and_then(|t| self.target_pos(t));
            let face = tpos.map(|(tx, ty)| facing(ux, uy, tx, ty, steps));
            let jitter = self.random(1 << 16);
            let mut r = jitter;
            let ev = self.units[i].aim.as_mut().unwrap().tick(face, |n| {
                r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (r >> 8) % n.max(1)
            });
            if let (Some((_, shots)), Some(to)) = (ev.shot, tpos) {
                let a = self.units[i].aim.as_ref().unwrap();
                let (sin, cos) = heading_degrees(a.facing, steps).to_radians().sin_cos();
                let fallback = (ux + (sin * 14.0) as i32, uy - (cos * 14.0) as i32);
                // the spawner `0x47cf40` loops over the step's shots, shot k using `.po1` table barrel + k (offset += 0x40 words): it
                // starts at position - raw point and ends at target + (`.po2` reference point - raw point) (`0x47d0a5..0x47d0cb`), so
                // the shots of a volley fly parallel (walker 1's two barrels = two bolts). The tables are the sprite-anchor offsets,
                // so `uy` loses the aim origin's `top_dy` again. Without the tables: one point 14 px along the facing.
                let (barrel, facing_now) = (ev.shot.map_or(0, |s| s.0) as usize, a.facing as usize);
                let anchor_y = uy - opensow_formats::aim::top_dy(kind);
                let reference = self.turret_muzzles.get(&(REFERENCE_KEY + kind)).and_then(|t| t.muzzle(0, facing_now)).unwrap_or((0, 0));
                for _ in 0..opensow_formats::aim::rounds(kind) {
                    // every round of the spawner scatters the end point (0x47cfe1..0x47d03f)
                    let (jx, jy) = self.volley_scatter(kind);
                    let to = (to.0 + jx, to.1 + jy);
                    for table in barrel..barrel + shots.max(1) as usize {
                        let (from, to) = match self.turret_muzzles.get(&kind).and_then(|t| t.muzzle(table, facing_now)) {
                            Some((mx, my)) => ((ux + mx, anchor_y + my), (to.0 + mx - reference.0, to.1 + my - reference.1)),
                            None => (fallback, to),
                        };
                        spawn.push((from, to));
                    }
                }
                // the spawner plays the weapon's muzzle sound once per volley (`sfx::muzzle`)
                if let Some((id, jitter)) = opensow_formats::sfx::muzzle(kind as u8) {
                    self.sound(id, ux, uy, jitter);
                }
            }
        } else if self.units[i].aim.is_some() {
            // anti-air tops shoot aircraft: the hit takes the shot's strength off the aircraft directly (events 1, 10, 31 in 0x40f590)
            self.units[i].target = None;
            let (min, max, steps) = {
                let p = &self.units[i].aim.as_ref().unwrap().params;
                (p.min_range, p.max_range, p.steps)
            };
            let (ux, uy) = self.units[i].px();
            let polls = self.units[i].aim.as_ref().is_some_and(|a| a.polls_now() || a.state != opensow_formats::aim::State::Idle);
            let ai = if polls {
                let poll = self.units[i].aim.as_ref().unwrap().polls_now();
                self.anti_air_aircraft((false, self.units[i].id as usize), poll, player, (ux, uy), min, max)
            } else {
                None
            };
            let oy = uy + opensow_formats::aim::top_dy(kind);
            // no aircraft: the air-grid search 0x47ad30 finds an enemy Advancer
            let adv = if ai.is_none() { self.advancer_in_range(index, player, cell, min, max) } else { None };
            let apos = adv.and_then(|id| self.unit(id)).map(|u| u.flyer_points().0);
            let face = match (ai, apos) {
                (Some(a), _) => {
                    let (tx, ty) = self.aircraft_aim_point(a);
                    Some(facing(ux, oy, tx, ty, steps))
                }
                (None, Some((tx, ty))) => Some(facing(ux, oy, tx, ty, steps)),
                _ => None,
            };
            let mut r = self.random(1 << 16);
            let ev = self.units[i].aim.as_mut().unwrap().tick(face, |n| {
                r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (r >> 8) % n.max(1)
            });
            if let Some(_) = ev.shot {
                if let Some((bt, strength)) = weapon_shot(kind) {
                    // a type 1 bullet flies from the muzzle to the aircraft (or the Advancer's shot point); the hit is by handle
                    let a = self.units[i].aim.as_ref().unwrap();
                    let (sin, cos) = heading_degrees(a.facing, steps).to_radians().sin_cos();
                    let from = (ux + (sin * 14.0) as i32, uy - (cos * 14.0) as i32);
                    if let Some((id, jitter)) = opensow_formats::sfx::muzzle(kind as u8) {
                        self.sound(id, ux, uy, jitter);
                    }
                    if let Some(a) = ai {
                        let (to, id) = (self.aircraft_aim_point(a), self.aircraft[a].id);
                        self.shoot_at(bt, strength as i32, player.min(1), from, to, HitTarget::Aircraft(id));
                    } else if let Some(id) = adv {
                        if let Some(to) = self.unit(id).map(|u| u.flyer_points().1) {
                            self.shoot_at(bt, strength as i32, player.min(1), from, to, HitTarget::Unit(id));
                        }
                    }
                }
            }
        } else {
            self.units[i].target = None;
        }
        for (from, to) in spawn {
            self.fire(kind, player, from, to);
        }
        // a Rogon bomb (kind 46) blows up when its target's handle is in the 3 x 3 cells around it (0x46df40, `rogon::touches`)
        if kind == 46 {
            if let Some(t) = self.units[i].chase {
                let cells: Vec<(i32, i32)> = match t {
                    Target::Unit(id) => self.unit(id).filter(|u| u.hp > 0 && opensow_formats::rogon::can_touch(u.kind)).map(|u| [Some(u.cell()), u.heading_cell()].into_iter().flatten().collect()).unwrap_or_default(),
                    Target::Structure(k) => self.structures.get(k).filter(|s| s.alive()).map(|s| s.blocked_cells()).unwrap_or_default(),
                };
                if opensow_formats::rogon::touches(cell, cells) {
                    self.units[i].hp = 0;
                    return;
                }
            }
        }
        // chasing (`0x46eb40`): the route to the target is replanned every 7 ticks. The weapon firing at something does not
        // stop the chassis (the exe updates top and chassis independently every tick); a unit stops where its route ends
        if let Some(t) = self.units[i].chase {
            match self.target_pos(t) {
                Some((tx, ty)) if self.hostile(player, t) => {
                    if self.units[i].path.is_empty() || self.tick % 7 == (i as u64 % 7) {
                        self.chase_step(i, t, (tx.div_euclid(CELL), ty.div_euclid(CELL)));
                    }
                }
                _ => self.units[i].chase = None,
            }
        }
        // walking: `steps_at(speed, tick % 60)` single-pixel moves per tick (0x46f550 + table 0x436df0); the unit only
        // starts once its chassis is within 4 turn steps of the wanted facing
        // (every kind plans around other units: the movement code has no branch on kind 46, docs/walking.md)
        let tick = self.tick;
        let ox = (self.units[i].x, self.units[i].y);
        if self.units[i].kind == 40 {
            let u = &mut self.units[i];
            if tick % HOVER_PERIOD == 0 {
                u.hover_phase = (u.hover_phase + 1) % HOVER_PHASES;
            }
            // the Advancer hovers: it spins all the time, whether it moves or not
            if let Some(c) = u.chassis.as_mut() {
                if tick % ADVANCER_SPIN_TICKS == 0 {
                    c.facing = (c.facing + 1) % 32;
                }
                c.target = c.facing;
            }
        }
        // the walking step (`0x46f550`) loops over the pixel steps owed this tick; each pass plans a segment when the pixel line
        // is empty, points the chassis at the step cell, turns it one counter tick and walks one pixel once the facing is within
        // 4 turn steps of the wanted one. The chassis tail below turns once more
        let moves = steps_at(self.walk_params_of(self.units[i].kind).map_or(20, |p| p.speed), (tick % 60) as u32);
        for _ in 0..moves {
            if self.units[i].step.is_none() && self.plan_step(i) {
                break; // waiting for a free cell
            }
            let u = &mut self.units[i];
            let Some((wx, wy)) = u.step else { break };
            let steps = walk_params(u.kind).map_or(32, |p| p.steps);
            let aligned = match u.chassis.as_mut() {
                Some(c) if u.kind != 40 => {
                    // the wanted facing is recomputed from the pixel to the step cell centre unless the unit stands on it
                    if (wx - u.x).abs() + (wy - u.y).abs() > 1.0 {
                        let want = facing(u.x.round() as i32, u.y.round() as i32, wx.round() as i32, wy.round() as i32, steps);
                        c.set_target(want);
                    }
                    if c.turn() {
                        let target = c.target;
                        if let Some(a) = u.aim.as_mut() {
                            a.set_target_facing(target);
                        }
                    }
                    c.within_turns(4)
                }
                _ => true,
            };
            if !aligned {
                break;
            }
            let goal = (wx.round() as i32, wy.round() as i32);
            let here = (u.x.round() as i32, u.y.round() as i32);
            if here == goal {
                // the segment is done: the next pass plans the following one (`avoid` = 0x473190 runs when the line is empty)
                if !u.path.is_empty() {
                    u.path.remove(0);
                }
                u.step = None;
                u.line = None;
                continue;
            }
            // Bresenham from the segment start: one pixel along the long axis per move, the short axis when due
            let mut line = u.line.filter(|l| l.goal == goal).unwrap_or_else(|| {
                let (dx, dy) = (goal.0 - here.0, goal.1 - here.1);
                Line { goal, major: dx.abs().max(dy.abs()), minor: dx.abs().min(dy.abs()), err: 0 }
            });
            let (dx, dy) = (goal.0 - here.0, goal.1 - here.1);
            line.err += line.minor;
            let minor_step = 2 * line.err >= line.major && line.minor > 0;
            if minor_step {
                line.err -= line.major;
            }
            u.line = Some(line);
            let (sx, sy) = (dx.signum() as f32, dy.signum() as f32);
            let before = u.cell();
            if dx.abs() >= dy.abs() {
                u.x += sx;
                if minor_step {
                    u.y += sy;
                }
            } else {
                u.y += sy;
                if minor_step {
                    u.x += sx;
                }
            }
            if u.cell() != before {
                self.occupancy_moved(i, before);
            }
        }
        let u = &mut self.units[i];
        u.moved = (u.x, u.y) != ox;
        // the chassis tick (0x46f906) advances the walk phase while the unit's position differs from the goal pixel of its current
        // order (`[chassis+0x5c/0x60]`), i.e. while it still has a route: a unit waiting for a free cell keeps stepping its legs
        let moved = !u.path.is_empty() || u.line.is_some() || u.straight.is_some();
        let then = if u.path.is_empty() { u.straight.take() } else { None };
        // the queued rally order starts when the exit orders are done (the order list's next entry, `0x470480`)
        let rally = if u.path.is_empty() && u.straight.is_none() && then.is_none() && u.chase.is_none() { u.queued.take() } else { None };
        if let Some(c) = u.chassis.as_mut() {
            // every chassis turn step hands the chassis's target facing to the weapon top (`0x479980`, docs/art.md)
            if c.tick(moved) {
                let target = c.target;
                if let Some(a) = u.aim.as_mut() {
                    a.set_target_facing(target);
                }
            }
        }
        if let Some(then) = then {
            let id = self.units[i].id;
            self.order_move(id, then, None);
        } else if let Some((x, y)) = rally {
            let id = self.units[i].id;
            self.order_move(id, (x.div_euclid(CELL), y.div_euclid(CELL)), Some((x, y)));
        }
    }

    /// Sets the rally point of own factory `b` to the centre of the cell holding map pixel `(x, y)` (`0x417489..0x4175a1`).
    /// Returns false when `b` is not a living factory of `player`'s.
    pub fn set_rally(&mut self, b: usize, player: usize, x: i32, y: i32) -> bool {
        let me = if player == 0 { Owner::Player } else { Owner::Enemy };
        match self.structures.get_mut(b) {
            Some(s) if s.alive() && s.owner == me && opensow_formats::rally::has_rally(s.kind_id) => {
                s.rally = Some(opensow_formats::rally::snap(x, y));
                true
            }
            _ => false,
        }
    }

    fn step_structure(&mut self, i: usize, index: &HashMap<(i32, i32), Vec<Target>>) {
        if !self.structures[i].alive() || self.structures[i].aim.is_none() || self.structures[i].heal > 0 {
            // a turret gets its gun top only when it has finished healing in (0x40de60 creates it when [bld+0x68] turns 0)
            return;
        }
        let owner_player = if self.structures[i].owner == Owner::Player { 0 } else { 1 };
        if self.structures[i].owner == Owner::Neutral {
            return;
        }
        // idle look-around (`0x46e114`, docs/art.md): every 240..1139 ticks the top turns to a random facing when it has no target
        let look = self.turret_look.entry(i).or_insert(0);
        *look -= 1;
        if *look <= 0 {
            let wait = 240 + self.random(900) as i32;
            let steps = self.structures[i].aim.as_ref().map_or(32, |a| a.params.steps);
            let face = self.random(steps);
            self.turret_look.insert(i, wait);
            if self.structures[i].target.is_none() {
                self.structures[i].aim.as_mut().unwrap().set_target_facing(face);
            }
        }
        let (min, max, steps) = {
            let p = &self.structures[i].aim.as_ref().unwrap().params;
            (p.min_range, p.max_range, p.steps)
        };
        let cell = self.structures[i].cell;
        if self.structures[i].weapon == 110 {
            // the anti-air turret shoots aircraft, then an enemy Advancer (air-grid search)
            let (sx, sy) = self.structures[i].center();
            // the aim origin is the gun top's anchor: the turret unit's position plus its draw offset (`0x4797c0`: `[unit+0x14] + [top+0x5c]`, `[unit+0x18] + [top+0x60]`)
            let (ox, oy) = self.turret_anchor(i);
            // the search runs from the turret unit's position (its gun top's cell middle, `turret_top_pos`)
            let _ = (sx, sy);
            let pos = opensow_formats::buildings::turret_top_pos(self.structures[i].kind_id, self.structures[i].cell);
            let aim = self.structures[i].aim.as_ref().unwrap();
            let (poll, busy) = (aim.polls_now(), aim.state != opensow_formats::aim::State::Idle);
            let ai = if poll || busy { self.anti_air_aircraft((true, i), poll, owner_player, pos, min, max) } else { None };
            let adv = if ai.is_none() { self.advancer_in_range(index, owner_player, cell, min, max) } else { None };
            let apos = adv.and_then(|id| self.unit(id)).map(|u| u.flyer_points());
            let face = match (ai, apos) {
                (Some(a), _) => {
                    let (tx, ty) = self.aircraft_aim_point(a);
                    Some(facing(ox, oy, tx, ty, steps))
                }
                (None, Some(((tx, ty), _))) => Some(facing(ox, oy, tx, ty, steps)),
                _ => None,
            };
            let mut r = self.random(1 << 16);
            let ev = self.structures[i].aim.as_mut().unwrap().tick(face, |n| {
                r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (r >> 8) % n.max(1)
            });
            if let Some((barrel, _)) = ev.shot {
                // the rockets fly to the target (type 1, drawn as `missile`); the damage is taken off the target directly (events 1, 10, 31)
                let target = match (ai, apos) {
                    (Some(a), _) => Some(self.aircraft_aim_point(a)),
                    (None, p) => p.map(|(_, shot)| shot),
                };
                if let (Some(to), Some((bt, strength))) = (target, weapon_shot(110)) {
                    let facing_now = self.structures[i].aim.as_ref().unwrap().facing;
                    let from = self.turret_muzzle(i, 110, barrel as usize, facing_now, steps);
                    let handle = match (ai, adv) {
                        (Some(a), _) => Some(HitTarget::Aircraft(self.aircraft[a].id)),
                        (None, Some(id)) => Some(HitTarget::Unit(id)),
                        _ => None,
                    };
                    if let Some(h) = handle {
                        self.shoot_at(bt, strength as i32, owner_player, from, to, h);
                    }
                }
            }
            return;
        }
        let t = self.find_target(index, owner_player, cell, min, max, None);
        self.structures[i].target = t;
        let (sx, sy) = self.structures[i].center();
        let tpos = t.and_then(|t| self.target_pos(t));
        let (ox, oy) = self.turret_anchor(i);
        let face = tpos.map(|(tx, ty)| facing(ox, oy, tx, ty, steps));
        let mut r = self.random(1 << 16);
        let weapon = self.structures[i].weapon;
        let ev = self.structures[i].aim.as_mut().unwrap().tick(face, |n| {
            r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (r >> 8) % n.max(1)
        });
        if let (Some((barrel, shots)), Some(to)) = (ev.shot, tpos) {
            // each shot fires the `.po1` tables barrel..barrel+shots-1 (`recoil`), each from the muzzle point of the top's facing
            let facing_now = self.structures[i].aim.as_ref().unwrap().facing;
            for _ in 0..opensow_formats::aim::rounds(weapon) {
                // every round of the spawner scatters the end point (0x47cfe1..0x47d03f)
                let (jx, jy) = self.volley_scatter(weapon);
                let to = (to.0 + jx, to.1 + jy);
                for table in barrel as usize..barrel as usize + shots.max(1) as usize {
                    let from = self.turret_muzzle(i, weapon, table, facing_now, steps);
                    // the spawner ends the shot at target + (`.po2` point - `.po1` point) (`0x47d0a5..0x47d0cb`): the shots fly parallel
                    let anchor = self.turret_muzzle_anchor(i);
                    let to = if self.turret_muzzles.contains_key(&weapon) {
                        let reference = self.turret_muzzles.get(&(REFERENCE_KEY + weapon)).and_then(|t| t.muzzle(0, (facing_now * 32 / steps.max(1)) as usize)).unwrap_or((0, 0));
                        (to.0 + from.0 - anchor.0 - reference.0, to.1 + from.1 - anchor.1 - reference.1)
                    } else {
                        to
                    };
                    self.fire(weapon, owner_player, from, to);
                }
            }
            if let Some((id, jitter)) = opensow_formats::sfx::muzzle(weapon as u8) {
                self.sound(id, sx, sy, jitter);
            }
        }
    }

    /// The anchor of a turret's gun top: the unit's position plus the top's vertical draw offset.
    fn turret_anchor(&self, i: usize) -> (i32, i32) {
        let st = &self.structures[i];
        let (kx, ky) = opensow_formats::buildings::turret_top_pos(st.kind_id, st.cell);
        (kx, ky + opensow_formats::buildings::turret_art(st.kind_id).map_or(0, |a| a.top_dy))
    }

    /// The gun top's anchor as [`World::turret_muzzle`] adds the `.po1` point to it.
    fn turret_muzzle_anchor(&self, i: usize) -> (i32, i32) {
        let st = &self.structures[i];
        let (kx, ky) = opensow_formats::buildings::turret_top_pos(st.kind_id, st.cell);
        (kx, ky + opensow_formats::buildings::turret_art(st.kind_id).map_or(0, |a| a.top_dy))
    }

    /// Where a turret's shot starts: the gun top's anchor (the turret unit's position plus the draw offset `[top+0x60]`) minus the
    /// `.po1` point of the table and facing; without the tables a point 14 px along the facing.
    fn turret_muzzle(&self, i: usize, weapon: i32, table: usize, facing_now: u32, steps: u32) -> (i32, i32) {
        let st = &self.structures[i];
        let (kx, ky) = opensow_formats::buildings::turret_top_pos(st.kind_id, st.cell);
        let dy = opensow_formats::buildings::turret_art(st.kind_id).map_or(0, |a| a.top_dy);
        if let Some((mx, my)) = self.turret_muzzles.get(&weapon).and_then(|t| t.muzzle(table, (facing_now * 32 / steps.max(1)) as usize)) {
            return (kx + mx, ky + dy + my);
        }
        let (sin, cos) = heading_degrees(facing_now, steps).to_radians().sin_cos();
        (kx + (sin * 14.0) as i32, ky - (cos * 14.0) as i32)
    }

    fn new_air_id(&mut self) -> u32 {
        self.next_air_id += 1;
        self.next_air_id - 1
    }

    /// A targeted shot (bullet types 1 and 31, `[+0x2688]`): the bullet flies to `to` and the strength comes off `target` when it lands.
    /// A bullet with no path (start equals end) hits at once.
    fn shoot_at(&mut self, bt: u8, strength: i32, owner: i32, from: (i32, i32), to: (i32, i32), target: HitTarget) {
        match build_path(bt, from.0, from.1, to.0, to.1, 0).filter(|p| !p.points.is_empty()) {
            Some(path) => {
                let mut b = Bullet::new(bt, strength, owner, path);
                b.target = Some(target);
                self.bullets.push(b);
            }
            None => self.hit_target(target, strength),
        }
    }

    /// `0x4108bc`: the strength comes off the unit or aircraft with the bullet's handle, if it is still there.
    fn hit_target(&mut self, target: HitTarget, strength: i32) {
        match target {
            HitTarget::Unit(id) => {
                if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
                    u.hp -= strength;
                }
            }
            HitTarget::Aircraft(id) => {
                if let Some(a) = self.aircraft.iter_mut().find(|a| a.id == id) {
                    a.hp -= strength;
                }
            }
        }
    }

    fn fire(&mut self, weapon: i32, owner: i32, from: (i32, i32), to: (i32, i32)) {
        let Some((bt, strength)) = weapon_shot(weapon) else { return };
        let Some(path) = build_path(bt, from.0, from.1, to.0, to.1, 0) else { return };
        if path.points.is_empty() {
            return;
        }
        self.bullets.push(Bullet::new(bt, strength as i32, owner, path));
    }

    fn step_bullets(&mut self) {
        let t = (self.tick % 60) as u32;
        let mut landed: Vec<(i32, i32, u8, i32, i32, Option<HitTarget>)> = Vec::new();
        let mut puffs: Vec<(i32, i32, AnimSpec)> = Vec::new();
        let mut smoke: Vec<(i32, i32, opensow_formats::effects::Trail)> = Vec::new();
        self.bullets.retain_mut(|b| {
            let steps = steps_at(b.speed, t);
            if b.bullet_type == 18 {
                b.speed += steps; // the bomb accelerates by the steps of every tick (`0x41146d`: `[+0x2664] += steps`)
            }
            for _ in 0..steps {
                b.index += 1;
                // shells of types 1, 3, 4, 24, 25 leave smoke (`effects::trail`), also not on the landing step
                if let Some(tr) = shell_trail(b.bullet_type) {
                    if b.index < b.path.points.len() {
                        b.trail -= 1;
                        if b.trail <= 0 {
                            b.trail = tr.reload;
                            let (x, y) = b.path.points[b.index - 1];
                            smoke.push((x, y, tr));
                        }
                    }
                }
                // flamer bullets (types 8, 9) and the sparks of types 27 and 31 (the Advancer gun's blue/green beam): a puff every `interval` path steps, none on the landing step (0x4118e2)
                if let Some(spec) = flame_trail(b.bullet_type).or_else(|| spark_trail(b.bullet_type)).or_else(|| gun_trail(b.bullet_type, b.owner)) {
                    if b.index < b.path.points.len() {
                        b.trail -= 1;
                        if b.trail <= 0 {
                            b.trail = bullet_type_interval(b.bullet_type);
                            // the exe stores the step's point [k] as the position, then moves its index past it: point index - 1
                            let (x, y) = b.path.points[b.index - 1];
                            puffs.push((x, y, spec));
                        }
                    }
                }
            }
            if let Some(a) = &mut b.anim {
                if !a.step(b.bullet_type, 2 * b.index as i32) {
                    return false; // a one-shot animation (types 22, 23) has played out
                }
            }
            if b.index >= b.path.points.len() {
                let (x, y) = *b.path.points.last().unwrap();
                landed.push((x, y, b.bullet_type, b.strength, b.owner, b.target));
                false
            } else {
                true
            }
        });
        for (x, y, spec) in puffs {
            self.effects.push(Effect { x, y, spec, age: 0 });
        }
        for (x, y, tr) in smoke {
            // the y offset is drawn first, then x (`0x411565`, `0x41156f`)
            let dy = tr.offset + self.fx_random(tr.jitter) as i32;
            let dx = tr.offset + self.fx_random(tr.jitter) as i32;
            self.effects.push(Effect { x: x + dx, y: y + dy, spec: tr.spec, age: 0 });
        }
        for (x, y, bt, strength, owner, target) in landed {
            if bt == 10 {
                continue; // the satellite laser only hits its target handle (0x4108bc): no area damage, decal or sound
            }
            for spec in impact_anims(bt) {
                self.effects.push(Effect { x, y, spec, age: 0 });
            }
            self.impact_ground_effects(x, y, bt);
            // the impact function's sound (`0x40f590`); the bomber's bomb (18) only half of the time, the meteors with pitch jitter
            if let Some(id) = opensow_formats::effects::impact_sound(bt) {
                if bt != 18 || self.random(2) == 0 {
                    self.sound(id, x, y, matches!(bt, 24 | 25));
                }
            }
            if matches!(bt, 1 | 31) {
                // the targeted hits (0x4108bc): only the shooter's target loses the strength, nothing else is touched
                if let Some(t) = target {
                    self.hit_target(t, strength);
                }
                continue;
            }
            self.explode(x, y, bt, strength, owner);
            if bt == 17 {
                // a Tripler pod: 34 blasts of 100 on an ellipse around the impact, each jittered by -5..4 px (0x40fd40..0x40ff44)
                // Every point also makes a small blast sprite (`0x405240(.., 1)`, `0x40fdae`) and, on a free cell that is not "no marks",
                // two smoke puffs (`0x40fe18..0x40ff16`); the jitter and the puff offsets come from the visual generator `0x437cd0`.
                for &(dx, dy) in &TRIPLER_RING[..TRIPLER_RING_USED] {
                    let (jx, jy) = (self.fx_random(10) as i32 - 5, self.fx_random(10) as i32 - 5);
                    let (px, py) = (x + dx + jx, y + dy + jy);
                    self.effects.push(Effect { x: px, y: py, spec: blast(1), age: 0 });
                    self.explode(px, py, 17, TRIPLER_BLAST_STRENGTH, owner);
                    let cell = (px.div_euclid(CELL), py.div_euclid(CELL));
                    if !self.is_blocked(cell.0, cell.1) && !self.is_no_marks(cell.0, cell.1) {
                        for _ in 0..2 {
                            let (oy, ox) = (self.fx_random(20) as i32 - 10, self.fx_random(20) as i32 - 10);
                            self.puff(px + ox, py + oy);
                        }
                    }
                }
            }
        }
    }

    /// Area damage (`0x42f2c0`): ground units within 32 px take the full strength (friendly fire included), a structure takes it
    /// when the blast lies in its cell.
    fn explode(&mut self, x: i32, y: i32, bt: u8, strength: i32, owner: i32) {
        if strength <= 0 {
            return;
        }
        let refs: Vec<UnitRef> = self.units.iter().map(|u| UnitRef { x: u.px().0, y: u.px().1, flying: u.kind == 40, owner: u.player as u32, kind: u.kind }).collect();
        // the Advancer is the one flyer (`[unit+0x4b]`) and `0x42f30e` skips flyers
        for v in splash_victims(&refs, x, y, bt, owner as u32) {
            self.units[v].hp -= strength;
        }
        let cell = (x.div_euclid(CELL), y.div_euclid(CELL));
        let mut destroyed = Vec::new();
        // A finished turret's gun top is a ground unit (kinds 109..113): within 32 px of the blast it does not lose hit points
        // itself, the turret does, the full strength for every bullet type (`0x42f346..0x42f38d`: `0x43a910(top owner handle)`,
        // `[bld+0x74] -= strength`, tally `[bld+0x7c + owner*4] += strength`). This comes on top of the cell test below, so a hit on
        // the turret's own cell does both.
        for (i, s) in self.structures.iter_mut().enumerate() {
            if s.alive() && s.aim.is_some() && s.heal == 0 && matches!(s.kind, ObjectKind::Turret(_)) {
                let (tx, ty) = opensow_formats::buildings::turret_top_pos(s.kind_id, s.cell);
                if opensow_formats::projectile::distance(x, y, tx, ty) < opensow_formats::projectile::SPLASH_RADIUS {
                    s.hp -= strength;
                    s.tally[owner.clamp(0, 1) as usize] += strength;
                    if s.hp <= 0 && !destroyed.contains(&i) {
                        destroyed.push(i);
                    }
                }
            }
        }
        for (i, s) in self.structures.iter_mut().enumerate() {
            // no owner check, as in the exe (0x42f3cc..0x42f462): buildings take friendly fire too
            if s.alive() && s.covers(cell) {
                // half damage for the meteors and the Rogon blast, none on a Headquarters for the air-strike types (0x42f3cc..0x42f462)
                let d = opensow_formats::projectile::building_damage(bt, strength, s.kind_id);
                s.hp -= d;
                s.tally[owner.clamp(0, 1) as usize] += d;
                if s.hp <= 0 && !destroyed.contains(&i) {
                    destroyed.push(i);
                }
            }
        }
        for i in destroyed {
            self.structure_destroyed(i);
        }
    }

    /// Test/headless helper: the structure covering `cell` loses all its hit points.
    pub fn debug_destroy_at(&mut self, cx: i32, cy: i32) {
        if let Some(i) = self.structures.iter().position(|s| s.alive() && s.covers((cx, cy))) {
            self.structures[i].hp = 0;
            self.structure_destroyed(i);
        }
    }

    /// One burst of a dying building's destruction animation (`0x409b60`): a blast at a random point of the footprint with area
    /// damage, the sound, and three bullets of type 14 and three of type 13 (strength 80) that fly off on random curves. Draw order:
    /// the visual `rand(7)` first (sound 24 once in seven), then lockstep `rand(15)`, x, y; the bullets draw their own numbers
    /// (`curves::RandomBullet::draw`), and the last lockstep draw `rand(15)` sets the passes until the next burst (returned).
    fn destruction_burst(&mut self, i: usize, turret: bool) -> i32 {
        let (cell, (w, h), owner) = (self.structures[i].cell, self.structures[i].size(), self.structures[i].owner);
        let owner = side(owner) as i32;
        let (cx, cy) = (cell.0 * CELL, cell.1 * CELL);
        if self.fx_random(7) == 2 {
            self.sound(24, cx, cy, false);
        }
        let pick = self.random(15);
        let x = self.random((w * CELL - 20).max(1) as u32) as i32 + cx + 10;
        let y = self.random((h * CELL - 20).max(1) as u32) as i32 + cy + 10;
        // turrets: strength 50 and `e_turret`; others 80 for a pick of 5 or less, else 50, and `e_bubg`
        let strength = if turret || pick > 5 { 50 } else { 80 };
        self.explode(x, y, 0, strength, owner);
        self.effects.push(Effect { x, y, spec: if turret { TURRET_DESTROYED } else { BUILDING_DESTROYED }, age: 0 });
        self.sound(25, cx, cy, false);
        for bt in [14u8, 14, 14, 13, 13, 13] {
            let Some(rb) = self.game_rng.as_mut().and_then(|r| RandomBullet::draw(bt, r)) else { break };
            let Some(path) = self.curves.as_ref().map(|c| c.path(rb.curve, x, y, 0)).filter(|p| !p.points.is_empty()) else { break };
            let mut b = Bullet::new(bt, 80, owner, path);
            b.speed = rb.speed;
            b.variant = rb.flag.unwrap_or(0);
            self.bullets.push(b);
        }
        BURST_GAP + self.random(BURST_JITTER) as i32
    }

    /// One pass of the destruction animation (`0x409b60` loop body): count the debris counter down; at 0 the animation counter
    /// goes down too, and either the next burst is thrown or the animation ends.
    fn destruction_pass(&mut self, i: usize) {
        let Some((anim, next)) = self.structures[i].dying else { return };
        if next > 1 {
            self.structures[i].dying = Some((anim, next - 1));
        } else if anim <= 1 {
            self.finish_destruction(i);
        } else {
            let turret = matches!(self.structures[i].kind, ObjectKind::Turret(_));
            let gap = self.destruction_burst(i, turret);
            self.structures[i].dying = Some((anim - 1, gap));
        }
    }

    /// The animations of all dying buildings: the exe runs [`DESTRUCTION_PASSES`] passes per tick.
    fn step_destruction(&mut self) {
        for i in 0..self.structures.len() {
            for _ in 0..DESTRUCTION_PASSES {
                if self.structures[i].dying.is_none() {
                    break;
                }
                self.destruction_pass(i);
            }
        }
    }

    /// A structure ran out of hit points (`0x409130`): its destruction animation starts (the first burst comes at once), the
    /// building stays on the map, dead, until the animation ends ([`Self::finish_destruction`]).
    fn structure_destroyed(&mut self, i: usize) {
        if self.structures[i].dying.is_some() {
            return;
        }
        let owner = side(self.structures[i].owner);
        let turret = matches!(self.structures[i].kind, ObjectKind::Turret(_));
        let (cx, cy) = self.structures[i].center();
        self.quake.start(if turret { QUAKE_TURRET } else { QUAKE_BUILDING }, cx as i32, cy as i32);
        self.structures[i].dying = Some((destruction_bursts(self.structures[i].kind_id), 1));
        self.destruction_pass(i);
        // `0x409270` (lost / turret destroyed) passes the building's centre
        self.say_at(owner, if turret { msg::TURRET_DESTROYED } else { msg::BUILDING_LOST }, cx as i32, cy as i32);
    }

    /// The end of the destruction animation (`0x409ead`, then the player loop `0x42dc60` -> `0x43ab40`): turrets and
    /// headquarters are removed, every other building is rebuilt for the player with the largest foreign damage tally, starting
    /// at 1 hit point and healing up to its maximum.
    fn finish_destruction(&mut self, i: usize) {
        self.structures[i].dying = None;
        let s = &self.structures[i];
        let owner = side(s.owner);
        // 0x43ab40..0x43ac38: with nobody else's damage on it the building goes to player 1 (single player: `[0x575720] == 0`)
        let winner = if removed_when_destroyed(s.kind_id) { None } else { Some(capture_winner(s.tally, owner).unwrap_or(1)) };
        let turret = matches!(self.structures[i].kind, ObjectKind::Turret(_));
        self.stats[owner][if turret { 5 } else { 1 }] += 1; // 0x409ee8 / 0x409ef0
        if let (Some(p), false) = (winner, turret) {
            self.stats[p.min(2)][0] += 1; // the capture builds the building again for its new owner (0x43a068)
        }
        let (cx, cy) = self.structures[i].center();
        // `0x43ad46` (captured) passes the building's centre
        if let Some(p) = winner {
            self.say_at(p, msg::BUILDING_CAPTURED, cx as i32, cy as i32);
        }
        // the rebuild (`0x43ab40`) frees the old handle slot and takes the first free one of the new owner's table
        self.slot_free(owner, i);
        match winner {
            Some(p) => {
                self.slot_alloc(p.min(2), i);
                let s = &mut self.structures[i];
                s.owner = match p {
                    0 => Owner::Player,
                    1 => Owner::Enemy,
                    _ => Owner::Neutral,
                };
                s.hp = 1;
                s.heal = s.max_hp - 1;
                s.tally = [0; 3];
                s.upgrade = 0;
                s.target = None;
                s.rally = None; // the exe rebuilds the building (`0x43ab40`): the constructor clears the point
                s.slots.iter_mut().for_each(|sl| sl.progress = 0);
            }
            None => {
                for c in self.structures[i].blocked_cells() {
                    if let Some(b) = self.cell_index(c.0, c.1) {
                        self.blocked[b] = false;
                    }
                }
            }
        }
    }

    /// The pointer over structure `target` while an Advancer of `player` is selected (`0x417b8c..0x417d0a`): destroy for
    /// enemy and neutral buildings, repair / upgrade for own ones (docs/cursor.md).
    pub fn advancer_pointer(&self, player: i32, target: usize) -> Pointer {
        let Some(s) = self.structures.get(target).filter(|s| s.alive()) else { return Pointer::Arrow };
        let me = if player == 0 { Owner::Player } else { Owner::Enemy };
        pointer_over_building(s.owner == me, s.kind_id, s.hp < s.max_hp, s.active < s.slots.len(), self.repair_mode)
    }

    /// What the Advancer does when it is ordered onto structure `target`: the action of the pointer it would show.
    pub fn action_for(&self, unit: &Unit, target: usize) -> Option<Action> {
        action_for_pointer(self.advancer_pointer(unit.player, target))
    }

    /// The exe re-checks the target before every shot (`0x46f3a6`): repair needs missing hit points, upgrade a free slot.
    fn action_valid(&self, unit: &Unit, action: Action, target: usize) -> bool {
        let Some(s) = self.structures.get(target) else { return false };
        let me = if unit.player == 0 { Owner::Player } else { Owner::Enemy };
        s.alive()
            && match action {
                Action::Destroy => s.owner != me,
                Action::Repair => s.owner == me && s.hp < s.max_hp,
                Action::Upgrade => s.owner == me && s.active < s.slots.len(),
            }
    }

    /// Orders the Advancer `id` onto structure `target`: it flies to the building's cell and shoots its beam there.
    pub fn order_action(&mut self, id: u32, target: usize) -> bool {
        let Some(u) = self.unit(id) else { return false };
        if u.kind != 40 {
            return false;
        }
        let Some(action) = self.action_for(u, target) else { return false };
        let cell = self.structures[target].action_cell();
        // `0x46b8b0` calls `0x4706a0` with order type 1 (a normal go-to, `0x46b9c6`), not the straight type 2 of the factory
        // exits (`0x408890`): the wave route and the local avoidance apply, so an Advancer already hovering over the cell blocks
        // it and the second one stops next to it, out of beam range (the original does the same)
        let c = centre(cell);
        self.order_move(id, cell, Some((c.0 as i32, c.1 as i32)));
        let u = self.units.iter_mut().find(|u| u.id == id).unwrap();
        u.action = Some((action, target));
        true
    }

    /// The structure under map pixel `(x, y)`, if any.
    /// Like [`structure_at`], also counting the art above the footprint; the footprint wins.
    pub fn structure_by_art(&self, x: i32, y: i32) -> Option<usize> {
        let c = (x.div_euclid(CELL), y.div_euclid(CELL));
        self.structure_at(x, y).or_else(|| self.structures.iter().position(|s| s.alive() && s.covers_art(c)))
    }

    pub fn structure_at(&self, x: i32, y: i32) -> Option<usize> {
        let c = (x.div_euclid(CELL), y.div_euclid(CELL));
        self.structures.iter().position(|s| s.alive() && s.covers(c))
    }

    /// Income (`0x42d6b0`): every finished gold mine adds `active + 1` credits per second to its owner, every research lab
    /// `active + 1` research points per second; buildings still healing in (`[bld+0x68]`) and destroyed ones give nothing.
    fn step_economy(&mut self) {
        self.wind = [0; 2];
        for s in self.structures.iter().filter(|s| s.alive() && s.heal == 0 && s.kind == ObjectKind::Wind) {
            if let Some(side) = side_of(s.owner) {
                self.wind[side] += s.active as u32;
            }
        }
        if self.tick % 60 != 0 {
            return; // the exe's income loop runs on the first tick of each second ([0x517b10 + i*4] == 0)
        }
        let before = (self.credits[0], self.research[0]);
        for s in self.structures.iter().filter(|s| s.alive() && s.heal == 0) {
            let side = match s.owner {
                Owner::Player => 0,
                Owner::Enemy => 1,
                _ => continue,
            };
            let gain = income_per_tick(s.active);
            match s.kind {
                ObjectKind::GoldMine => self.credits[side] = (self.credits[side] + gain).min(CREDITS_CAP),
                ObjectKind::ResearchLab => self.research[side] = (self.research[side] + gain).min(RESEARCH_CAP),
                _ => {}
            }
        }
        // crossing a turret price (200..1000) or 500 research tells the player (0x42d88c, 0x42d8f5..0x42d98a)
        if [200, 400, 600, 800, 1000].iter().any(|&p| before.0 < p && self.credits[0] >= p) {
            self.say(0, msg::NEW_TURRET);
        }
        if before.1 < 500 && self.research[0] >= 500 {
            self.say(0, msg::NEW_TECH_LEVEL);
        }
    }

    /// Buys the next tech-level of slot `slot` of structure `s` for `side` (`0x454c19` + `0x40e730`): the slot must be
    /// active, below its level limit and affordable in research points; its kind becomes the next tier, progress is kept.
    pub fn tech_up(&mut self, s: usize, slot: usize, side: usize) -> bool {
        let Some(st) = self.structures.get_mut(s) else { return false };
        if !st.alive() || side_of(st.owner) != Some(side) || slot >= st.active {
            return false;
        }
        let Some(sl) = st.slots.get_mut(slot) else { return false };
        let (Some(next), Some(price)) = (next_tier(sl.kind), slot_unlock_cost(sl.kind)) else { return false };
        if sl.level >= sl.max_level || self.research[side] < price as i32 {
            return false;
        }
        self.research[side] -= price as i32;
        sl.kind = next;
        sl.level += 1;
        if let Some(t) = slot_target(next) {
            sl.target = t;
        }
        true
    }

    /// Raises every affordable tech-level of every factory of `side` (the exe's AI does this in `0x42f490`).
    pub fn tech_up_all(&mut self, side: usize) -> usize {
        let mut n = 0;
        for s in 0..self.structures.len() {
            for slot in 0..5 {
                n += self.tech_up(s, slot, side) as usize;
            }
        }
        n
    }

    /// The placement test per footprint cell (`0x408be0`, results `[pointer+0x170..]`, row by row from the top-left `cell`): a cell is
    /// good (state 2) when it is free (units, terrain, buildings, no-build layer) and some cell within two cells of the footprint
    /// belongs to one of `side`'s buildings; otherwise state 1 and the whole spot is refused.
    pub fn turret_cells(&self, side: usize, tier: u32, cell: (i32, i32)) -> Vec<bool> {
        let me = if side == 0 { Owner::Player } else { Owner::Enemy };
        let (w, h) = footprint(24 + tier as i32).map_or((1, 1), |(w, h, _)| (w, h));
        let own_near = |x: i32, y: i32| self.structures.iter().any(|s| s.alive() && s.owner == me && s.covers((x, y)));
        let near = (cell.1 - 2..cell.1 + h + 2).any(|y| (cell.0 - 2..cell.0 + w + 2).any(|x| own_near(x, y)));
        let free = |x: i32, y: i32| !self.is_blocked(x, y) && !self.is_no_build(x, y) && !self.units.iter().any(|u| u.hp > 0 && u.cell() == (x, y)) && !self.structures.iter().any(|s| s.alive() && s.covers((x, y)));
        (0..h).flat_map(|dy| (0..w).map(move |dx| (dx, dy))).map(|(dx, dy)| near && free(cell.0 + dx, cell.1 + dy)).collect()
    }

    /// Dev tools: puts a turret (25..=29) or building (100..=108) of `side` on `cell` (its top-left footprint cell) with full hit
    /// points, without cost, range or terrain tests. Refused (false) when the footprint leaves the map or overlaps a living
    /// structure. Buildings get no production slots (the level scripts supply them); an HQ links the side's first free Advancer.
    pub fn dev_place_structure(&mut self, kind_id: i32, side: usize, cell: (i32, i32)) -> bool {
        let kind = ObjectKind::from_id(kind_id);
        let Some((w, h, _)) = footprint(kind_id).filter(|_| !matches!(kind, ObjectKind::Unknown(_))) else { return false };
        let cells: Vec<(i32, i32)> = (0..h).flat_map(|dy| (0..w).map(move |dx| (cell.0 + dx, cell.1 + dy))).collect();
        if cells.iter().any(|&(x, y)| x < 0 || y < 0 || x >= self.cells.0 || y >= self.cells.1 || self.structures.iter().any(|s| s.alive() && s.covers((x, y)))) {
            return false;
        }
        let owner = if side == 0 { Owner::Player } else { Owner::Enemy };
        let (weapon, aim) = match kind {
            ObjectKind::Turret(n) => (turret_weapon(n as u32), TopAim::new(turret_weapon(n as u32), 0)),
            _ => (0, None),
        };
        let max_hp = building_max_hp(kind_id);
        self.stats[side][4] += 1;
        self.add_structure(Structure { object: usize::MAX, kind, owner, cell, hp: max_hp, max_hp, aim, weapon, target: None, slots: Vec::new(), active: 0, kind_id, tally: [0; 3], heal: 0, upgrade: 0, protected: kind_id == 100, attacked: false, sat_cd: 0, advancer: None, rally: None, dying: None });
        if kind_id == 100 {
            let k = self.structures.len() - 1;
            let taken: Vec<u32> = self.structures.iter().filter_map(|s| s.advancer).collect();
            self.structures[k].advancer = self.units.iter().find(|u| u.kind == 40 && u.hp > 0 && (u.player != 0) == (side != 0) && !taken.contains(&u.id)).map(|u| u.id);
        }
        self.sound(72, cell.0 * CELL, cell.1 * CELL, false);
        true
    }

    /// Builds a turret of `tier` (1-5) for `side` on `cell` (`0x417335`..`0x4173da`): costs 200 * tier credits, the new
    /// turret starts at 1 hit point and heals in. The placement test is `0x408be0` (docs/ai.md): free of units, terrain, buildings and the map's no-build layer, and an own building within two cells.
    pub fn place_turret(&mut self, side: usize, tier: u32, cell: (i32, i32)) -> bool {
        if !self.build_turret(side, tier, cell) {
            return false;
        }
        self.sound(72, cell.0 * CELL, cell.1 * CELL, false); // `0x4173f9`: sound 0x48 at the cell's top-left pixel
        true
    }

    /// Places a turret and pays for it (`0x439e30`; the computer's `0x431120` calls it without the click sound).
    fn build_turret(&mut self, side: usize, tier: u32, cell: (i32, i32)) -> bool {
        let Some(price) = turret_price(tier) else { return false };
        let me = if side == 0 { Owner::Player } else { Owner::Enemy };
        let kind_id = 24 + tier as i32;
        let free = |x: i32, y: i32| !self.is_blocked(x, y) && !self.is_no_build(x, y) && !self.units.iter().any(|u| u.hp > 0 && u.cell() == (x, y)) && !self.structures.iter().any(|s| s.alive() && s.covers((x, y)));
        let own_near = |x: i32, y: i32| self.structures.iter().any(|s| s.alive() && s.owner == me && s.covers((x, y)));
        if self.credits[side] < price || !can_place(kind_id, cell, free, own_near) {
            return false;
        }
        self.credits[side] -= price;
        self.stats[side][4] += 1; // 0x43a04e
        let weapon = turret_weapon(tier);
        let aim = TopAim::new(weapon, 0);
        let max_hp = building_max_hp(24 + tier as i32);
        let owner = if side == 0 { Owner::Player } else { Owner::Enemy };
        self.add_structure(Structure { object: usize::MAX, kind: ObjectKind::Turret(tier as u8), owner, cell, hp: 1, max_hp, aim, weapon, target: None, slots: Vec::new(), active: 0, kind_id: 24 + tier as i32, tally: [0; 3], heal: max_hp - 1, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None, dying: None });
        true
    }

    /// The computer player (`0x42f490`, run for the non-local side every tick outside the tutorial): buys tech-levels,
    /// picks a strategy every 900-1500 ticks, sends idle units after the nearest target of the strategy's class, steers
    /// its Advancers (`0x42fae0`) and builds turrets next to threatened buildings (`0x431120`).
    fn step_ai(&mut self, side: usize) {
        self.tech_up_all(side);
        let me = if side == 0 { Owner::Player } else { Owner::Enemy };
        let mine = |o: Owner| o == me;
        let hq = self.structures.iter().position(|s| s.alive() && s.kind_id == 100 && !mine(s.owner));
        let enemy_turrets = self.structures.iter().filter(|s| s.alive() && !mine(s.owner) && matches!(s.kind, ObjectKind::Turret(_))).count();
        // strategy: re-rolled when it no longer fits the enemy's turret count (0x42f67e..0x42f6c1) or its timer expires
        if (enemy_turrets == 0 && self.ai_strategy == 2) || (enemy_turrets > 4 && self.ai_strategy != 2) || self.ai_strategy == 0 {
            self.ai_timer = 0;
        }
        self.ai_timer -= 1;
        if self.ai_timer <= 0 {
            self.ai_strategy = 1 + self.random(4);
            self.ai_timer = 900 + self.random(600) as i32;
        }
        if enemy_turrets == 0 && self.ai_strategy == 2 {
            self.ai_strategy = 4;
        }
        if enemy_turrets > 4 {
            self.ai_strategy = 2;
        }
        // idle units attack: strategy 1 the HQ, 2 turret > unit > building, 3 unit > turret > building, 4 building > turret > unit
        let enemy_adv = self.units.iter().filter(|u| u.hp > 0 && u.kind == 40 && (u.player != 0) != (side != 0)).map(|u| u.id).last();
        let idle: Vec<usize> = (0..self.units.len()).filter(|&i| {
            let u = &self.units[i];
            u.hp > 0 && (u.player != 0) == (side != 0) && u.kind != 40 && u.chase.is_none() && u.path.is_empty()
        }).collect();
        for i in idle {
            let (cell, kind) = (self.units[i].cell(), self.units[i].kind);
            let pick = if is_anti_air(kind) {
                enemy_adv.map(Target::Unit)
            } else {
                let (u, t, b) = self.nearest_enemy(side, cell);
                let order = match self.ai_strategy {
                    1 => [None, None, None],
                    2 => [t, u, b],
                    3 => [u, t, b],
                    _ => [b, t, u],
                };
                order.into_iter().flatten().next()
            };
            self.units[i].chase = pick.or(hq.map(Target::Structure));
        }
        // Advancers choose a building: score = random(3) + bonuses - distance (0x42fb94..0x42fd9f)
        let advancers: Vec<usize> = (0..self.units.len()).filter(|&i| {
            let u = &self.units[i];
            u.hp > 0 && u.kind == 40 && (u.player != 0) == (side != 0) && u.action.is_none()
        }).collect();
        for i in advancers {
            let (ux, uy) = (self.units[i].x as i32 / CELL, self.units[i].y as i32 / CELL);
            let mut best: Option<(i32, usize)> = None;
            for k in 0..self.structures.len() {
                if !self.structures[k].alive() {
                    continue;
                }
                let mut score = self.random(3) as i32;
                let s = &self.structures[k];
                if s.kind_id == 100 {
                    score = -10_000;
                }
                if mine(s.owner) {
                    if matches!(s.kind, ObjectKind::Turret(_)) || s.active >= s.slots.len() {
                        score = -10_000;
                    }
                } else {
                    if s.protected {
                        score = -10_000;
                    }
                    if matches!(s.kind, ObjectKind::Turret(_)) {
                        score += 2;
                    }
                    if s.kind_id == 26 {
                        score += 2;
                    }
                    score += s.active as i32;
                }
                let sc = s.center_cell();
                let d = (((sc.0 - ux).pow(2) + (sc.1 - uy).pow(2)) as f32).sqrt() as i32;
                score -= d;
                if best.map_or(true, |(b, _)| score > b) {
                    best = Some((score, k));
                }
            }
            if let Some((_, k)) = best {
                let id = self.units[i].id;
                self.order_action(id, k);
            }
        }
        // air strikes every 600 ticks: bomber, fighter or tripler at random if in stock (0x42f930..0x42fa2a)
        self.ai_strike -= 1;
        if self.ai_strike <= 0 {
            self.ai_strike = 600;
            let force = [AirForce::Bomber, AirForce::Fighter, AirForce::Tripler][self.random(3) as usize];
            if let Some((x, y)) = self.strike_target(side) {
                self.launch_strike(side, force, x, y);
            }
        }
        // `0x42fa30`: the turret routine runs right after the air-strike pick, on the same 600-step timer (twice at most, the
        // second time only while the credits still reach 200)
        if self.credits[side] >= 200 {
            self.ai_build_turret(side);
            if self.credits[side] >= 200 {
                self.ai_build_turret(side);
            }
        }
    }

    /// The computer's turret routine (`0x431120`): the decision is `formats::turretai::plan` (exe-verified, docs/ai.md item 7);
    /// this gathers its inputs from the world and builds the result.
    fn ai_build_turret(&mut self, side: usize) {
        use opensow_formats::turretai::{plan, Building, Slot};
        let me = side as i32;
        // the unit slots; dead units stay in theirs, and the gun tops of finished turrets are units 109..113 in the exe
        let mut slots: Vec<Slot> = self.units.iter().filter(|u| u.hp > 0).map(|u| Slot { cell: u.cell(), owner: u.player, kind: u.kind }).collect();
        slots.extend(self.corpses.iter().map(|c| Slot { cell: (c.x.div_euclid(CELL), c.y.div_euclid(CELL)), owner: c.player, kind: c.kind }));
        for t in self.structures.iter().filter(|t| t.alive() && t.heal == 0) {
            if let (ObjectKind::Turret(n), Some(o)) = (t.kind, side_of(t.owner)) {
                // the top is created at the building's cell, one row lower for the 1x2 turrets 4 and 5 (`0x40e029`, `0x40e03e`)
                slots.push(Slot { cell: (t.cell.0, t.cell.1 + (n >= 4) as i32), owner: o as i32, kind: 108 + n as i32 });
            }
        }
        // own buildings in handle-table order (ties go to the lower slot); structures the table does not know follow
        let wanted = |k: &usize| self.structures.get(*k).map_or(false, |s| s.alive() && side_of(s.owner) == Some(side) && !matches!(s.kind, ObjectKind::Turret(_)));
        let mut order: Vec<usize> = self.slot_tables[side].iter().flatten().copied().filter(wanted).collect();
        let known = order.clone();
        order.extend((0..self.structures.len()).filter(|k| wanted(k) && !known.contains(k)));
        let buildings: Vec<Building> = order.iter().map(|&k| Building { kind_id: self.structures[k].kind_id, cell: self.structures[k].cell }).collect();
        let flags = self.turret_flags[side];
        let credits = self.credits[side];
        let me_owner = if side == 0 { Owner::Player } else { Owner::Enemy };
        let aa = |c: (i32, i32)| self.structures.iter().any(|t| t.alive() && t.kind_id == 26 && t.covers(c));
        // the generator is lent to the closure (one `rand(8)` per try, as in the exe)
        let (mut table, mut state) = (self.game_rng.take(), self.rng);
        let placed = plan(&buildings, &slots, me, credits, &flags, aa, |n| draw(&mut table, &mut state, n), |x, y, w, h| {
            let free = |cx: i32, cy: i32| !self.is_blocked(cx, cy) && !self.is_no_build(cx, cy) && !self.units.iter().any(|u| u.hp > 0 && u.cell() == (cx, cy)) && !self.structures.iter().any(|s| s.alive() && s.covers((cx, cy)));
            let own_near = |cx: i32, cy: i32| self.structures.iter().any(|s| s.alive() && s.owner == me_owner && s.covers((cx, cy)));
            can_place(if h == 2 && w == 1 { 28 } else { 25 }, (x, y), free, own_near)
        });
        self.game_rng = table;
        self.rng = state;
        if let Some(p) = placed {
            self.build_turret(side, p.tier, p.cell);
        }
    }

    /// Where the computer sends an air strike (`0x435ad0`, `aircraft::ai_strike_target`).
    fn strike_target(&self, side: usize) -> Option<(i32, i32)> {
        let units: Vec<opensow_formats::aircraft::StrikeUnit> = self
            .units
            .iter()
            .filter(|u| u.hp > 0)
            .map(|u| opensow_formats::aircraft::StrikeUnit { x: u.px().0, y: u.px().1, kind: u.kind, own: (u.player != 0) == (side != 0), idle: u.path.is_empty() })
            .collect();
        opensow_formats::aircraft::ai_strike_target(&units)
    }

    /// The hostile unit / turret / other building a reachability wave from `cell` meets first (`0x472a90`, exact port
    /// `wave::find_targets`): the static grid holds terrain, buildings and the ground units' handles; anything whose owner
    /// differs from `side` counts, so neutral buildings are targets too. Objects stop the wave.
    fn nearest_enemy(&mut self, side: usize, cell: (i32, i32)) -> (Option<Target>, Option<Target>, Option<Target>) {
        let dims = self.cells;
        let Some(o) = self.occupancy() else { return (None, None, None) };
        let mut grid = o.stat.clone();
        let mut base = o.stat_base;
        for u in self.units.iter().filter(|u| u.hp > 0 && u.kind != 40) {
            let (x, y) = u.cell();
            if (0..128).contains(&x) && (0..128).contains(&y) && grid[(x * 128 + y) as usize] == 0 {
                grid[(x * 128 + y) as usize] = 0x3000000 + u.id;
            }
        }
        let classify = |h: u32| {
            if h >= 0x3000000 {
                match self.unit(h - 0x3000000) {
                    Some(u) if (u.player != 0) != (side != 0) => Seen::Unit,
                    _ => Seen::Ignore,
                }
            } else if h >= 0x2000000 {
                match self.structures.get((h - 0x2000000) as usize) {
                    Some(s) if s.alive() && side_of(s.owner) != Some(side) => {
                        if matches!(s.kind, ObjectKind::Turret(_)) { Seen::Turret } else { Seen::Building }
                    }
                    _ => Seen::Ignore,
                }
            } else {
                Seen::Ignore
            }
        };
        let t = find_targets(&mut grid, &mut base, dims, cell, classify);
        let unit = (t.unit != 0).then(|| Target::Unit(t.unit - 0x3000000));
        let structure = |h: u32| (h != 0).then(|| Target::Structure((h - 0x2000000) as usize));
        (unit, structure(t.turret), structure(t.building))
    }

    /// The nearest enemy aircraft whose ground position is within `min..=max` cells of `pos`.
    /// The Advancer's target (`0x46c990` -> `0x47aed0`, then `0x47ad30` when nothing was found): the nearest enemy ground
    /// anti-air unit (kinds 1, 2, 3, 42), else the nearest enemy Advancer.
    fn advancer_target(&self, index: &HashMap<(i32, i32), Vec<Target>>, player: i32, cell: (i32, i32), min: u32, max: u32) -> Option<u32> {
        for &(dx, dy) in self.rings.search(min as usize, (max as usize).min(63)) {
            let Some(list) = index.get(&(cell.0 + dx as i32, cell.1 + dy as i32)) else { continue };
            for &t in list {
                if let Target::Unit(id) = t {
                    if self.hostile(player, t) && self.unit(id).map_or(false, |u| matches!(u.kind, 1 | 2 | 3 | 42)) {
                        return Some(id);
                    }
                }
            }
        }
        self.advancer_in_range(index, player, cell, min, max)
    }

    /// Air-grid search (`0x47ad30`): the nearest enemy flying unit (the Advancer) in the range rings.
    fn advancer_in_range(&self, index: &HashMap<(i32, i32), Vec<Target>>, player: i32, cell: (i32, i32), min: u32, max: u32) -> Option<u32> {
        for &(dx, dy) in self.rings.search(min as usize, (max as usize).min(63)) {
            let Some(list) = index.get(&(cell.0 + dx as i32, cell.1 + dy as i32)) else { continue };
            for &t in list {
                if let Target::Unit(id) = t {
                    if self.hostile(player, t) && self.unit(id).map_or(false, |u| u.kind == 40) {
                        return Some(id);
                    }
                }
            }
        }
        None
    }

    /// The aircraft an anti-air top of `player` at `pos` (the unit's position `[unit+0x14/0x18]`) shoots at, range `min..max` cells:
    /// on the top's poll the exe's search `0x47b3c0` (`aircraft::anti_air_target`, it may keep the last target), otherwise the
    /// kept target while it is still in the aircraft list (`0x479902..0x47996a`: aiming and firing do not search again). Returns
    /// the index into `self.aircraft`.
    fn anti_air_aircraft(&mut self, shooter: (bool, usize), polls: bool, player: i32, pos: (i32, i32), min: u32, max: u32) -> Option<usize> {
        use opensow_formats::aircraft::{anti_air_target, AirSlot};
        let current = self.air_targets.get(&shooter).copied();
        let found = if polls {
            let mut slots: Vec<AirSlot> = self.aircraft.iter().map(|a| AirSlot { id: a.id, side: a.side, force: a.force, x: a.x, y: a.y, aim_dx: a.aim_dx, hp: a.hp }).collect();
            slots.sort_by_key(|a| a.side); // per-side slot arrays, scanned side by side
            anti_air_target(&slots, pos.0, pos.1, current, min, max, if player == 0 { 0 } else { 1 })
        } else {
            current.filter(|id| self.aircraft.iter().any(|a| a.id == *id))
        };
        match found {
            Some(id) => self.air_targets.insert(shooter, id),
            None => self.air_targets.remove(&shooter),
        };
        found.and_then(|id| self.aircraft.iter().position(|a| a.id == id))
    }

    /// Where anti-air aims at an aircraft and its rockets fly to (`0x479953..0x479962`): `x + [+0x18]`, `y - altitude`.
    fn aircraft_aim_point(&self, a: usize) -> (i32, i32) {
        let a = &self.aircraft[a];
        (a.x + a.aim_dx, a.y - a.altitude)
    }

    /// Queue a sound effect at a map position (dropped when nobody listens, so headless runs stay small).
    fn sound(&mut self, id: u8, x: i32, y: i32, jitter: bool) {
        if self.sounds.len() < 64 {
            self.sounds.push((id, x, y, jitter));
        }
    }

    /// A message for `side`; only the local player (side 0) hears it (`0x402ec0` runs on the audio object of that player).
    fn say(&mut self, side: usize, id: u8) {
        if side == 0 {
            self.log.push((self.tick, id));
        }
    }

    /// `say` for an event with a map position (pixels), which the Space key jumps to (docs/hotkeys.md).
    fn say_at(&mut self, side: usize, id: u8, x: i32, y: i32) {
        if side == 0 {
            self.last_event = Some((x, y));
        }
        self.say(side, id);
    }

    /// Space / numpad 0 (`0x4037b0`): the position of the last message event, while any message line is still shown (they live 480 ticks).
    pub fn event_position(&self) -> Option<(i32, i32)> {
        self.log.iter().any(|&(t, _)| self.tick.saturating_sub(t) < 480).then_some(self.last_event).flatten()
    }

    /// Launches an air strike of `force` for `side` at map pixel `(x, y)` from the stock (`0x467ee0`, `0x455520`).
    pub fn launch_strike(&mut self, side: usize, force: AirForce, x: i32, y: i32) -> bool {
        let Some(g) = HEADER_ORDER.iter().position(|&h| h == force) else { return false };
        if self.air_stock[side][g] == 0 {
            return false;
        }
        self.air_stock[side][g] -= 1;
        self.stats[side][6] += 1; // 0x467eef
        if side == 0 {
            self.strike_lock = STRIKE_LOCK_TICKS; // 0x467efb..0x467f03: the local player's air icons are disabled for 3 s
        }
        let mut a = Aircraft::launch(force, x, y, side as i32, self.cells.0, self.carry_counter);
        a.id = self.new_air_id();
        if force == AirForce::Carryall {
            self.carry_counter = launch_counter(self.carry_counter);
        }
        self.aircraft.push(a);
        if side == 0 {
            // the launch for the local player pings `airforce-dest` at the target (0x417221, 0x417636: the marker animation + effect 68)
            self.sound(68, x, y, false);
            // ... and shows the `cross_as` marker (`0x4171d5`: 21 frames, 3 ticks each) at the clicked point, unsnapped
            if let Some(sp) = opensow_formats::effects::SPAWNS.iter().find(|s| s.site == 0x4171d5) {
                self.effects.push(Effect { x, y, spec: sp.spec, age: 0 });
            }
        }
        true
    }

    /// Flies the aircraft (`0x467b50`): their bombs, missiles, pods and meteors become bullets, carryalls land a squad.
    fn step_aircraft(&mut self) {
        let t = (self.tick % 60) as u32;
        let mut aircraft = std::mem::take(&mut self.aircraft);
        let mut events = Vec::new();
        for a in &mut aircraft {
            let mut r = self.rng;
            let mut rand = |n: u32| {
                r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
                ((r >> 8) % n.max(1)) as i32
            };
            for e in a.step(t, &mut rand) {
                events.push((a.side, a.right, e));
            }
            self.rng = r;
            for (id, x, y) in std::mem::take(&mut a.sounds) {
                if self.sounds.len() < 64 {
                    self.sounds.push((id, x, y, false));
                }
            }
        }
        for a in aircraft.iter().filter(|a| !a.alive()) {
            if a.hp <= 0 {
                self.stats[a.side.clamp(0, 2) as usize][7] += 1; // 0x467b12
            }
        }
        aircraft.retain(|a| a.alive());
        self.aircraft = aircraft;
        for (side, right, e) in events {
            match e {
                AirEvent::Shot { bullet_type, strength, from, to } => {
                    if let Some(path) = build_path(bullet_type, from.0, from.1, to.0, to.1, 0) {
                        if !path.points.is_empty() {
                            let mut b = Bullet::new(bullet_type, strength, side, path);
                            // 0x4684f2: a bomber's bomb takes the flight direction `[aircraft+0x34]` as `[+0x268c]` (bomba_l / bomba_r)
                            b.variant = (bullet_type == 18 && right) as u32;
                            self.bullets.push(b);
                        }
                    }
                }
                AirEvent::Fireball { x, y, period } => {
                    self.effects.push(Effect { x, y, spec: AnimSpec { period, ..AIR_DESTROYED }, age: 0 });
                }
                AirEvent::Unload { cell_x, cell_y, squad: n } => {
                    // mission 1 always drops the same squad (`0x468970`: `[0x49a33c] == 1`)
                    let members = if self.mission_no == Some(1) {
                        MODE1_SQUAD.iter().enumerate().map(|(i, &k)| (i as i32 / 2 - 1, i as i32 % 2, k)).collect()
                    } else {
                        squad(n).unwrap_or_default()
                    };
                    for (dx, dy, kind) in members {
                        let (x, y) = (cell_x + dx, cell_y + dy);
                        if !self.is_blocked(x, y) && !self.units.iter().any(|u| u.hp > 0 && u.cell() == (x, y)) {
                            self.add_unit(kind, side.min(1) as i32, x * CELL + CELL / 2, y * CELL + CELL / 2);
                        }
                    }
                }
            }
        }
    }

    /// Corpses (`0x46b600`): the timer runs down; below [`CORPSE_BLAST_BELOW`] the final blasts go off once; below 0 the slot is freed.
    fn step_corpses(&mut self) {
        let mut corpses = std::mem::take(&mut self.corpses);
        for c in corpses.iter_mut() {
            c.left -= 1;
            if c.left < CORPSE_BLAST_BELOW && !c.blasted {
                c.blasted = true;
                for &size in corpse_blasts(c.variant) {
                    let (dy, dx) = (self.random(30) as i32 - 15, self.random(30) as i32 - 15);
                    self.effects.push(Effect { x: c.x + dx, y: c.y + dy, spec: blast(size), age: 0 });
                }
            }
        }
        corpses.retain(|c| c.left >= 0);
        self.corpses = corpses;
        self.occ = None;
    }

    /// Burning wrecks (`0x45b0a0`): remove at 0 columns left, else count down and throw a rising smoke bullet every 20 steps.
    fn step_wrecks(&mut self) {
        let mut wrecks = std::mem::take(&mut self.wrecks);
        wrecks.retain_mut(|w| {
            if w.left <= 0 {
                return false;
            }
            w.timer -= 1;
            if w.timer <= 0 {
                w.left -= 1;
                w.timer = WRECK_SMOKE_PERIOD;
                let (dx, dy) = (80 + self.fx_random(80) as i32, 80 + self.fx_random(40) as i32);
                if let Some(path) = build_path(WRECK_SMOKE_TYPE, w.x, w.y, w.x + dx, w.y - dy, 0) {
                    if !path.points.is_empty() {
                        self.bullets.push(Bullet::new(WRECK_SMOKE_TYPE, 0, 0, path));
                    }
                }
            }
            true
        });
        self.wrecks = wrecks;
    }

    /// Advances the explosions (`0x411220`, type 35).
    fn step_nukes(&mut self) {
        use opensow_formats::rogon::Explosion;
        let words: Vec<i32> = self.rings.offsets.iter().flat_map(|&(dx, dy)| [dx as i32, dy as i32]).collect();
        // the list used goes up to ring 7: `[0x6b3a34]` = 2 * (offsets up to and including ring 7)
        let limit = Explosion::limit(&self.rings.ends);
        let mut blasts: Vec<(i32, i32, i32, i32, bool)> = Vec::new();
        self.nukes.retain_mut(|n| {
            let (list, alive) = n.ex.step(&words, limit);
            for b in list {
                blasts.push((n.x + 16 + b.dx * CELL, n.y + 16 + b.dy * CELL, b.strength, n.owner, b.sound));
            }
            alive
        });
        // per blast, in the exe's order (0x411310..0x411442): the rumble every 17th blast (the walker nuke 19, now and then the
        // fighter missile's boom 67), then a `e_unit2` cloud (the unit-death cloud, not a blast sprite) at the centre moved by
        // rand(32) - 16 on y (first draw) and x, then the area damage
        for (x, y, strength, owner, sound) in blasts {
            if sound {
                let id = if self.random(4) == 1 { 67 } else { 19 };
                self.sound(id, x, y, true);
            }
            let (dy, dx) = (self.random(32) as i32 - 16, self.random(32) as i32 - 16);
            self.effects.push(Effect { x: x + dx, y: y + dy, spec: UNIT_CLOUD, age: 0 });
            self.explode(x, y, 35, strength, owner);
        }
    }

    /// A headquarters whose Advancer is gone builds a new one on the cell diagonally next to it (`0x42e38d`), unless the level
    /// header forbids it for that side (words 34 / 35).
    fn step_hq(&mut self) {
        for k in 0..self.structures.len() {
            let s = &self.structures[k];
            if s.kind_id != 100 || !s.alive() {
                continue;
            }
            let Some(side) = side_of(s.owner) else { continue };
            if self.no_advancer_respawn[side] {
                continue;
            }
            let alive = s.advancer.is_some_and(|id| self.unit(id).is_some_and(|u| u.hp > 0));
            if !alive {
                let c = (s.cell.0 + 1, s.cell.1 + 1);
                let id = self.add_unit(40, side as i32, c.0 * CELL + CELL / 2, c.1 * CELL + CELL / 2);
                self.structures[k].advancer = Some(id);
            }
        }
    }

    /// Missions 3 and 8 drive the player's Rogon pods by script (`0x42e580`): in mission 3 every pod is sent to (176, 2352);
    /// in mission 8 a pod west of x = 2880 and south of y = 352 is sent to (2960, 272).
    fn step_convoy(&mut self) {
        let goal = |u: &Unit| match self.mission_no {
            Some(3) => Some((176, 2352)),
            Some(8) if u.px().0 < 0xb40 && u.px().1 >= 0x180 => Some((0xb90, 0x110)),
            _ => None,
        };
        let orders: Vec<(u32, (i32, i32))> = self.units.iter().filter(|u| u.hp > 0 && u.player == 0 && u.kind == 46 && u.path.is_empty()).filter_map(|u| goal(u).map(|g| (u.id, g))).collect();
        for (id, (x, y)) in orders {
            self.order_move(id, (x.div_euclid(CELL), y.div_euclid(CELL)), Some((x, y)));
        }
    }

    /// Satellite protection (`0x409f50`): on a protected building a recently attacked flag plus an expired 240-tick timer
    /// destroys every foreign Advancer above the building; the flag is cleared on every pass.
    fn step_satellites(&mut self) {
        for i in 0..self.structures.len() {
            let s = &mut self.structures[i];
            if !s.protected || !s.alive() {
                continue;
            }
            if s.sat_cd > 0 {
                s.sat_cd -= 1;
            }
            let strike = s.attacked && s.sat_cd <= 0;
            if strike {
                s.sat_cd = SATELLITE_TICKS;
            }
            s.attacked = false;
            if strike {
                let owner = side(s.owner);
                let s = self.structures[i].clone_footprint();
                let mut shots = Vec::new();
                for u in self.units.iter_mut().filter(|u| u.kind == 40 && u.hp > 0 && s.covers(u.cell()) && (if u.player == 0 { 0 } else { 1 }) != owner) {
                    u.hp = 0;
                    shots.push((u.px(), u.player.min(1)));
                }
                // the laser bullet (type 10, `effects::SATLASER_*`): purely visual, the Advancer is already dead
                for ((x, y), player) in shots {
                    let lift = opensow_formats::anim::HOVER_ALTITUDE;
                    if let Some(path) = build_path(10, x, y + lift + SATLASER_START_DY, x, y + lift + SATLASER_END_DY, 0) {
                        if !path.points.is_empty() {
                            self.bullets.push(Bullet::new(10, SATLASER_STRENGTH, player, path));
                        }
                    }
                }
            }
        }
    }

    /// Advancer beams: once the Advancer hovers over the target cell it shoots every `ADVANCER_BEAM_TICKS`; each shot
    /// moves `2 * rate` hit points (destroy / repair, `0x40ffe9`) or adds `rate` upgrade points (`0x40e3d0`). New
    /// buildings heal in at `HEAL_PER_TICK`.
    fn step_actions(&mut self) {
        for s in &mut self.structures {
            if s.heal > 0 && s.hp > 0 {
                let h = s.heal.min(HEAL_PER_TICK);
                s.hp += h;
                s.heal -= h;
            }
        }
        for i in 0..self.units.len() {
            if self.units[i].kind == 40 && self.units[i].player == 0 {
                let u = &mut self.units[i];
                if u.action.is_none() && u.path.is_empty() && u.hp > 0 {
                    u.idle += 1;
                    if u.idle >= 420 {
                        u.idle = -840;
                        self.last_event = Some((u.x as i32, u.y as i32)); // `0x46f30a`
                        self.log.push((self.tick, msg::ADVANCER_DISENGAGED));
                    }
                } else {
                    u.idle = 0;
                }
            }
            // `0x46f210` ticks the cooldown member first, every tick, whatever the unit does (`0x46f321`)
            self.units[i].beam.tick();
            let Some((action, t)) = self.units[i].action else { continue };
            let u = &self.units[i];
            let side_u = if u.player == 0 { 0 } else { 1 };
            let still_valid = self.action_valid(u, action, t);
            let target_cell = self.structures.get(t).map(|s| s.action_cell());
            // a new move order (path set elsewhere) or an invalid target cancels the action
            if !still_valid || target_cell.is_none() || (u.path.is_empty() && u.cell() != target_cell.unwrap()) {
                self.units[i].action = None;
                continue;
            }
            if u.cell() != target_cell.unwrap() || !u.path.is_empty() {
                continue;
            }
            if !self.units[i].beam.ready() {
                continue;
            }
            self.units[i].beam.fire();
            let rate = self.rates[side_u];
            // the beam's sound (`0x4111b0`: destroy 53, upgrade 58, repair 57) on every third shot
            self.units[i].beam_shots += 1;
            if self.units[i].beam_shots % 3 == 1 {
                let id = match action {
                    Action::Destroy => 53,
                    Action::Upgrade => 58,
                    Action::Repair => 57,
                };
                let (bx, by) = self.structures[t].center();
                self.sound(id, bx, by, false);
            }
            // the shot: a bullet of type 19 / 20 / 21 from the hovering Advancer straight down to the building's cell (`0x47cf40`)
            let (sx, sy) = (self.units[i].x as i32, self.units[i].sprite_y() as i32);
            let (cx, cy) = self.structures[t].action_cell();
            let (ex, ey) = (cx * CELL + CELL / 2, cy * CELL + CELL / 2);
            if let Some(path) = build_path(action as u8, sx, sy, ex, ey, 0) {
                let mut frags = Vec::new();
                for k in 0..fragment_count(action) {
                    let mut rnd = |n: u32| self.random(n);
                    frags.push(fragment(action, k, &mut rnd));
                }
                self.beams.push(BeamShot { action, target: t, side: side_u, rate, path, index: 0, trail: 2, frags });
            }
        }
    }

    /// Flies the Advancers' beam bullets (speed 60: one step per tick). The upgrade bullet drops two yellow sparks every
    /// second step (`0x411993`); on landing the action happens (`0x40f590`).
    fn step_beams(&mut self) {
        let mut landed = Vec::new();
        let mut trails = Vec::new();
        for (n, b) in self.beams.iter_mut().enumerate() {
            let (x, y) = b.path.points[b.index.min(b.path.points.len() - 1)];
            if b.action == Action::Upgrade {
                b.trail -= 1;
                if b.trail <= 0 {
                    b.trail = 2;
                    for &(fx, fy, frame) in &b.frags {
                        trails.push(Effect { x: x + fx, y: y + fy, spec: AnimSpec { array: BODKY_Y, from: frame, to: frame + 1, period: 11, repeat: 1, mode: 6 }, age: 0 });
                    }
                }
            }
            b.index += 1;
            if b.index >= b.path.points.len() {
                landed.push(n);
            }
        }
        self.effects.extend(trails);
        for &n in landed.iter().rev() {
            let b = self.beams.remove(n);
            self.beam_lands(b);
        }
    }

    fn beam_lands(&mut self, b: BeamShot) {
        let (x, y) = *b.path.points.last().unwrap();
        let t = b.target;
        // the target is looked up by its id (`[bullet+0x2688]`): a building that changed hands meanwhile is a new object
        let me = if b.side == 0 { Owner::Player } else { Owner::Enemy };
        let valid = self.structures.get(t).is_some_and(|s| s.alive() && (s.owner != me) == (b.action == Action::Destroy));
        if !valid {
            return;
        }
        let (mut upgraded, mut repaired) = (false, false);
        let s = &mut self.structures[t];
        match b.action {
            Action::Destroy => {
                s.attacked = true;
                s.hp -= hp_per_event(b.rate);
                s.tally[b.side] += hp_per_event(b.rate);
                // one red spark animation at the landing point (`0x410342`)
                self.effects.push(Effect { x, y, spec: AnimSpec { array: BODKY_R, from: 6, to: 16, period: 2, repeat: 1, mode: 6 }, age: 0 });
                if self.structures[t].hp <= 0 {
                    self.structure_destroyed(t);
                }
            }
            Action::Repair => {
                s.hp = (s.hp + hp_per_event(b.rate)).min(s.max_hp);
                s.tally[b.side] = (s.tally[b.side] - hp_per_event(b.rate)).max(0);
                repaired = s.hp >= s.max_hp;
                // a blue spark per fragment (`0x410063`)
                for &(fx, fy, _) in &b.frags {
                    self.effects.push(Effect { x: x + fx, y: y + fy, spec: AnimSpec { array: BODKY_B, from: 6, to: 9, period: 4, repeat: 1, mode: 6 }, age: 0 });
                }
            }
            Action::Upgrade => {
                if s.active < s.slots.len() {
                    s.upgrade += upgrade_per_event(b.rate);
                    if let Some(slot) = s.slots.get(s.active) {
                        if s.upgrade >= slot.target as i32 {
                            s.active += 1;
                            s.upgrade = 0;
                            upgraded = true;
                        }
                    }
                }
            }
        }
        let turret = matches!(self.structures[t].kind, ObjectKind::Turret(_));
        if upgraded {
            let (x, y) = self.structures[t].center(); // `0x40e445`
            self.say_at(b.side, msg::BUILDING_UPGRADED, x, y);
        }
        if repaired {
            // `0x410150` passes the Advancer's y (`[unit+0x18]`) for both x and y
            let y = self.units.iter().find(|u| u.kind == 40 && u.player as usize == b.side).map_or(0, |u| u.y as i32);
            self.say_at(b.side, if turret { msg::TURRET_REPAIRED } else { msg::BUILDING_REPAIRED }, y, y);
        }
    }

    /// Win and lose rules (`0x4212f0`): the player loses without a headquarters; the enemy loses when it has no headquarters
    /// (missions 4, 8, 13, 16, 21 and 22: no units and no buildings at all). A countdown that runs out ends the game by its
    /// mode (header word 67: 0 victory, 1 defeat, 2 only reinforcements).
    fn check_outcome(&mut self) {
        if self.tutorial.is_some() {
            return; // the tutorial has no HQ rules (`0x42e306`): it ends through its own script
        }
        let hq = |o: Owner| self.structures.iter().any(|s| s.kind_id == 100 && s.owner == o && s.present());
        let had_player_hq = self.structures.iter().any(|s| s.kind_id == 100 && s.owner == Owner::Player);
        let destroy_all = matches!(self.mission_no, Some(4 | 8 | 13 | 16 | 21 | 22));
        let enemy_left = self.units.iter().any(|u| u.player != 0 && u.hp > 0) || self.structures.iter().any(|s| s.present() && s.owner == Owner::Enemy);
        if self.timer_expired {
            match self.timer.map(|t| t.2) {
                Some(0) => self.outcome = Some(Outcome::Victory),
                Some(1) => self.outcome = Some(Outcome::Defeat),
                _ => {}
            }
            if self.outcome.is_some() {
                return;
            }
        }
        // missions 3 and 8: the convoy must survive; mission 3 is won when a pod stands at cells x 4..=6, y 72..=74
        if matches!(self.mission_no, Some(3 | 8)) {
            let pods = self.units.iter().filter(|u| u.hp > 0 && u.player == 0 && u.kind == 46).count();
            if pods == 0 {
                self.outcome = Some(Outcome::Defeat);
                return;
            }
            if self.mission_no == Some(3) && self.units.iter().any(|u| u.hp > 0 && u.player == 0 && u.kind == 46 && (4..=6).contains(&u.cell().0) && (72..=74).contains(&u.cell().1)) {
                self.outcome = Some(Outcome::Victory);
                return;
            }
        }
        if self.mission_no == Some(3) {
            return; // mission 3 ends only through its pods (0x42147e)
        }
        if had_player_hq && !hq(Owner::Player) {
            self.outcome = Some(Outcome::Defeat);
        } else if self.enemy_at_start == Some(true) && ((destroy_all && !enemy_left) || (!destroy_all && self.has_enemy_hq && !hq(Owner::Enemy)) || (!destroy_all && !self.has_enemy_hq && !enemy_left)) {
            self.outcome = Some(Outcome::Victory);
        }
    }

    /// Per-second mission logic (`0x42ebd0`, `0x42e580`): the countdown, its reinforcements and the mission-1 reward.
    fn step_mission(&mut self) {
        if self.tick % 60 != 0 {
            return;
        }
        // `0x42ebd0` looks at the clock first (the end flag `[0x5baa99]` once it shows 0:00, the 20/10/5/2 minute messages at
        // seconds == 1, the rewards in the call that sets the flag) and only then counts one second off (`0x42f1ff`)
        if let Some((min, sec, mode)) = self.timer {
            if !self.timer_expired && min <= 0 && sec <= 0 {
                self.timer_expired = true;
                if mode == 2 {
                    self.reinforcements();
                }
            }
            if mode == 2 && sec == 1 {
                match min {
                    20 => self.say(0, 14),
                    10 => self.say(0, 15),
                    5 => self.say(0, 16),
                    2 => self.say(0, 17),
                    _ => {}
                }
            }
            let (mut m, mut s) = (min, sec - 1);
            if s < 0 {
                m -= 1;
                s = 59;
            }
            if m < 0 {
                m = 0;
                s = 0;
            }
            self.timer = Some((m, s, mode));
        }
        // mission 1: raising both tech-levels of the medium factory pays out the air forces (8 fighters, 3 carryalls)
        if self.mission_no == Some(1) && !self.timer_expired {
            let done = self.structures.iter().any(|s| s.owner == Owner::Player && s.kind_id == 102 && s.slots.len() >= 2 && s.slots[0].level >= s.slots[0].max_level && s.slots[1].level >= s.slots[1].max_level); // no `max > 0` test in `0x42e73c`
            if done {
                self.say(0, msg::REINFORCEMENTS);
                self.air_stock[0][4] += 8;
                self.air_stock[0][2] += 3;
                self.timer_expired = true;
            }
        }
        self.update_hint();
    }

    /// The hint boxes of missions 1 and 6 (`0x4303e0`, `0x430770`): once the player has 500 research points (`[0x517b04]`)
    /// "Now increase tech-level in your factory." is shown while the medium factory's tech-levels are not both at their
    /// limit (mission 6: while none is). Mission 1 then shows "Now use fighters to destroy A.A. turrets on the island."
    /// from the moment the 8 fighters are in stock until fewer than 8 remain.
    fn update_hint(&mut self) {
        self.hint = None;
        let maxed = |s: &Structure, k: usize| {
            let (l, m) = s.slots.get(k).map_or((0, 0), |sl| (sl.level, sl.max_level));
            l >= m
        };
        let factories: Vec<&Structure> = self.structures.iter().filter(|s| s.owner == Owner::Player && s.kind_id == 102).collect();
        let research = self.research[0] >= 500;
        match self.mission_no {
            Some(1) => {
                // the first medium factory: number of slots (0 and 1) at their limit
                let state = factories.first().map(|s| maxed(s, 0) as i32 + maxed(s, 1) as i32);
                if research {
                    match state {
                        Some(0) => self.hint = Some(Hint { text: 281, half: 0x94, text_dx: 0x89 }),
                        Some(1) => self.hint = Some(Hint { text: 281, half: 0xbc, text_dx: 0xb1 }),
                        _ => {}
                    }
                }
                let fighters = self.air_stock[0][4];
                if self.hint_phase == 1 && fighters < 8 {
                    self.hint_phase = 2;
                }
                if state == Some(2) && fighters >= 8 && self.hint_phase != 2 {
                    self.hint_phase = 1;
                    self.hint = Some(Hint { text: 282, half: 0xc1, text_dx: 0xb6 });
                }
            }
            Some(6) => {
                // nothing maxed yet in any medium factory (and at least one exists)
                let count: Option<i32> = (!factories.is_empty()).then(|| factories.iter().map(|s| maxed(s, 0) as i32 + maxed(s, 1) as i32).sum());
                if research && count == Some(0) {
                    self.hint = Some(Hint { text: 281, half: 0x94, text_dx: 0x89 });
                }
            }
            _ => {}
        }
    }

    /// The reinforcements of missions 7, 12 and 17 when their countdown ends (`0x42ec3a..0x42f1fa`): credits, research,
    /// air forces and carryalls that land at fixed map points.
    fn reinforcements(&mut self) {
        // (credits, research, air stock in HEADER_ORDER, carryall drops, their squad `[+0x88]` set after the launch)
        let (credits, research, stock, drops, squad_no): (i32, i32, [u32; 5], &[(i32, i32)], usize) = match self.mission_no {
            Some(7) => (5000, 0, [0; 5], &[(0x8c0, 0x600), (0x9a0, 0x480)], 11),
            // 0x42ef3a..: six asteroids (push 0x22 x 6, `0x453e80`) and three bombers
            Some(12) => (10_000, 2500, [3, 6, 0, 0, 0], &[(0x600, 0x4c0), (0x640, 0x420), (0x600, 0x2e0)], 16),
            Some(17) => (5000, 5000, [0; 5], &[(0x9a0, 0x560), (0xb40, 0x660)], 10),
            _ => return,
        };
        // the exe adds the rewards without a cap (mines cap at 99999) and announces them: 13 always, 11 "5000 credits arrived" in
        // missions 7 and 17, 12 "5000 research points arrived" in mission 17 (`0x42ee0e`, `0x42f1d7`)
        self.say(0, msg::REINFORCEMENTS);
        self.credits[0] += credits;
        self.research[0] += research;
        match self.mission_no {
            Some(7) => self.say(0, msg::CREDITS_ARRIVED),
            Some(17) => {
                self.say(0, msg::CREDITS_ARRIVED);
                self.say(0, 12);
            }
            _ => {}
        }
        for g in 0..5 {
            self.air_stock[0][g] += stock[g];
        }
        for &(x, y) in drops {
            self.scripted_carryall(x, y, 0, squad_no);
        }
    }

    /// A carryall the mission scripts launch (`0x467ee0` for the local player, then `[+0x88]` = the squad and optionally the
    /// start x moved by `dx`): the global squad counter still advances, the squad it would have chosen is replaced.
    fn scripted_carryall(&mut self, x: i32, y: i32, dx: i32, squad_no: usize) {
        let mut a = Aircraft::launch(AirForce::Carryall, x, y, 0, self.cells.0, self.carry_counter);
        a.id = self.new_air_id();
        a.x += dx;
        a.squad = squad_no;
        self.carry_counter = launch_counter(self.carry_counter);
        self.stats[0][6] += 1;
        self.strike_lock = STRIKE_LOCK_TICKS; // every `0x467ee0` call of the local player
        self.aircraft.push(a);
    }

    /// The carryalls of a freshly started mission (level loader `0x41f140`, cases `0x41f3a2` and `0x41f34a`): mission 1 gets two
    /// (squad 12) at (0x620, 0x6e0) and (0x6e0, 0x720), their start x moved by -200 and +200; mission 4 one (squad 17: two
    /// walkers) at (0x360, 0xe0), moved by -300. Not run when a savegame is loaded.
    pub fn start_airlift(&mut self) {
        match self.mission_no {
            Some(1) => {
                self.scripted_carryall(0x620, 0x6e0, -200, 12);
                self.scripted_carryall(0x6e0, 0x720, 200, 12);
            }
            Some(4) => self.scripted_carryall(0x360, 0xe0, -300, 17),
            _ => {}
        }
    }
}

/// `rand(n)`: the exe's table when installed, else a xorshift generator.
fn draw(table: &mut Option<GameRng>, state: &mut u32, n: u32) -> u32 {
    if let Some(r) = table.as_mut() {
        return r.next(n.min(u16::MAX as u32) as u16) as u32;
    }
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state % n.max(1)
}

fn side_of(o: Owner) -> Option<usize> {
    match o {
        Owner::Player => Some(0),
        Owner::Enemy => Some(1),
        _ => None,
    }
}

fn side(o: Owner) -> usize {
    match o {
        Owner::Player => 0,
        Owner::Enemy => 1,
        _ => 2,
    }
}

fn centre(c: (i32, i32)) -> (f32, f32) {
    ((c.0 * CELL + CELL / 2) as f32, (c.1 * CELL + CELL / 2) as f32)
}

/// Does an enemy unit have `kind` in the AA list? (exposed for the renderer's range display)
pub fn is_anti_air(kind: i32) -> bool {
    ANTI_AIR.contains(&kind)
}

pub fn weapon_range(kind: i32) -> Option<(u32, u32)> {
    aim_params(kind).map(|p| (p.min_range, p.max_range))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_world(w: i32, h: i32) -> World {
        World::new((w, h), vec![false; (w * h) as usize])
    }

    /// The statistics screen's counters (docs/results.md): dead units are counted per side.
    #[test]
    fn statistics_count_lost_units() {
        let mut w = open_world(20, 20);
        w.add_unit(7, 0, 5 * CELL, 5 * CELL);
        w.add_unit(7, 1, 9 * CELL, 5 * CELL);
        w.add_unit(7, 1, 9 * CELL, 9 * CELL);
        w.units[1].hp = 0;
        w.units[2].hp = 0;
        w.step();
        assert_eq!(w.stats[1][3], 2, "two enemy units lost");
        assert_eq!(w.stats[0][3], 0);
    }

    /// Dev tools (`devtools.rs`): the cheats only touch what they promise.
    #[test]
    fn cheats_keep_units_alive_fill_the_purse_and_freeze_the_enemy() {
        let mut w = open_world(20, 20);
        w.add_unit(6, 0, 5 * CELL, 5 * CELL);
        w.add_unit(6, 1, 12 * CELL, 5 * CELL);
        w.units[0].hp = 0;
        w.units[1].hp = 0;
        w.step();
        assert_eq!(w.units.len(), 0, "without cheats both die");
        let mut w = open_world(20, 20);
        w.cheats = Cheats { invulnerable: true, rich: true, freeze_enemy: true };
        w.add_unit(6, 0, 5 * CELL, 5 * CELL);
        w.add_unit(6, 1, 12 * CELL, 5 * CELL);
        w.units[0].hp = 0;
        w.units[1].hp = 0;
        w.ai = true;
        w.step();
        assert_eq!(w.units.iter().find(|u| u.player == 0).map(|u| u.hp), Some(UnitKind::max_hp(6).unwrap()), "own unit restored");
        assert!(w.units.iter().all(|u| u.player == 0 || u.hp <= 0), "the enemy is not protected");
        assert_eq!((w.credits[0], w.research[0]), (CREDITS_CAP, RESEARCH_CAP));
        assert_eq!(w.ai_timer, 0, "the computer player did not run");
        w.cheats.freeze_enemy = false;
        w.step();
        assert_ne!(w.ai_timer, 0, "it runs again once thawed");
    }

    /// Unit destruction (docs/effects.md): a dying tank throws a piece of strength 40 with its top index as variant, a walker
    /// none, and every kind but the Advancer leaves a burning wreck that throws 20 smoke columns (0x45b0a0).
    #[test]
    fn dying_units_throw_pieces_and_leave_burning_wrecks() {
        let mut w = open_world(20, 20);
        w.add_unit(6, 0, 5 * CELL, 5 * CELL); // kind 6: tier 3 artillery -> top index 2 * 5 + 1
        w.units[0].hp = 0;
        w.step();
        let piece = w.bullets.iter().find(|b| b.bullet_type == 15).expect("a piece");
        assert_eq!((piece.strength, piece.variant, piece.index), (40, 11, 0));
        assert_eq!(w.wrecks.len(), 1);
        for _ in 0..400 {
            w.step();
        }
        assert!(w.wrecks.is_empty(), "removed after the last column");
        let mut w = open_world(20, 20);
        w.add_unit(43, 0, 5 * CELL, 5 * CELL);
        w.units[0].hp = 0;
        w.step();
        assert!(w.bullets.iter().all(|b| b.bullet_type != 15), "walkers leave no piece");
        assert_eq!(w.wrecks.len(), 1);
        let mut w = open_world(20, 20);
        w.add_unit(40, 0, 5 * CELL, 5 * CELL);
        w.units[0].hp = 0;
        w.step();
        assert!(w.wrecks.is_empty(), "the Advancer leaves no wreck");
    }

    /// Shell smoke (`effects::trail`): an anti-air missile (type 1) leaves a darkening puff of `missileSmoke` (mode 14) after 8 path
    /// steps and every 4 steps after, none on the landing step; a meteor (type 24) drops one at once.
    #[test]
    fn flying_shells_leave_smoke_trails() {
        let mut w = open_world(20, 20);
        let path = build_path(1, 100, 300, 400, 300, 0).unwrap();
        let steps = path.points.len() as i32;
        w.bullets.push(Bullet::new(1, 0, 0, path));
        let mut puffs = 0;
        for _ in 0..200 {
            w.step();
            puffs += w.effects.iter().filter(|e| e.spec.array == 0x5e6788 && e.age == 1).count();
            assert!(w.effects.iter().filter(|e| e.spec.array == 0x5e6788).all(|e| e.spec.mode == 14));
        }
        // first after 8 steps, then every 4, none on the last (the landing) step: 1 + (steps - 1 - 8) / 4
        assert_eq!(puffs as i32, 1 + (steps - 1 - 8) / 4, "steps {steps}");
        let mut w = open_world(20, 20);
        let path = build_path(24, 100, 300, 160, 300, 0).unwrap();
        w.bullets.push(Bullet::new(24, 0, 0, path));
        w.step();
        assert!(w.effects.iter().any(|e| e.spec.array == 0x5e9fb8), "the first meteor smoke comes with the first step");
    }

    /// 0x472a90: the AI's target classes come from a reachability wave, not from the straight distance.
    #[test]
    fn the_ai_picks_reachable_targets_only() {
        let mut w = open_world(20, 20);
        for y in 0..20 {
            w.blocked[(y * 20 + 4) as usize] = true; // a wall splits the map
        }
        let near_but_walled = w.add_unit(7, 0, 6 * CELL + 16, 10 * CELL + 16);
        let far_but_reachable = w.add_unit(7, 0, 2 * CELL + 16, 2 * CELL + 16);
        let (u, _, _) = w.nearest_enemy(1, (2, 10));
        assert_eq!(u, Some(Target::Unit(far_but_reachable)));
        assert_ne!(u, Some(Target::Unit(near_but_walled)));
        // with a gap in the wall the nearer one is met first
        w.blocked[(10 * 20 + 4) as usize] = false;
        w.occ = None; // the grids are built once per tick
        let (u, _, _) = w.nearest_enemy(1, (2, 10));
        assert_eq!(u, Some(Target::Unit(near_but_walled)));
    }

    /// Own buildings are skipped, everything else (enemy and neutral) is a target; turrets are their own class.
    #[test]
    fn the_scan_sorts_buildings_into_turrets_and_others_and_skips_own() {
        let mut w = open_world(20, 20);
        let mut own = factory(Owner::Enemy, 100, 101);
        own.cell = (5, 5);
        w.add_structure(own);
        let mut neutral = factory(Owner::Neutral, 100, 101);
        neutral.cell = (10, 5);
        w.add_structure(neutral);
        let (u, t, b) = w.nearest_enemy(1, (3, 5));
        assert_eq!((u, t), (None, None));
        assert_eq!(b, Some(Target::Structure(1)), "own building skipped, neutral one found");
        let (_, _, b) = w.nearest_enemy(0, (3, 5));
        assert_eq!(b, Some(Target::Structure(0)), "for the player the enemy building comes first");
    }

    /// `0x42ebd0` countdown rewards and `0x41f140` start carryalls: squads are set after the launch, mission 12 pays six asteroids.
    #[test]
    fn mission_scripts_launch_carryalls_with_their_own_squads() {
        let mut w = open_world(20, 20);
        w.mission_no = Some(12);
        w.reinforcements();
        assert_eq!(w.air_stock[0], [3, 6, 0, 0, 0], "3 bombers and 6 asteroids (HEADER_ORDER)");
        assert_eq!(w.aircraft.iter().map(|a| a.squad).collect::<Vec<_>>(), vec![16, 16, 16]);
        let mut w = open_world(20, 20);
        w.mission_no = Some(7);
        w.reinforcements();
        assert_eq!(w.aircraft.iter().map(|a| a.squad).collect::<Vec<_>>(), vec![11, 11]);
        let mut w = open_world(20, 20);
        w.mission_no = Some(17);
        w.reinforcements();
        assert_eq!(w.aircraft.iter().map(|a| a.squad).collect::<Vec<_>>(), vec![10, 10]);
        // the start carryalls: mission 1 two (squad 12), mission 4 one (squad 17), nothing elsewhere
        for (m, squads) in [(1, vec![12, 12]), (4, vec![17]), (2, vec![])] {
            let mut w = open_world(20, 20);
            w.mission_no = Some(m);
            w.start_airlift();
            assert_eq!(w.aircraft.iter().map(|a| a.squad).collect::<Vec<_>>(), squads, "mission {m}");
        }
    }

    /// 0x4303e0 / 0x430770: the tech-level hint of missions 1 and 6 and the fighter hint of mission 1.
    #[test]
    fn the_hint_boxes_of_missions_1_and_6_follow_the_exe() {
        let mk = |mission: u32| {
            let mut w = open_world(20, 20);
            w.mission_no = Some(mission);
            w.add_unit(7, 1, 18 * CELL, 18 * CELL);
            let mut f = factory(Owner::Player, 5000, 102);
            f.slots[0].max_level = 2;
            f.slots[1].max_level = 2;
            w.add_structure(f);
            w
        };
        let mut w = mk(1);
        w.update_hint();
        assert_eq!(w.hint, None, "needs 500 research points");
        w.research[0] = 500;
        w.update_hint();
        assert_eq!(w.hint, Some(Hint { text: 281, half: 0x94, text_dx: 0x89 }));
        w.structures[0].slots[0].level = 2;
        w.update_hint();
        assert_eq!(w.hint, Some(Hint { text: 281, half: 0xbc, text_dx: 0xb1 }), "one slot at its limit: wider box");
        w.structures[0].slots[1].level = 2;
        w.update_hint();
        assert_eq!(w.hint, None, "both at the limit, no fighters yet");
        w.air_stock[0][4] = 8;
        w.update_hint();
        assert_eq!(w.hint, Some(Hint { text: 282, half: 0xc1, text_dx: 0xb6 }));
        assert_eq!(w.hint_phase, 1);
        w.air_stock[0][4] = 7; // a fighter was used
        w.update_hint();
        assert_eq!((w.hint, w.hint_phase), (None, 2));
        // mission 6: only while no slot of any medium factory is at its limit
        let mut w = mk(6);
        w.research[0] = 600;
        w.update_hint();
        assert_eq!(w.hint, Some(Hint { text: 281, half: 0x94, text_dx: 0x89 }));
        w.structures[0].slots[1].level = 2;
        w.update_hint();
        assert_eq!(w.hint, None);
        let mut w = mk(2);
        w.research[0] = 600;
        w.update_hint();
        assert_eq!(w.hint, None, "other missions have no hint");
    }

    /// Runs the tutorial script frame by frame, clicking the arrow button whenever it waits.
    fn tut_run(w: &mut World, frames: usize) {
        for _ in 0..frames {
            if w.tutorial.as_ref().is_some_and(|t| t.arrow) {
                w.tutorial_arrow_clicked();
            }
            w.step();
        }
    }

    fn tut_step(w: &World) -> u32 {
        w.tutorial.as_ref().unwrap().step
    }

    /// 0x4325b0, cases 0..7 (docs/tutorial.md): arrow clicks, the scroll tasks, the Advancer, the training factory.
    #[test]
    fn the_tutorial_starts_with_the_scroll_lessons_and_the_advancer() {
        let mut w = open_world(64, 64);
        w.start_tutorial();
        w.add_unit(7, 1, 60 * CELL, 60 * CELL);
        // without a click on the arrow button the script halts at step 2
        for _ in 0..10 {
            w.step();
        }
        assert_eq!(tut_step(&w), 2, "0 -> 1 -> 2 at once (step 1 has no arrow), then it waits for the arrow");
        assert!(w.tutorial.as_ref().unwrap().arrow);
        for _ in 0..10 {
            w.step();
        }
        assert_eq!(tut_step(&w), 2);
        w.tutorial_arrow_clicked();
        w.step();
        assert!(!w.tutorial.as_ref().unwrap().arrow);
        assert_eq!(tut_step(&w), 2, "the task: scroll with the right mouse button");
        w.tutorial.as_mut().unwrap().rmb_scroll = true;
        w.step();
        assert_eq!(tut_step(&w), 3);
        tut_run(&mut w, 1); // arrow
        w.step();
        for k in 0..3 {
            w.tutorial.as_mut().unwrap().keys[k] = true;
        }
        w.step();
        assert_eq!(tut_step(&w), 3, "all four scroll keys are needed");
        w.tutorial.as_mut().unwrap().keys[3] = true;
        w.step();
        assert_eq!(tut_step(&w), 4);
        tut_run(&mut w, 3);
        assert_eq!(tut_step(&w), 5, "Lesson A: info, then the Advancer");
        tut_run(&mut w, 2);
        assert!(w.units.iter().any(|u| u.player == 0 && u.kind == 40 && u.cell() == (9, 46)), "the Advancer appears at (9, 46)");
        assert_eq!(w.camera_request, Some((0x120, 0x5c0)));
        // selected + go-to order -> step 6, then 7 builds the training light factory (neutral) and a fresh Advancer
        let adv = w.units.iter().find(|u| u.kind == 40).unwrap().id;
        w.units.iter_mut().find(|u| u.id == adv).unwrap().selected = true;
        w.order_move_selected(12 * CELL, 46 * CELL);
        tut_run(&mut w, 1);
        assert_eq!(tut_step(&w), 6);
        tut_run(&mut w, 4);
        assert_eq!(tut_step(&w), 7);
        w.step();
        assert_eq!(w.structures.len(), 1);
        assert_eq!((w.structures[0].kind_id, w.structures[0].owner, w.structures[0].cell), (101, Owner::Neutral, (8, 8)));
        assert_eq!(w.units.iter().filter(|u| u.player == 0).count(), 1, "the map was wiped; the new Advancer stands at (5, 9)");
        // capturing the factory moves on to step 8
        w.structures[0].owner = Owner::Player;
        tut_run(&mut w, 3);
        assert_eq!(tut_step(&w), 8);
    }

    /// The lessons B..E build their scenes and wait for their conditions (cases 10..27).
    #[test]
    fn the_tutorial_builds_the_scenes_and_ends_with_the_enemy_headquarters() {
        let mut w = open_world(64, 64);
        w.start_tutorial();
        w.ai = false;
        let set = |w: &mut World, step: u32| {
            let t = w.tutorial.as_mut().unwrap();
            t.step = step;
            t.entered = true;
            t.arrow = false;
        };
        // step 10: a damaged own light factory at (46, 39) to repair
        set(&mut w, 10);
        tut_run(&mut w, 1);
        assert_eq!(tut_step(&w), 10);
        let f = w.structures.iter().position(|s| s.kind_id == 101).unwrap();
        assert!(w.structures[f].hp < w.structures[f].max_hp / 2);
        w.structures[f].hp = w.structures[f].max_hp;
        tut_run(&mut w, 1);
        assert_eq!(tut_step(&w), 11);
        // step 12: gold mine, two enemy cannons, 120 credits; 200 credits -> 13, killing the cannons -> 14
        set(&mut w, 12);
        tut_run(&mut w, 1);
        assert_eq!((w.credits[0], w.units.iter().filter(|u| u.player == 1).count()), (120, 2));
        w.credits[0] = 200;
        tut_run(&mut w, 1);
        assert_eq!(tut_step(&w), 13);
        w.units.retain(|u| u.player == 0);
        tut_run(&mut w, 1);
        assert_eq!(tut_step(&w), 14);
        // step 15: air base and four enemy cannons; a fighter in stock -> 16; the icon -> 17; all enemies dead -> 18
        set(&mut w, 15);
        tut_run(&mut w, 1);
        assert_eq!((w.units.iter().filter(|u| u.player == 1).count(), w.credits), (4, [0, 0]));
        let fighter = HEADER_ORDER.iter().position(|&h| h == AirForce::Fighter).unwrap();
        w.air_stock[0][fighter] = 1;
        tut_run(&mut w, 1);
        assert_eq!(tut_step(&w), 16);
        w.tutorial.as_mut().unwrap().arrow = false;
        w.tutorial.as_mut().unwrap().air_icon = true;
        tut_run(&mut w, 1);
        assert_eq!(tut_step(&w), 17);
        assert!(!w.tutorial.as_ref().unwrap().arrow, "step 17 has no arrow");
        w.units.retain(|u| u.player == 0);
        tut_run(&mut w, 1);
        assert_eq!((tut_step(&w), w.air_stock[0]), (18, [0; 5]), "the air force icons are cleared");
        // lesson D: step 19 builds a neutral research lab and a medium factory with one tech-level to gain, then 20 at once
        set(&mut w, 19);
        tut_run(&mut w, 1);
        assert_eq!(w.research[0], 400);
        assert_eq!(tut_step(&w), 19);
        tut_run(&mut w, 1);
        assert_eq!(tut_step(&w), 20);
        w.tutorial.as_mut().unwrap().arrow = false;
        let lab = w.structures.iter().position(|s| s.kind_id == 106).unwrap();
        w.structures[lab].owner = Owner::Player;
        tut_run(&mut w, 1);
        assert_eq!(tut_step(&w), 21 + w.tut_buildings().iter().position(|&b| b == lab).unwrap() as u32);
        set(&mut w, 21);
        w.research[0] = 500;
        tut_run(&mut w, 1);
        assert_eq!(tut_step(&w), 22);
        w.tutorial.as_mut().unwrap().arrow = false;
        let mf = w.structures.iter().position(|s| s.kind_id == 102).unwrap();
        w.structures[mf].owner = Owner::Player;
        w.structures[mf].slots[0].level = 1;
        tut_run(&mut w, 1);
        assert!(tut_step(&w) >= 23);
        // lesson E: step 24 builds the bases; the enemy HQ gone -> 26, then 27 ends the mission with a victory
        set(&mut w, 24);
        tut_run(&mut w, 1);
        let ehq = w.structures.iter().position(|s| s.kind_id == 100 && s.owner == Owner::Enemy).unwrap();
        assert!(w.structures[ehq].hp < w.structures[ehq].max_hp);
        assert!(w.structures.iter().any(|s| s.kind_id == 100 && s.owner == Owner::Player) && w.units.iter().any(|u| u.kind == 40));
        assert_eq!(w.outcome, None, "no HQ rules inside the tutorial");
        w.structures[ehq].hp = 0;
        tut_run(&mut w, 1);
        assert_eq!(tut_step(&w), 26);
        tut_run(&mut w, 3);
        assert_eq!(w.outcome, Some(Outcome::Victory));
    }

    /// Step 8 jumps to `9 + index` of the last non-Advancer unit (case 0x4330e4): with the Advancer at index 0 the first produced
    /// unit (index 1) skips the upgrade lesson (step 9), as in the exe.
    #[test]
    fn tutorial_step_8_follows_the_exes_index_arithmetic() {
        let mut w = open_world(64, 64);
        w.start_tutorial();
        w.ai = false;
        w.add_unit(7, 1, 60 * CELL, 60 * CELL);
        {
            let t = w.tutorial.as_mut().unwrap();
            t.step = 8;
            t.arrow = false;
        }
        w.add_unit(40, 0, 5 * CELL, 9 * CELL);
        w.add_unit(7, 0, 6 * CELL, 9 * CELL);
        tut_run(&mut w, 1);
        assert_eq!(tut_step(&w), 10);
    }

    /// The global route is kept as it is (the exe only drops its first cell per planned step, `0x473190`) and a unit finishes
    /// the segment in flight when it gets a new order, so a crowd crossing open ground changes direction only a few times.
    #[test]
    fn a_crowd_in_open_ground_does_not_wobble() {
        let mut w = open_world(24, 24);
        let ids: Vec<u32> = (0..6).map(|i| w.add_unit(7, 0, (4 + i % 3) * CELL + 16, (4 + i / 3) * CELL + 16)).collect();
        for &id in &ids {
            w.order_move(id, (20, 20), None);
        }
        let mut last = vec![u32::MAX; ids.len()];
        let mut changes = vec![0; ids.len()];
        for t in 0..2500 {
            w.step();
            for (k, &id) in ids.iter().enumerate() {
                let c = w.unit(id).unwrap().chassis.as_ref().unwrap().target;
                if c != last[k] {
                    last[k] = c;
                    changes[k] += 1;
                }
            }
            // a new order every 7 ticks (the chase does this) must not make the units turn back mid-segment
            if t % 7 == 0 {
                for &id in &ids {
                    w.order_move(id, (20, 20), None);
                }
            }
        }
        assert!(changes.iter().all(|&c| c <= 6), "direction changes per unit: {changes:?}");
        assert!(ids.iter().all(|&id| w.unit(id).unwrap().cell().0 >= 15), "they got there: {:?}", ids.iter().map(|&id| { let u = w.unit(id).unwrap(); (u.cell(), u.step, u.path.len()) }).collect::<Vec<_>>());
    }

    /// A walking unit turns faster than a standing one: the walking step runs the turn counter once per pixel step and the
    /// tail once more (`0x46f65f`, `0x46f8b9`).
    #[test]
    fn a_walking_unit_turns_twice_per_tick_when_it_steps() {
        let mut w = open_world(14, 9);
        let id = w.add_unit(7, 0, 2 * CELL + 16, 4 * CELL + 16);
        w.order_move(id, (10, 4), None); // due east, the unit starts facing north
        let mut ticks_to_turn = 0;
        while w.unit(id).unwrap().chassis.as_ref().unwrap().facing != 8 {
            w.step();
            ticks_to_turn += 1;
            assert!(ticks_to_turn < 100);
        }
        // 8 turn steps at one per 5 counter ticks: 40 counter ticks; the walking step adds one on every tick that owes a pixel step
        assert!(ticks_to_turn < 40, "turned in {ticks_to_turn} ticks");
    }

    #[test]
    fn units_walk_around_walls() {
        let mut w = open_world(12, 12);
        for y in 0..9 {
            w.blocked[(y * 12 + 6) as usize] = true;
        }
        let id = w.add_unit(7, 0, 2 * CELL + 16, 2 * CELL + 16);
        w.order_move(id, (10, 2), None);
        let path = w.unit(id).unwrap().path.clone();
        assert!(path.len() >= 9, "must detour below the wall, got {path:?}");
        assert!(path.iter().all(|&(x, y)| !w.is_blocked((x as i32) / CELL, (y as i32) / CELL)));
        for _ in 0..3000 {
            w.step();
        }
        let u = w.unit(id).unwrap();
        assert_eq!(u.cell(), (10, 2));
    }

    #[test]
    fn a_destroyed_unit_makes_four_clouds_a_wreck_and_flying_pieces() {
        use opensow_formats::effects::{UNIT_CLOUD, UNIT_WRECK};
        let mut w = open_world(14, 9);
        let id = w.add_unit(7, 0, 5 * CELL + 16, 4 * CELL + 16);
        w.units.iter_mut().find(|u| u.id == id).unwrap().hp = 0;
        w.step();
        let clouds = |w: &World| w.effects.iter().filter(|e| e.spec == UNIT_CLOUD).count();
        assert_eq!(clouds(&w), 4);
        assert_eq!(w.effects.iter().filter(|e| e.spec == UNIT_WRECK).count(), 1);
        assert!(w.bullets.iter().any(|b| b.bullet_type == 15), "a cannon unit throws type 15 pieces");
        // the Advancer: clouds only
        let mut w = open_world(14, 9);
        let id = w.add_unit(40, 0, 5 * CELL + 16, 4 * CELL + 16);
        w.units.iter_mut().find(|u| u.id == id).unwrap().hp = 0;
        w.step();
        assert_eq!(clouds(&w), 4);
        assert_eq!(w.effects.iter().filter(|e| e.spec == UNIT_WRECK).count(), 0);
        assert!(w.bullets.is_empty());
    }

    #[test]
    fn a_bomb_dropped_by_an_eastbound_bomber_uses_the_right_sprite_variant() {
        let mut w = open_world(14, 9);
        w.air_stock[0][0] = 1;
        assert!(w.launch_strike(0, AirForce::Bomber, 8 * CELL, 4 * CELL));
        let right = w.aircraft[0].right;
        let mut seen = None;
        for _ in 0..2000 {
            w.step();
            if let Some(b) = w.bullets.iter().find(|b| b.bullet_type == 18) {
                seen = Some(b.variant);
                break;
            }
        }
        assert_eq!(seen, Some(right as u32));
    }

    /// `0x40fd30`: every one of the 34 ring points of a Tripler pod makes a small blast sprite and (free ground) two smoke puffs.
    #[test]
    fn a_tripler_pod_lands_with_a_ring_of_blasts() {
        let mut w = open_world(14, 9);
        let path = build_path(17, 5 * CELL, 2 * CELL, 5 * CELL, 4 * CELL, 0).unwrap();
        w.bullets.push(Bullet::new(17, 0, 0, path));
        let small = blast(1).array;
        for _ in 0..200 {
            w.step();
            if w.bullets.is_empty() {
                break;
            }
        }
        assert!(w.bullets.is_empty(), "the pod landed");
        let blasts = w.effects.iter().filter(|e| e.spec.array == small).count();
        assert_eq!(blasts, 1 + TRIPLER_RING_USED, "the impact's own blast plus one per ring point");
        assert!(w.effects.iter().filter(|e| e.spec.mode == 8).count() >= 2 * TRIPLER_RING_USED, "two puffs on every free ring point");
    }

    #[test]
    fn legs_move_while_a_unit_has_a_route_and_stop_at_rest() {
        let mut w = open_world(14, 9);
        let id = w.add_unit(7, 0, 2 * CELL + 16, 4 * CELL + 16);
        w.order_move(id, (10, 4), None);
        let mut phases = std::collections::HashSet::new();
        for _ in 0..200 {
            w.step();
            phases.insert(w.unit(id).unwrap().chassis.as_ref().unwrap().phase);
        }
        assert!(phases.len() > 1, "the walk phase advances on the way");
        for _ in 0..3000 {
            w.step();
        }
        assert!(w.unit(id).unwrap().path.is_empty());
        let rest = w.unit(id).unwrap().chassis.as_ref().unwrap().phase;
        for _ in 0..300 {
            w.step();
            assert_eq!(w.unit(id).unwrap().chassis.as_ref().unwrap().phase, rest, "no animation at rest");
        }
    }

    #[test]
    fn a_corpse_blocks_its_cell_for_430_steps_and_ends_in_blasts() {
        use opensow_formats::effects::{blast, CORPSE_LIFE};
        let mut w = open_world(14, 9);
        let id = w.add_unit(7, 0, 5 * CELL + 16, 4 * CELL + 16); // kind 7: tier 0 (spread 30), two size-2 blasts at the end
        w.units.iter_mut().find(|u| u.id == id).unwrap().hp = 0;
        w.step();
        assert_eq!(w.corpses.len(), 1);
        assert_eq!(w.corpses[0].left, CORPSE_LIFE - 1);
        assert_eq!(w.occupancy().unwrap().ground.cells[(5 * 128 + 4) as usize], 0xFFFFFD, "the corpse blocks its cell");
        let blasts = |w: &World| w.effects.iter().filter(|e| e.spec == blast(2)).count();
        let before = blasts(&w);
        for _ in 0..(CORPSE_LIFE - 31) {
            w.step();
        }
        assert_eq!(blasts(&w), before, "nothing before the timer is below 30");
        w.step();
        w.step();
        assert_eq!(blasts(&w), before + 2);
        for _ in 0..40 {
            w.step();
        }
        assert!(w.corpses.is_empty(), "the slot is freed after the timer runs out");
        assert_eq!(w.occupancy().unwrap().ground.cells[(5 * 128 + 4) as usize], 0);
    }

    #[test]
    fn units_do_not_walk_through_each_other() {
        // a one-cell-wide corridor with a unit standing in it: the walker behind it cannot pass, but in the open it goes around
        let mut w = open_world(14, 9);
        let blocker = w.add_unit(7, 0, 7 * CELL + 16, 4 * CELL + 16);
        let walker = w.add_unit(7, 0, 2 * CELL + 16, 4 * CELL + 16);
        w.order_move(walker, (12, 4), None);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..3000 {
            w.step();
            seen.insert(w.unit(walker).unwrap().cell());
            assert_ne!(w.unit(walker).unwrap().cell(), w.unit(blocker).unwrap().cell(), "walked into the other unit");
        }
        assert_eq!(w.unit(walker).unwrap().cell(), (12, 4), "goes around in the open");
        assert!(seen.iter().any(|c| c.1 != 4), "left the straight line to avoid the unit");
        // in a corridor it stops (the exe resets the goal to the unit's own position)
        let mut w = open_world(14, 9);
        for x in 0..14 {
            for y in [3, 5] {
                w.blocked[(y * 14 + x) as usize] = true;
            }
        }
        w.add_unit(7, 0, 7 * CELL + 16, 4 * CELL + 16);
        let walker = w.add_unit(7, 0, 2 * CELL + 16, 4 * CELL + 16);
        w.order_move(walker, (12, 4), None);
        for _ in 0..3000 {
            w.step();
        }
        assert!(w.unit(walker).unwrap().cell().0 < 7, "cannot pass the unit in the corridor");
    }

    #[test]
    fn a_walled_off_destination_moves_to_the_nearest_reachable_cell() {
        // 0x4774c0: two units plug a corridor (the local search finds no way around them); the destination behind them moves to
        // the nearest cell the walker can reach, the one in front of the plug
        let mut w = open_world(16, 9);
        for x in 0..16 {
            for y in [3, 5] {
                w.blocked[(y * 16 + x) as usize] = true;
            }
        }
        w.add_unit(7, 0, 8 * CELL + 16, 4 * CELL + 16);
        w.add_unit(7, 0, 9 * CELL + 16, 4 * CELL + 16);
        let walker = w.add_unit(7, 0, 2 * CELL + 16, 4 * CELL + 16);
        w.order_move(walker, (13, 4), None);
        for _ in 0..4000 {
            w.step();
        }
        assert_eq!(w.unit(walker).unwrap().cell(), (7, 4), "walks up to the plug instead of waiting at the start");
    }

    #[test]
    fn units_sent_to_one_cell_do_not_stack() {
        // the exe stamps the cell a unit is heading to (0x473190), so a second unit sees it taken and stops short
        let mut w = open_world(16, 9);
        let ids: Vec<u32> = (0..4).map(|k| w.add_unit(7, 0, (2 + k) * CELL + 16, (2 + 2 * k) * CELL + 16)).collect();
        for &id in &ids {
            w.order_move(id, (12, 4), None);
        }
        for _ in 0..3000 {
            w.step();
        }
        for (a, &ia) in ids.iter().enumerate() {
            for &ib in &ids[a + 1..] {
                let (ua, ub) = (w.unit(ia).unwrap(), w.unit(ib).unwrap());
                assert!((ua.x - ub.x).abs() + (ua.y - ub.y).abs() > 16.0, "units {ia} and {ib} stand on the same spot ({}, {})", ua.x, ua.y);
            }
        }
    }

    #[test]
    fn a_new_route_still_avoids_units_that_stopped_ahead() {
        // re-ordering a walking unit (the chase does it every 7 ticks) must plan the next segment around units again
        let mut w = open_world(16, 5);
        for x in 0..16 {
            for y in [0, 1, 3, 4] {
                w.blocked[(y * 16 + x) as usize] = true;
            }
        }
        let walker = w.add_unit(7, 0, 2 * CELL + 16, 2 * CELL + 16);
        let blocker = w.add_unit(7, 0, 8 * CELL + 16, 2 * CELL + 16);
        for t in 0..2000 {
            if t % 7 == 0 {
                w.order_move(walker, (13, 2), None);
            }
            w.step();
            let (a, b) = (w.unit(walker).unwrap(), w.unit(blocker).unwrap());
            assert!((a.x - b.x).abs() + (a.y - b.y).abs() > 16.0, "walked into the unit at tick {t}");
        }
    }

    #[test]
    fn a_pod_plans_around_units_like_every_other_ground_unit() {
        // the exe's movement code has no branch on kind 46 (docs/walking.md): in a one-cell corridor a pod queues behind a
        // unit that stands in its way instead of walking through it
        let mut w = open_world(16, 5);
        for x in 0..16 {
            for y in [0, 1, 3, 4] {
                w.blocked[(y * 16 + x) as usize] = true;
            }
        }
        let pod = w.add_unit(46, 0, 2 * CELL + 16, 2 * CELL + 16);
        let blocker = w.add_unit(7, 0, 8 * CELL + 16, 2 * CELL + 16);
        for t in 0..2000 {
            if t % 7 == 0 {
                w.order_move(pod, (13, 2), None);
            }
            w.step();
            let (a, b) = (w.unit(pod).unwrap(), w.unit(blocker).unwrap());
            assert!((a.x - b.x).abs() + (a.y - b.y).abs() > 16.0, "the pod walked into the unit at tick {t}");
        }
    }

    #[test]
    fn a_walking_tank_keeps_its_top_on_its_heading_and_an_idle_one_looks_around() {
        let mut w = open_world(16, 16);
        let id = w.add_unit(7, 0, 2 * CELL + 16, 2 * CELL + 16);
        assert_eq!(w.unit(id).unwrap().aim.as_ref().unwrap().facing, 0);
        w.order_move(id, (12, 2), None); // due east = facing 8
        for _ in 0..300 {
            w.step();
        }
        let u = w.unit(id).unwrap();
        assert_eq!(u.chassis.as_ref().unwrap().facing, 8);
        assert_eq!(u.aim.as_ref().unwrap().facing, 8, "the top follows the chassis while walking");
        // standing still it looks around by itself within 240 + 900 ticks (once the walk is over)
        let mut seen = std::collections::HashSet::new();
        for _ in 0..20000 {
            w.step();
            seen.insert(w.unit(id).unwrap().aim.as_ref().unwrap().facing);
        }
        assert!(seen.len() > 3, "idle top turns to random facings: {seen:?}");
        assert_eq!(w.unit(id).unwrap().chassis.as_ref().unwrap().facing, 8, "the hull stays");
    }

    #[test]
    fn the_advancer_flies_over_the_ground_and_always_spins() {
        let mut w = open_world(12, 12);
        for y in 0..12 {
            w.blocked[(y * 12 + 6) as usize] = true;
        }
        let id = w.add_unit(40, 0, 2 * CELL + 16, 2 * CELL + 16);
        let f0 = w.unit(id).unwrap().chassis.as_ref().unwrap().facing;
        for _ in 0..30 {
            w.step();
        }
        assert_ne!(w.unit(id).unwrap().chassis.as_ref().unwrap().facing, f0, "spins while idle");
        w.order_move(id, (10, 2), None);
        // the air layer's wave (0x474b30) knows only the border terrain: the ground wall is no obstacle
        assert!(w.unit(id).unwrap().path.len() >= 8, "a route of cells, not one leg");
        for _ in 0..1000 {
            w.step();
        }
        assert_eq!(w.unit(id).unwrap().cell(), (10, 2));
    }

    #[test]
    fn the_advancer_goes_around_no_fly_areas_of_the_air_layer() {
        let mut w = open_world(12, 12);
        // a no-fly wall in the border layer with a gap at the bottom; the ground map is open
        for y in 0..9 {
            w.air_blocked[(y * 12 + 6) as usize] = true;
        }
        let id = w.add_unit(40, 0, 2 * CELL + 16, 2 * CELL + 16);
        w.order_move(id, (10, 2), None);
        let mut deepest = 0;
        for _ in 0..2000 {
            w.step();
            deepest = deepest.max(w.unit(id).unwrap().cell().1);
        }
        assert_eq!(w.unit(id).unwrap().cell(), (10, 2));
        assert!(deepest >= 9, "flew through the gap, not across the wall (deepest cell row {deepest})");
        // a beam approach (a type-2 order) is a straight line whatever the layers hold
        w.order_fly_straight(id, (2, 2));
        assert_eq!(w.unit(id).unwrap().path.len(), 1);
    }

    #[test]
    fn a_factory_produces_a_unit_every_22_seconds() {
        let mut w = open_world(12, 12);
        w.tutorial_step = Some(1); // tutorial speed: 22 s per ground unit
        w.add_unit(10, 0, 16, 16); // keeps the player alive (no defeat)
        let slots = vec![SlotState { kind: 7, progress: 0, target: 2000, level: 0, max_level: 0, waiting: 0 }];
        w.add_structure(Structure { object: 0, kind: ObjectKind::LightFactory, owner: Owner::Player, cell: (5, 5), hp: 100, max_hp: 100, aim: None, weapon: 0, target: None, slots, active: 1, kind_id: 101, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None, dying: None });
        for _ in 0..1300 {
            w.step();
        }
        assert_eq!(w.units.len(), 1, "90 steps/s: 2000 steps need 1334 ticks");
        for _ in 0..100 {
            w.step();
        }
        assert_eq!(w.units.len(), 2);
        assert_eq!((w.units[1].kind, w.units[1].player), (7, 0));
    }

    #[test]
    fn factory_units_leave_by_the_bottom_right_exit_else_the_top_left_one_else_wait() {
        // exits are tested in the static grid: only terrain and buildings block them, units never do
        let build = |blockers: &[(i32, i32)], unit_on_exit: bool| {
            let mut w = open_world(14, 14);
            w.tutorial_step = Some(1);
            w.add_unit(10, 0, 16, 16);
            for &(x, y) in blockers {
                let i = w.cell_index(x, y).unwrap();
                w.blocked[i] = true;
            }
            if unit_on_exit {
                w.add_unit(10, 0, 8 * CELL + 16, 8 * CELL + 16);
            }
            let slots = vec![SlotState { kind: 7, progress: 0, target: 2000, level: 0, max_level: 0, waiting: 0 }];
            w.add_structure(Structure { object: 0, kind: ObjectKind::LightFactory, owner: Owner::Player, cell: (5, 5), hp: 100, max_hp: 100, aim: None, weapon: 0, target: None, slots, active: 1, kind_id: 101, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None, dying: None });
            w
        };
        // the order the exe gives: a straight walk to the exit cell (type 2), then a route two cells further (type 1)
        let order = |w: &World| w.units.iter().find(|u| u.kind == 7).map(|u| (u.path.first().map(|&(x, y)| ((x as i32).div_euclid(CELL), (y as i32).div_euclid(CELL))), u.straight));
        let mut w = build(&[], true); // a unit standing on the exit does not matter
        for _ in 0..1340 {
            w.step();
        }
        assert_eq!(order(&w), Some((Some((8, 8)), Some((8, 10)))));
        let mut w = build(&[(8, 8)], false);
        for _ in 0..1340 {
            w.step();
        }
        assert_eq!(order(&w), Some((Some((4, 4)), Some((4, 2)))));
        let mut w = build(&[(8, 8), (4, 4)], false);
        for _ in 0..1340 + 240 {
            w.step();
        }
        assert!(w.units.iter().all(|u| u.kind != 7));
        assert!(w.log.iter().any(|&(_, id)| id == msg::NO_SPACE));
    }

    #[test]
    fn factory_units_walk_on_to_the_rally_point_and_a_new_order_replaces_it() {
        let build = |owner: Owner, rally: Option<(i32, i32)>| {
            let mut w = open_world(16, 16);
            w.tutorial_step = Some(1);
            w.add_unit(10, 0, 16, 16);
            let slots = vec![SlotState { kind: 7, progress: 0, target: 2000, level: 0, max_level: 0, waiting: 0 }];
            w.add_structure(Structure { object: 0, kind: ObjectKind::LightFactory, owner, cell: (5, 5), hp: 100, max_hp: 100, aim: None, weapon: 0, target: None, slots, active: 1, kind_id: 101, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None, dying: None });
            if let Some((x, y)) = rally {
                assert!(w.set_rally(0, if owner == Owner::Player { 0 } else { 1 }, x, y));
            }
            w
        };
        let cannon = |w: &World| w.units.iter().find(|u| u.kind == 7).map(|u| u.cell());
        // the point snaps to the cell centre, and the unit ends on it after the exit orders
        let mut w = build(Owner::Player, Some((13 * CELL + 3, 4 * CELL + 30)));
        assert_eq!(w.structures[0].rally, Some((13 * CELL + 16, 4 * CELL + 16)));
        for _ in 0..1340 + 1500 {
            w.step();
        }
        let u = w.units.iter().find(|u| u.kind == 7).unwrap();
        assert_eq!(u.px(), (13 * CELL + 16, 4 * CELL + 16), "reached the rally point");
        // without a point it stops two cells beyond the exit
        let mut w = build(Owner::Player, None);
        for _ in 0..1340 + 1500 {
            w.step();
        }
        assert_eq!(cannon(&w), Some((8, 10)));
        // an order given on the way replaces the queued rally order
        let mut w = build(Owner::Player, Some((13 * CELL + 16, 4 * CELL + 16)));
        for _ in 0..1340 + 60 {
            w.step();
        }
        let id = w.units.iter().find(|u| u.kind == 7).unwrap().id;
        w.units.iter_mut().find(|u| u.id == id).unwrap().selected = true;
        w.order_move_selected(2 * CELL + 16, 12 * CELL + 16);
        for _ in 0..2000 {
            w.step();
        }
        assert_eq!(cannon(&w), Some((2, 12)));
        // only the player's factories use it, and a capture clears it
        let mut w = build(Owner::Enemy, None);
        w.structures[0].rally = Some((13 * CELL + 16, 4 * CELL + 16));
        for _ in 0..1340 + 1500 {
            w.step();
        }
        assert!(w.units.iter().filter(|u| u.player == 1).all(|u| u.cell() != (13, 4)));
        // points outside the map are skipped
        let mut w = build(Owner::Player, None);
        w.structures[0].rally = Some((16 * CELL + 1, 4 * CELL));
        for _ in 0..1340 + 1500 {
            w.step();
        }
        assert_eq!(cannon(&w), Some((8, 10)));
        // other building kinds cannot be given one
        let mut w = build(Owner::Player, None);
        w.structures[0].kind_id = 104;
        assert!(!w.set_rally(0, 0, 64, 64));
    }

    #[test]
    fn a_flamer_bullet_leaves_a_trail_of_puffs() {
        // 0x4118e2: types 8 and 9 drop a blast_verysmall tail puff every 6th path step, none on the landing step
        let mut w = open_world(20, 20);
        let path = opensow_formats::projectile::build_path(9, 100, 100, 160, 100, 0).unwrap(); // 60 steps
        w.bullets.push(Bullet::new(9, 0, 0, path));
        for _ in 0..200 {
            w.step();
            if w.bullets.is_empty() {
                break;
            }
        }
        let at: Vec<i32> = w.effects.iter().filter(|e| e.spec.array == 0x5e6230).map(|e| e.x).collect();
        assert_eq!(at, vec![106, 112, 118, 124, 130, 136, 142, 148, 154], "puffs every 6 px, none at the end x=160");
    }

    #[test]
    fn a_type_27_bullet_leaves_blue_sparks_every_fifth_step() {
        // 0x411bc2: bullet type 27 (1S, walker 5W1) drops a bodky_b spark animation (frames 4..6, period 1, x3) every 5 path steps
        let mut w = open_world(20, 20);
        let path = opensow_formats::projectile::build_path(27, 100, 100, 160, 100, 0).unwrap();
        w.bullets.push(Bullet::new(27, 0, 0, path));
        let mut seen = Vec::new();
        for _ in 0..200 {
            w.step();
            seen.extend(w.effects.iter().filter(|e| e.spec.array == 0x5e6bd0 && e.age == 1).map(|e| e.x));
            if w.bullets.is_empty() {
                break;
            }
        }
        assert!(seen.len() >= 8, "sparks along the path: {seen:?}");
        assert!(seen.windows(2).all(|p| p[1] - p[0] == 5), "every 5 px: {seen:?}");
    }

    #[test]
    fn the_advancer_gun_beam_starts_at_the_sprite_and_plays_tt_fire() {
        // the spawner starts at position + hover offset + the top's -16 (not the half-height aim point) and the constructor plays id 56
        let mut w = open_world(20, 20);
        w.add_unit(7, 1, 19 * CELL, 19 * CELL);
        let a = w.add_unit(40, 0, 5 * CELL + 16, 5 * CELL + 16);
        let v = w.add_unit(1, 1, 9 * CELL + 16, 5 * CELL + 16);
        let mut start = None;
        for _ in 0..200 {
            w.step();
            if w.sounds.iter().any(|s| s.0 == GUN_SOUND) {
                start = w.sounds.iter().find(|s| s.0 == GUN_SOUND).map(|s| (s.1, s.2));
                break;
            }
        }
        let (fx, fy) = w.unit(a).unwrap().flyer_points().1;
        let (sx, sy) = start.expect("tt-fire plays");
        assert!((sx - fx).abs() <= 3 && (sy - (fy - 16)).abs() <= 3, "start {sx},{sy} vs sprite {fx},{fy}");
        assert!(w.unit(v).is_some());
    }

    #[test]
    fn the_advancer_gun_draws_a_blue_beam_for_the_player_and_a_green_one_for_the_enemy() {
        // 0x411b0c: bullet type 31 drops a bodky_b (player 0) / bodky_g (other sides) spark animation every 2 path steps
        for (shooter, victim, array) in [(0, 1, 0x5e6bd0u32), (1, 0, 0x5e6c70)] {
            let mut w = open_world(20, 20);
            w.add_unit(7, 1 - shooter, 19 * CELL, 19 * CELL); // keeps the mission from ending
            w.add_unit(40, shooter, 5 * CELL + 16, 5 * CELL + 16);
            let v = w.add_unit(1, victim, 9 * CELL + 16, 5 * CELL + 16);
            let hp = w.unit(v).unwrap().hp;
            let mut sparks = 0;
            for _ in 0..200 {
                w.step();
                sparks += w.effects.iter().filter(|e| e.age == 1 && e.spec.array == array).count();
                if w.unit(v).map_or(true, |u| u.hp < hp) {
                    break;
                }
            }
            assert!(sparks > 20, "shooter {shooter}: {sparks} sparks along the 4-cell beam");
            assert!(w.unit(v).map_or(true, |u| u.hp < hp), "and the shot lands");
        }
    }

    /// Walker 1 fires two shots per volley: shot k starts at `.po1` table k's point and ends at the target plus the `.po2` point
    /// minus that point (`0x47d0a5..0x47d0cb`), so the two bolts are parallel and a barrel apart.
    #[test]
    fn a_two_barrel_volley_flies_two_parallel_bolts() {
        let table = |f: &dyn Fn(usize) -> (i16, i16)| -> Vec<u8> { (0..32).flat_map(|i| { let (x, y) = f(i); [x.to_le_bytes(), y.to_le_bytes()].concat() }).collect() };
        // raw points: barrel 0 at (-6, 0), barrel 1 at (6, 0) for every facing (muzzle = -raw)
        let mut po1 = table(&|_| (-6, 0));
        po1.extend(table(&|_| (6, 0)));
        let mut w = open_world(30, 30);
        w.turret_muzzles.insert(41, opensow_formats::fire::FireTables::parse(&po1).unwrap());
        w.turret_muzzles.insert(REFERENCE_KEY + 41, opensow_formats::fire::FireTables::parse(&table(&|_| (0, 0))).unwrap());
        w.add_unit(41, 0, 5 * CELL + 16, 10 * CELL + 16);
        w.add_unit(7, 1, 11 * CELL + 16, 10 * CELL + 16);
        let mut starts = Vec::new();
        let mut ends = Vec::new();
        for _ in 0..200 {
            w.step();
            let volley: Vec<&Bullet> = w.bullets.iter().filter(|b| b.bullet_type == 27).collect();
            if volley.len() >= 2 {
                for b in volley.iter().take(2) {
                    starts.push(b.path.points[0]);
                    ends.push(*b.path.points.last().unwrap());
                }
                break;
            }
        }
        assert!(starts.len() >= 2, "two bolts in the first volley, got {}", starts.len());
        let (a, b) = (starts[0], starts[1]);
        assert_eq!((a.0 - b.0).abs(), 12, "the barrels are 12 px apart: {a:?} {b:?}");
        assert_eq!(ends[0].0 - ends[1].0, a.0 - b.0, "the bolts end as far apart as they start (parallel)");
    }

    /// The volley spawner plays the weapon's muzzle sound for ground units and walkers too (`sfx::muzzle`): walker 1 = effect 16.
    #[test]
    fn firing_units_raise_their_muzzle_sound() {
        for (kind, want) in [(41, 16u8), (43, 17), (45, 18), (7, 4), (4, 1)] {
            let mut w = open_world(30, 30);
            w.add_unit(kind, 0, 5 * CELL + 16, 10 * CELL + 16);
            w.add_unit(7, 1, 5 * CELL + 16 + if kind == 45 { CELL / 2 } else { 4 * CELL }, 10 * CELL + 16);
            let mut heard = false;
            for _ in 0..400 {
                w.step();
                heard |= w.sounds.iter().any(|s| s.0 == want);
                w.sounds.clear();
            }
            assert!(heard, "kind {kind} should play effect {want}");
        }
    }

    /// An attack order beats the regular target priority (`0x47b140`): a unit in range of both an enemy unit and the building it
    /// was ordered to attack shoots the building.
    #[test]
    fn units_shoot_what_they_were_ordered_to_attack_when_it_is_in_range() {
        let mut w = open_world(30, 30);
        let a = w.add_unit(7, 0, 5 * CELL + 16, 10 * CELL + 16);
        let enemy = w.add_unit(7, 1, 8 * CELL + 16, 10 * CELL + 16);
        w.add_structure(Structure { object: 0, kind: ObjectKind::LightFactory, owner: Owner::Enemy, cell: (9, 12), hp: 4000, max_hp: 4000, aim: None, weapon: 0, target: None, slots: Vec::new(), active: 0, kind_id: 101, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None, dying: None });
        w.cheats.freeze_enemy = true;
        w.units.iter_mut().find(|u| u.id == a).unwrap().selected = true;
        // no order: the nearest unit is the target
        w.step();
        assert_eq!(w.unit(a).unwrap().target, Some(Target::Unit(enemy)));
        // ordered onto the building (in range, 4 cells away): it is shot at although the unit is closer
        w.order_attack_selected(Target::Structure(0));
        for _ in 0..40 {
            w.step();
            assert_eq!(w.unit(a).map(|u| u.target), Some(Some(Target::Structure(0))), "tick {}", w.tick);
        }
    }

    #[test]
    fn a_unit_in_range_of_both_prefers_its_order_target_over_the_priority_rule() {
        let mut w = open_world(30, 30);
        w.add_unit(7, 1, 8 * CELL + 16, 10 * CELL + 16);
        let far = w.add_unit(7, 1, 11 * CELL + 16, 10 * CELL + 16);
        let me = (5, 10);
        let index = w.index();
        assert_eq!(w.find_target(&index, 0, me, 0, 8, Some(Target::Unit(far))), Some(Target::Unit(far)));
        assert_ne!(w.find_target(&index, 0, me, 0, 8, None), Some(Target::Unit(far)));
        // out of range (a 6-cell weapon): the nearest unit again
        assert_ne!(w.find_target(&index, 0, me, 0, 4, Some(Target::Unit(far))), Some(Target::Unit(far)));
    }

    #[test]
    fn a_cannon_kills_a_unit_in_range() {
        let mut w = open_world(20, 20);
        w.add_unit(7, 0, 5 * CELL + 16, 5 * CELL + 16); // cannon tier 1, range 0..6 cells
        w.add_unit(10, 1, 9 * CELL + 16, 5 * CELL + 16); // enemy flamer, 4 cells away (outside its own range 4? 4 is inside)
        for _ in 0..4000 {
            w.step();
        }
        // the cannon (20 dmg per hit, 180 hp) beats the flamer's 3-damage hits at range; at least one side died
        assert!(w.units.iter().filter(|u| u.player == 1).count() == 0 || w.units.iter().filter(|u| u.player == 0).count() == 0);
    }

    #[test]
    fn units_attack_neutral_and_enemy_buildings() {
        for owner in [Owner::Neutral, Owner::Enemy] {
            let mut w = open_world(20, 20);
            w.add_unit(9, 0, 5 * CELL + 16, 5 * CELL + 16);
            w.add_structure(Structure { object: 0, kind: ObjectKind::LightFactory, owner, cell: (8, 5), hp: 400, max_hp: 400, aim: None, weapon: 0, target: None, slots: Vec::new(), active: 0, kind_id: 101, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None, dying: None });
            for _ in 0..600 {
                w.step();
            }
            assert!(w.structures[0].hp < 400 || w.structures[0].owner == Owner::Player, "{owner:?} building untouched");
        }
    }

    #[test]
    fn an_attack_order_chases_a_neutral_building() {
        let mut w = open_world(30, 30);
        let u = w.add_unit(7, 0, 2 * CELL + 16, 2 * CELL + 16);
        w.add_structure(Structure { object: 0, kind: ObjectKind::LightFactory, owner: Owner::Neutral, cell: (25, 25), hp: 400, max_hp: 400, aim: None, weapon: 0, target: None, slots: Vec::new(), active: 0, kind_id: 101, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None, dying: None });
        w.units.iter_mut().find(|x| x.id == u).unwrap().selected = true;
        w.order_attack_selected(Target::Structure(0));
        for _ in 0..3 {
            w.step();
        }
        assert!(w.unit(u).unwrap().chase.is_some(), "the chase must survive");
        for _ in 0..6000 {
            w.step();
        }
        assert!(w.structures[0].hp < 400 || w.structures[0].owner != Owner::Neutral);
    }

    /// Regression: shooting a neutral or enemy building down captures it for the shooter instead of destroying it.
    #[test]
    fn shooting_a_building_down_captures_it() {
        for owner in [Owner::Neutral, Owner::Enemy] {
            let mut w = open_world(20, 20);
            w.add_unit(7, 1, 18 * CELL, 18 * CELL); // keeps the mission from ending
            w.add_unit(7, 0, 5 * CELL + 16, 5 * CELL + 16);
            let mut b = factory(owner, 300, 101);
            b.cell = (8, 5);
            w.add_structure(b);
            for _ in 0..3000 {
                w.step();
            }
            let s = &w.structures[0];
            assert!(s.alive(), "{owner:?} building was destroyed instead of captured");
            assert_eq!(s.owner, Owner::Player, "{owner:?} building not captured");
        }
    }

    /// The building goes to whoever dealt most damage; every source (units, turrets, air units, nukes) goes through `explode`
    /// with its owner, the Advancer through its beam tally.
    #[test]
    fn the_biggest_damage_dealer_takes_a_contested_building() {
        for (blue, green, winner) in [(300, 100, Owner::Player), (100, 300, Owner::Enemy), (250, 120, Owner::Player)] {
            let mut w = open_world(20, 20);
            w.add_unit(7, 0, 1 * CELL, 1 * CELL);
            w.add_unit(7, 1, 18 * CELL, 18 * CELL);
            let mut b = factory(Owner::Neutral, blue + green, 101);
            b.cell = (8, 8);
            w.add_structure(b);
            let (x, y) = (8 * CELL + 16, 8 * CELL + 16);
            w.explode(x, y, 2, blue, 0);
            w.explode(x, y, 2, green, 1);
            settle(&mut w);
            let s = &w.structures[0];
            assert_eq!(s.owner, winner, "blue {blue} green {green}");
        }
    }

    /// The map's no-marks layer (`+0x2c0cc`) keeps scorch decals and single puffs away (`0x410459`, `0x4106ce`); a cell outside
    /// it takes them. Type 15 hits make the three lockstep puffs wherever the ground is free.
    #[test]
    fn no_marks_cells_get_no_decals_or_single_puffs() {
        let mut w = open_world(20, 20);
        w.no_marks = vec![false; 400];
        w.no_marks[5 * 20 + 5] = true;
        let puffs_before = w.effects.len();
        w.impact_ground_effects(5 * 32 + 8, 5 * 32 + 8, 3);
        assert!(w.decals.is_empty(), "no decal on a no-marks cell");
        assert_eq!(w.effects.len(), puffs_before, "type 3 never makes puffs");
        w.impact_ground_effects(6 * 32 + 8, 6 * 32 + 8, 3);
        assert_eq!(w.decals.len(), 1);
        // type 18: two puffs on an open cell, none on a no-marks one
        let n = w.effects.len();
        w.impact_ground_effects(5 * 32 + 8, 5 * 32 + 8, 18);
        assert_eq!(w.effects.len(), n);
        w.impact_ground_effects(7 * 32 + 8, 7 * 32 + 8, 18);
        assert_eq!(w.effects.len(), n + 2);
        // type 15 on the no-marks cell: no single puff, but the roll-1 hits still make their three
        let mut counts = std::collections::BTreeSet::new();
        for _ in 0..40 {
            let n = w.effects.len();
            w.impact_ground_effects(5 * 32 + 8, 5 * 32 + 8, 15);
            counts.insert(w.effects.len() - n);
        }
        assert_eq!(counts, [0, 3].into_iter().collect(), "only the roll-1 hits puff on a no-marks cell");
    }

    /// `0x458930`: a building's quake lasts 0x57 / 2 = 43 ticks of steps (2..=9 px each axis, magnitude shrinking with the
    /// distance to the screen centre), then the steps are undone one per tick; a turret's lasts 15; QUAKE off does nothing.
    #[test]
    fn a_far_away_destruction_does_not_shake_the_screen() {
        let mut w = open_world(8, 8);
        w.quake.start(QUAKE_BUILDING, 4000, 4000);
        for _ in 0..60 {
            assert_eq!(w.quake_step(true, (0, 0), (640, 480)), None);
        }
        assert!(w.quake.stack.is_empty());
    }

    #[test]
    fn quake_shakes_then_undoes_every_step() {
        for (duration, steps) in [(QUAKE_BUILDING, 43usize), (QUAKE_TURRET, 15)] {
            let mut w = open_world(20, 20);
            w.quake.start(duration, 320, 240);
            let mut sum = (0, 0);
            let mut ticks = 0;
            while let Some((dx, dy)) = w.quake_step(true, (0, 0), (640, 480)) {
                sum = (sum.0 + dx, sum.1 + dy);
                ticks += 1;
                if ticks <= steps {
                    assert!(dx.abs() >= 2 && dx.abs() <= 2 + 3 && dy.abs() >= 2 && dy.abs() <= 2 + 3, "{dx} {dy}"); // distance 0: 9 / 2 = 4 -> r in 0..4
                }
            }
            assert_eq!(ticks, 2 * steps);
            assert_eq!(sum, (0, 0), "the camera comes back");
        }
        let mut w = open_world(20, 20);
        w.quake.start(QUAKE_BUILDING, 0, 0);
        assert_eq!(w.quake_step(false, (0, 0), (640, 480)), None);
        assert_eq!(w.quake_step(true, (0, 0), (640, 480)), None, "switching it off forgets the shake");
    }

    /// The exe has friendly fire on buildings (0x42f2c0 has no owner check): a shell of the captor that lands on the freshly
    /// rebuilt 1-hp building destroys it again, and with no foreign tally it goes to player 1 (0x43ac2f).
    #[test]
    fn friendly_shots_hit_buildings_as_in_the_exe() {
        let mut w = open_world(20, 20);
        w.add_unit(7, 1, 18 * CELL, 18 * CELL);
        let mut b = factory(Owner::Neutral, 100, 101);
        b.cell = (8, 8);
        w.add_structure(b);
        let (x, y) = (8 * CELL + 16, 8 * CELL + 16);
        w.explode(x, y, 2, 100, 0);
        settle(&mut w);
        assert_eq!(w.structures[0].owner, Owner::Player, "captured");
        w.explode(x, y, 2, 100, 0); // a second shell of the same volley lands on the 1-hp rebuild
        settle(&mut w);
        assert_eq!(w.structures[0].owner, Owner::Enemy, "friendly fire destroyed it; nobody else's damage -> player 1");
    }

    #[test]
    fn a_building_nobody_damaged_goes_to_the_green_side() {
        let mut w = open_world(20, 20);
        w.add_unit(7, 0, 1 * CELL, 1 * CELL);
        w.add_unit(7, 1, 18 * CELL, 18 * CELL);
        for owner in [Owner::Neutral, Owner::Player, Owner::Enemy] {
            let mut b = factory(owner, 0, 101);
            b.cell = (8, 8);
            b.tally = [0, 0, 0];
            w.structures.clear();
            w.add_structure(b);
            w.structure_destroyed(0);
            settle(&mut w);
            assert_eq!(w.structures[0].owner, Owner::Enemy, "{owner:?}");
            assert!(w.structures[0].hp > 0);
        }
    }

    #[test]
    fn a_destroyed_building_throws_a_blast_and_six_debris_bullets_on_random_curves() {
        let mut w = open_world(20, 20);
        let table: Vec<u8> = (0..opensow_formats::rng::TABLE_LEN as u32 * 2).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        w.set_game_rng(GameRng::from_file(&table).unwrap());
        w.add_unit(7, 0, 1 * CELL, 1 * CELL);
        let mut b = factory(Owner::Player, 0, 101);
        b.cell = (8, 8);
        w.add_structure(b);
        w.structure_destroyed(0);
        let types: Vec<u8> = w.bullets.iter().map(|b| b.bullet_type).collect();
        assert_eq!(types, [14, 14, 14, 13, 13, 13]);
        let (bw, bh) = w.structures[0].size();
        let start = w.bullets[0].path.points[0];
        // the burst point lies inside the footprint (10 px margin), the curves start next to it
        assert!(w.bullets.iter().all(|b| b.strength == 80 && b.speed >= 30));
        assert!(start.0 > 8 * CELL - 160 && start.0 < (8 + bw) * CELL + 160 && start.1 > 8 * CELL - 160 && start.1 < (8 + bh) * CELL + 160);
        assert!(w.effects.iter().any(|e| e.spec == BUILDING_DESTROYED), "the e_bubg blast");
        assert!(w.sounds.iter().any(|s| s.0 == 25));
    }

    #[test]
    fn a_captured_building_and_a_destroyed_turret_both_burst_and_the_burst_spares_the_rebuild() {
        let mut w = open_world(20, 20);
        let table: Vec<u8> = (0..opensow_formats::rng::TABLE_LEN as u32 * 2).map(|i| (i.wrapping_mul(2246822519) >> 11) as u8).collect();
        w.set_game_rng(GameRng::from_file(&table).unwrap());
        w.add_unit(7, 0, 1 * CELL, 1 * CELL);
        let mut b = factory(Owner::Enemy, 0, 101);
        b.cell = (8, 8);
        b.tally = [500, 0, 0]; // the player did the damage: capture
        w.add_structure(b);
        w.structures[0].hp = 0;
        w.structure_destroyed(0);
        assert_eq!(w.structures[0].owner, Owner::Enemy, "the dead building stays where it was until its animation ends");
        assert_eq!(w.bullets.len(), 6);
        settle(&mut w);
        assert_eq!(w.structures[0].owner, Owner::Player, "captured");
        assert_eq!(w.structures[0].hp, 1, "the bursts' own blasts ran while the building was dead, so the rebuild keeps its hit point");
        assert_eq!(w.bullets.len(), 24, "four bursts of six");
        // a turret: removed, but it still throws the burst with the turret blast
        let mut w = open_world(20, 20);
        w.set_game_rng(GameRng::from_file(&table).unwrap());
        w.add_unit(7, 0, 1 * CELL, 1 * CELL);
        let mut t = factory(Owner::Enemy, 0, 25);
        t.cell = (8, 8);
        t.kind = ObjectKind::Turret(1);
        w.add_structure(t);
        w.structures[0].hp = 0;
        w.structure_destroyed(0);
        assert_eq!(w.bullets.len(), 6);
        assert!(w.effects.iter().any(|e| e.spec == TURRET_DESTROYED));
        assert!(w.structures[0].present() && !w.structures[0].alive());
        settle(&mut w);
        assert_eq!(w.bullets.len(), 6, "a turret bursts once");
        assert!(!w.structures[0].present());
    }

    #[test]
    fn a_unit_leaving_a_cell_frees_it_for_units_planning_later_in_the_same_tick() {
        let mut w = open_world(12, 12);
        let id = w.add_unit(7, 0, 3 * CELL + 31, 3 * CELL + 16);
        let i = w.units.iter().position(|u| u.id == id).unwrap();
        let own = 0x3000000 + id;
        let at = |w: &mut World, c: (i32, i32)| w.occupancy().unwrap().ground.cells[(c.0 * 128 + c.1) as usize];
        assert_eq!(at(&mut w, (3, 3)), own, "the grid holds the unit's cell");
        w.units[i].x += 1.0; // one pixel step across the cell border
        w.occupancy_moved(i, (3, 3));
        assert_eq!(at(&mut w, (3, 3)), 0, "the cell it left is free at once");
        assert_eq!(at(&mut w, (4, 3)), own, "and the new one is its own");
    }

    /// Runs the destruction animations to their end; returns the passes it took.
    fn settle(w: &mut World) -> usize {
        let mut passes = 0;
        while w.structures.iter().any(|s| s.dying.is_some()) {
            for i in 0..w.structures.len() {
                w.destruction_pass(i);
            }
            passes += 1;
            assert!(passes < 2000);
        }
        passes
    }

    #[test]
    fn the_destruction_animation_takes_four_bursts_for_a_building_and_one_for_a_turret() {
        let table: Vec<u8> = (0..opensow_formats::rng::TABLE_LEN as u32 * 2).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        for (kind, bursts) in [(101, 4), (100, 4), (25, 1), (29, 1)] {
            let mut w = open_world(20, 20);
            w.set_game_rng(GameRng::from_file(&table).unwrap());
            w.add_unit(7, 0, 1 * CELL, 1 * CELL);
            let mut b = factory(Owner::Enemy, 0, kind);
            b.cell = (8, 8);
            if kind < 30 {
                b.kind = ObjectKind::Turret((kind - 24) as u8);
            }
            w.add_structure(b);
            w.structure_destroyed(0);
            let passes = settle(&mut w);
            assert_eq!(w.bullets.len(), 6 * bursts, "kind {kind}");
            // each gap is 90..=104 passes: the end comes after `bursts` gaps
            assert!((90 * bursts..=104 * bursts).contains(&passes), "kind {kind}: {passes} passes");
        }
    }

    /// An enemy factory at (8, 8) with `n` player units next to it.
    fn turret_world(n: usize, credits: i32, flags: [bool; 5]) -> World {
        let mut w = open_world(24, 24);
        w.add_unit(7, 0, 1 * CELL, 1 * CELL);
        let mut b = factory(Owner::Enemy, 100, 101);
        b.cell = (8, 8);
        w.add_structure(b);
        for i in 0..n {
            w.add_unit(7, 0, (6 + i as i32) * CELL + 16, 12 * CELL + 16);
        }
        w.credits[1] = credits;
        w.turret_flags[1] = flags;
        w
    }

    fn turrets(w: &World) -> Vec<(i32, (i32, i32))> {
        w.structures.iter().filter(|s| matches!(s.kind, ObjectKind::Turret(_))).map(|s| (s.kind_id, s.cell)).collect()
    }

    #[test]
    fn the_computer_builds_only_the_turret_tiers_its_level_allows() {
        // four player units around the factory: threat 4; 2000 credits want a tier 5 turret
        let mut w = turret_world(4, 2000, [false; 5]);
        w.ai_build_turret(1);
        assert!(turrets(&w).is_empty(), "no flag, no turret (levels 1, 3, 4, 7, 8)");
        let mut w = turret_world(4, 2000, [true, false, false, false, false]);
        w.ai_build_turret(1);
        assert_eq!(turrets(&w).iter().map(|t| t.0).collect::<Vec<_>>(), [25], "5 -> 4 -> 3 -> 1 down to the one allowed tier");
        assert_eq!(w.credits[1], 1800);
        let mut w = turret_world(4, 2000, [false, false, false, false, true]);
        w.ai_build_turret(1);
        assert_eq!(turrets(&w).iter().map(|t| t.0).collect::<Vec<_>>(), [29]);
        assert_eq!(w.credits[1], 1000);
        // tier 3 falls to tier 1, skipping tier 2; tier 2 alone never steps down
        let mut w = turret_world(4, 1200, [true, true, false, false, false]); // 1001..1500: threat 4 wants tier 3
        w.ai_build_turret(1);
        assert_eq!(turrets(&w).iter().map(|t| t.0).collect::<Vec<_>>(), [25]);
        let mut w = turret_world(0, 1200, [true, true, true, true, true]); // threat 0: nothing is wanted
        w.ai_build_turret(1);
        assert!(turrets(&w).is_empty());
    }

    #[test]
    fn thresholds_follow_the_credit_bands() {
        // (credits, units around the factory) -> the tier the exe asks for with every flag set
        for (credits, units, tier) in [(500, 2, None), (500, 3, Some(1)), (500, 5, Some(1)), (700, 5, Some(3)), (900, 6, Some(4)), (900, 7, Some(4)), (1000, 7, Some(5)), (1200, 1, None), (1200, 2, Some(1)), (1200, 4, Some(3)), (1200, 5, Some(4)), (1200, 6, Some(5)), (3000, 2, Some(3)), (3000, 4, Some(5))] {
            let mut w = turret_world(units, credits, [true; 5]);
            w.ai_build_turret(1);
            let got = turrets(&w).first().map(|t| t.0 - 24);
            assert_eq!(got, tier, "{credits} credits, threat {units}");
        }
    }

    #[test]
    fn the_turret_stands_on_one_of_the_constructors_eight_spots() {
        // a light factory (3x3, first blocked row 1) at (8, 8)
        let spots = [(10, 7), (12, 9), (12, 10), (10, 12), (8, 12), (6, 10), (6, 9), (8, 7)];
        let mut w = turret_world(3, 500, [true; 5]);
        w.ai_build_turret(1);
        let t = turrets(&w);
        assert_eq!(t.len(), 1);
        assert!(spots.contains(&t[0].1), "{:?}", t);
    }

    #[test]
    fn a_dead_units_slot_counts_for_the_threat_and_walkers_count_double() {
        let mut w = turret_world(0, 500, [true; 5]);
        w.corpses.push(Corpse { x: 9 * CELL, y: 12 * CELL, left: 100, variant: 0, air: false, blasted: false, player: 0, kind: 43 });
        w.corpses.push(Corpse { x: 9 * CELL, y: 12 * CELL, left: 100, variant: 0, air: false, blasted: false, player: 0, kind: 7 });
        w.ai_build_turret(1); // 2 + 1 = 3
        assert_eq!(turrets(&w).iter().map(|t| t.0).collect::<Vec<_>>(), [25]);
    }

    #[test]
    fn an_advancer_over_the_building_makes_the_computer_build_anti_air() {
        let mut w = turret_world(0, 500, [true; 5]);
        let a = w.structures[1.min(w.structures.len() - 1)].action_cell();
        let a = if w.structures.len() > 1 { a } else { w.structures[0].action_cell() };
        w.add_unit(40, 0, a.0 * CELL + 16, a.1 * CELL + 16);
        w.ai_build_turret(1); // threat 1 (the Advancer), free 6: free wins, 500 credits -> tier 2 (kind 26)
        assert_eq!(turrets(&w).iter().map(|t| t.0).collect::<Vec<_>>(), [26]);
    }

    #[test]
    fn handle_slots_are_reused_and_decide_ties_between_buildings() {
        // two enemy factories: a freed slot is refilled first, and a tie between buildings goes to the lower slot
        let mut w = open_world(40, 24);
        w.add_unit(7, 0, 1 * CELL, 1 * CELL);
        for x in [4, 12] {
            let mut b = factory(Owner::Enemy, 100, 101);
            b.cell = (x, 8);
            w.add_structure(b);
        }
        assert_eq!(w.slot_tables[1], [Some(0), Some(1)]);
        w.slot_free(1, 0);
        let mut b = factory(Owner::Enemy, 100, 101);
        b.cell = (24, 8);
        w.add_structure(b);
        assert_eq!(w.slot_tables[1], [Some(2), Some(1)], "the hole at slot 0 is refilled");
        // the same number of player units next to the factory in slot 0 (cell 24) and the one in slot 1 (cell 12)
        for cx in [25, 13] {
            for i in 0..4 {
                w.add_unit(7, 0, (cx + i % 2) * CELL + 16, 12 * CELL + 16);
            }
        }
        w.credits[1] = 500;
        w.turret_flags[1] = [true; 5];
        w.ai_build_turret(1);
        let t = w.structures.iter().find(|s| matches!(s.kind, ObjectKind::Turret(_))).expect("a turret");
        assert!(t.cell.0 > 18, "next to the factory in slot 0 (cell 24), not the one in slot 1 (cell 12): {:?}", t.cell);
    }

    #[test]
    fn units_of_both_sides_contest_a_neutral_building() {
        for (blue, green, winner) in [(1, 3, Owner::Enemy), (3, 1, Owner::Player)] {
            let mut w = open_world(24, 12);
            for k in 0..blue {
                w.add_unit(7, 0, 8 * CELL + 16, (3 + 2 * k) * CELL + 16);
            }
            for k in 0..green {
                w.add_unit(7, 1, 17 * CELL + 16, (3 + 2 * k) * CELL + 16);
            }
            let mut b = factory(Owner::Neutral, 2500, 101);
            b.cell = (12, 5);
            b.max_hp = 2500;
            w.add_structure(b);
            for _ in 0..6000 {
                w.step();
                if w.structures[0].owner != Owner::Neutral {
                    break;
                }
            }
            assert_eq!(w.structures[0].owner, winner, "blue {blue} vs green {green}, hp {} tally {:?}", w.structures[0].hp, w.structures[0].tally);
        }
    }

    #[test]
    fn anti_air_does_not_shoot_ground() {
        let mut w = open_world(20, 20);
        let a = w.add_unit(1, 0, 5 * CELL + 16, 5 * CELL + 16);
        let e = w.add_unit(7, 1, 7 * CELL + 16, 5 * CELL + 16);
        w.units.iter_mut().find(|u| u.id == e).unwrap().hp = 100000;
        for _ in 0..600 {
            w.step();
        }
        assert_eq!(w.unit(e).unwrap().hp, 100000, "anti-air weapons ignore ground targets");
        assert!(w.unit(a).is_some());
    }

    /// The ground search (0x47b140) reads grid 0x5eac38, the flying Advancer lives in the air grid 0x60ac38 that only
    /// anti-air shooters search (0x47ad30).
    #[test]
    fn only_anti_air_units_shoot_the_advancer() {
        for (kind, hit) in [(7, false), (1, true)] {
            let mut w = open_world(20, 20);
            let _g = w.add_unit(kind, 0, 5 * CELL + 16, 5 * CELL + 16);
            let adv = w.add_unit(40, 1, 7 * CELL + 16, 5 * CELL + 16);
            let hp = w.unit(adv).unwrap().hp;
            for _ in 0..600 {
                w.step();
                if w.unit(adv).map_or(true, |u| u.hp < hp) {
                    break;
                }
            }
            let hurt = w.unit(adv).map_or(true, |u| u.hp < hp);
            assert_eq!(hurt, hit, "kind {kind}");
        }
    }

    /// `0x47b3c0`: anti-air range is `range * 50` px against aircraft, so a tier-1 anti-air unit (0-9) engages a bomber 400 px away
    /// (the port used to stop at 9.5 cells of 32 px).
    #[test]
    fn anti_air_engages_aircraft_out_to_fifty_pixels_per_range_cell() {
        let mut w = open_world(40, 12);
        let aa = w.add_unit(1, 0, 20 * CELL + 16, 5 * CELL + 16);
        w.air_stock[1][0] = 1;
        assert!(w.launch_strike(1, AirForce::Bomber, 30 * CELL, 5 * CELL + 16));
        let (ux, _) = w.unit(aa).unwrap().px();
        w.aircraft[0].x = ux - 420; // flying right, towards the unit
        let mut first = None;
        for t in 0..60 {
            w.step();
            if first.is_none() && w.air_targets.contains_key(&(false, aa as usize)) {
                first = Some((t, ux - w.aircraft[0].x));
            }
        }
        let (_, d) = first.expect("the bomber was picked as a target");
        assert!(d > 320, "picked {d} px away, beyond the old 304 px");
    }

    /// `0x42f30e`: the area damage skips flyers, so a bomb does not hurt the hovering Advancer standing under it.
    #[test]
    fn bombs_do_not_hurt_the_hovering_advancer() {
        let mut w = open_world(14, 9);
        let adv = w.add_unit(40, 0, 5 * CELL + 16, 4 * CELL + 16);
        let tank = w.add_unit(7, 0, 6 * CELL, 4 * CELL + 16);
        let (hp_a, hp_t) = (w.unit(adv).unwrap().hp, w.unit(tank).unwrap().hp);
        w.explode(5 * CELL + 16, 4 * CELL + 16, 18, 110, 1);
        assert_eq!(w.unit(adv).unwrap().hp, hp_a);
        assert_eq!(w.unit(tank).unwrap().hp, hp_t - 110, "ground units within 32 px take the bomb's 110");
    }

    #[test]
    fn the_anti_air_turret_shoots_an_enemy_advancer() {
        let mut w = open_world(20, 20);
        let mut t = factory(Owner::Player, 1, 0);
        t.kind = ObjectKind::Turret(2);
        t.cell = (5, 5);
        t.weapon = 110;
        t.aim = TopAim::new(110, 0);
        t.max_hp = 1000;
        t.hp = 1000;
        w.structures.clear();
        w.add_structure(t);
        let adv = w.add_unit(40, 1, 7 * CELL + 16, 5 * CELL + 16);
        let hp = w.unit(adv).unwrap().hp;
        for _ in 0..600 {
            w.step();
            if w.unit(adv).map_or(true, |u| u.hp < hp) {
                return;
            }
        }
        panic!("the turret never hit the Advancer");
    }

    #[test]
    fn the_anti_air_turret_launches_a_missile_that_hits_its_target_by_handle_when_it_lands() {
        let mut w = open_world(20, 20);
        let mut t = factory(Owner::Player, 1, 0);
        t.kind = ObjectKind::Turret(2);
        t.kind_id = 26;
        t.cell = (5, 5);
        t.weapon = 110;
        t.aim = TopAim::new(110, 0);
        t.max_hp = 1000;
        t.hp = 1000;
        w.structures.clear();
        w.add_structure(t);
        let adv = w.add_unit(40, 1, 7 * CELL + 16, 5 * CELL + 16);
        let hp = w.unit(adv).unwrap().hp;
        for _ in 0..600 {
            w.step();
            if let Some(b) = w.bullets.iter().find(|b| b.bullet_type == 1) {
                // the strength is taken off the handle's unit when the bullet lands (0x4108bc), not at the launch
                assert_eq!((b.strength, b.target), (15, Some(HitTarget::Unit(adv))));
                assert_eq!(w.unit(adv).unwrap().hp, hp);
                for _ in 0..200 {
                    w.step();
                }
                assert!(w.unit(adv).map_or(true, |u| u.hp < hp), "the missile hit");
                return;
            }
        }
        panic!("no missile was launched");
    }

    #[test]
    fn a_shot_at_an_advancer_meets_its_hovering_sprite() {
        let mut w = open_world(20, 20);
        let id = w.add_unit(40, 1, 8 * CELL + 16, 8 * CELL + 16);
        let u = w.unit(id).unwrap();
        let (aim, shot) = u.flyer_points();
        let h = u.sprite_y().round() as i32 - u.px().1;
        assert!(h < -40, "the Advancer hovers well above the ground point");
        assert_eq!(shot, (u.px().0, u.px().1 + h));
        assert_eq!(aim, (u.px().0, u.px().1 + h / 2));
    }

    #[test]
    fn a_turret_shell_starts_at_the_muzzle_point_of_the_table() {
        // a synthetic `.po1` table: every facing's stored point is (0, 30), so the muzzle is 30 px above the top's anchor
        let table: Vec<u8> = (0..32).flat_map(|_| [0i16.to_le_bytes(), 30i16.to_le_bytes()].concat()).collect();
        let mut w = open_world(20, 20);
        w.turret_muzzles.insert(109, opensow_formats::fire::FireTables::parse(&table).unwrap());
        let mut t = factory(Owner::Player, 1, 0);
        t.kind = ObjectKind::Turret(1);
        t.kind_id = 25;
        t.cell = (5, 5);
        t.weapon = 109;
        t.aim = TopAim::new(109, 0);
        t.max_hp = 1000;
        t.hp = 1000;
        w.structures.clear();
        w.add_structure(t);
        w.add_unit(46, 1, 7 * CELL + 16, 8 * CELL + 16);
        let want = (5 * CELL + 16, 5 * CELL + 16 - 11 - 30);
        for _ in 0..900 {
            w.step();
            if let Some(b) = w.bullets.first() {
                assert_eq!(b.path.points.first().map(|p| (p.0 - want.0).abs() <= 2 && (p.1 - want.1).abs() <= 2), Some(true), "first point {:?}, muzzle {want:?}", b.path.points.first());
                return;
            }
        }
        panic!("the turret never fired");
    }

    /// The Advancer's gun top (0x47a8af; flyer search 0x47aed0 then 0x47ad30) fires at enemy ground anti-air units and
    /// enemy Advancers, but not at other ground units or the anti-air turret's kind.
    #[test]
    fn the_advancer_shoots_anti_air_units_and_advancers_only() {
        for (kind, victim_is_adv, hit) in [(1, false, true), (2, false, true), (7, false, false), (110, false, false), (40, true, true)] {
            let mut w = open_world(20, 20);
            let _a = w.add_unit(40, 0, 5 * CELL + 16, 5 * CELL + 16);
            let v = w.add_unit(kind, 1, 8 * CELL + 16, 5 * CELL + 16);
            assert_eq!(victim_is_adv, kind == 40);
            let hp = w.unit(v).unwrap().hp;
            for _ in 0..900 {
                w.step();
                if w.unit(v).map_or(true, |u| u.hp < hp) {
                    break;
                }
            }
            assert_eq!(w.unit(v).map_or(true, |u| u.hp < hp), hit, "victim kind {kind}");
        }
    }

    #[test]
    fn advancers_and_nuclear_vehicles_are_selected_alone() {
        let mut w = open_world(20, 20);
        let a = w.add_unit(40, 0, 3 * CELL + 16, 2 * CELL + 16);
        let n = w.add_unit(46, 0, 4 * CELL + 16, 2 * CELL + 16);
        w.add_unit(7, 0, 5 * CELL + 16, 2 * CELL + 16);
        w.add_unit(8, 0, 6 * CELL + 16, 2 * CELL + 16);
        // box and select-all take only regular units
        assert_eq!(w.select_rect(0, 0, 10 * CELL, 4 * CELL, false), 2);
        assert!(!w.unit(a).unwrap().selected && !w.unit(n).unwrap().selected);
        w.select_all();
        assert_eq!(w.units.iter().filter(|u| u.selected).count(), 2);
        // clicking one selects it alone, even additively; a regular unit added afterwards replaces it
        assert_eq!(w.select_at(3 * CELL + 16, w.unit(a).unwrap().sprite_y() as i32, 10.0, true), 1);
        assert!(w.unit(a).unwrap().selected);
        assert_eq!(w.select_at(5 * CELL + 16, 2 * CELL + 16, 10.0, true), 1);
        assert!(!w.unit(a).unwrap().selected);
        assert_eq!(w.select_at(4 * CELL + 16, w.unit(n).unwrap().sprite_y() as i32, 10.0, true), 1);
        assert!(w.unit(n).unwrap().selected);
    }

    #[test]
    fn a_box_also_selects_enemy_units_but_orders_ignore_them() {
        let mut w = open_world(20, 20);
        let own = w.add_unit(7, 0, 3 * CELL + 16, 2 * CELL + 16);
        let foe = w.add_unit(7, 1, 5 * CELL + 16, 2 * CELL + 16);
        let adv = w.add_unit(40, 1, 6 * CELL + 16, 2 * CELL + 16);
        assert_eq!(w.select_rect(0, 0, 10 * CELL, 4 * CELL, false), 2);
        assert!(w.unit(own).unwrap().selected && w.unit(foe).unwrap().selected && !w.unit(adv).unwrap().selected);
        w.order_move_selected(8 * CELL, 8 * CELL);
        assert!(w.unit(foe).unwrap().path.is_empty(), "an enemy in the selection takes no order");
        // a click on an enemy never selects it (`0x417a0f`: owner test)
        assert_eq!(w.select_at(5 * CELL + 16, 2 * CELL + 16, 10.0, false), 0);
    }

    #[test]
    fn selection_and_group_orders_spread_units() {
        let mut w = open_world(20, 20);
        for k in 0..4 {
            w.add_unit(7, 0, (2 + k) * CELL + 16, 2 * CELL + 16);
        }
        assert_eq!(w.select_rect(0, 0, 10 * CELL, 4 * CELL, false), 4);
        w.order_move_selected(10 * CELL + 16, 10 * CELL + 16);
        let mut ends: Vec<_> = w.units.iter().map(|u| u.path.last().copied().unwrap()).collect();
        ends.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ends.dedup();
        assert_eq!(ends.len(), 4, "each unit gets its own cell");
    }

    #[test]
    fn destroying_the_enemy_hq_wins() {
        let mut w = open_world(20, 20);
        w.add_unit(9, 0, 5 * CELL + 16, 5 * CELL + 16);
        w.add_structure(Structure { object: 0, kind: ObjectKind::Headquarters, owner: Owner::Enemy, cell: (8, 5), hp: 50, max_hp: 50, aim: None, weapon: 0, target: None, slots: Vec::new(), active: 0, kind_id: 100, tally: [0; 3], heal: 0, upgrade: 0, protected: true, attacked: false, sat_cd: 0, advancer: None, rally: None, dying: None });
        for _ in 0..4000 {
            w.step();
        }
        assert_eq!(w.outcome, Some(Outcome::Victory));
    }

    fn factory(owner: Owner, hp: i32, kind_id: i32) -> Structure {
        let slots = vec![SlotState { kind: 7, progress: 0, target: 2000, level: 0, max_level: 0, waiting: 0 }, SlotState { kind: 13, progress: 0, target: 2000, level: 0, max_level: 0, waiting: 0 }];
        Structure { object: 0, kind: ObjectKind::LightFactory, owner, cell: (6, 5), hp, max_hp: 13536, aim: None, weapon: 0, target: None, slots, active: 1, kind_id, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None, dying: None }
    }

    #[test]
    fn the_advancer_captures_an_enemy_building_and_it_heals_in() {
        let mut w = open_world(16, 12);
        w.add_structure(factory(Owner::Enemy, 400, 101));
        w.add_unit(7, 1, 15 * CELL, 11 * CELL); // keeps the mission from ending
        let id = w.add_unit(40, 0, 3 * CELL, 5 * CELL + 16);
        assert!(w.order_action(id, 0));
        for _ in 0..500 {
            w.step();
        }
        // the dead building stays where it was while its animation runs (about 3 s), then changes hands
        assert!(w.structures[0].dying.is_some() || w.structures[0].owner == Owner::Player);
        for _ in 0..400 {
            w.step();
        }
        let s = &w.structures[0];
        assert_eq!(s.owner, Owner::Player, "captured");
        assert!(s.hp > 1000 && s.heal < s.max_hp, "hp {} heal {}", s.hp, s.heal);
        w.step();
        for _ in 0..300 {
            w.step();
        }
        assert_eq!(w.structures[0].hp, 13536, "fully healed");
    }

    #[test]
    fn a_second_advancer_stops_next_to_the_one_working_on_the_building() {
        // the order is a normal go-to (`0x46b8b0` -> `0x4706a0` type 1): the cell above the building is taken, so the late
        // Advancer ends beside the first one and never shoots (the original behaves the same)
        let mut w = open_world(16, 12);
        w.add_structure(factory(Owner::Enemy, 13000, 101));
        w.add_unit(7, 1, 15 * CELL, 11 * CELL);
        let a = w.add_unit(40, 0, 3 * CELL, 5 * CELL + 16);
        let b = w.add_unit(40, 0, 3 * CELL, 8 * CELL);
        assert!(w.order_action(a, 0));
        for _ in 0..600 {
            w.step();
        }
        let cell = w.structures[0].action_cell();
        assert_eq!(w.unit(a).unwrap().cell(), cell);
        assert!(w.order_action(b, 0));
        for _ in 0..900 {
            w.step();
        }
        let (ua, ub) = (w.unit(a).unwrap(), w.unit(b).unwrap());
        assert_eq!(ua.cell(), cell, "the first Advancer keeps the spot");
        assert_ne!(ub.cell(), cell, "the second one cannot enter it");
        assert!(ub.action.is_none(), "and its order lapses, it does not shoot");
    }

    #[test]
    fn the_advancer_upgrades_and_repairs_own_buildings() {
        let mut w = open_world(16, 12);
        w.add_structure(factory(Owner::Player, 13000, 101));
        let id = w.add_unit(40, 0, 3 * CELL, 5 * CELL + 16);
        assert!(w.order_action(id, 0));
        for _ in 0..1200 {
            w.step();
        }
        assert_eq!(w.structures[0].active, 2, "second slot unlocked after 2000 points at 40 per shot");
        w.repair_mode = true;
        assert!(w.order_action(id, 0));
        for _ in 0..400 {
            w.step();
        }
        assert_eq!(w.structures[0].hp, 13536);
    }

    #[test]
    fn destroyed_turrets_vanish_instead_of_changing_hands() {
        let mut w = open_world(16, 12);
        let mut t = factory(Owner::Enemy, 100, 26);
        t.slots.clear();
        t.active = 0;
        w.add_structure(t);
        w.add_unit(7, 1, 15 * CELL, 11 * CELL);
        let id = w.add_unit(40, 0, 3 * CELL, 5 * CELL + 16);
        assert!(w.order_action(id, 0));
        for _ in 0..500 {
            w.step();
        }
        assert!(!w.structures[0].alive());
        assert_eq!(w.structures[0].owner, Owner::Enemy);
    }

    #[test]
    fn satellite_protection_kills_a_foreign_advancer() {
        let mut w = open_world(16, 12);
        let mut f = factory(Owner::Enemy, 13000, 102);
        f.protected = true;
        w.add_structure(f);
        w.add_unit(7, 1, 15 * CELL, 11 * CELL);
        let id = w.add_unit(40, 0, 3 * CELL, 5 * CELL + 16);
        assert!(w.order_action(id, 0));
        let mut laser = 0;
        for _ in 0..600 {
            w.step();
            laser = laser.max(w.bullets.iter().filter(|b| b.bullet_type == 10).count());
        }
        assert!(w.unit(id).is_none(), "the Advancer is destroyed");
        assert_eq!(laser, 1, "one satellite laser was fired");
        assert!(w.bullets.iter().all(|b| b.bullet_type != 10), "and it has landed without doing anything else");
        assert_eq!(w.structures[0].owner, Owner::Enemy);
    }

    #[test]
    fn mines_and_labs_earn_every_second_and_tech_levels_cost_research() {
        let mut w = open_world(16, 12);
        let mut mine = factory(Owner::Player, 13536, 105);
        mine.kind = ObjectKind::GoldMine;
        mine.cell = (2, 2);
        mine.active = 2;
        w.add_structure(mine);
        let mut f = factory(Owner::Player, 13536, 102);
        f.slots[0].max_level = 2;
        w.add_structure(f);
        w.add_unit(7, 0, 8 * CELL, 8 * CELL);
        w.add_unit(7, 1, 15 * CELL, 11 * CELL);
        for _ in 0..130 {
            w.step();
        }
        assert_eq!(w.credits[0], 6, "3 credits per second at two levels, paid on ticks 60 and 120");
        w.research[0] = 1200;
        assert!(w.tech_up(1, 0, 0));
        assert_eq!((w.structures[1].slots[0].kind, w.research[0]), (8, 700));
        assert!(w.tech_up(1, 0, 0));
        assert!(!w.tech_up(1, 0, 0), "level limit reached");
        assert_eq!(w.structures[1].slots[0].kind, 9);
        assert!(!w.place_turret(0, 3, (12, 7)), "turret 3 costs 600, only 6 credits");
        w.credits[0] = 700;
        assert!(!w.place_turret(0, 3, (14, 9)), "too far from every own building");
        w.no_build = vec![false; (w.cells.0 * w.cells.1) as usize];
        w.no_build[(7 * w.cells.0 + 12) as usize] = true;
        assert!(!w.place_turret(0, 3, (12, 7)), "the map's no-build layer refuses the cell");
        w.no_build.clear();
        assert!(w.place_turret(0, 3, (12, 7)));
        assert_eq!(w.credits[0], 100);
        for _ in 0..30 {
            w.step();
        }
        let t = w.structures.last().unwrap();
        assert_eq!((t.hp, t.max_hp), (1540, 1540), "healed in at 150 per tick");
    }

    #[test]
    fn an_air_base_fills_the_stock_and_a_bomber_strike_hits_a_turret() {
        let mut w = open_world(40, 12);
        w.tutorial_step = Some(1); // fast production: 44 s per air strike
        let mut base = factory(Owner::Player, 13536, 104);
        base.kind = ObjectKind::AirBase;
        base.slots = vec![SlotState { kind: 30, progress: 0, target: 4000, level: 0, max_level: 0, waiting: 0 }];
        w.add_structure(base);
        w.add_unit(7, 0, 8 * CELL, 8 * CELL);
        w.add_unit(7, 1, 39 * CELL, 11 * CELL);
        let mut t = factory(Owner::Enemy, 900, 25);
        t.kind = ObjectKind::Turret(1);
        t.slots.clear();
        t.active = 0;
        t.cell = (20, 5);
        w.add_structure(t);
        for _ in 0..2700 {
            w.step();
        }
        assert_eq!(w.air_stock[0][0], 1, "one bomber after 44 s");
        assert!(w.launch_strike(0, AirForce::Bomber, 20 * CELL + 16, 5 * CELL + 16));
        assert!(w.sounds.iter().any(|s| s.0 == 68), "the local launch pings airforce-dest");
        assert!(w.effects.iter().any(|e| e.spec.array == 0x5e5fb0), "and shows the cross_as marker at the target");
        assert_eq!(w.air_stock[0][0], 0);
        for _ in 0..1500 {
            w.step();
        }
        assert!(w.structures[1].hp <= 900 - 3 * 110, "the carpet bombing hit the turret (hp {})", w.structures[1].hp);
        assert!(w.aircraft.is_empty(), "the bomber left the map");
    }

    /// `0x41146d`: a bomb's speed grows by the steps it took on every tick, so a 200 px fall lasts about 2 s, not the 6.7 s a constant
    /// 30 steps per second would take.
    #[test]
    fn a_bomb_falls_faster_and_faster() {
        let mut w = open_world(20, 12);
        let path = build_path(18, 5 * CELL, 0, 5 * CELL + 45, 200, 0).unwrap();
        w.bullets.push(Bullet::new(18, 110, 0, path));
        let mut ticks = 0;
        while !w.bullets.is_empty() && ticks < 1000 {
            w.step();
            ticks += 1;
        }
        assert!((100..160).contains(&ticks), "landed after {ticks} ticks");
    }

    /// `0x467f03` / `0x454da9`: a launch of the local player disables the air icons for 180 ticks; the enemy's launches do not.
    #[test]
    fn a_launch_locks_the_air_icons_for_three_seconds() {
        let mut w = open_world(20, 12);
        w.air_stock[0][0] = 2;
        w.air_stock[1][0] = 1;
        assert!(w.launch_strike(1, AirForce::Bomber, 8 * CELL, 4 * CELL));
        assert_eq!(w.strike_lock, 0, "the computer's strike does not lock the player's icons");
        assert!(w.launch_strike(0, AirForce::Bomber, 8 * CELL, 4 * CELL));
        assert_eq!(w.strike_lock, STRIKE_LOCK_TICKS);
        for _ in 0..STRIKE_LOCK_TICKS - 1 {
            w.step();
        }
        assert_eq!(w.strike_lock, 1);
        w.step();
        assert_eq!(w.strike_lock, 0);
    }

    fn turret_with_top(w: &mut World, cell: (i32, i32), hp: i32) -> usize {
        let mut t = factory(Owner::Enemy, hp, 25);
        t.kind = ObjectKind::Turret(1);
        t.slots.clear();
        t.active = 0;
        t.cell = cell;
        t.aim = TopAim::new(109, 0);
        w.add_structure(t);
        w.structures.len() - 1
    }

    /// `0x42f346..0x42f38d`: the gun top (a unit of kind 109..113) within 32 px redirects the blast's full strength to its turret,
    /// on top of the cell test. A bomb on the turret's cell does 110 (cell) + 110 (top).
    #[test]
    fn a_blast_next_to_a_turrets_gun_top_damages_the_turret_twice() {
        let mut w = open_world(20, 12);
        let t = turret_with_top(&mut w, (10, 5), 900);
        w.explode(10 * CELL + 16, 5 * CELL + 16, 18, 110, 0);
        assert_eq!(w.structures[t].hp, 900 - 220);
        // 40 px away, in the neighbouring cell: nothing hits the cell and the top is out of reach
        w.explode(11 * CELL + 24, 5 * CELL + 16, 18, 110, 0);
        assert_eq!(w.structures[t].hp, 900 - 220);
        // 30 px away in the neighbouring cell: only the top
        w.explode(11 * CELL + 14, 5 * CELL + 16, 18, 110, 0);
        assert_eq!(w.structures[t].hp, 900 - 330);
        // a turret still building has no top: the cell test alone
        let u = turret_with_top(&mut w, (4, 5), 900);
        w.structures[u].heal = 100;
        w.explode(4 * CELL + 16, 5 * CELL + 16, 18, 110, 0);
        assert_eq!(w.structures[u].hp, 900 - 110);
    }

    #[test]
    fn a_fighter_strike_damages_a_turret() {
        let mut w = open_world(40, 12);
        let t = turret_with_top(&mut w, (20, 5), 5000);
        w.air_stock[0][4] = 1;
        assert!(w.launch_strike(0, AirForce::Fighter, 20 * CELL + 16, 5 * CELL + 16));
        for _ in 0..600 {
            w.step();
        }
        // the missiles land 22 px left and right of the aim point: outside the 1x1 cell, so it is the gun top that takes them
        assert!(w.structures[t].hp <= 5000 - 4 * 150, "the fighter's missiles hurt the turret (hp {})", w.structures[t].hp);
    }

    /// The exe never stops a walking chassis because its weapon has a target: the unit loop updates the top (`0x479c60`) and the
    /// chassis (`0x46f140`) every tick independently, and no weapon code clears the chassis orders (only `0x4706a0` and the chase
    /// replan `0x46eb40` write them). A unit chasing a far target keeps walking while it shoots at whatever is in range.
    #[test]
    fn units_keep_walking_while_they_shoot() {
        let mut w = open_world(40, 14);
        w.ai = false;
        let a = w.add_unit(7, 0, 3 * CELL + 16, 5 * CELL + 16);
        w.add_unit(7, 1, 8 * CELL + 16, 5 * CELL + 16); // stands next to the route, in range
        let far = w.add_unit(7, 1, 36 * CELL + 16, 9 * CELL + 16);
        w.units.iter_mut().find(|u| u.id == a).unwrap().chase = Some(Target::Unit(far));
        let (mut shooting, mut moving_and_shooting) = (0, 0);
        let mut last = w.unit(a).unwrap().x;
        for _ in 0..600 {
            w.step();
            let Some(u) = w.unit(a) else { break };
            let moved = (u.x - last).abs() > 0.01;
            last = u.x;
            if u.target.is_some() {
                shooting += 1;
                if moved {
                    moving_and_shooting += 1;
                }
            }
        }
        assert!(shooting > 20, "the unit shot ({shooting} ticks with a target)");
        // a walker only moves on some ticks (7..30 px/s of 60 ticks), so compare with the walking speed instead of the ticks
        assert!(moving_and_shooting > 40, "it kept walking while shooting ({moving_and_shooting} of {shooting} ticks)");
        assert!(w.unit(a).map_or(true, |u| u.x > 8.0 * CELL as f32), "it walked past the enemy it was shooting at");
    }

    #[test]
    fn convoy_pods_take_no_player_orders_and_walk_slowly() {
        let mut w = open_world(30, 12);
        w.set_mission(Some(3));
        let pod = w.add_unit(46, 0, 8 * CELL + 16, 5 * CELL + 16);
        let tank = w.add_unit(7, 0, 9 * CELL + 16, 5 * CELL + 16);
        w.units.iter_mut().for_each(|u| u.selected = true);
        w.order_move_selected(20 * CELL, 5 * CELL);
        assert!(w.unit(pod).unwrap().path.is_empty(), "no go-to order for the pod");
        assert!(!w.unit(tank).unwrap().path.is_empty(), "other units still obey");
        assert_eq!(w.walk_params_of(46).unwrap().speed, 7);
        w.set_mission(Some(1));
        assert_eq!(w.walk_params_of(46).unwrap().speed, 30);
    }

    #[test]
    fn a_rogon_bomb_blows_up_on_its_target_and_damages_around_it() {
        let mut w = open_world(30, 12);
        let victim = w.add_unit(7, 1, 12 * CELL + 16, 5 * CELL + 16);
        let near = w.add_unit(7, 1, 13 * CELL + 16, 5 * CELL + 16);
        let far = w.add_unit(7, 1, 25 * CELL + 16, 5 * CELL + 16);
        w.add_unit(7, 0, 2 * CELL, 2 * CELL);
        let pod = w.add_unit(46, 0, 8 * CELL + 16, 5 * CELL + 16);
        w.units.iter_mut().find(|u| u.id == pod).unwrap().chase = Some(Target::Unit(victim));
        for _ in 0..900 {
            w.step();
        }
        assert!(w.unit(pod).is_none(), "the bomb is gone");
        assert!(w.unit(victim).is_none(), "the target died");
        assert!(w.unit(near).is_none(), "a unit next to the blast died too");
        assert!(w.unit(far).is_some(), "far away units survive");
        assert!(w.nukes.is_empty());
    }

    #[test]
    fn dev_tools_place_turrets_and_buildings_anywhere_but_not_on_each_other() {
        let mut w = open_world(30, 12);
        assert!(w.dev_place_structure(29, 1, (5, 5)), "a turret far from any own building");
        let t = w.structures.last().unwrap();
        assert!(t.alive() && t.hp == t.max_hp && t.aim.is_some() && t.owner == Owner::Enemy);
        assert!(!w.dev_place_structure(103, 0, (3, 4)), "overlaps the turret (footprint 5 x 4 from (3, 4))");
        assert!(w.dev_place_structure(103, 0, (10, 4)));
        assert!(w.structures.last().unwrap().covers((14, 7)));
        assert!(!w.dev_place_structure(100, 0, (28, 10)), "leaves the map");
        assert!(!w.dev_place_structure(40, 0, (1, 1)), "not a structure kind");
    }

    #[test]
    fn volley_scatter_stays_inside_the_weapons_spread_and_is_not_constant() {
        let mut w = open_world(30, 12);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let (jx, jy) = w.volley_scatter(113);
            assert!(jx.abs() < 20 && jy.abs() < 20);
            seen.insert((jx, jy));
            assert_eq!(w.volley_scatter(15), (0, 0), "spread 1 never moves the end point");
        }
        assert!(seen.len() > 20, "the turret's six rounds land on different points");
    }

    #[test]
    fn a_rogon_bomb_never_touches_a_walker_and_a_dying_pod_leaves_no_wreck() {
        let mut w = open_world(30, 12);
        let walker = w.add_unit(41, 1, 12 * CELL + 16, 5 * CELL + 16);
        w.add_unit(7, 0, 2 * CELL, 2 * CELL);
        let pod = w.add_unit(46, 0, 8 * CELL + 16, 5 * CELL + 16);
        w.units.iter_mut().find(|u| u.id == pod).unwrap().chase = Some(Target::Unit(walker));
        for _ in 0..600 {
            w.step();
        }
        assert!(w.unit(pod).is_some(), "the walker's handle is in the other grid: no touch");
        assert!(w.unit(walker).is_some());
        // a pod shot down explodes too, without the generic death clouds
        let before = w.effects.len();
        w.units.iter_mut().find(|u| u.id == pod).unwrap().hp = 0;
        w.step();
        assert_eq!(w.nukes.len(), 1);
        assert!(w.effects.len() <= before + 21, "only blast sprites, no death clouds");
        // each blast is an e_unit2 cloud jittered by up to 16 px, not a fixed blast sprite on the ring cell (0x4113b0..0x4113f1)
        let fresh: Vec<_> = w.effects.iter().skip(before).collect();
        assert!(fresh.iter().all(|e| e.spec == UNIT_CLOUD));
        assert!(fresh.iter().any(|e| (e.x - 16).rem_euclid(CELL) != 0 || (e.y - 16).rem_euclid(CELL) != 0), "off the cell grid");
    }
}
