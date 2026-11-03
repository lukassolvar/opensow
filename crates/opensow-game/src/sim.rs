//! The skirmish simulation: units walk, weapon tops aim and fire, bullets fly and explode, structures take damage.
//!
//! Everything here is pure (no I/O, no rendering) and built from the decoded game rules: chassis animation
//! (`anim`), weapon tops (`aim`), target search over range rings (`rings`), bullet paths and area damage
//! (`projectile`). Values the exe analysis has not produced are placeholders and marked `PLACEHOLDER`:
//! movement speed, the weapons of static turrets and the enemy's behaviour.

use opensow_formats::advancer::{building_max_hp, capture_winner, hp_per_event, rate as advancer_rate, removed_when_destroyed, upgrade_per_event, Action, HEAL_PER_TICK};
use opensow_formats::aim::{aim_params, TopAim};
use opensow_formats::buildings::{blocked_cells, can_place, footprint};
use opensow_formats::air::{AirForce, HEADER_ORDER};
use opensow_formats::aircraft::{launch_counter, squad, Aircraft, Event as AirEvent};
use opensow_formats::anim::{walk_params, ChassisAnim, HOVER_ALTITUDE, HOVER_PERIOD, HOVER_PHASES, HOVER_TABLE};
use opensow_formats::effects::{blast, impact_anims, impact_leaves_decal, impact_smoke, smoke_puff, UNIT_DEATH_PUFFS, AnimSpec, BulletAnim, AIR_DESTROYED, BUILDING_DESTROYED, TURRET_DESTROYED, UNIT_CLOUD, UNIT_WRECK, debris, DEBRIS_DELAY, UNIT_DEATH_CLOUDS};
use opensow_formats::facing::{facing, heading_degrees, turn_step};
use opensow_formats::messages as msg;
use opensow_formats::level::{Level, ObjectKind, Owner, UnitKind};
use opensow_formats::production::{factory_exit, income_per_tick, next_tier, rate_row, slot_target, slot_unlock_cost, turret_price, Slots, CREDITS_CAP, RESEARCH_CAP};
use opensow_formats::projectile::{TRIPLER_BLAST_STRENGTH, TRIPLER_RING, TRIPLER_RING_USED, build_path, bullet_type, splash_victims, steps_at, weapon_shot, Path, UnitRef};
use opensow_formats::rings::Rings;
use opensow_formats::wave::{approach, Wave};
use opensow_formats::rng::GameRng;
use std::collections::HashMap;

pub const CELL: i32 = 32;
/// Game ticks per second: the exe's frame accumulator (0x42e890) emits one tick per 1666 hundredths of a millisecond = 16.66 ms.
pub const TICKS_PER_SECOND: u32 = 60;
/// Ticks per facing step of the always-spinning Advancer (3 ticks = 50 ms, measured from a recording).
pub const ADVANCER_SPIN_TICKS: u64 = 3;
/// Ticks between Advancer beam shots: the beam state machine (`0x47c6b0`) counts a length of 10 (`[beam+0x20]`) down once per tick.
pub const ADVANCER_BEAM_TICKS: u32 = 10;
/// Ticks between satellite checks of a protected building (`[bld+0x11c]` reloads to 240).
pub const SATELLITE_TICKS: i32 = 240;
/// Anti-air weapons never fire at ground targets (they use the air search functions of the exe).
const ANTI_AIR: [i32; 5] = [1, 2, 3, 42, 110];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Unit(u32),
    Structure(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Victory,
    Defeat,
}

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
    /// Hover bob phase of the Advancer (`[obj+0x84]`).
    pub hover_phase: u32,
    /// The Advancer's building order (destroy / repair / upgrade) and the structure it targets (docs/advancer-actions.md).
    pub action: Option<(Action, usize)>,
    /// Ticks until the Advancer's next beam shot.
    pub fire_cooldown: u32,
    /// Target the unit chases (`[unit+0x7c]`, set by the AI order `0x469fe0`).
    pub chase: Option<Target>,
    /// Advancer idle counter (`[top+0x150]`): at 420 the player hears "Advancer disengaged" and it restarts from -840.
    pub idle: i32,
}

/// Bresenham state of the path segment a unit is walking.
#[derive(Clone, Copy)]
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

    pub fn cell(&self) -> (i32, i32) {
        (self.x.floor() as i32 / CELL, self.y.floor() as i32 / CELL)
    }
}

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
}

/// One production slot: the unit kind, progress and the target the progress must reach.
#[derive(Clone, Copy, Debug)]
pub struct SlotState {
    pub kind: i32,
    pub progress: u32,
    pub target: u32,
    /// Tech-levels bought so far (`[slot+0x20]`) and the most that can be bought (`[slot+0x24]`, level word 8-12).
    pub level: u32,
    pub max_level: u32,
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

    /// The cell in the middle of the footprint, where the Advancer hovers.
    pub fn center_cell(&self) -> (i32, i32) {
        let (w, h) = self.size();
        (self.cell.0 + w / 2, self.cell.1 + h / 2)
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
}

/// A Rogon bomb explosion (event type 35, `0x411220`): after an initial burst of 19 steps it advances one step per tick along the
/// range-ring offset list, each step an area damage of `370 - k` at the cell `k` words into the list (read as overlapping
/// `(w[k], w[k+1])` pairs, a quirk of the exe), until `NUKE_STEPS` words are used (the list up to ring 7).
pub struct Nuke {
    pub x: i32,
    pub y: i32,
    pub owner: i32,
    counter: i32,
    k: usize,
}

pub const NUKE_START_COUNTER: i32 = 20;

pub struct Footprint {
    cell: (i32, i32),
    size: (i32, i32),
}

impl Footprint {
    fn covers(&self, c: (i32, i32)) -> bool {
        c.0 >= self.cell.0 && c.0 < self.cell.0 + self.size.0 && c.1 >= self.cell.1 && c.1 < self.cell.1 + self.size.1
    }
}

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
}

impl Bullet {
    pub fn new(bullet_type: u8, strength: i32, owner: i32, path: Path) -> Bullet {
        let index = path.first_step;
        let anim = crate::sim::bullet_type(bullet_type).and_then(|t| t.anim).map(BulletAnim::new);
        let (a, b) = (path.points[0], *path.points.last().unwrap());
        let facing = facing(a.0, a.1, b.0, b.1, 32) as u8;
        Bullet { bullet_type, strength, owner, path, index, anim, facing }
    }

    pub fn position(&self) -> (i32, i32) {
        self.path.points[self.index.min(self.path.points.len() - 1)]
    }
}

/// A scorch mark on the ground (`effects::impact_leaves_decal`): frame 0-4 of the decal sprite, one per cell.
#[derive(Debug, Clone, Copy)]
pub struct Decal {
    pub x: i32,
    pub y: i32,
    pub cell: (i32, i32),
    pub frame: u8,
}

/// An animation sprite playing at a map position (explosions, smoke, wrecks; `effects::AnimSpec`).
#[derive(Debug, Clone, Copy)]
pub struct Effect {
    pub x: i32,
    pub y: i32,
    pub spec: AnimSpec,
    pub age: u32,
}

/// The grids of the exe's local avoidance search (docs/walking.md).
pub struct Occupancy {
    ground: Wave,
    walker: Wave,
    stat: Vec<u32>,
    /// `[0x49a360]`, the base of the static grid.
    stat_base: u32,
}

pub struct World {
    pub cells: (i32, i32),
    blocked: Vec<bool>,
    pub units: Vec<Unit>,
    pub structures: Vec<Structure>,
    pub bullets: Vec<Bullet>,
    pub effects: Vec<Effect>,
    /// Sound effects raised this tick (effect id, map x, map y, pitch jitter); the game drains them (`sfx.rs`).
    pub sounds: Vec<(u8, i32, i32, bool)>,
    pub decals: Vec<Decal>,
    /// The exe's visual generator (`0x437cd0`), kept apart from the lockstep one.
    fx_rng: u32,
    /// Built lazily once per tick.
    occ: Option<Occupancy>,
    pub tick: u64,
    pub outcome: Option<Outcome>,
    next_id: u32,
    rng: u32,
    game_rng: Option<GameRng>,
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
    /// The computer plays the enemy (every mission except the tutorial; `0x42dc22`).
    pub ai: bool,
    /// Air-force stock of the player (0) and the enemy (1) in `HEADER_ORDER` (bomber, asteroid, carryall, tripler, fighter).
    pub air_stock: [[u32; 5]; 2],
    pub aircraft: Vec<Aircraft>,
    carry_counter: usize,
    /// Detonating Rogon bombs (event type 35).
    pub nukes: Vec<Nuke>,
    /// Messages for the local player: (tick, event id of `formats::messages`).
    pub log: Vec<(u64, u8)>,
    /// Level header words 34 / 35: no replacement Advancer for the player / the enemy (`[0x5baab8 + p*4]`).
    pub no_advancer_respawn: [bool; 2],
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
        World {
            cells,
            blocked,
            units: Vec::new(),
            structures: Vec::new(),
            bullets: Vec::new(),
            effects: Vec::new(),
            sounds: Vec::new(),
            decals: Vec::new(),
            fx_rng: 0x9E3779B9,
            occ: None,
            tick: 0,
            outcome: None,
            next_id: 1,
            rng: 0x2545_f491,
            game_rng: None,
            rings: Rings::generate(),
            has_enemy_hq: false,
            rates: [40; 3],
            repair_mode: false,
            credits: [0; 2],
            research: [0; 2],
            tutorial_step: None,
            ai: false,
            air_stock: [[0; 5]; 2],
            aircraft: Vec::new(),
            carry_counter: 0,
            nukes: Vec::new(),
            log: Vec::new(),
            no_advancer_respawn: [false; 2],
            mission_no: None,
            timer: None,
            timer_expired: false,
            ai_strategy: 0,
            ai_timer: 0,
            ai_strike: 600,
            wind: [0; 2],
            enemy_at_start: None,
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
        world.rates = [advancer_rate(level, 0), advancer_rate(level, 1), 40];
        world.ai = true;
        world.no_advancer_respawn = [level.header[34] != 0, level.header[35] != 0];
        if level.header[65] != 0 {
            world.timer = Some((level.header[66], 0, level.header[67]));
        }
        for (g, st) in level.air_stock().iter().enumerate() {
            world.air_stock[0][g] = st.counts[0].max(0) as u32;
            world.air_stock[1][g] = st.counts[1].max(0) as u32;
        }
        // header words 2/3 = credits, 37/38 = research points of player/enemy (`0x41d9c5..0x41d9f5`)
        world.credits = [level.header[2], level.header[3]];
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
                slots = rec.kinds.iter().zip(rec.values).filter_map(|(&k, v)| slot_target(k).filter(|_| k != 0).map(|target| SlotState { kind: k, progress: 0, target, level: 0, max_level: v.max(0) as u32 })).collect();
                active = (rec.active as usize).min(slots.len());
            }
            world.add_structure(Structure { object: i, kind, owner: o.owner(), cell: (o.cell_x, o.cell_y), hp: o.hp(), max_hp: building_max_hp(o.kind).max(o.hp()), aim, weapon, target: None, slots, active, kind_id: o.kind, tally: [0; 3], heal: 0, upgrade: 0, protected: o.raw[15] == 1 || o.kind == 100, attacked: false, sat_cd: 0, advancer: None });
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
        self.units.push(Unit { id, kind, player, x: x as f32, y: y as f32, hp: max_hp, max_hp, chassis, aim, path: Vec::new(), target: None, selected: false, moved: false, line: None, hover_phase, action: None, fire_cooldown: 0, chase: None, idle: 0 });
        id
    }

    fn cell_index(&self, cx: i32, cy: i32) -> Option<usize> {
        (cx >= 0 && cy >= 0 && cx < self.cells.0 && cy < self.cells.1).then(|| (cy * self.cells.0 + cx) as usize)
    }

    pub fn is_blocked(&self, cx: i32, cy: i32) -> bool {
        self.cell_index(cx, cy).map_or(true, |i| self.blocked[i])
    }

    /// Installs the exe's random table (`Data/generic.file`); without it a xorshift generator is used.
    pub fn set_game_rng(&mut self, rng: GameRng) {
        self.game_rng = Some(rng);
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
        for u in self.units.iter_mut().filter(|u| u.selected && u.player == 0) {
            u.chase = None;
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
    pub fn order_attack_selected(&mut self, target: Target) {
        for u in self.units.iter_mut().filter(|u| u.selected && u.player == 0 && u.kind != 40) {
            u.chase = Some(target);
        }
    }

    /// The hostile unit or structure under map pixel `(x, y)` for the player, if any.
    pub fn enemy_at(&self, x: i32, y: i32) -> Option<Target> {
        let near = self.units.iter().filter(|u| u.hp > 0 && u.player != 0).find(|u| ((u.x - x as f32).powi(2) + (u.y - y as f32).powi(2)).sqrt() < 20.0).map(|u| Target::Unit(u.id));
        near.or_else(|| self.structure_at(x, y).filter(|&k| self.structures[k].owner != Owner::Player).map(Target::Structure))
    }

    /// Walks unit `id` to `cell`, ending at the pixel `exact` when given and reachable.
    pub fn order_move(&mut self, id: u32, cell: (i32, i32), exact: Option<(i32, i32)>) {
        let Some(u) = self.unit(id) else { return };
        if u.kind == 40 {
            // the Advancer flies straight over everything
            let to = exact.map_or(centre(cell), |(x, y)| (x as f32, y as f32));
            self.units.iter_mut().find(|u| u.id == id).unwrap().path = vec![to];
            return;
        }
        let start = u.cell();
        let mut path = match self.find_path(start, cell) {
            Some(p) => p,
            None => return,
        };
        if let (Some((ex, ey)), Some(last)) = (exact, path.last_mut()) {
            *last = (ex as f32, ey as f32);
        }
        let u = self.units.iter_mut().find(|u| u.id == id).unwrap();
        u.path = path;
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
        let goal = if self.is_blocked(goal.0, goal.1) { self.nearest_free(goal)? } else { goal };
        if start == goal {
            return Some(vec![centre(goal)]);
        }
        let mut wave = Wave::new(|x, y| self.is_blocked(x, y));
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
        if self.units[i].kind == 46 {
            // a Rogon bomb has to touch its target (0x46df40), so it walks onto the target's cell
            let id = self.units[i].id;
            self.order_move(id, tcell, None);
            return;
        }
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
        let dims = self.cells;
        let id = self.units[i].id;
        let dest = self.occupancy().and_then(|o| approach(&mut o.stat, &mut o.stat_base, dims, at, tcell, handle, range));
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
            let mut ground = Wave::new(|_, _| false);
            let mut walker = Wave::new(|_, _| false);
            ground.cells = stat.clone();
            walker.cells = stat.clone();
            for u in self.units.iter().filter(|u| u.hp > 0 && u.kind != 40) {
                let (x, y) = u.cell();
                if (0..128).contains(&x) && (0..128).contains(&y) {
                    let grid = if (41..=45).contains(&u.kind) { &mut walker } else { &mut ground };
                    grid.cells[(x * 128 + y) as usize] = 0x3000000 + u.id;
                }
            }
            self.occ = Some(Occupancy { ground, walker, stat, stat_base: 1 });
        }
        self.occ.as_mut()
    }

    /// Re-plans the first stretch of unit `i`'s route around other units and buildings (`0x473190` -> `0x476870`): the cells
    /// up to six ahead are replaced by the route of a local wave over the occupancy grid.
    /// Returns `true` when no way around the units in front was found: the unit waits where it is.
    fn avoid(&mut self, i: usize) -> bool {
        let (at, own, walker, cells) = {
            let u = &self.units[i];
            let cells: Vec<(i32, i32)> = u.path.iter().map(|&(x, y)| ((x.floor() as i32).div_euclid(CELL), (y.floor() as i32).div_euclid(CELL))).collect();
            (u.cell(), 0x3000000 + u.id, (41..=45).contains(&u.kind), cells)
        };
        let mut route = vec![at];
        route.extend(&cells);
        let k = cells.len().min(6);
        let Some(occ) = self.occupancy() else { return false };
        let Occupancy { ground, walker: wgrid, stat, stat_base } = occ;
        let r = if walker { wgrid } else { ground }.local_step(stat, *stat_base, own, &route, at);
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
            return route.len() > 1 && route[k] != at;
        }
        false
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

    pub fn clear_selection(&mut self) {
        self.units.iter_mut().for_each(|u| u.selected = false);
    }

    /// Selects the player's units inside the pixel rectangle; returns how many are selected afterwards.
    pub fn select_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, additive: bool) -> usize {
        if !additive {
            self.clear_selection();
        }
        let (lx, hx, ly, hy) = (x0.min(x1) as f32, x0.max(x1) as f32, y0.min(y1) as f32, y0.max(y1) as f32);
        for u in self.units.iter_mut().filter(|u| u.player == 0) {
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
            self.units[i].selected = true;
        }
        self.units.iter().filter(|u| u.selected).count()
    }

    pub fn select_all(&mut self) {
        self.units.iter_mut().for_each(|u| u.selected = u.player == 0);
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
        if self.ai {
            self.step_ai(1);
        }
        self.step_aircraft();
        self.step_actions();
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
            self.nukes.push(Nuke { x, y, owner: u.player.min(1), counter: NUKE_START_COUNTER, k: 0 });
        }
        let dying: Vec<(i32, i32, i32, i32)> = self.units.iter().filter(|u| u.hp <= 0).map(|u| (u.px().0, u.px().1, u.kind, u.player)).collect();
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
                let off = |w: &mut World| (2 * w.random(2) as i32 - 1) * (w.random(d.spread as u32) as i32 + d.spread);
                let (sx, sy) = (off(self), off(self));
                let bt = if player == 0 { 15 } else { 16 };
                if let Some(path) = build_path(bt, x, y, x + sx, y + sy, DEBRIS_DELAY as u32) {
                    if !path.points.is_empty() {
                        self.bullets.push(Bullet::new(bt, 0, player.min(1), path));
                    }
                }
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
            // the unit appears at the factory's exit cell and walks to the second exit cell (`0x408740`); while the
            // exit is blocked the slot waits (the exe retries every tick)
            let kind_id = self.structures[i].kind_id;
            let (spawn, walk) = factory_exit(kind_id).unwrap_or(((1, 1), (3, 3)));
            let exit = (cell.0 + spawn.0, cell.1 + spawn.1);
            if !self.units.iter().any(|u| u.hp > 0 && u.cell() == exit) {
                let id = self.add_unit(kind, player, exit.0 * CELL + CELL / 2, exit.1 * CELL + CELL / 2);
                self.order_move(id, (cell.0 + walk.0, cell.1 + walk.1), None);
                self.structures[i].slots[j].progress = 0;
            }
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
                let o = self.structures[i].owner;
                self.structures[i].alive() && if player == 0 { o == Owner::Enemy } else { o == Owner::Player }
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
        // weapon top
        let mut spawn: Vec<((i32, i32), (i32, i32))> = Vec::new();
        if self.units[i].aim.is_some() && !ANTI_AIR.contains(&kind) {
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
            let face = ai.map(|a| facing(ux, uy, self.aircraft[a].x, self.aircraft[a].y, steps));
            let mut r = self.random(1 << 16);
            let ev = self.units[i].aim.as_mut().unwrap().tick(face, |n| {
                r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (r >> 8) % n.max(1)
            });
            if let (Some(_), Some(a)) = (ev.shot, ai) {
                if let Some((_, strength)) = weapon_shot(kind) {
                    self.aircraft[a].hp -= strength as i32;
                }
            }
        } else {
            self.units[i].target = None;
        }
        for (from, to) in spawn {
            self.fire(kind, player, from, to);
        }
        // a Rogon bomb (kind 46) blows up when it touches its target (0x46df40: the target id is on its cell in one of the occupancy layers)
        if kind == 46 {
            if let Some(t) = self.units[i].chase {
                let touching = match t {
                    Target::Unit(id) => self.unit(id).is_some_and(|u| u.hp > 0 && u.cell() == cell),
                    Target::Structure(k) => self.structures.get(k).is_some_and(|s| s.alive() && s.covers(cell)),
                };
                if touching {
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
        let hold = self.units[i].kind != 40 && self.units[i].line.is_none() && !self.units[i].path.is_empty() && self.avoid(i);
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
                        u.path.remove(0);
                        u.line = None;
                        continue;
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
        if let Some(c) = u.chassis.as_mut() {
            c.tick(moved);
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
        let (min, max, steps) = {
            let p = &self.structures[i].aim.as_ref().unwrap().params;
            (p.min_range, p.max_range, p.steps)
        };
        let cell = self.structures[i].cell;
        if self.structures[i].weapon == 110 {
            // the anti-air turret only shoots aircraft
            let (sx, sy) = self.structures[i].center();
            let ai = self.aircraft_in_range(owner_player, (sx, sy), min, max);
            let face = ai.map(|a| facing(sx, sy, self.aircraft[a].x, self.aircraft[a].y, steps));
            let mut r = self.random(1 << 16);
            let ev = self.structures[i].aim.as_mut().unwrap().tick(face, |n| {
                r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (r >> 8) % n.max(1)
            });
            if let (Some(_), Some(a)) = (ev.shot, ai) {
                if let Some((_, strength)) = weapon_shot(110) {
                    self.aircraft[a].hp -= strength as i32;
                }
            }
            return;
        }
        let t = self.find_target(index, owner_player, cell, min, max);
        self.structures[i].target = t;
        let (sx, sy) = self.structures[i].center();
        let tpos = t.and_then(|t| self.target_pos(t));
        let face = tpos.map(|(tx, ty)| facing(sx, sy, tx, ty, steps));
        let mut r = self.random(1 << 16);
        let weapon = self.structures[i].weapon;
        let ev = self.structures[i].aim.as_mut().unwrap().tick(face, |n| {
            r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (r >> 8) % n.max(1)
        });
        if let (Some((_, shots)), Some(to)) = (ev.shot, tpos) {
            for _ in 0..shots.max(1) as u32 * burst(weapon) {
                self.fire(weapon, owner_player, (sx, sy), to);
            }
            if let Some((id, jitter)) = opensow_formats::sfx::muzzle(weapon as u8) {
                self.sound(id, sx, sy, jitter);
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
        let mut landed: Vec<(i32, i32, u8, i32, i32)> = Vec::new();
        self.bullets.retain_mut(|b| {
            let speed = bullet_type(b.bullet_type).and_then(|t| t.speed).unwrap_or(60);
            b.index += steps_at(speed, t) as usize;
            if let Some(a) = &mut b.anim {
                if !a.step(b.bullet_type, 2 * b.index as i32) {
                    return false; // a one-shot animation (types 22, 23) has played out
                }
            }
            if b.index >= b.path.points.len() {
                let (x, y) = *b.path.points.last().unwrap();
                landed.push((x, y, b.bullet_type, b.strength, b.owner));
                false
            } else {
                true
            }
        });
        for (x, y, bt, strength, owner) in landed {
            for spec in impact_anims(bt) {
                self.effects.push(Effect { x, y, spec, age: 0 });
            }
            self.impact_ground_effects(x, y, bt);
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
            if s.alive() && s.covers(cell) {
                s.hp -= strength;
                s.tally[owner.clamp(0, 1) as usize] += strength;
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
        let winner = if removed_when_destroyed(s.kind_id) { None } else { capture_winner(s.tally, owner) };
        let turret = matches!(self.structures[i].kind, ObjectKind::Turret(_));
        let (cx, cy) = self.structures[i].center();
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

    /// What the Advancer does when it is ordered onto structure `target` (the exe lets the UI choose: enemy or neutral
    /// buildings are attacked, own ones repaired or upgraded, RMB toggles the mode).
    pub fn action_for(&self, unit: &Unit, target: usize) -> Option<Action> {
        let s = self.structures.get(target)?;
        if !s.alive() {
            return None;
        }
        let me = if unit.player == 0 { Owner::Player } else { Owner::Enemy };
        if s.owner != me {
            return Some(Action::Destroy);
        }
        if s.kind_id == 100 {
            return None; // the exe shows no cursor for an own headquarters
        }
        let upgradable = s.active < s.slots.len();
        let damaged = s.hp < s.max_hp;
        match (self.repair_mode, upgradable, damaged) {
            (true, _, true) | (false, false, true) => Some(Action::Repair),
            (_, true, _) => Some(Action::Upgrade),
            _ => None,
        }
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
        let cell = self.structures[target].center_cell();
        self.order_move(id, cell, None);
        let u = self.units.iter_mut().find(|u| u.id == id).unwrap();
        u.action = Some((action, target));
        u.fire_cooldown = 0;
        true
    }

    /// The structure under map pixel `(x, y)`, if any.
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
        let weapon = turret_weapon(tier);
        let aim = TopAim::new(weapon, 0);
        let max_hp = building_max_hp(24 + tier as i32);
        let owner = if side == 0 { Owner::Player } else { Owner::Enemy };
        self.sound(72, cell.0 * CELL + CELL / 2, cell.1 * CELL + CELL / 2, false);
        self.add_structure(Structure { object: usize::MAX, kind: ObjectKind::Turret(tier as u8), owner, cell, hp: 1, max_hp, aim, weapon, target: None, slots: Vec::new(), active: 0, kind_id: 24 + tier as i32, tally: [0; 3], heal: max_hp - 1, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None });
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

    /// Nearest hostile unit / turret / other building to `cell` (the three classes of the exe's reachability search `0x472a90`).
    fn nearest_enemy(&self, side: usize, cell: (i32, i32)) -> (Option<Target>, Option<Target>, Option<Target>) {
        let d = |c: (i32, i32)| (c.0 - cell.0).abs().max((c.1 - cell.1).abs());
        let unit = self.units.iter().filter(|u| u.hp > 0 && (u.player != 0) != (side != 0)).min_by_key(|u| d(u.cell())).map(|u| Target::Unit(u.id));
        let me = if side == 0 { Owner::Player } else { Owner::Enemy };
        let near = |turret: bool| {
            self.structures.iter().enumerate().filter(|(_, s)| s.alive() && s.owner != me && s.owner != Owner::Neutral && matches!(s.kind, ObjectKind::Turret(_)) == turret).min_by_key(|(_, s)| d(s.cell)).map(|(i, _)| Target::Structure(i))
        };
        (unit, near(true), near(false))
    }

    /// The nearest enemy aircraft whose ground position is within `min..=max` cells of `pos`.
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
        let a = Aircraft::launch(force, x, y, side as i32, self.cells.0, self.carry_counter);
        if force == AirForce::Carryall {
            self.carry_counter = launch_counter(self.carry_counter);
        }
        self.aircraft.push(a);
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
        }
        for a in aircraft.iter().filter(|a| !a.alive()) {
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

    /// Advances the explosions (`0x411220`, type 35).
    fn step_nukes(&mut self) {
        let words: Vec<i32> = self.rings.offsets.iter().flat_map(|&(dx, dy)| [dx as i32, dy as i32]).collect();
        // the list used goes up to ring 7: `[0x6b3a34]` = 2 * (offsets up to and including ring 7)
        let limit = self.rings.ends.get(7).map_or(words.len(), |&e| (2 * e as usize).min(words.len()));
        let mut blasts: Vec<(i32, i32, i32, i32)> = Vec::new();
        self.nukes.retain_mut(|n| {
            n.counter += 1;
            while n.counter > 1 {
                n.k += 1;
                n.counter -= 1;
                if n.k + 1 >= limit {
                    return false;
                }
                let (dx, dy) = (words[n.k], words[n.k + 1]);
                blasts.push((n.x + 16 + dx * CELL, n.y + 16 + dy * CELL, 370 - n.k as i32, n.owner));
            }
            true
        });
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
                for u in self.units.iter_mut().filter(|u| u.kind == 40 && u.hp > 0 && s.covers(u.cell()) && (if u.player == 0 { 0 } else { 1 }) != owner) {
                    u.hp = 0;
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
            let Some((action, t)) = self.units[i].action else { continue };
            let u = &self.units[i];
            let side_u = if u.player == 0 { 0 } else { 1 };
            let still_valid = self.action_valid(u, action, t);
            let target_cell = self.structures.get(t).map(|s| s.center_cell());
            // a new move order (path set elsewhere) or an invalid target cancels the action
            if !still_valid || target_cell.is_none() || (u.path.is_empty() && u.cell() != target_cell.unwrap()) {
                self.units[i].action = None;
                continue;
            }
            if u.cell() != target_cell.unwrap() || !u.path.is_empty() {
                continue;
            }
            if self.units[i].fire_cooldown > 0 {
                self.units[i].fire_cooldown -= 1;
                continue;
            }
            self.units[i].fire_cooldown = ADVANCER_BEAM_TICKS - 1;
            let rate = self.rates[side_u];
            let (mut upgraded, mut repaired) = (false, false);
            let s = &mut self.structures[t];
            match action {
                Action::Destroy => {
                    s.attacked = true;
                    s.hp -= hp_per_event(rate);
                    s.tally[side_u] += hp_per_event(rate);
                    if s.hp <= 0 {
                        self.structure_destroyed(t);
                        self.units[i].action = None;
                    }
                }
                Action::Repair => {
                    s.hp = (s.hp + hp_per_event(rate)).min(s.max_hp);
                    s.tally[side_u] = (s.tally[side_u] - hp_per_event(rate)).max(0);
                    if s.hp >= s.max_hp {
                        self.units[i].action = None;
                        repaired = true;
                    }
                }
                Action::Upgrade => {
                    s.upgrade += upgrade_per_event(rate);
                    if let Some(slot) = s.slots.get(s.active) {
                        if s.upgrade >= slot.target as i32 {
                            s.active += 1;
                            s.upgrade = 0;
                            upgraded = true;
                        }
                    }
                    if s.active >= s.slots.len() {
                        self.units[i].action = None;
                    }
                }
            }
            let turret = matches!(self.structures[t].kind, ObjectKind::Turret(_));
            if upgraded {
                self.say(side_u, msg::BUILDING_UPGRADED);
            }
            if repaired {
                self.say(side_u, if turret { msg::TURRET_REPAIRED } else { msg::BUILDING_REPAIRED });
            }
        }
    }

    /// Win and lose rules (`0x4212f0`): the player loses without a headquarters; the enemy loses when it has no headquarters
    /// (missions 4, 8, 13, 16, 21 and 22: no units and no buildings at all). A countdown that runs out ends the game by its
    /// mode (header word 67: 0 victory, 1 defeat, 2 only reinforcements).
    fn check_outcome(&mut self) {
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
            let a = Aircraft::launch(AirForce::Carryall, x, y, 0, self.cells.0, self.carry_counter);
            self.carry_counter = launch_counter(self.carry_counter);
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
    fn the_advancer_flies_straight_and_always_spins() {
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
        assert_eq!(w.unit(id).unwrap().path.len(), 1, "one straight leg over the wall");
        for _ in 0..1000 {
            w.step();
        }
        assert_eq!(w.unit(id).unwrap().cell(), (10, 2));
    }

    #[test]
    fn a_factory_produces_a_unit_every_22_seconds() {
        let mut w = open_world(12, 12);
        w.tutorial_step = Some(1); // tutorial speed: 22 s per ground unit
        w.add_unit(10, 0, 16, 16); // keeps the player alive (no defeat)
        let slots = vec![SlotState { kind: 7, progress: 0, target: 2000, level: 0, max_level: 0 }];
        w.add_structure(Structure { object: 0, kind: ObjectKind::LightFactory, owner: Owner::Player, cell: (5, 5), hp: 100, max_hp: 100, aim: None, weapon: 0, target: None, slots, active: 1, kind_id: 101, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None });
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
        w.add_structure(Structure { object: 0, kind: ObjectKind::Headquarters, owner: Owner::Enemy, cell: (8, 5), hp: 50, max_hp: 50, aim: None, weapon: 0, target: None, slots: Vec::new(), active: 0, kind_id: 100, tally: [0; 3], heal: 0, upgrade: 0, protected: true, attacked: false, sat_cd: 0, advancer: None });
        for _ in 0..4000 {
            w.step();
        }
        assert_eq!(w.outcome, Some(Outcome::Victory));
    }

    fn factory(owner: Owner, hp: i32, kind_id: i32) -> Structure {
        let slots = vec![SlotState { kind: 7, progress: 0, target: 2000, level: 0, max_level: 0 }, SlotState { kind: 13, progress: 0, target: 2000, level: 0, max_level: 0 }];
        Structure { object: 0, kind: ObjectKind::LightFactory, owner, cell: (6, 5), hp, max_hp: 13536, aim: None, weapon: 0, target: None, slots, active: 1, kind_id, tally: [0; 3], heal: 0, upgrade: 0, protected: false, attacked: false, sat_cd: 0, advancer: None }
    }

    #[test]
    fn the_advancer_captures_an_enemy_building_and_it_heals_in() {
        let mut w = open_world(16, 12);
        w.add_structure(factory(Owner::Enemy, 400, 101));
        w.add_unit(7, 1, 15 * CELL, 11 * CELL); // keeps the mission from ending
        let id = w.add_unit(40, 0, 3 * CELL, 5 * CELL + 16);
        assert!(w.order_action(id, 0));
        for _ in 0..400 {
            w.step();
        }
        let s = &w.structures[0];
        assert_eq!(s.owner, Owner::Player, "captured");
        assert!(s.hp > 1000 && s.heal < s.max_hp, "hp {} heal {}", s.hp, s.heal);
        w.step();
        for _ in 0..200 {
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
        for _ in 0..600 {
            w.step();
        }
        assert!(w.unit(id).is_none(), "the Advancer is destroyed");
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
        base.slots = vec![SlotState { kind: 30, progress: 0, target: 4000, level: 0, max_level: 0 }];
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
}
