//! Air-force behaviour: how a launched strike flies and what it fires (`State of War.exe`, class at `0x467240..0x468c30`).
//!
//! Field names follow the object offsets of the exe (`[obj+0x..]`), see docs/air.md. All positions are map pixels; `y` is
//! the ground y of the flight line and `altitude` is subtracted for the sprite. One call of [`Aircraft::step`] is one game
//! tick; `tick` is its index in the 60-tick speed cycle (see `projectile::steps_at`).
//!
//! * launch (`0x467ee0`) -> [`Aircraft::launch`]; per-tick update (`0x467b50`) -> [`Aircraft::step`];
//!   bomb/missile/pod/meteor/unload spawns (`0x468240`) -> [`Event`]; carryall squads (`0x468960`) -> [`squad`].
//! * Not ported: the death sequence effects (smoke/explosions; an aircraft with `hp <= 0` is removed after `death_timer`
//!   ticks), engine sounds and the smoke puffs of the fighter, and the damage a bullet does on impact (see `projectile`).

use crate::air::AirForce;
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aircraft {
    pub force: AirForce,
    pub side: i32,
    /// `[+4]`, `[+8]`: position of the flight line (map pixels).
    pub x: i32,
    pub y: i32,
    /// `[+0xc]`, `[+0x10]`: the strike's target point.
    pub target: (i32, i32),
    /// `[+0x14]`: height above the ground line.
    pub altitude: i32,
    /// `[+0x24]`: base speed (path steps per 60 ticks, see `projectile::steps_at`).
    pub speed: i32,
    /// `[+0x2c]`: hit points.
    pub hp: i32,
    /// `[+0x34]`: true = flies towards +x.
    pub right: bool,
    /// `[+0x38]`: ticks a destroyed aircraft keeps falling before it is removed.
    pub death_timer: i32,
    /// `[+0x40]`, `[+0x44]`, `[+0x48]`, `[+0x4c]`: frame, frame count, countdown, reload of the main animation.
    pub frame: i32,
    pub frames: i32,
    pub anim_count: i32,
    pub anim_reload: i32,
    /// `[+0x70]`, `[+0x74]`: per-type counters (bomb timer / unload cooldown / shot cooldown, tripler firing state, ...).
    pub counter: i32,
    pub state: i32,
    /// `[+0x78]`, `[+0x7c]`, `[+0x80]`: bomb travel, x and y offsets of the spawn point.
    pub reach: i32,
    pub off_x: i32,
    pub off_y: i32,
    /// `[+0x84]`: the tripler has reached its target and may fire.
    pub armed: bool,
    /// `[+0x88]`: carryall squad index.
    pub squad: usize,
    map_width_px: i32,
}

/// Launches a strike. `map_cells` is the map width in cells (`[0x71cec0]`), `carry_counter` the exe's global carryall
/// counter (`[0x89cf50]`, 0..9) which selects the squad: the returned aircraft's `squad` is the incremented value.
pub fn launch_counter(carry_counter: usize) -> usize {
    let n = carry_counter + 1;
    if n >= 10 {
        0
    } else {
        n
    }
}

impl Aircraft {
    pub fn launch(force: AirForce, target_x: i32, target_y: i32, side: i32, map_cells: i32, carry_counter: usize) -> Aircraft {
        let map_width_px = map_cells * 32;
        let right = target_x <= 0x400;
        let mut a = Aircraft {
            force,
            side,
            x: if right { -MARGIN } else { map_width_px + MARGIN },
            y: target_y,
            target: (target_x, target_y),
            altitude: 200,
            speed: 120,
            hp: 0,
            right,
            death_timer: 180,
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
            map_width_px,
        };
        match force {
            AirForce::Bomber => {
                a.hp = 480;
                a.frames = 3;
                a.anim_reload = 2;
                a.counter = 4;
                (a.reach, a.off_x) = if right { (45, 75) } else { (-45, 70) };
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

    /// Advances one game tick. `rand(n)` must return a value in `0..n` (the exe's lockstep generator `0x437b10`).
    pub fn step(&mut self, tick: u32, rand: &mut dyn FnMut(u32) -> i32) -> Vec<Event> {
        let mut events = Vec::new();
        if self.hp <= 0 {
            self.death_timer -= 1;
            if self.death_timer <= 0 {
                return events;
            }
        }
        let sign = if self.right { 1 } else { -1 };
        match self.force {
            AirForce::Bomber => {
                self.animate();
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
        let d = (self.x - self.target.0).abs();
        let lead = if d >= 120 { 0 } else { 119 - d };
        if (41..=199).contains(&d) {
            self.altitude = d;
        }
        self.x -= steps_at((self.speed - lead) as u32, tick) as i32;
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
            if d < 7 {
                if self.state < 100 {
                    step = 0;
                    self.armed = true;
                }
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
        const SHOTS: [(i32, i32, i32, i32, i32, i32); 6] = [
            (0, 0, -55, -80, 2, 3),
            (2, 0, 83, -23, 4, 3),
            (4, 0, -30, 96, 1, 13),
            (1, 6, 9, -109, 3, 3),
            (3, 6, 66, 69, 5, 3),
            (5, 6, -72, 41, 0x457, 0),
        ];
        let Some(&(_, _, oy, ox, next, cool)) = SHOTS.iter().find(|s| s.0 == self.state && s.1 == self.frame) else { return };
        let x = self.x + self.off_x + ox;
        events.push(Event::Shot {
            bullet_type: 17,
            strength: 0,
            from: (x, self.y - self.altitude + 20 + self.off_y + oy),
            to: (x, self.y + self.off_y + oy),
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn no_rand() -> impl FnMut(u32) -> i32 {
        |_| 0
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
    fn squads_match_the_exe() {
        assert_eq!(squad(1).unwrap(), vec![(-1, 0, 2), (-1, 1, 2), (0, 0, 2), (0, 1, 2)]);
        assert_eq!(squad(4).unwrap(), vec![(-1, 0, 5), (-1, 1, 8), (0, 0, 13), (0, 1, 11)]);
        assert_eq!(squad(17).unwrap(), vec![(-1, 0, 41), (-1, 1, 41)]);
        assert!(squad(18).is_none());
    }
}
