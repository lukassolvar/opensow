//! The skirmish simulation: units walk, weapon tops aim and fire, bullets fly and explode, structures take damage.
//!
//! Everything here is pure (no I/O, no rendering) and built from the decoded game rules: chassis animation
//! (`anim`), weapon tops (`aim`), target search over range rings (`rings`), bullet paths and area damage
//! (`projectile`). Values the exe analysis has not produced are placeholders and marked `PLACEHOLDER`:
//! movement speed, the weapons of static turrets and the enemy's behaviour.

use opensow_formats::cursor::Pointer;
use opensow_formats::wave::{find_targets, Seen};
mod tutorial;
pub use tutorial::{text_index as tutorial_text_index, TutEvent, Tutorial};
use opensow_formats::advancer::{action_for_pointer, pointer_over_building, building_max_hp, capture_winner, hp_per_event, rate as advancer_rate, removed_when_destroyed, upgrade_per_event, fragment, fragment_count, Action, BODKY_B, BODKY_R, BODKY_Y, HEAL_PER_TICK, BeamTimer};
use opensow_formats::aim::{aim_params, TopAim};
use opensow_formats::buildings::{blocked_cells, can_place, footprint};
use opensow_formats::air::{AirForce, HEADER_ORDER};
use opensow_formats::aircraft::{launch_counter, squad, Aircraft, Event as AirEvent};
use opensow_formats::anim::{walk_params, ChassisAnim, HOVER_ALTITUDE, HOVER_PERIOD, HOVER_PHASES, HOVER_TABLE};
use opensow_formats::effects::{flame_trail, trail as shell_trail, blast, impact_anims, impact_leaves_decal, impact_smoke, smoke_puff, UNIT_DEATH_PUFFS, AnimSpec, BulletAnim, AIR_DESTROYED, BUILDING_DESTROYED, TURRET_DESTROYED, UNIT_CLOUD, UNIT_WRECK, debris, debris_variant, DEBRIS_STRENGTH, UNIT_DEATH_CLOUDS, WRECK_SMOKE_COLUMNS, WRECK_SMOKE_FIRST, WRECK_SMOKE_PERIOD, WRECK_SMOKE_TYPE, SATLASER_STRENGTH, SATLASER_START_DY, SATLASER_END_DY};
use opensow_formats::facing::{facing, heading_degrees, turn_step};
use opensow_formats::messages as msg;
use opensow_formats::level::{Level, ObjectKind, Owner, UnitKind};
use opensow_formats::production::{factory_exit, FactoryExit, NO_SPACE_TICKS, income_per_tick, next_tier, rate_row, slot_target, slot_unlock_cost, turret_price, Slots, CREDITS_CAP, RESEARCH_CAP};
use opensow_formats::projectile::{TRIPLER_BLAST_STRENGTH, TRIPLER_RING, TRIPLER_RING_USED, build_path, bullet_type, splash_victims, steps_at, weapon_shot, Path, UnitRef};
use opensow_formats::freecell::free_cell_near;
use opensow_formats::rings::Rings;
use opensow_formats::wave::{approach, approach_air, Wave};
use opensow_formats::rng::GameRng;
use std::collections::HashMap;

pub const CELL: i32 = 32;
/// Game ticks per second: the exe's frame accumulator (0x42e890) emits one tick per 1666 hundredths of a millisecond = 16.66 ms.
pub const TICKS_PER_SECOND: u32 = 60;
/// Ticks per facing step of the always-spinning Advancer (3 ticks = 50 ms, measured from a recording).
pub const ADVANCER_SPIN_TICKS: u64 = 3;
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
        self.path.first().map(|&(x, y)| ((x.floor() as i32).div_euclid(CELL), (y.floor() as i32).div_euclid(CELL)))
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

/// Enemy factories stop producing above this many living enemy units (PLACEHOLDER: the AI is not decoded).
const ENEMY_UNIT_CAP: usize = 60;

impl Structure {
    pub fn alive(&self) -> bool {
        self.hp > 0
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

impl Bullet {
    pub fn new(bullet_type: u8, strength: i32, owner: i32, path: Path) -> Bullet {
        let index = path.first_step;
        let anim = crate::sim::bullet_type(bullet_type).and_then(|t| t.anim).map(BulletAnim::new);
        let (a, b) = (path.points[0], *path.points.last().unwrap());
        // the exe's own value for the missile (start to end, `Path::facing`); other types only need a heading for the viewer
        let facing = path.facing.unwrap_or_else(|| facing(a.0, a.1, b.0, b.1, 32) as u8);
        let trail = bullet_type_interval(bullet_type);
        Bullet { bullet_type, strength, owner, path, index, anim, facing, trail, variant: 0, target: None }
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

#[derive(serde::Serialize, serde::Deserialize)]
pub struct World {
    pub cells: (i32, i32),
    blocked: Vec<bool>,
    /// The terrain Advancers cannot enter: the `.map` border layer (the outer ring and, on a few maps, no-fly areas).
    air_blocked: Vec<bool>,
    pub units: Vec<Unit>,
    pub structures: Vec<Structure>,
    pub bullets: Vec<Bullet>,
    /// The Advancers' beam bullets in flight.
    pub beams: Vec<BeamShot>,
    /// Buildings flashing after the player ordered units onto them (`0x4086a0`), by structure index.
    pub blinks: HashMap<usize, Blink>,
    /// Idle look-around counters of the turret tops (`[unit+0x8c]`, a turret's top is a unit, so it looks around like one), by structure index.
    pub turret_look: HashMap<usize, i32>,
    /// `.po1` muzzle tables of the turret tops, by weapon (109-113); filled by the game from the install, empty in pure tests.
    #[serde(skip)]
    pub turret_muzzles: HashMap<i32, opensow_formats::fire::FireTables>,
    pub effects: Vec<Effect>,
    /// Sound effects raised this tick (effect id, map x, map y, pitch jitter); the game drains them (`sfx.rs`).
    pub sounds: Vec<(u8, i32, i32, bool)>,
    pub decals: Vec<Decal>,
    /// The exe's visual generator (`0x437cd0`), kept apart from the lockstep one.
    fx_rng: u32,
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
    carry_counter: usize,
    next_air_id: u32,
    /// Detonating Rogon bombs (event type 35).
    pub nukes: Vec<Nuke>,
    /// Burning wrecks of destroyed units (`0x45b0a0`).
    pub wrecks: Vec<Wreck>,
    /// Messages for the local player: (tick, event id of `formats::messages`).
    pub log: Vec<(u64, u8)>,
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

/// Projectiles per shot of a top: the fifth turret fires six (`[top+0x30]` = 6), everything else one per table.
fn burst(weapon: i32) -> u32 {
    if weapon == 113 { 6 } else { 1 }
}

impl World {
    pub fn new(cells: (i32, i32), blocked: Vec<bool>) -> World {
        assert_eq!(blocked.len(), (cells.0 * cells.1) as usize);
        // without a map the Advancers are only kept inside the outer ring
        let air_blocked = (0..cells.0 * cells.1).map(|i| { let (x, y) = (i % cells.0, i / cells.0); x == 0 || y == 0 || x == cells.0 - 1 || y == cells.1 - 1 }).collect();
        World {
            cells,
            blocked,
            air_blocked,
            units: Vec::new(),
            structures: Vec::new(),
            bullets: Vec::new(),
            beams: Vec::new(),
            blinks: HashMap::new(),
            turret_look: HashMap::new(),
            turret_muzzles: HashMap::new(),
            effects: Vec::new(),
            sounds: Vec::new(),
            decals: Vec::new(),
            fx_rng: 0x9E3779B9,
            occ: None,
            tick: 0,
            outcome: None,
            stats: [[0; 8]; 3],
            next_id: 1,
            rng: 0x2545_f491,
            game_rng: None,
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
            next_air_id: 1,
            nukes: Vec::new(),
            wrecks: Vec::new(),
            log: Vec::new(),
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
            if matches!(kind, ObjectKind::Unknown(_)) || o.hp() <= 0 {
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
            world.add_structure(Structure { object: i, kind, owner: o.owner(), cell: (o.cell_x, o.cell_y), hp: o.hp(), max_hp: building_max_hp(o.kind).max(o.hp()), aim, weapon, target: None, slots, active, kind_id: o.kind, tally: [0; 3], heal: 0, upgrade: 0, protected: o.raw[15] == 1 || o.kind == 100, attacked: false, sat_cd: 0, advancer: None, rally: None });
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
        self.structures.push(s);
    }

    pub fn add_unit(&mut self, kind: i32, player: i32, x: i32, y: i32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let params = walk_params(kind);
        let chassis = params.map(|p| ChassisAnim::new(p, p.steps, 0));
        let aim = TopAim::new(kind, 0);
        let hover_phase = self.random(20);
        let max_hp = UnitKind::max_hp(kind).unwrap_or(300);
        self.units.push(Unit { id, kind, player, x: x as f32, y: y as f32, hp: max_hp, max_hp, chassis, aim, path: Vec::new(), target: None, selected: false, moved: false, line: None, straight: None, queued: None, hover_phase, action: None, beam: BeamTimer::default(), beam_shots: 0, chase: None, idle: 0, look: 0 });
        id
    }

    fn cell_index(&self, cx: i32, cy: i32) -> Option<usize> {
        (cx >= 0 && cy >= 0 && cx < self.cells.0 && cy < self.cells.1).then(|| (cy * self.cells.0 + cx) as usize)
    }

    pub fn is_blocked(&self, cx: i32, cy: i32) -> bool {
        self.cell_index(cx, cy).map_or(true, |i| self.blocked[i])
    }

    /// Terrain of the air layer (`0x60ac38`): what an Advancer cannot enter.
    pub fn is_air_blocked(&self, cx: i32, cy: i32) -> bool {
        self.cell_index(cx, cy).map_or(true, |i| self.air_blocked[i])
    }

    /// Installs the exe's random table (`Data/generic.file`); without it a xorshift generator is used.
    pub fn set_game_rng(&mut self, rng: GameRng) {
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

    fn fx_random(&mut self, n: u32) -> u32 {
        self.fx_rng ^= self.fx_rng << 13;
        self.fx_rng ^= self.fx_rng >> 17;
        self.fx_rng ^= self.fx_rng << 5;
        self.fx_rng % n.max(1)
    }

    /// Scorch decal and smoke puffs of a landing bullet (the generic tail of the impact function, docs/effects.md).
    fn impact_ground_effects(&mut self, x: i32, y: i32, bt: u8) {
        let cell = (x.div_euclid(CELL), y.div_euclid(CELL));
        let inside = cell.0 > 0 && cell.0 < self.cells.0 && cell.1 > 0 && cell.1 < self.cells.1;
        let free = inside && !self.is_blocked(cell.0, cell.1);
        if matches!(bt, 17 | 18) {
            if free {
                for _ in 0..impact_smoke(bt, 0) {
                    let (dx, dy) = (self.fx_random(20) as i32 - 10, self.fx_random(20) as i32 - 10);
                    self.puff(x + dx, y + dy);
                }
            }
            return;
        }
        if !impact_leaves_decal(bt) || !inside {
            return;
        }
        if free && !self.decals.iter().any(|d| d.cell == cell) {
            let frame = (self.fx_random(9) / 2) as u8;
            self.decals.push(Decal { x, y, cell, frame });
        }
        let roll = self.random(3);
        if free {
            for _ in 0..impact_smoke(bt, roll) {
                let (dx, dy) = (self.fx_random(20) as i32 - 10, self.fx_random(20) as i32 - 10);
                self.puff(x + dx, y + dy);
            }
        }
    }

    fn puff(&mut self, x: i32, y: i32) {
        let (variant, extra) = (self.fx_random(2) as u8, self.fx_random(13) as u8);
        self.effects.push(Effect { x, y, spec: smoke_puff(variant, extra), age: 0 });
    }

    fn random(&mut self, n: u32) -> u32 {
        if let Some(r) = self.game_rng.as_mut() {
            return r.next(n.min(u16::MAX as u32) as u16) as u32;
        }
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng % n.max(1)
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
        if self.units.iter().any(|u| u.selected && u.player == 0 && u.kind == 40) {
            if let Some(t) = &mut self.tutorial {
                t.advancer_moved = true; // step 5: the selected Advancer got a go-to order
            }
        }
        for u in self.units.iter_mut().filter(|u| u.selected && u.player == 0) {
            u.chase = None;
            u.queued = None; // a go-to replaces the whole order list (`0x4706a0`)
        }
        let goal = (x.div_euclid(CELL), y.div_euclid(CELL));
        let mut slots = std::iter::once((0i16, 0i16)).chain(self.rings.search(0, 8).iter().copied().skip(0)).collect::<Vec<_>>();
        slots.dedup();
        let mut next = 0usize;
        let ids: Vec<u32> = self.units.iter().filter(|u| u.selected && u.player == 0).map(|u| u.id).collect();
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
        let (start, air) = (u.cell(), u.kind == 40);
        // a type-1 order (`0x470480`): the wave search of the unit's own layer (`0x4753a0` mode 0, or `0x474b30` for the Advancer)
        let mut path = match self.find_path_on(air, start, cell) {
            Some(p) => p,
            None => return,
        };
        if let (Some((ex, ey)), Some(last)) = (exact, path.last_mut()) {
            *last = (ex as f32, ey as f32);
        }
        let u = self.units.iter_mut().find(|u| u.id == id).unwrap();
        u.path = path;
        // a new route has no pixel line yet: the next step plans it (0x473190), avoiding units
        u.line = None;
    }

    /// A type-2 order (`0x470480`) for the Advancer: it flies the straight line to `cell` without path finding (the beam
    /// approach of `0x408890`).
    pub fn order_fly_straight(&mut self, id: u32, cell: (i32, i32)) {
        if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
            u.straight = None;
            u.line = None;
            u.path = vec![centre(cell)];
        }
    }

    pub fn stop(&mut self, id: u32) {
        if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
            u.path.clear();
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
            None => self.units[i].path.clear(),
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
            u.path.clear();
        } else if let Some(next) = r.next {
            if r.path.is_empty() {
                // the look-ahead cell is next to the unit: step straight there
                u.path.drain(..k.saturating_sub(1));
            } else {
                let last = u.path.get(k - 1).copied().filter(|_| k == u.path.len());
                let mut local: Vec<(f32, f32)> = r.path.iter().map(|&c| centre(c)).collect();
                if let (Some(exact), Some(end)) = (last, local.last_mut()) {
                    *end = exact;
                }
                u.path.splice(..k, local);
            }
            let _ = next;
        } else {
            // no local route (0x476870 failed): the exe then looks for a free cell near the destination (0x4774c0) and keeps
            // its previous step cell; here the unit waits and tries again on the next tick
            let waiting = route.len() > 1 && route[k] != at;
            if waiting {
                self.retarget_to_free_cell(i);
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

    /// Selects the player's units inside the pixel rectangle; returns how many are selected afterwards.
    pub fn select_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, additive: bool) -> usize {
        if !additive {
            self.clear_selection();
        }
        let (lx, hx, ly, hy) = (x0.min(x1) as f32, x0.max(x1) as f32, y0.min(y1) as f32, y0.max(y1) as f32);
        // a box never takes Advancers or nuclear vehicles, and drops one that was selected alone
        if self.units.iter().any(|u| u.selected && Self::is_single_select(u.kind)) {
            self.clear_selection();
        }
        for u in self.units.iter_mut().filter(|u| u.player == 0 && !Self::is_single_select(u.kind)) {
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
        self.occ = None;
        if self.enemy_at_start.is_none() {
            self.enemy_at_start = Some(self.units.iter().any(|u| u.player != 0) || self.structures.iter().any(|s| s.owner == Owner::Enemy));
        }
        let index = self.index();
        for i in 0..self.units.len() {
            self.step_unit(i, &index);
        }
        for i in 0..self.structures.len() {
            self.step_structure(i, &index);
        }
        for i in 0..self.structures.len() {
            self.step_production(i);
        }
        self.step_convoy();
        self.step_hq();
        self.step_economy();
        self.step_mission();
        self.step_tutorial();
        if self.ai {
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
        let dead_adv: Vec<usize> = self.units.iter().filter(|u| u.hp <= 0 && u.kind == 40).map(|u| if u.player == 0 { 0 } else { 1 }).collect();
        for side in dead_adv {
            self.say(side, msg::ADVANCER_DESTROYED);
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
                    self.say(player as usize, id);
                }
                continue;
            }
            if player != 0 && self.units.iter().filter(|u| u.player != 0 && u.hp > 0).count() >= ENEMY_UNIT_CAP {
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
                        self.say(player as usize, msg::NO_SPACE);
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
    /// over a turret, a turret over any other building.
    pub fn find_target(&self, index: &HashMap<(i32, i32), Vec<Target>>, player: i32, cell: (i32, i32), min: u32, max: u32) -> Option<Target> {
        let (mut unit, mut turret, mut building) = (None, None, None);
        for &(dx, dy) in self.rings.search(min as usize, (max as usize).min(63)) {
            let Some(list) = index.get(&(cell.0 + dx as i32, cell.1 + dy as i32)) else { continue };
            for &t in list {
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
            if unit.is_some() {
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
            let tpos = t.and_then(|id| self.unit(id)).map(|u| if u.kind == 40 { u.flyer_points().0 } else { u.px() });
            let face = tpos.map(|(tx, ty)| facing(ux, uy, tx, ty, steps));
            let mut r = self.random(1 << 16);
            let ev = self.units[i].aim.as_mut().unwrap().tick(face, |n| {
                r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (r >> 8) % n.max(1)
            });
            if ev.shot.is_some() {
                if let (Some(id), Some(to), Some((bt, strength))) = (t, tpos, weapon_shot(40)) {
                    // a bullet of type 31 (invisible, speed 600); the strength comes off the unit by handle when it lands (0x4108bc..0x4109b8)
                    let from = self.units[i].flyer_points().0;
                    self.shoot_at(bt, strength as i32, player.min(1), from, to, HitTarget::Unit(id));
                }
            }
        } else if self.units[i].aim.is_some() && !ANTI_AIR.contains(&kind) {
            let (min, max, steps) = {
                let p = &self.units[i].aim.as_ref().unwrap().params;
                (p.min_range, p.max_range, p.steps)
            };
            let t = self.find_target(index, player, cell, min, max);
            self.units[i].target = t;
            let (ux, uy) = self.units[i].px();
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
                let from = (ux + (sin * 14.0) as i32, uy - (cos * 14.0) as i32);
                for _ in 0..shots.max(1) as u32 * burst(kind) {
                    spawn.push((from, to));
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
            let ai = self.aircraft_in_range(player, (ux, uy), min, max);
            // no aircraft: the air-grid search 0x47ad30 finds an enemy Advancer
            let adv = if ai.is_none() { self.advancer_in_range(index, player, cell, min, max) } else { None };
            let apos = adv.and_then(|id| self.unit(id)).map(|u| u.flyer_points().0);
            let face = match (ai, apos) {
                (Some(a), _) => Some(facing(ux, uy, self.aircraft[a].x, self.aircraft[a].y, steps)),
                (None, Some((tx, ty))) => Some(facing(ux, uy, tx, ty, steps)),
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
                    if let Some(a) = ai {
                        let (to, id) = ((self.aircraft[a].x, self.aircraft[a].y - self.aircraft[a].altitude), self.aircraft[a].id);
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
        // chasing (`0x46eb40`): the route to the target is replanned every 7 ticks; once something is in weapon range the unit stops
        if let Some(t) = self.units[i].chase {
            match self.target_pos(t) {
                Some((tx, ty)) if self.hostile(player, t) => {
                    if self.units[i].target.is_some() {
                        self.units[i].path.clear();
                    } else if self.units[i].path.is_empty() || self.tick % 7 == (i as u64 % 7) {
                        self.chase_step(i, t, (tx.div_euclid(CELL), ty.div_euclid(CELL)));
                    }
                }
                _ => self.units[i].chase = None,
            }
        }
        // walking: `steps_at(speed, tick % 60)` single-pixel moves per tick (0x46f550 + table 0x436df0); the unit only
        // starts once its chassis is within 4 turn steps of the wanted facing
        // (every kind plans around other units: the movement code has no branch on kind 46, docs/walking.md)
        let hold = self.units[i].line.is_none() && self.units[i].straight.is_none() && !self.units[i].path.is_empty() && self.avoid(i);
        let tick = self.tick;
        let u = &mut self.units[i];
        let (ox, oy) = (u.x, u.y);
        if u.kind == 40 {
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
        if let Some(&(wx, wy)) = u.path.first().filter(|_| !hold) {
            let params = walk_params(u.kind);
            let steps = params.map_or(32, |p| p.steps);
            let want = facing(u.x.round() as i32, u.y.round() as i32, wx.round() as i32, wy.round() as i32, steps);
            let aligned = match u.chassis.as_mut() {
                Some(c) if u.kind != 40 => {
                    if (wx - u.x).abs() + (wy - u.y).abs() > 1.0 {
                        c.set_target(want);
                    }
                    let mut f = c.facing;
                    for _ in 0..4 {
                        f = turn_step(f, c.target, steps);
                    }
                    f == c.target
                }
                _ => true,
            };
            if aligned {
                let moves = steps_at(params.map_or(20, |p| p.speed), (tick % 60) as u32);
                for _ in 0..moves {
                    let Some(&(wx, wy)) = u.path.first() else { break };
                    let goal = (wx.round() as i32, wy.round() as i32);
                    let here = (u.x.round() as i32, u.y.round() as i32);
                    if here == goal {
                        // the segment is done: the next tick plans the following one (`avoid` = 0x473190 runs when the line is empty)
                        u.path.remove(0);
                        u.line = None;
                        break;
                    }
                    // Bresenham from the segment start: one pixel along the long axis per move, the short axis when due
                    let line = u.line.filter(|l| l.goal == goal).unwrap_or_else(|| {
                        let (dx, dy) = (goal.0 - here.0, goal.1 - here.1);
                        Line { goal, major: dx.abs().max(dy.abs()), minor: dx.abs().min(dy.abs()), err: 0 }
                    });
                    let (dx, dy) = (goal.0 - here.0, goal.1 - here.1);
                    let mut line = line;
                    line.err += line.minor;
                    let minor_step = 2 * line.err >= line.major && line.minor > 0;
                    if minor_step {
                        line.err -= line.major;
                    }
                    u.line = Some(line);
                    let (sx, sy) = (dx.signum() as f32, dy.signum() as f32);
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
                }
            }
        }
        u.moved = (u.x, u.y) != (ox, oy);
        let moved = u.moved;
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
            let ai = self.aircraft_in_range(owner_player, (sx, sy), min, max);
            let adv = if ai.is_none() { self.advancer_in_range(index, owner_player, cell, min, max) } else { None };
            let apos = adv.and_then(|id| self.unit(id)).map(|u| u.flyer_points());
            let face = match (ai, apos) {
                (Some(a), _) => Some(facing(ox, oy, self.aircraft[a].x, self.aircraft[a].y, steps)),
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
                    (Some(a), _) => Some((self.aircraft[a].x, self.aircraft[a].y - self.aircraft[a].altitude)),
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
        let t = self.find_target(index, owner_player, cell, min, max);
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
            for table in barrel as usize..barrel as usize + shots.max(1) as usize {
                let from = self.turret_muzzle(i, weapon, table, facing_now, steps);
                for _ in 0..burst(weapon) {
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
            let speed = bullet_type(b.bullet_type).and_then(|t| t.speed).unwrap_or(60);
            for _ in 0..steps_at(speed, t) {
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
                // flamer bullets (types 8, 9): a puff every `interval` path steps, none on the landing step (0x4118e2)
                if let Some(spec) = flame_trail(b.bullet_type) {
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
                for &(dx, dy) in &TRIPLER_RING[..TRIPLER_RING_USED] {
                    let (jx, jy) = (self.random(10) as i32 - 5, self.random(10) as i32 - 5);
                    self.explode(x + dx + jx, y + dy + jy, 17, TRIPLER_BLAST_STRENGTH, owner);
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
        let refs: Vec<UnitRef> = self.units.iter().map(|u| UnitRef { x: u.px().0, y: u.px().1, flying: false, owner: u.player as u32, kind: u.kind }).collect();
        for v in splash_victims(&refs, x, y, bt, owner as u32) {
            self.units[v].hp -= strength;
        }
        let cell = (x.div_euclid(CELL), y.div_euclid(CELL));
        let mut destroyed = Vec::new();
        for (i, s) in self.structures.iter_mut().enumerate() {
            // no owner check, as in the exe (0x42f3cc..0x42f462): buildings take friendly fire too
            if s.alive() && s.covers(cell) {
                // half damage for the meteors and the Rogon blast, none on a Headquarters for the air-strike types (0x42f3cc..0x42f462)
                let d = opensow_formats::projectile::building_damage(bt, strength, s.kind_id);
                s.hp -= d;
                s.tally[owner.clamp(0, 1) as usize] += d;
                if s.hp <= 0 {
                    destroyed.push(i);
                }
            }
        }
        for i in destroyed {
            self.structure_destroyed(i);
        }
    }

    /// A structure ran out of hit points (`0x409130` + `0x42dc60`): turrets and headquarters are removed, every other
    /// building is rebuilt for the player with the largest foreign damage tally (`0x43ab40`), starting at 1 hit point
    /// and healing up to its maximum.
    fn structure_destroyed(&mut self, i: usize) {
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
        self.quake.start(if turret { QUAKE_TURRET } else { QUAKE_BUILDING }, cx as i32, cy as i32);
        let spec = if turret { TURRET_DESTROYED } else { BUILDING_DESTROYED };
        self.effects.push(Effect { x: cx as i32, y: cy as i32, spec, age: 0 });
        self.sound(if turret { 28 } else { 24 }, cx as i32, cy as i32, false);
        self.say(owner, if turret { msg::TURRET_DESTROYED } else { msg::BUILDING_LOST });
        if let Some(p) = winner {
            self.say(p, msg::BUILDING_CAPTURED);
        }
        match winner {
            Some(p) => {
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
        self.order_fly_straight(id, cell);
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

    /// Builds a turret of `tier` (1-5) for `side` on `cell` (`0x417335`..`0x4173da`): costs 200 * tier credits, the new
    /// turret starts at 1 hit point and heals in. Placement rules beyond a free cell are not decoded.
    pub fn place_turret(&mut self, side: usize, tier: u32, cell: (i32, i32)) -> bool {
        let Some(price) = turret_price(tier) else { return false };
        let me = if side == 0 { Owner::Player } else { Owner::Enemy };
        let kind_id = 24 + tier as i32;
        let free = |x: i32, y: i32| !self.is_blocked(x, y) && !self.units.iter().any(|u| u.hp > 0 && u.cell() == (x, y)) && !self.structures.iter().any(|s| s.alive() && s.covers((x, y)));
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
        self.sound(72, cell.0 * CELL + CELL / 2, cell.1 * CELL + CELL / 2, false);
        self.add_structure(Structure { object: usize::MAX, kind: ObjectKind::Turret(tier as u8), owner, cell, hp: 1, max_hp, aim, weapon, target: None, slots: Vec::new(), active: 0, kind_id: 24 + tier as i32, tally: [0; 3], heal: max_hp - 1, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None });
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
        // turrets: the most threatened own building gets a turret once credits allow (0x431120, simplified tiers)
        if self.credits[side] >= 200 && self.tick % 30 == 0 {
            let mut best: Option<(i32, usize)> = None;
            for (k, s) in self.structures.iter().enumerate().filter(|(_, s)| s.alive() && mine(s.owner) && !matches!(s.kind, ObjectKind::Turret(_))) {
                let threat: i32 = self.units.iter().filter(|u| u.hp > 0 && (u.cell().0 - s.cell.0).abs() <= 6 && (u.cell().1 - s.cell.1).abs() <= 6).map(|u| if (u.player != 0) == (side != 0) { -1 } else { 1 + (u.kind >= 41 && u.kind <= 45) as i32 }).sum();
                if best.map_or(true, |(b, _)| threat > b) {
                    best = Some((threat, k));
                }
            }
            if let Some((threat, k)) = best.filter(|&(t, _)| t >= 2) {
                let tier = match threat {
                    2..=3 => 1,
                    4 => 3,
                    5 => 4,
                    _ => 5,
                }
                .min((self.credits[side] / 200).clamp(1, 5) as u32);
                let c = self.structures[k].cell;
                let start = self.random(8);
                let spot = (0..8).map(|n| {
                    let r = [(-2, -2), (4, -2), (-2, 3), (4, 3), (-2, 0), (4, 0), (0, -2), (0, 3)][((start + n) % 8) as usize];
                    (c.0 + r.0, c.1 + r.1)
                }).find(|&(x, y)| !self.is_blocked(x, y));
                if let Some(cell) = spot {
                    self.place_turret(side, tier, cell);
                }
            }
        }
    }

    /// Where the computer sends an air strike (`0x435ad0`): an idle enemy pod (kind 46) if there is one, otherwise the centre of
    /// the 128-px block with the biggest unit density (enemy units +3 on their block, +2 left/right, +1 above/below, doubled for
    /// idle walkers; own units subtract 3/2/1) if that density reaches 9.
    fn strike_target(&self, side: usize) -> Option<(i32, i32)> {
        if let Some(u) = self.units.iter().find(|u| u.hp > 0 && u.kind == 46 && (u.player != 0) != (side != 0) && u.path.is_empty()) {
            return Some(u.px());
        }
        const W: i32 = 24;
        let mut grid = [0i32; 480];
        let mut add = |x: i32, y: i32, v: i32| {
            let i = y * W + x;
            if (0..480).contains(&i) && (0..W).contains(&x) {
                grid[i as usize] += v;
            }
        };
        for u in self.units.iter().filter(|u| u.hp > 0 && u.kind != 40) {
            let (bx, by) = (u.px().0 >> 7, u.px().1 >> 7);
            let own = (u.player != 0) == (side != 0);
            let w = if (41..=45).contains(&u.kind) && u.path.is_empty() { 2 } else { 1 };
            let (c, side_w, vert) = if own { (-3, -2, -1) } else { (3 * w, 2 * w, w) };
            add(bx, by, c);
            add(bx + 1, by, side_w);
            add(bx - 1, by, side_w);
            add(bx, by + 1, vert);
            add(bx, by - 1, vert);
        }
        let (best, v) = grid.iter().copied().enumerate().fold((0usize, 0i32), |acc, (i, v)| if v > acc.1 { (i, v) } else { acc });
        (v >= 9).then(|| ((best as i32 % W) * 128 + 64, (best as i32 / W) * 128 + 64))
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

    fn aircraft_in_range(&self, player: i32, pos: (i32, i32), min: u32, max: u32) -> Option<usize> {
        let side = if player == 0 { 0 } else { 1 };
        self.aircraft
            .iter()
            .enumerate()
            .filter(|(_, a)| a.side != side && a.hp > 0)
            .map(|(i, a)| (i, (((a.x - pos.0).pow(2) + (a.y - pos.1).pow(2)) as f32).sqrt() / CELL as f32))
            .filter(|&(_, d)| d >= min as f32 && d <= max as f32 + 0.5)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
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

    /// Launches an air strike of `force` for `side` at map pixel `(x, y)` from the stock (`0x467ee0`, `0x455520`).
    pub fn launch_strike(&mut self, side: usize, force: AirForce, x: i32, y: i32) -> bool {
        let Some(g) = HEADER_ORDER.iter().position(|&h| h == force) else { return false };
        if self.air_stock[side][g] == 0 {
            return false;
        }
        self.air_stock[side][g] -= 1;
        self.stats[side][6] += 1; // 0x467eef
        let mut a = Aircraft::launch(force, x, y, side as i32, self.cells.0, self.carry_counter);
        a.id = self.new_air_id();
        if force == AirForce::Carryall {
            self.carry_counter = launch_counter(self.carry_counter);
        }
        self.aircraft.push(a);
        if side == 0 {
            // the launch for the local player pings `airforce-dest` at the target (0x417221, 0x417636: the marker animation + effect 68)
            self.sound(68, x, y, false);
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
                events.push((a.side, e));
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
            self.effects.push(Effect { x: a.x as i32, y: (a.y - a.altitude) as i32, spec: AIR_DESTROYED, age: 0 });
            self.sound(23, a.x as i32, (a.y - a.altitude) as i32, false);
        }
        aircraft.retain(|a| a.alive());
        self.aircraft = aircraft;
        for (side, e) in events {
            match e {
                AirEvent::Shot { bullet_type, strength, from, to } => {
                    if let Some(path) = build_path(bullet_type, from.0, from.1, to.0, to.1, 0) {
                        if !path.points.is_empty() {
                            self.bullets.push(Bullet::new(bullet_type, strength, side, path));
                        }
                    }
                }
                AirEvent::Unload { cell_x, cell_y, squad: n } => {
                    for (dx, dy, kind) in squad(n).unwrap_or_default() {
                        let (x, y) = (cell_x + dx, cell_y + dy);
                        if !self.is_blocked(x, y) && !self.units.iter().any(|u| u.hp > 0 && u.cell() == (x, y)) {
                            self.add_unit(kind, side.min(1) as i32, x * CELL + CELL / 2, y * CELL + CELL / 2);
                        }
                    }
                }
            }
        }
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
        let mut blasts: Vec<(i32, i32, i32, i32)> = Vec::new();
        let mut nuke_sounds: Vec<(i32, i32)> = Vec::new();
        self.nukes.retain_mut(|n| {
            let (list, alive) = n.ex.step(&words, limit);
            for b in list {
                let (x, y) = (n.x + 16 + b.dx * CELL, n.y + 16 + b.dy * CELL);
                blasts.push((x, y, b.strength, n.owner));
                if b.sound {
                    nuke_sounds.push((x, y));
                }
            }
            alive
        });
        // every 17th blast of the ring rumbles (`0x411386`): the walker nuke 19, now and then the fighter missile's boom 67
        for (x, y) in nuke_sounds {
            let id = if self.random(4) == 1 { 67 } else { 19 };
            self.sound(id, x, y, true);
        }
        for (x, y, strength, owner) in blasts {
            self.effects.push(Effect { x, y, spec: blast(2), age: 0 });
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
            Some(8) if u.px().0 < 0xb40 && u.px().1 > 0x160 => Some((0xb90, 0x110)),
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
            self.say(b.side, msg::BUILDING_UPGRADED);
        }
        if repaired {
            self.say(b.side, if turret { msg::TURRET_REPAIRED } else { msg::BUILDING_REPAIRED });
        }
    }

    /// Win and lose rules (`0x4212f0`): the player loses without a headquarters; the enemy loses when it has no headquarters
    /// (missions 4, 8, 13, 16, 21 and 22: no units and no buildings at all). A countdown that runs out ends the game by its
    /// mode (header word 67: 0 victory, 1 defeat, 2 only reinforcements).
    fn check_outcome(&mut self) {
        if self.tutorial.is_some() {
            return; // the tutorial has no HQ rules (`0x42e306`): it ends through its own script
        }
        let hq = |o: Owner| self.structures.iter().any(|s| s.kind_id == 100 && s.owner == o && s.alive());
        let had_player_hq = self.structures.iter().any(|s| s.kind_id == 100 && s.owner == Owner::Player);
        let destroy_all = matches!(self.mission_no, Some(4 | 8 | 13 | 16 | 21 | 22));
        let enemy_left = self.units.iter().any(|u| u.player != 0 && u.hp > 0) || self.structures.iter().any(|s| s.alive() && s.owner == Owner::Enemy);
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
        if let Some((min, sec, mode)) = self.timer {
            if !self.timer_expired {
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
                if mode == 2 && s == 1 {
                    match m {
                        20 => self.say(0, 14),
                        10 => self.say(0, 15),
                        5 => self.say(0, 16),
                        2 => self.say(0, 17),
                        _ => {}
                    }
                }
                if m == 0 && s == 0 {
                    self.timer_expired = true;
                    if mode == 2 {
                        self.reinforcements();
                    }
                }
            }
        }
        // mission 1: raising both tech-levels of the medium factory pays out the air forces (8 fighters, 3 carryalls)
        if self.mission_no == Some(1) && !self.timer_expired {
            let done = self.structures.iter().any(|s| s.owner == Owner::Player && s.kind_id == 102 && s.slots.len() >= 2 && s.slots[0].level >= s.slots[0].max_level && s.slots[1].level >= s.slots[1].max_level && s.slots[0].max_level > 0 && s.slots[1].max_level > 0);
            if done {
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
        let (credits, research, stock, drops): (i32, i32, [u32; 5], &[(i32, i32)]) = match self.mission_no {
            Some(7) => (5000, 0, [0; 5], &[(0x8c0, 0x600), (0x9a0, 0x480)]),
            Some(12) => (10_000, 2500, [3, 3, 0, 0, 0], &[(0x600, 0x4c0), (0x640, 0x420), (0x600, 0x2e0)]),
            Some(17) => (5000, 5000, [0; 5], &[(0x9a0, 0x560), (0xb40, 0x660)]),
            _ => return,
        };
        self.say(0, msg::REINFORCEMENTS);
        self.credits[0] = (self.credits[0] + credits).min(CREDITS_CAP);
        self.research[0] = (self.research[0] + research).min(RESEARCH_CAP);
        for g in 0..5 {
            self.air_stock[0][g] += stock[g];
        }
        for &(x, y) in drops {
            let mut a = Aircraft::launch(AirForce::Carryall, x, y, 0, self.cells.0, self.carry_counter);
            a.id = self.new_air_id();
            self.carry_counter = launch_counter(self.carry_counter);
            self.stats[0][6] += 1;
            self.aircraft.push(a);
        }
    }
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
        w.add_structure(Structure { object: 0, kind: ObjectKind::LightFactory, owner: Owner::Player, cell: (5, 5), hp: 100, max_hp: 100, aim: None, weapon: 0, target: None, slots, active: 1, kind_id: 101, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None });
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
            w.add_structure(Structure { object: 0, kind: ObjectKind::LightFactory, owner: Owner::Player, cell: (5, 5), hp: 100, max_hp: 100, aim: None, weapon: 0, target: None, slots, active: 1, kind_id: 101, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None });
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
            w.add_structure(Structure { object: 0, kind: ObjectKind::LightFactory, owner, cell: (5, 5), hp: 100, max_hp: 100, aim: None, weapon: 0, target: None, slots, active: 1, kind_id: 101, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None });
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
            w.add_structure(Structure { object: 0, kind: ObjectKind::LightFactory, owner, cell: (8, 5), hp: 400, max_hp: 400, aim: None, weapon: 0, target: None, slots: Vec::new(), active: 0, kind_id: 101, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None });
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
        w.add_structure(Structure { object: 0, kind: ObjectKind::LightFactory, owner: Owner::Neutral, cell: (25, 25), hp: 400, max_hp: 400, aim: None, weapon: 0, target: None, slots: Vec::new(), active: 0, kind_id: 101, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None });
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
            let s = &w.structures[0];
            assert_eq!(s.owner, winner, "blue {blue} green {green}");
        }
    }

    /// `0x458930`: a building's quake lasts 0x57 / 2 = 43 ticks of steps (2..=9 px each axis, magnitude shrinking with the
    /// distance to the screen centre), then the steps are undone one per tick; a turret's lasts 15; QUAKE off does nothing.
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
        assert_eq!(w.structures[0].owner, Owner::Player, "captured");
        w.explode(x, y, 2, 100, 0); // a second shell of the same volley lands on the 1-hp rebuild
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
            assert_eq!(w.structures[0].owner, Owner::Enemy, "{owner:?}");
            assert!(w.structures[0].hp > 0);
        }
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
        w.add_structure(Structure { object: 0, kind: ObjectKind::Headquarters, owner: Owner::Enemy, cell: (8, 5), hp: 50, max_hp: 50, aim: None, weapon: 0, target: None, slots: Vec::new(), active: 0, kind_id: 100, tally: [0; 3], heal: 0, upgrade: 0, protected: true, attacked: false, sat_cd: 0, advancer: None, rally: None });
        for _ in 0..4000 {
            w.step();
        }
        assert_eq!(w.outcome, Some(Outcome::Victory));
    }

    fn factory(owner: Owner, hp: i32, kind_id: i32) -> Structure {
        let slots = vec![SlotState { kind: 7, progress: 0, target: 2000, level: 0, max_level: 0, waiting: 0 }, SlotState { kind: 13, progress: 0, target: 2000, level: 0, max_level: 0, waiting: 0 }];
        Structure { object: 0, kind: ObjectKind::LightFactory, owner, cell: (6, 5), hp, max_hp: 13536, aim: None, weapon: 0, target: None, slots, active: 1, kind_id, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None, rally: None }
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
        assert_eq!(w.air_stock[0][0], 0);
        for _ in 0..1500 {
            w.step();
        }
        assert!(w.structures[1].hp <= 900 - 3 * 110, "the carpet bombing hit the turret (hp {})", w.structures[1].hp);
        assert!(w.aircraft.is_empty(), "the bomber left the map");
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
    }
}
