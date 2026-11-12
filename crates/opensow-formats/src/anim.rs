//! Chassis walk animation, ported from `State of War.exe` (tick at `0x46f8b9`/`0x46fe00`, per-kind values from the
//! chassis initialiser `0x46ff50`).
//!
//! Each tick of a chassis (all counters are in game ticks, 60 per second):
//! 1. if the facing differs from the target facing, a turn-delay counter (`[obj+0x38]`, reload `[obj+0x3c]`, default 4,
//!    0 for the Advancer) is decremented and, when it goes negative, reloaded and the facing takes one step
//!    ([`crate::facing::turn_step`]);
//! 2. once the facing equals the target, an animation counter (`[obj+0x4c]`, reload `[obj+0x50]` = the *period*) is
//!    decremented; when it goes negative it is reloaded, the phase (`[obj+0x44]`) advances **only if the unit moved**, and the
//!    phase wraps to 0 at `[obj+0x48]` (the number of phases);
//! 3. the sprite frame is `phase * steps + facing` (not updated for the Advancer, kind 40).
//!
//! Sprite sets hold `phases * steps` frames, so `steps = frames / phases`: 32 for `chA/chB/chC` (96 frames, 3 phases),
//! 16 for `chW` (160 frames, 10 phases) and `chN` (48 frames, 3 phases); `chI1` (the Advancer) has one phase of 32.
//! The chassis initialiser sets `[obj+8] = 16` explicitly for kinds 41-46 (kinds 1-15 and 40 keep the default 32).

use crate::facing::turn_step;
use crate::kinds;

/// A chassis kind's walking values: animation phases and speed, turn speed, facings, pixels per second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WalkParams {
    /// Number of animation phases (0 = not animated).
    pub phases: u32,
    /// Counter reload value: the phase can advance every `period + 1` ticks.
    pub period: u32,
    /// Ticks between facing steps (`period`-style counter): a turn step every `turn_delay + 1` ticks.
    pub turn_delay: u32,
    /// Facings per phase (`[obj+8]`): 32 by default, 16 for the walkers and the `chN` pod.
    pub steps: u32,
    /// `[obj+0x18]`: walking speed in pixels per second (60 ticks). Each tick the unit advances
    /// `projectile::steps_at(speed, tick % 60)` pixels along its path (`0x46f550`, table `0x436df0`).
    pub speed: u32,
}

/// Hover bob of the Advancer, the only unit with the air flag (`[obj+0x4b]`): the table at `0x89cf58`, built by `0x46bc70`
/// (emulation-verified, `opensow hover`). The phase `[obj+0x84]` starts at a random value below 20, advances every
/// 4th tick (`0x46e069`, tick-in-second % 4 == 0) and wraps to 0 after [`HOVER_PHASES`] - 1; the sprite is drawn
/// `HOVER_TABLE[phase] + HOVER_ALTITUDE` pixels below the unit's ground position (`[obj+0x88]` = -64 for every unit,
/// but only air-flagged units add it), with the shadow on the ground.
pub const HOVER_TABLE: [i32; 42] =
    [-6, -6, -6, -6, -5, -5, -5, -4, -4, -3, -2, -1, 0, 1, 2, 3, 4, 4, 5, 5, 5, 6, 6, 6, 6, 5, 5, 5, 4, 4, 3, 2, 1, 0, -1, -2, -3, -4, -4, -5, -5, -5];
/// Number of entries of [`HOVER_TABLE`].
pub const HOVER_PHASES: u32 = 42;
/// `[obj+0x88]`: how far above its ground position a flying unit is drawn, in pixels (negative = up).
pub const HOVER_ALTITUDE: i32 = -64;
/// Ticks between hover phase steps.
pub const HOVER_PERIOD: u64 = 4;

/// Walking speeds of kinds 1-15 (tiers 1-3 of five classes), straight from the chassis initialiser `0x46ff50`.
const GROUND_SPEEDS: [u32; 15] = [18, 14, 10, 15, 12, 8, 22, 18, 14, 20, 17, 12, 16, 15, 10];
const WALKER_SPEEDS: [u32; 5] = [20, 15, 20, 15, 20];

/// Kind 6 (tier-3 artillery) never sets a period, so it keeps the default 0x3e8 and in practice does not animate.
const GROUND_PERIODS: [u32; 15] = [8, 8, 11, 7, 8, 0x3e8, 5, 6, 10, 4, 8, 11, 7, 8, 12];
const WALKER_PERIODS: [u32; 5] = [8, 6, 6, 6, 6];

/// Per-kind animation values from the exe's chassis initialiser; `None` for kinds without a chassis.
pub fn walk_params(kind: i32) -> Option<WalkParams> {
    Some(match kind {
        1..=15 => WalkParams { phases: 3, period: GROUND_PERIODS[(kind - 1) as usize], turn_delay: 4, steps: 32, speed: GROUND_SPEEDS[(kind - 1) as usize] },
        40 => WalkParams { phases: 0, period: 0x3e8, turn_delay: 0, steps: 32, speed: 30 },
        41..=45 => {
            WalkParams { phases: 10, period: WALKER_PERIODS[(kind - 41) as usize], turn_delay: 4, steps: 16, speed: WALKER_SPEEDS[(kind - 41) as usize] }
        }
        46 => WalkParams { phases: 3, period: 3, turn_delay: 4, steps: 16, speed: 30 },
        _ => return None,
    })
}

/// Like [`walk_params`], with the one value that depends on the game mode (`[0x49a33c]` in the exe):
/// kind 46 (the pod) falls into shared code in modes 3 and 8 that sets period 13 and speed 7 (30 otherwise).
/// What those modes are was not traced.
pub fn walk_params_for_mode(kind: i32, mode: u32) -> Option<WalkParams> {
    let p = walk_params(kind)?;
    Some(if kind == kinds::POD && (mode == 3 || mode == 8) { WalkParams { period: 13, speed: 7, ..p } } else { p })
}

/// Facings per phase for a sprite set with `frames` frames.
pub fn steps_for(frames: usize, phases: u32) -> u32 {
    (frames as u32 / phases.max(1)).max(1)
}

/// Animation phase after `ticks` ticks of continuous walking from a standstill with a fresh counter
/// (the counter starts at 0, so the first tick already advances the phase).
pub fn phase_after(params: WalkParams, ticks: u32) -> u32 {
    if params.phases == 0 {
        return 0;
    }
    ((ticks + params.period) / (params.period + 1)) % params.phases
}

/// A chassis' animation state: the facing it turns through and the walk phase.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChassisAnim {
    params: WalkParams,
    steps: u32,
    /// The facing shown now.
    pub facing: u32,
    /// The facing the chassis is turning to.
    pub target: u32,
    /// The walk animation phase shown now.
    pub phase: u32,
    turn_counter: i32,
    anim_counter: i32,
}

impl ChassisAnim {
    /// A chassis with `steps` facings that stands still and points at `facing`.
    pub fn new(params: WalkParams, steps: u32, facing: u32) -> ChassisAnim {
        ChassisAnim { params, steps, facing, target: facing, phase: 0, turn_counter: params.turn_delay as i32, anim_counter: 0 }
    }

    /// Sets the facing to turn to (taken modulo the number of facings).
    pub fn set_target(&mut self, facing: u32) {
        self.target = facing % self.steps;
    }

    /// The counters a savegame keeps (`[chassis+0x38]` turn countdown, `[chassis+0x4c]` animation countdown) and the facing step count.
    pub fn counters(&self) -> (i32, i32, u32) {
        (self.turn_counter, self.anim_counter, self.steps)
    }

    /// Puts the counters back (savegame load).
    pub fn set_counters(&mut self, turn_counter: i32, anim_counter: i32) {
        self.turn_counter = turn_counter;
        self.anim_counter = anim_counter;
    }

    /// One game tick. `moving` = the unit's position differs from the one the chassis remembers.
    /// Returns true when the facing took a turn step this tick: the exe then hands the chassis's *target* facing to the
    /// weapon top (`0x479980`, called right after every `0x46a190` turn step in `0x46f4a0` / `0x46f550`), see
    /// [`crate::aim::TopAim::set_target_facing`].
    pub fn tick(&mut self, moving: bool) -> bool {
        let turned = self.turn();
        if self.facing == self.target {
            self.anim_counter -= 1;
            if self.anim_counter < 0 {
                self.anim_counter = self.params.period as i32;
                if moving {
                    self.phase += 1;
                }
                if self.phase >= self.params.phases {
                    self.phase = 0;
                }
            }
        }
        turned
    }

    /// The turning half of [`ChassisAnim::tick`]: while the facing is off its target the delay counter counts down and, below
    /// zero, reloads and takes one turn step. The walking step `0x46f550` runs it once per pixel step it takes (`0x46f65f`)
    /// *and* the tail runs it again (`0x46f8b9`), so a walking unit turns faster than a standing one. True after a turn step.
    pub fn turn(&mut self) -> bool {
        if self.facing != self.target {
            self.turn_counter -= 1;
            if self.turn_counter < 0 {
                self.turn_counter = self.params.turn_delay as i32;
                self.facing = turn_step(self.facing, self.target, self.steps);
                return true;
            }
        }
        false
    }

    /// The walking gate (`0x46f692`): four turn steps from the current facing reach the target.
    pub fn within_turns(&self, n: u32) -> bool {
        let mut f = self.facing;
        for _ in 0..n {
            f = turn_step(f, self.target, self.steps);
        }
        f == self.target
    }

    /// The sprite frame to draw: `phase * steps + facing`.
    pub fn frame(&self) -> u32 {
        crate::facing::frame(self.phase, self.facing, self.steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_kind_values() {
        assert_eq!(walk_params(1), Some(WalkParams { phases: 3, period: 8, turn_delay: 4, steps: 32, speed: 18 }));
        assert_eq!(walk_params(10).unwrap().period, 4);
        assert_eq!(walk_params(15).unwrap().period, 12);
        assert_eq!(walk_params(6).unwrap().period, 0x3e8);
        assert_eq!(walk_params(41).unwrap(), WalkParams { phases: 10, period: 8, turn_delay: 4, steps: 16, speed: 20 });
        assert_eq!(walk_params(46).unwrap().phases, 3);
        assert_eq!(walk_params(46).unwrap().period, 3);
        assert_eq!(walk_params_for_mode(46, 3).unwrap().period, 13);
        assert_eq!(walk_params_for_mode(46, 1).unwrap().period, 3);
        assert_eq!(walk_params_for_mode(1, 3), walk_params(1));
        assert_eq!(walk_params(40).unwrap().phases, 0);
        assert_eq!(walk_params(100), None);
        assert_eq!((steps_for(96, 3), steps_for(160, 10), steps_for(48, 3), steps_for(32, 0)), (32, 16, 16, 32));
    }

    #[test]
    fn walking_cycles_through_the_phases() {
        let p = walk_params(1).unwrap(); // 3 phases, period 8 -> a phase every 9 ticks
        let mut a = ChassisAnim::new(p, 32, 8);
        let mut phases = Vec::new();
        for t in 1..=30 {
            a.tick(true);
            phases.push((t, a.phase));
            assert_eq!(a.phase, phase_after(p, t), "tick {t}");
        }
        // phase 1 from tick 1, phase 2 from tick 10, back to 0 at tick 19, 1 at 28
        assert_eq!(phases[0], (1, 1));
        assert_eq!(phases[9], (10, 2));
        assert_eq!(phases[18], (19, 0));
        assert_eq!(phases[27], (28, 1));
        assert_eq!(a.frame(), 32 + 8);
    }

    #[test]
    fn standing_still_never_advances_and_resets() {
        let mut a = ChassisAnim::new(walk_params(4).unwrap(), 32, 0);
        for _ in 0..50 {
            a.tick(false);
        }
        assert_eq!((a.phase, a.frame()), (0, 0));
    }

    #[test]
    fn turning_blocks_animation_until_facing_matches() {
        let mut a = ChassisAnim::new(walk_params(1).unwrap(), 32, 0);
        a.set_target(2);
        // a turn step every 5 ticks (delay 4): facing reaches 2 after 10 ticks, the phase cannot move before that
        for t in 1..=10 {
            a.tick(true);
            if t < 10 {
                assert_eq!(a.phase, 0, "tick {t}");
            }
        }
        assert_eq!(a.facing, 2);
        a.tick(true);
        assert_eq!(a.phase, 1);
    }

    #[test]
    fn advancer_never_animates_or_waits_to_turn() {
        let mut a = ChassisAnim::new(walk_params(40).unwrap(), 32, 0);
        a.set_target(1);
        a.tick(true);
        assert_eq!(a.facing, 1);
        for _ in 0..10 {
            a.tick(true);
        }
        assert_eq!(a.phase, 0);
    }
}
