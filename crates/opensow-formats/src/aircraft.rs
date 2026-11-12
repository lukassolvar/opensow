//! Air-force behaviour: how a launched strike flies and what it fires (`State of War.exe`, class at `0x467240..0x468c30`).
//!
//! Field names follow the object offsets of the exe (`[obj+0x..]`), see docs/air.md. All positions are map pixels; `y` is
//! the ground y of the flight line and `altitude` is subtracted for the sprite. One call of [`Aircraft::step`] is one game
//! tick; `tick` is its index in the 60-tick speed cycle (see `projectile::steps_at`).
//!
//! * launch (`0x467ee0`) -> [`Aircraft::launch`]; per-tick update (`0x467b50`) -> [`Aircraft::step`];
//!   bomb/missile/pod/meteor/unload spawns (`0x468240`) -> [`Event`]; carryall squads (`0x468960`) -> [`squad`].
//! * The death sequence (`0x467850`, [`Aircraft::step`] -> `die`): explosions while falling, a final blast; an aircraft with
//!   `hp <= 0` is removed after `death_timer` ticks. Engine sounds and the fighter's smoke are ported; bullet damage is in `projectile`.

use crate::air::AirForce;
use crate::kinds;
use crate::projectile::steps_at;

/// Distance beyond the map edge where aircraft enter and leave.
const MARGIN: i32 = 400;

/// What an aircraft spawns while flying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A bullet created with `0x411dc0(type, strength, x1, y1, x2, y2, side)`.
    Shot { bullet_type: u8, strength: i32, from: (i32, i32), to: (i32, i32) },
    /// A carryall lands a squad on the 4x2 cells starting at `(cell_x - 1, cell_y)` (`squad` = index into [`squad`]).
    Unload { cell_x: i32, cell_y: i32, squad: usize },
    /// A falling aircraft's `e_air2` explosion sprite (`0x4054a0`, array `0x5e96f8`, 52 frames, `period` ticks each: 2 while it
    /// falls, 4 for the four of the final blast) at a map position.
    Fireball { x: i32, y: i32, period: u8 },
}

/// One aircraft of a launched strike, with the exe's per-object state (see the module docs for the field naming).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Aircraft {
    /// Handle of the aircraft for targeted hits (`[+0x1c]` in the exe, a number with side and class bits); here a number the
    /// simulation hands out (0 until it does).
    pub id: u32,
    /// Which air force this is (bomber, meteor storm, carryall, tripler, fighter).
    pub force: AirForce,
    /// The side that launched it (0 or 1).
    pub side: i32,
    /// `[+4]`, `[+8]`: position of the flight line (map pixels).
    pub x: i32,
    /// `[+8]`: y of the flight line (map pixels).
    pub y: i32,
    /// `[+0xc]`, `[+0x10]`: the strike's target point.
    pub target: (i32, i32),
    /// `[+0x14]`: height above the ground line.
    pub altitude: i32,
    /// `[+0x24]`: base speed (path steps per 60 ticks, see `projectile::steps_at`).
    pub speed: i32,
    /// `[+0x2c]`: hit points.
    pub hp: i32,
    /// `[+0x18]`: x offset of the point anti-air aims at (`0x479953`) and of the range test for a kept target (`0x47b461`): the
    /// bomber's +50 flying right / -50 flying left (`0x468063..0x468070`), 0 for the others (`0x467f4e`).
    #[serde(default)]
    pub aim_dx: i32,
    /// `[+0x34]`: true = flies towards +x.
    pub right: bool,
    /// `[+0x38]`: ticks a destroyed aircraft keeps falling before it is removed.
    pub death_timer: i32,
    /// `[+0x3c]`: ticks until the next explosion of the falling aircraft (0 at launch, so the first comes at once).
    pub cloud_timer: i32,
    /// `[+0x40]`, `[+0x44]`, `[+0x48]`, `[+0x4c]`: frame, frame count, countdown, reload of the main animation.
    pub frame: i32,
    /// `[+0x44]`: number of frames of the main animation.
    pub frames: i32,
    /// `[+0x48]`: ticks until the main animation's next frame.
    pub anim_count: i32,
    /// `[+0x4c]`: the value `anim_count` restarts from.
    pub anim_reload: i32,
    /// `[+0x70]`, `[+0x74]`: per-type counters (bomb timer / unload cooldown / shot cooldown, tripler firing state, ...).
    pub counter: i32,
    /// `[+0x74]`: the second per-type counter (see `counter`).
    pub state: i32,
    /// `[+0x78]`, `[+0x7c]`, `[+0x80]`: bomb travel, x and y offsets of the spawn point.
    pub reach: i32,
    /// `[+0x7c]`: x offset of the spawn point.
    pub off_x: i32,
    /// `[+0x80]`: y offset of the spawn point.
    pub off_y: i32,
    /// `[+0x84]`: the tripler has reached its target and may fire.
    pub armed: bool,
    /// `[+0x88]`: carryall squad index.
    pub squad: usize,
    /// Carryall exhaust (`carryAll_rear`, `[+0x50]` frame of 4, `[+0x58]` countdown, reload 2) and landing glow (`carryAll_down`,
    /// `[+0x60]` frame of 2, `[+0x68]` countdown, reload 4); both advance once per step (`0x467450`).
    pub rear_frame: i32,
    rear_count: i32,
    /// `[+0x60]`: frame of the landing glow (see `rear_frame`).
    pub down_frame: i32,
    down_count: i32,
    map_width_px: i32,
    /// `[+0x94]`: the bomber's drone countdown (a sound every 9 steps); also marks "sound already played" for the others.
    sound: i32,
    /// `[+0x8d]`: the carryall's departure sound has played.
    sound_out: bool,
    /// Sound effects the last steps asked for: `(effect id, x, y)` in map pixels. The game drains this.
    pub sounds: Vec<(u8, i32, i32)>,
}

/// The exe's global carryall counter (`[0x89cf50]`) after one more launch: it counts 0..9 and wraps to 0.
pub fn launch_counter(carry_counter: usize) -> usize {
    let n = carry_counter + 1;
    if n >= 10 {
        0
    } else {
        n
    }
}

impl Aircraft {
    /// Launches a strike. `map_cells` is the map width in cells (`[0x71cec0]`), `carry_counter` the exe's global carryall
    /// counter (`[0x89cf50]`, 0..9) which selects the squad: the returned aircraft's `squad` is the incremented value.
    pub fn launch(force: AirForce, target_x: i32, target_y: i32, side: i32, map_cells: i32, carry_counter: usize) -> Aircraft {
        let map_width_px = map_cells * 32;
        let right = target_x <= 0x400;
        let mut a = Aircraft {
            id: 0,
            force,
            side,
            x: if right { -MARGIN } else { map_width_px + MARGIN },
            y: target_y,
            target: (target_x, target_y),
            altitude: 200,
            speed: 120,
            hp: 0,
            aim_dx: 0,
            right,
            death_timer: 180,
            cloud_timer: 0,
            frame: 0,
            frames: 1,
            anim_count: 0,
            anim_reload: 1000,
            counter: 0,
            state: 0,
            reach: 0,
            off_x: 0,
            off_y: 0,
            armed: false,
            squad: 0,
            rear_frame: 0,
            rear_count: 0,
            down_frame: 0,
            down_count: 0,
            map_width_px,
            sound: 0,
            sound_out: false,
            sounds: Vec::new(),
        };
        match force {
            AirForce::Bomber => {
                a.hp = 480;
                a.frames = 3;
                a.anim_reload = 2;
                a.counter = 4;
                (a.reach, a.off_x, a.aim_dx) = if right { (45, 75, 50) } else { (-45, 70, -50) };
                a.off_y = 20;
            }
            AirForce::Carryall => {
                a.hp = 360;
                a.y = snap_to_cell_centre(target_y);
                a.target.1 = a.y;
                a.right = false;
                a.x = map_width_px + MARGIN;
                a.off_y = 32;
                a.squad = launch_counter(carry_counter);
            }
            AirForce::Tripler => {
                a.hp = 270;
                a.frames = 10;
                a.anim_reload = 4;
                a.off_y = 35;
            }
            AirForce::Fighter => {
                a.hp = 8000;
                a.altitude = 120;
                a.death_timer = 0;
                a.right = true;
                a.x = -MARGIN;
                a.off_y = 32;
            }
            AirForce::Asteroid => {
                a.hp = 120;
                a.x = 1000;
                a.y = -8000;
                a.counter = 60;
                a.state = 20;
            }
        }
        a
    }

    /// False once the aircraft left the map or finished falling.
    /// The countdowns of the exhaust and landing-glow animations (`[+0x58]`, `[+0x68]`), kept by a savegame.
    pub fn glow_counts(&self) -> (i32, i32) {
        (self.rear_count, self.down_count)
    }

    /// Sets the two countdowns of the carryall's exhaust and landing glow (used when a savegame is loaded).
    pub fn set_glow_counts(&mut self, rear: i32, down: i32) {
        self.rear_count = rear;
        self.down_count = down;
    }

    /// Whether the aircraft is still in the world: false once it is destroyed and has finished falling.
    pub fn alive(&self) -> bool {
        if self.hp <= 0 && self.death_timer <= 0 {
            return false;
        }
        match self.force {
            AirForce::Asteroid => self.state > 0,
            _ if !self.right => self.x >= -MARGIN,
            _ => self.x <= self.map_width_px + MARGIN,
        }
    }

    /// The destroyed aircraft's tick (`0x467850`, called every tick while `hp <= 0`). The cloud timer `[+0x3c]` counts down; at 0 it
    /// restarts at `30 + rand(5)`, sound 23 plays and one fireball appears: with chance 1/4 (`rand(4) == 0`) within +-50 px
    /// vertically and +-100 px horizontally of the aircraft, else within +-30 / +-60 (each offset `(rand(2) * 2 - 1) * rand(n)`, the
    /// vertical one drawn first). When the death timer `[+0x38]` runs out three more sounds and four slow fireballs (the wide
    /// spread) mark the end. All draws use the lockstep generator.
    fn die(&mut self, rand: &mut dyn FnMut(u32) -> i32, events: &mut Vec<Event>) {
        let (x, y) = (self.x, self.y - self.altitude);
        let spread = |rand: &mut dyn FnMut(u32) -> i32, ny: u32, nx: u32| {
            let sy = rand(2) * 2 - 1;
            let dy = rand(ny) * sy;
            let sx = rand(2) * 2 - 1;
            let dx = rand(nx) * sx;
            (x + dx, y + dy)
        };
        self.cloud_timer -= 1;
        if self.cloud_timer <= 0 {
            self.cloud_timer = 30 + rand(5);
            self.sounds.push((23, x, y));
            let (ny, nx) = if rand(4) == 0 { (50, 100) } else { (30, 60) };
            let (fx, fy) = spread(rand, ny, nx);
            events.push(Event::Fireball { x: fx, y: fy, period: 2 });
        }
        self.death_timer -= 1;
        if self.death_timer <= 0 {
            for _ in 0..3 {
                self.sounds.push((23, x, y));
            }
            for _ in 0..4 {
                let (fx, fy) = spread(rand, 50, 100);
                events.push(Event::Fireball { x: fx, y: fy, period: 4 });
            }
        }
    }

    /// Advances one game tick. `rand(n)` must return a value in `0..n` (the exe's lockstep generator `0x437b10`).
    pub fn step(&mut self, tick: u32, rand: &mut dyn FnMut(u32) -> i32) -> Vec<Event> {
        let mut events = Vec::new();
        if self.hp <= 0 {
            self.die(rand, &mut events);
            if self.death_timer <= 0 {
                return events;
            }
        }
        let sign = if self.right { 1 } else { -1 };
        match self.force {
            AirForce::Bomber => {
                self.animate();
                // the engine drone (sound 63) every 9th step (`0x467bde`)
                if self.hp > 0 {
                    self.sound -= 1;
                    if self.sound <= 0 {
                        self.sound = 9;
                        self.sounds.push((63, self.x, self.y - self.altitude));
                    }
                }
                let d = (self.x - self.target.0).abs();
                let slow = match d {
                    190.. => 0,
                    180..=189 => 15,
                    170..=179 => 30,
                    160..=169 => 40,
                    150..=159 => 50,
                    _ => 60,
                };
                self.x += sign * steps_at((self.speed - slow) as u32, tick) as i32;
                if self.hp > 0 && (self.reach - self.target.0 + self.x).abs() < 150 {
                    self.counter -= 1;
                    if self.counter <= 0 {
                        self.drop_bomb(rand, &mut events);
                    }
                }
            }
            AirForce::Carryall => {
                self.carryall_step(tick, &mut events);
            }
            AirForce::Fighter => {
                self.x += 12;
                if self.hp > 0 {
                    let ahead = self.x - self.target.0;
                    // the fly-by roar (sound 66, `0x46765d`): once, as it closes in (the exe gates it on the view; approximate)
                    if self.sound == 0 && ahead >= -400 {
                        self.sound = 1;
                        self.sounds.push((66, self.x, self.y - self.altitude));
                    }
                    if self.counter <= 0 && ahead >= -226 {
                        self.fire_missiles(&mut events);
                    }
                    if self.counter <= 5000 && ahead >= -182 {
                        self.fire_missiles(&mut events);
                    }
                    if self.counter <= 0 && ahead >= -206 {
                        self.fire_missiles(&mut events);
                    }
                }
            }
            AirForce::Tripler => self.tripler_step(&mut events),
            AirForce::Asteroid => {
                self.counter -= 1;
                if self.counter < 0 {
                    self.counter = 24;
                    self.meteor(rand, &mut events);
                    self.state -= 1;
                }
            }
        }
        events
    }

    fn animate(&mut self) {
        self.anim_count -= 1;
        if self.anim_count < 0 {
            self.anim_count = self.anim_reload;
            self.frame += 1;
            if self.frame >= self.frames {
                self.frame = 0;
            }
        }
    }

    fn drop_bomb(&mut self, rand: &mut dyn FnMut(u32) -> i32, events: &mut Vec<Event>) {
        self.counter = rand(2) + 4;
        let dx = rand(26) - 13;
        let dy = rand(40) - 20;
        events.push(Event::Shot {
            bullet_type: 18,
            strength: 110,
            from: (self.x + dx, self.y - self.altitude + dy),
            to: (self.x + self.reach + dx, self.y + dy),
        });
    }

    fn fire_missiles(&mut self, events: &mut Vec<Event>) {
        self.counter += 5000;
        let (x, up) = (self.x + self.off_x, self.y - self.altitude + self.off_y);
        let ground = self.y + self.off_y;
        events.push(Event::Shot { bullet_type: 26, strength: 150, from: (x - 4, up - 10), to: (x + 204, ground - 40) });
        events.push(Event::Shot { bullet_type: 26, strength: 150, from: (x, up + 10), to: (x + 204, ground - 20) });
    }

    fn meteor(&mut self, rand: &mut dyn FnMut(u32) -> i32, events: &mut Vec<Event>) {
        let dx = rand(300);
        let dy = rand(300);
        let medium = rand(2) != 0;
        let to = (self.target.0 - 150 + dx, self.target.1 - 150 + dy);
        events.push(Event::Shot { bullet_type: if medium { 24 } else { 25 }, strength: 500, from: (to.0 + 300, to.1 - 400), to });
    }

    fn carryall_step(&mut self, tick: u32, events: &mut Vec<Event>) {
        // the two exhaust animations count down and wrap (`0x467458..0x467494`)
        self.rear_count -= 1;
        if self.rear_count < 0 {
            self.rear_frame += 1;
            self.rear_count = 2;
            if self.rear_frame >= 4 {
                self.rear_frame = 0;
            }
        }
        self.down_count -= 1;
        if self.down_count < 0 {
            self.down_frame += 1;
            self.down_count = 4;
            if self.down_frame >= 2 {
                self.down_frame = 0;
            }
        }
        let d = (self.x - self.target.0).abs();
        let approaching = self.target.0 <= self.x;
        // body frame 0..4 from the distance (`0x4674c2..0x467565`): approaching (target at or left of x) and leaving use
        // different thresholds
        let thresholds: [i32; 4] = if approaching { [190, 175, 160, 145] } else { [190, 180, 170, 160] };
        self.frame = thresholds.iter().filter(|&&t| d < t).count() as i32;
        let lead = if d >= 120 { 0 } else { 119 - d };
        if (41..=199).contains(&d) {
            self.altitude = d;
        }
        self.x -= steps_at((self.speed - lead) as u32, tick) as i32;
        // the landing sound (62, `0x467592`, at the aircraft's position and altitude) plays twice per visit, each time once: while
        // approaching (target at or left of x) as soon as it is closer than 190 px (flag `[+0x8c]`), and while leaving (target right
        // of x) as soon as it climbs above 45 (flag `[+0x8d]`, `0x467570`)
        if approaching {
            if self.sound == 0 && d < 190 {
                self.sound = 1;
                self.sounds.push((62, self.x, self.y - self.altitude));
            }
        } else if self.altitude > 45 && !self.sound_out {
            self.sound_out = true;
            self.sounds.push((62, self.x, self.y - self.altitude));
        }
        if d < 5 && self.counter <= 0 {
            self.counter = 600;
            events.push(Event::Unload { cell_x: self.target.0 / 32, cell_y: self.target.1 / 32, squad: self.squad });
        }
    }

    fn tripler_step(&mut self, events: &mut Vec<Event>) {
        self.anim_count -= 1;
        if self.anim_count < 0 {
            self.anim_count = self.anim_reload;
            let d = (self.x - self.target.0).abs();
            let mut step = match d {
                170.. => 10,
                150..=169 => 9,
                130..=149 => 8,
                110..=129 => 7,
                90..=109 => 6,
                70..=89 => 5,
                50..=69 => 4,
                14..=49 => 3,
                7..=13 => 2,
                _ => 2,
            };
            if d < 7 && self.state < 100 {
                step = 0;
                self.armed = true;
            }
            self.x += if self.right { step } else { -step };
            self.frame += 1;
            if self.frame >= self.frames {
                self.frame = 0;
            }
        }
        if self.armed && self.hp > 0 {
            if self.counter > 0 {
                self.counter -= 1;
            } else {
                self.tripler_fire(events);
            }
        }
    }

    /// One pod of the six-shot strafe (`0x468240`, tripler case): each state fires on a fixed frame of the animation.
    fn tripler_fire(&mut self, events: &mut Vec<Event>) {
        // (state, required frame, y offset, x offset, next state, cooldown multiplier of the reload)
        const SHOTS: [(i32, i32, i32, i32, i32, i32); 6] =
            [(0, 0, -55, -80, 2, 3), (2, 0, 83, -23, 4, 3), (4, 0, -30, 96, 1, 13), (1, 6, 9, -109, 3, 3), (3, 6, 66, 69, 5, 3), (5, 6, -72, 41, 0x457, 0)];
        let Some(&(_, _, oy, ox, next, cool)) = SHOTS.iter().find(|s| s.0 == self.state && s.1 == self.frame) else { return };
        let x = self.x + self.off_x + ox;
        self.sounds.push((69, x, self.y - self.altitude + self.off_y + oy)); // `0x46843a`
        events.push(Event::Shot { bullet_type: 17, strength: 0, from: (x, self.y - self.altitude + 20 + self.off_y + oy), to: (x, self.y + self.off_y + oy) });
        self.state = next;
        if next == 0x457 {
            self.armed = false;
        } else {
            self.counter = self.anim_reload * cool;
        }
    }
}

fn snap_to_cell_centre(v: i32) -> i32 {
    v - v.rem_euclid(32) + 16
}

/// Units a carryall lands (`0x468960`), in the exe's order: `(dx, dy, unit kind)` relative to the target cell, columns
/// `-1..=2`, two rows each. `index` 0..=17 is the carryall's squad number (`[+0x88]`; the launch counter only produces
/// 0..=9). Kinds 1..=15 are ground units (class = (kind - 1) / 3, tier = (kind - 1) % 3 + 1), 41 a walker. A cell is only
/// filled when it holds no unit, is not blocked and has no building. Larger indices reset to 1 and use the squad
/// `[7,7,7,7,10,10,10,10]`; with the game-mode global `[0x49a33c] == 1` the squad is always [`MODE1_SQUAD`].
pub fn squad(index: usize) -> Option<Vec<(i32, i32, i32)>> {
    const KINDS: [&[i32]; 18] = [
        &[7, 7, 7, 7, 10, 10, 10, 10],
        &[2, 2, 2, 2],
        &[8, 8, 11, 11],
        &[4, 4, 7, 7, 13, 13, 10, 10],
        &[5, 8, 13, 11],
        &[1, 1, 1, 1, 1, 1, 1, 1],
        &[4, 4, 13, 13, 8, 8],
        &[10, 10, 10, 10, 14, 14],
        &[7, 7, 4, 4, 2, 2],
        &[1, 1, 1, 1, 5, 5],
        &[6, 6, 9, 9, 12, 12],
        &[8, 8, 8, 8, 14, 14, 14, 14],
        &[7, 7, 4, 4, 10, 10],
        &[1, 1, 1, 1],
        &[13, 13, 13, 13],
        &[7, 7, 4, 4, 13, 13],
        &[8, 8, 11, 11, 14, 14, 5, 5],
        &[41, 41],
    ];
    Some(KINDS.get(index)?.iter().enumerate().map(|(i, &k)| (i as i32 / 2 - 1, i as i32 % 2, k)).collect())
}

/// The squad used when the game-mode global `[0x49a33c]` is 1.
pub const MODE1_SQUAD: [i32; 6] = [7, 7, 7, 7, 10, 10];

/// One unit as the computer opponent's strike pick (`0x435ad0`) sees it.
#[derive(Debug, Clone, Copy)]
pub struct StrikeUnit {
    /// X position in map pixels.
    pub x: i32,
    /// Y position in map pixels.
    pub y: i32,
    /// Unit kind number.
    pub kind: i32,
    /// Belongs to the side that picks the target.
    pub own: bool,
    /// Standing at its destination (`[chassis+0x14/0x18] == [top+0x5c/0x60]`).
    pub idle: bool,
}

/// Where the computer opponent sends an air strike (`0x435ad0`, port of the pick, not emulation-verified).
/// 1. An idle pod (kind 46) of another side: its position.
/// 2. Otherwise a density map over a 24-column grid of 128-px blocks, `index = bx + 24 * by`, built from every unit but the
///    Advancer (40): an own unit subtracts 3 on its block, 2 on the blocks left and right, 1 above and below (indices `0..=480`
///    only); a foreign one adds `3w`, `2w`, `w` the same way with `w` = 2 for an idle walker (41..=45) else 1, but only for
///    indices `0..=256` (so the lower part of the map never attracts a strike), with no column check (a neighbour past
///    column 23 lands on the next row). The first cell with the highest value wins when it reaches 9 (the running best is
///    kept as a signed byte); the strike goes to the cell's centre.
pub fn ai_strike_target(units: &[StrikeUnit]) -> Option<(i32, i32)> {
    if let Some(u) = units.iter().find(|u| u.kind == kinds::POD && !u.own && u.idle) {
        return Some((u.x, u.y));
    }
    let mut grid = [0i32; 481];
    for u in units.iter().filter(|u| u.kind != kinds::ADVANCER) {
        let i = (u.x >> 7) + 24 * (u.y >> 7);
        let w = if kinds::WALKERS.contains(&u.kind) && u.idle { 2 } else { 1 };
        let (limit, c, side, vert) = if u.own { (480, -3, -2, -1) } else { (256, 3 * w, 2 * w, w) };
        for (j, v) in [(i, c), (i + 1, side), (i - 1, side), (i + 24, vert), (i - 24, vert)] {
            if (0..=limit).contains(&j) {
                grid[j as usize] += v;
            }
        }
    }
    let (mut best, mut at) = (0i8, 0usize);
    for (i, &v) in grid[..480].iter().enumerate() {
        if (best as i32) < v {
            best = v as i8;
            at = i;
        }
    }
    (best >= 9).then(|| ((at as i32 % 24) * 128 + 64, (at as i32 / 24) * 128 + 64))
}

/// Pixels per range cell in the anti-air aircraft search (`0x47b3e7`, `0x47b3f3`: `imul .., 0x32`).
pub const AIR_RANGE_STEP: i32 = 50;

/// What the anti-air search sees of one aircraft slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AirSlot {
    /// The aircraft's handle ([`Aircraft::id`]).
    pub id: u32,
    /// The side that launched it.
    pub side: i32,
    /// Which air force it is.
    pub force: AirForce,
    /// X of the flight line in map pixels.
    pub x: i32,
    /// Y of the flight line in map pixels.
    pub y: i32,
    /// X offset of the point anti-air aims at ([`Aircraft::aim_dx`]).
    pub aim_dx: i32,
    /// Hit points; a destroyed aircraft (0 or less) is still falling.
    pub hp: i32,
}

/// The anti-air top's aircraft search (`0x47b3c0`, called by `0x46c990` for the anti-air kinds 1, 2, 3, 42 and the turret 110 on
/// the top's 30-tick poll) from the shooter's position `(x, y)` with the range `min..max` in cells and `current` the aircraft
/// it aimed at last (`[unit+0x80]`). Slots are scanned side by side (0, 1, 2, skipping `own_side`) in slot order; aircraft
/// with `hp <= 0` and fighters (code 32) are skipped. The current target is kept while `min*50 < d < max*50`, `d` measured to
/// `x + aim_dx` (`0x47b461..0x47b49f`); otherwise (the current one is no candidate) the nearest other aircraft by the plain
/// `x`, on a tie the later slot, is taken if `min*50 <= d <= max*50` (`0x47b4cf..0x47b55a`). Distances are `trunc(sqrt(..))`.
/// Returns the new target (`None` clears both `[unit+0x80]` and the top's target, `0x47b5b2`).
pub fn anti_air_target(slots: &[AirSlot], x: i32, y: i32, current: Option<u32>, min: u32, max: u32, own_side: i32) -> Option<u32> {
    let (lo, hi) = (min as i32 * AIR_RANGE_STEP, max as i32 * AIR_RANGE_STEP);
    let dist = |dx: i32, dy: i32| ((dx as f64 * dx as f64 + dy as f64 * dy as f64).sqrt()) as i32;
    let mut candidates = Vec::new();
    for side in 0..3 {
        if side == own_side {
            continue;
        }
        for a in slots.iter().filter(|a| a.side == side && a.hp > 0 && a.force != AirForce::Fighter) {
            if Some(a.id) == current {
                let d = dist(x - a.aim_dx - a.x, y - a.y);
                if d > lo && d < hi {
                    return Some(a.id);
                }
            } else {
                candidates.push(a);
            }
        }
    }
    let mut best: Option<&AirSlot> = None;
    let mut best_d = hi + 100;
    for a in candidates {
        let d = dist(x - a.x, y - a.y);
        if best_d < d {
            continue;
        }
        best_d = d;
        best = Some(a);
    }
    best.filter(|_| best_d >= lo && best_d <= hi).map(|a| a.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_rand() -> impl FnMut(u32) -> i32 {
        |_| 0
    }

    #[test]
    fn a_falling_bomber_explodes_every_30_ticks_and_ends_with_four_slow_fireballs() {
        let mut a = Aircraft::launch(AirForce::Bomber, 500, 700, 0, 64, 0);
        a.hp = 0;
        let mut r = no_rand();
        let mut fireballs = Vec::new();
        let mut ticks = 0;
        while a.alive() && ticks < 400 {
            ticks += 1;
            for e in a.step(0, &mut r) {
                if let Event::Fireball { period, .. } = e {
                    fireballs.push((ticks, period));
                }
            }
        }
        // the first at once (timer 0 at launch), then every 30 ticks with rand(5) = 0; the 180th tick (death timer) adds the four slow ones and removes it
        let slow = fireballs.iter().filter(|f| f.1 == 4).count();
        let fast: Vec<i32> = fireballs.iter().filter(|f| f.1 == 2).map(|f| f.0).collect();
        assert_eq!(slow, 4);
        assert_eq!(fast, vec![1, 31, 61, 91, 121, 151]);
        assert_eq!(ticks, 180);
        assert_eq!(a.sounds.len(), 6 + 3); // one per explosion, three at the end
    }

    fn foe(x: i32, y: i32, kind: i32, idle: bool) -> StrikeUnit {
        StrikeUnit { x, y, kind, own: false, idle }
    }

    #[test]
    fn the_strike_pick_prefers_an_idle_enemy_pod_then_the_densest_block() {
        let pod = foe(300, 400, 46, true);
        assert_eq!(ai_strike_target(&[foe(10, 10, 1, false), pod]), Some((300, 400)));
        assert_eq!(ai_strike_target(&[foe(300, 400, 46, false)]), None); // a moving pod and a single unit: 3 < 9
                                                                         // three foes on one block: 9 on it
        let three = [foe(200, 200, 1, false), foe(210, 200, 1, false), foe(220, 200, 1, false)];
        assert_eq!(ai_strike_target(&three), Some((192, 192)));
        // an own unit on the same block takes 3 away
        let mut with_own = three.to_vec();
        with_own.push(StrikeUnit { own: true, ..foe(200, 200, 1, false) });
        assert_eq!(ai_strike_target(&with_own), None);
        // idle walkers count double
        assert_eq!(ai_strike_target(&[foe(200, 200, 41, true), foe(200, 200, 41, true)]), Some((192, 192)));
    }

    #[test]
    fn the_strike_pick_keeps_the_exes_grid_quirks() {
        // below index 256 (row 10 and further down) foreign units add nothing
        let low = [foe(200, 11 * 128 + 5, 1, false), foe(200, 11 * 128 + 5, 1, false), foe(200, 11 * 128 + 5, 1, false)];
        assert_eq!(ai_strike_target(&low), None);
        // two idle walkers in column 23: the block scores 12; its right neighbour is column 0 of the next row (index 24, +8, never
        // the maximum), so the pick stays in column 23
        let edge = [foe(23 * 128, 0, 41, true); 2];
        assert_eq!(ai_strike_target(&edge), Some((23 * 128 + 64, 64)));
    }

    #[test]
    fn launch_picks_the_far_edge() {
        let b = Aircraft::launch(AirForce::Bomber, 500, 700, 0, 64, 0);
        assert!(b.right && b.x == -400 && (b.reach, b.off_x) == (45, 75));
        let b = Aircraft::launch(AirForce::Bomber, 1500, 700, 0, 64, 0);
        assert!(!b.right && b.x == 64 * 32 + 400 && (b.reach, b.off_x) == (-45, 70));
        let c = Aircraft::launch(AirForce::Carryall, 100, 701, 1, 64, 9);
        assert!(!c.right && c.y == 688 && c.target.1 == 688);
        assert_eq!(c.squad, 0);
    }

    #[test]
    fn fighter_fires_two_salvos_per_pass() {
        let mut f = Aircraft::launch(AirForce::Fighter, 1000, 900, 0, 64, 0);
        let mut shots = 0;
        let mut r = no_rand();
        for t in 0..400 {
            shots += f.step(t % 60, &mut r).len();
        }
        assert_eq!(shots, 4);
    }

    #[test]
    fn asteroid_drops_twenty_meteors() {
        let mut a = Aircraft::launch(AirForce::Asteroid, 800, 800, 0, 64, 0);
        let mut n = 0;
        let mut t = 0;
        let mut r = |m: u32| (m / 2) as i32;
        while a.alive() && t < 5000 {
            n += a.step(t % 60, &mut r).len();
            t += 1;
        }
        assert_eq!(n, 20);
        assert_eq!(t, 61 + 25 * 19);
    }

    #[test]
    fn carryall_unloads_once_over_the_target() {
        let mut c = Aircraft::launch(AirForce::Carryall, 600, 600, 1, 64, 3);
        let mut r = no_rand();
        let mut unloads = Vec::new();
        for t in 0..60_000 {
            for e in c.step(t % 60, &mut r) {
                unloads.push(e);
            }
            if !c.alive() {
                break;
            }
        }
        assert_eq!(unloads, vec![Event::Unload { cell_x: 18, cell_y: 18, squad: 4 }]);
    }

    #[test]
    fn carryall_draw_state_follows_the_exe() {
        // body frame by distance while approaching (0x4674c2): 0 beyond 190 px, 4 below 145; exhaust frames cycle 0..3 every 3 steps
        let mut c = Aircraft::launch(AirForce::Carryall, 1000, 900, 0, 64, 0);
        let mut r = no_rand();
        let (mut seen, mut rear) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
        for t in 0..3000 {
            c.step(t % 60, &mut r);
            seen.insert(c.frame);
            rear.insert(c.rear_frame);
            if !c.alive() || c.x < 0 {
                break;
            }
        }
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        assert_eq!(rear.into_iter().collect::<Vec<_>>(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn squads_match_the_exe() {
        assert_eq!(squad(1).unwrap(), vec![(-1, 0, 2), (-1, 1, 2), (0, 0, 2), (0, 1, 2)]);
        assert_eq!(squad(4).unwrap(), vec![(-1, 0, 5), (-1, 1, 8), (0, 0, 13), (0, 1, 11)]);
        assert_eq!(squad(17).unwrap(), vec![(-1, 0, 41), (-1, 1, 41)]);
        assert!(squad(18).is_none());
    }

    #[test]
    fn aircraft_ask_for_their_sounds() {
        let mut rand = no_rand();
        // a bomber drones every 9 steps
        let mut b = Aircraft::launch(AirForce::Bomber, 1000, 900, 0, 64, 0);
        let mut n = 0;
        for tick in 0..30 {
            b.step(tick, &mut rand);
            n += b.sounds.drain(..).filter(|s| s.0 == 63).count();
        }
        assert_eq!(n, 4, "steps 1, 10, 19 and 28");
        // a fighter roars once, a carryall announces its landing twice (arriving and leaving)
        let mut f = Aircraft::launch(AirForce::Fighter, 1000, 900, 0, 64, 0);
        let mut c = Aircraft::launch(AirForce::Carryall, 1000, 900, 0, 64, 0);
        let (mut fs, mut cs) = (0, 0);
        for k in 0..1500 {
            let tick = k % 60;
            f.step(tick, &mut rand);
            c.step(tick, &mut rand);
            fs += f.sounds.drain(..).filter(|s| s.0 == 66).count();
            cs += c.sounds.drain(..).filter(|s| s.0 == 62).count();
        }
        assert_eq!((fs, cs), (1, 2));
    }

    fn slot(id: u32, side: i32, force: AirForce, x: i32, y: i32, aim_dx: i32) -> AirSlot {
        AirSlot { id, side, force, x, y, aim_dx, hp: 100 }
    }

    #[test]
    fn anti_air_measures_range_in_steps_of_50_px_and_ignores_fighters() {
        // an anti-air unit with range 0-9 reaches 450 px
        let a = [slot(1, 1, AirForce::Bomber, 450, 0, 0)];
        assert_eq!(anti_air_target(&a, 0, 0, None, 0, 9, 0), Some(1));
        let a = [slot(1, 1, AirForce::Bomber, 451, 0, 0)];
        assert_eq!(anti_air_target(&a, 0, 0, None, 0, 9, 0), None);
        let a = [slot(1, 1, AirForce::Fighter, 100, 0, 0)];
        assert_eq!(anti_air_target(&a, 0, 0, None, 0, 9, 0), None, "fighters are immune");
        let a = [slot(1, 0, AirForce::Bomber, 100, 0, 0)];
        assert_eq!(anti_air_target(&a, 0, 0, None, 0, 9, 0), None, "own side");
        let a = [AirSlot { hp: 0, ..slot(1, 1, AirForce::Tripler, 100, 0, 0) }];
        assert_eq!(anti_air_target(&a, 0, 0, None, 0, 9, 0), None, "a falling aircraft");
    }

    #[test]
    fn anti_air_keeps_its_target_and_otherwise_takes_the_nearest() {
        let a = [slot(1, 1, AirForce::Bomber, 300, 0, 0), slot(2, 1, AirForce::Tripler, 100, 0, 0)];
        assert_eq!(anti_air_target(&a, 0, 0, None, 0, 9, 0), Some(2));
        assert_eq!(anti_air_target(&a, 0, 0, Some(1), 0, 9, 0), Some(1), "the current target stays while in range");
        // the kept target is measured to x + aim_dx and strictly inside the range
        let a = [slot(1, 1, AirForce::Bomber, 420, 0, -50)];
        assert_eq!(anti_air_target(&a, 0, 0, Some(1), 0, 9, 0), Some(1), "|0 + 50 - 420| = 370 px");
        let a = [slot(1, 1, AirForce::Bomber, 420, 0, 50)];
        assert_eq!(anti_air_target(&a, 0, 0, Some(1), 0, 9, 0), None, "|0 - 50 - 420| = 470 px: lost, and the current target is no candidate");
        let a = [slot(1, 1, AirForce::Bomber, 450, 0, 0)];
        assert_eq!(anti_air_target(&a, 0, 0, Some(1), 0, 9, 0), None, "at exactly 450 px a kept target is lost (and is no candidate)");
        // ties go to the later slot
        let a = [slot(1, 1, AirForce::Bomber, 100, 0, 0), slot(2, 1, AirForce::Bomber, 0, 100, 0)];
        assert_eq!(anti_air_target(&a, 0, 0, None, 0, 9, 0), Some(2));
        // the minimum: new targets from min*50 on
        let a = [slot(1, 1, AirForce::Bomber, 49, 0, 0)];
        assert_eq!(anti_air_target(&a, 0, 0, None, 1, 9, 0), None);
    }

    #[test]
    fn a_bomber_aims_50_px_ahead() {
        assert_eq!(Aircraft::launch(AirForce::Bomber, 500, 700, 0, 64, 0).aim_dx, 50);
        assert_eq!(Aircraft::launch(AirForce::Bomber, 1500, 700, 0, 64, 0).aim_dx, -50);
        assert_eq!(Aircraft::launch(AirForce::Tripler, 500, 700, 0, 64, 0).aim_dx, 0);
    }
}
