//! Weapon-top aiming and the fire cycle, ported from `State of War.exe` (top update `0x479c60`, aim `0x4797c0`,
//! cooldown base class `0x47c5f0` / `0x47c6b0`, target search `0x46c990`). All counters are in game ticks.
//!
//! Each tick, in this order:
//! 1. **Cooldown tick** (`0x47c6b0`): a counter above `2 * reload` is reset to `reload`; in state 6 it is decremented and, when it
//!    reaches 0, the state returns to 0.
//! 2. **Aim** (`0x4797c0`): when the state is not 0 and a target exists, the target facing is
//!    [`crate::facing::facing`] from the top to the target's aim point (the building centre, or the unit position plus
//!    half its height for flying targets). Then, always, a counter (`[obj+0x74]`, reload `[obj+0x78]` = 4, initial 4) is
//!    decremented and at 0 the top turns one step ([`crate::facing::turn_step`]) toward the target facing.
//! 3. **State machine**: state 0 polls for a target every 30 ticks (counter `[obj+0x80]`, starts at 0, so the first tick
//!    searches); a hit sets the cooldown counter to `reload - jitter/2 + random(jitter)` and enters state 3.
//!    State 3 returns to 0 if the target is gone, and starts the fire sequence (state 4) once the top's facing equals the
//!    target facing. State 4 plays [`crate::recoil`] and ends in state 6 (cooldown), which counts down in step 1.
//!
//! The top does not follow its chassis by itself: after each chassis turn step the exe calls `0x479980` with the chassis's
//! target facing (see [`TopAim::set_target_facing`]), which only takes effect in state 0. A unit with no orders also picks a
//! random facing every 240..1139 ticks (`0x46e114..0x46e198`, docs/art.md).
//!
//! Target choice (`0x47b140`, ground search): cells are scanned in rings from the minimum to the maximum range (in
//! cells; the ring table is built at runtime at `0x436230`, an expanding diamond that is not reproduced here). The current
//! target is kept while it is still found; otherwise the nearest *unit* wins, then a turret-like building, then any
//! building ("units, turrets and buildings", as the manual says). Anti-air tops search air targets first.
//!
//! Per-kind values come from the top constructor cases (`0x47a000..`): `[obj+0x18]` min range, `[obj+0x1c]` max range,
//! `[obj+0x28]` cooldown reload and `[obj+0x2c]` jitter. Cases share tail code, so the values are read by *following* the
//! case's jumps (an earlier version stopped at the first jump and wrongly gave several kinds reload 0); jitter is 0 for the
//! kinds that never set it (assumes freshly allocated objects are zeroed).

use crate::facing::turn_step;
use crate::recoil::{fire_sequence, FireSequence};

pub const SEARCH_POLL_TICKS: i32 = 30;
pub const TURN_DELAY: i32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AimParams {
    /// Target range in cells (`min`, `max`).
    pub min_range: u32,
    pub max_range: u32,
    pub reload: u32,
    pub jitter: u32,
    /// Facings of the top sprite (32, or 16 for the walker tops).
    pub steps: u32,
}

const fn p(min_range: u32, max_range: u32, reload: u32, jitter: u32, steps: u32) -> AimParams {
    AimParams { min_range, max_range, reload, jitter, steps }
}

/// Scatter of a top's shots in pixels (`[top+0x20]`, member field `[+0x18]` of the volley spawner `0x47cf40`): each round's end
/// point moves by `+-random(spread)` on both axes (sign from `random(2) == 1`). Read from the constructor cases (exe-tested).
pub fn spread(kind: i32) -> u32 {
    match kind {
        5 => 20,
        13 | 14 => 5,
        44 => 24,
        112 => 16,
        113 => 20,
        1..=4 | 6..=12 => 10,
        _ => 1,
    }
}

/// Rounds per shot (`[top+0x30]`, the spawner's outer loop): the fifth turret throws six, everything else one.
pub fn rounds(kind: i32) -> u32 {
    if kind == 113 { 6 } else { 1 }
}

/// Aim and cooldown values of a unit kind's weapon top (ground kinds 1..=15, walkers 41..=45).
pub fn aim_params(kind: i32) -> Option<AimParams> {
    Some(match kind {
        1 => p(0, 9, 120, 24, 32),
        2 | 3 => p(0, 9, 120, 24, 32),
        4 => p(2, 8, 72, 0, 32),
        5 => p(3, 9, 72, 0, 32),
        6 => p(4, 10, 72, 0, 32),
        7 => p(0, 6, 72, 0, 32),
        8 => p(0, 7, 72, 0, 32),
        9 => p(0, 8, 72, 0, 32),
        10 => p(0, 4, 2, 0, 32),
        11 => p(0, 5, 2, 0, 32),
        12 => p(0, 6, 2, 0, 32),
        13 => p(0, 5, 180, 36, 32),
        14 => p(0, 6, 180, 36, 32),
        15 => p(0, 7, 180, 36, 32),
        41 => p(2, 8, 18, 2, 16),
        42 => p(2, 9, 180, 36, 16),
        43 => p(2, 6, 0, 0, 16),
        44 => p(2, 11, 2, 1, 16),
        45 => p(0, 1, 180, 36, 16),
        // the Advancer's own gun top (case at 0x47a8af: range 0-10, reload 180 +- 36 from the shared tail 0x47a57a)
        40 => p(0, 10, 180, 36, 32),
        // turret tops (kinds 109-113 = turrets 1-5, constructor cases at 0x47a000..)
        109 => p(0, 6, 72, 0, 32),
        110 => p(0, 9, 120, 24, 32),
        111 => p(0, 7, 39, 12, 32),
        112 => p(0, 8, 1, 0, 32),
        113 => p(2, 10, 70, 0, 32),
        _ => return None,
    })
}

/// The top's vertical offset `[top+0x60]` (the horizontal `[top+0x5c]` is always 0): the aim origin of `0x4797c0` is the chassis
/// position plus `(0, dy)`, and the muzzle-flash rectangle `0x479bc0` is placed with it. The constructor case stores a base for a
/// few kinds (kind 2: 4, kind 12: 7, the turret tops -11, -2, -20, -17, -31) and the shared tail `0x47ab76` adds `-4`, `-10` or
/// `-16` by kind through the byte table `0x47acf4` (units 1..=45 only). Exe-tested by `top_offsets_match_the_exes_constructor`.
pub fn top_dy(kind: i32) -> i32 {
    match kind {
        1 | 4 | 5 | 7 | 8 | 10 | 11 | 13 | 14 => -4,
        3 | 6 | 9 => -10,
        2 | 15 | 45 => 0,
        12 => -3,
        40..=44 => -16,
        109 => -11,
        110 => -2,
        111 => -20,
        112 => -17,
        113 => -31,
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum State {
    /// Looking for a target (every 30 ticks).
    Idle,
    /// Has a target, turning towards it.
    Aiming,
    /// Playing the fire sequence.
    Firing,
    /// Cooling down after a volley.
    Cooldown,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TopAim {
    pub params: AimParams,
    /// The weapon kind the tables came from (a snapshot keeps this and rebuilds `seq`, see [`TopAim::restore`]).
    kind: i32,
    #[serde(skip, default = "no_sequence")]
    seq: FireSequence,
    pub state: State,
    /// Current and target facing of the top (`[obj+0x6c]`, `[obj+0x70]`).
    pub facing: u32,
    pub target_facing: u32,
    turn_counter: i32,
    search_counter: i32,
    cooldown: i32,
    /// Tick inside the fire sequence (1 = first).
    seq_tick: u32,
}

/// What happened during one tick.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Events {
    /// A target was acquired this tick.
    pub acquired: bool,
    /// Tables fired this tick: `(first barrel, shots)`.
    pub shot: Option<(u8, u8)>,
}

fn no_sequence() -> FireSequence {
    FireSequence { flash_set: None, reload: 0, steps: &[] }
}

impl TopAim {
    /// The counters a savegame keeps: turn countdown (`[top+0x74]`), target search countdown, weapon cooldown (`[top+0x210]`),
    /// tick inside the fire sequence.
    pub fn counters(&self) -> (i32, i32, i32, u32) {
        (self.turn_counter, self.search_counter, self.cooldown, self.seq_tick)
    }

    /// Puts the counters back (savegame load).
    pub fn set_counters(&mut self, turn_counter: i32, search_counter: i32, cooldown: i32, seq_tick: u32) {
        self.turn_counter = turn_counter;
        self.search_counter = search_counter;
        self.cooldown = cooldown;
        self.seq_tick = seq_tick;
    }

    /// Rebuilds the static fire sequence after the aim was deserialized from a savegame.
    pub fn restore(&mut self) {
        if let Some(s) = fire_sequence(self.kind) {
            self.seq = s;
        }
    }

    pub fn new(kind: i32, facing: u32) -> Option<TopAim> {
        let params = aim_params(kind)?;
        Some(TopAim {
            params,
            kind,
            seq: fire_sequence(kind)?,
            state: State::Idle,
            facing,
            target_facing: 0,
            turn_counter: TURN_DELAY,
            search_counter: 0,
            cooldown: 0,
            seq_tick: 0,
        })
    }

    /// `0x479980`: the chassis (or the idle look-around, `0x46e198`) sets the top's target facing, but only while the top is
    /// idle (state 0). In every other state the aim code (`0x4797c0`) owns the target facing (the enemy's direction).
    /// The top then turns to it at one step per 4 ticks like for any target.
    pub fn set_target_facing(&mut self, facing: u32) {
        if self.state == State::Idle {
            self.target_facing = facing;
        }
    }

    /// The sequence being played and the tick inside it, for drawing.
    pub fn firing(&self) -> Option<(&FireSequence, u32)> {
        (self.state == State::Firing && self.seq_tick > 0).then_some((&self.seq, self.seq_tick))
    }

    /// Whether this tick's [`TopAim::tick`] runs the target search (state 0 and the 30-tick poll counter runs out, `0x479cb5`):
    /// the searches with side effects (the anti-air top's kept aircraft, `0x47b3c0`) run only then.
    pub fn polls_now(&self) -> bool {
        self.state == State::Idle && self.search_counter <= 1
    }

    /// One game tick. `target` = the facing from the top to its current target (`None` = no target in range);
    /// `random(n)` returns a number in `0..n` (the game's random generator `0x437b10`).
    pub fn tick(&mut self, target: Option<u32>, mut random: impl FnMut(u32) -> u32) -> Events {
        let mut ev = Events::default();
        // 1. cooldown counter (0x47c6b0)
        if self.cooldown > 2 * self.params.reload as i32 {
            self.cooldown = self.params.reload as i32;
        }
        if self.state == State::Cooldown {
            self.cooldown -= 1;
            if self.cooldown <= 0 {
                self.cooldown = 0;
                self.state = State::Idle;
            }
        }
        // 2. aim (0x4797c0)
        if self.state != State::Idle {
            if let Some(t) = target {
                self.target_facing = t % self.params.steps;
            }
        }
        self.turn_counter -= 1;
        if self.turn_counter <= 0 {
            self.turn_counter = TURN_DELAY;
            self.facing = turn_step(self.facing, self.target_facing, self.params.steps);
        }
        // 3. state machine (0x479c60)
        match self.state {
            State::Idle => {
                self.search_counter -= 1;
                if self.search_counter <= 0 {
                    self.search_counter = SEARCH_POLL_TICKS;
                    if let Some(t) = target {
                        let j = self.params.jitter;
                        let rnd = if j == 0 { 0 } else { random(j) as i32 };
                        self.cooldown = self.params.reload as i32 - j as i32 / 2 + rnd;
                        self.target_facing = t % self.params.steps;
                        self.search_counter = 0;
                        self.state = State::Aiming;
                        ev.acquired = true;
                    }
                }
            }
            State::Aiming => {
                if target.is_none() {
                    self.state = State::Idle;
                } else if self.facing == self.target_facing {
                    self.state = State::Firing;
                    self.seq_tick = 0;
                }
            }
            State::Firing => {
                self.seq_tick += 1;
                match self.seq.at(self.seq_tick) {
                    Some(m) => ev.shot = m.shot,
                    None => {
                        self.state = State::Cooldown;
                        self.seq_tick = 0;
                    }
                }
            }
            State::Cooldown => {}
        }
        ev
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(kind: i32, start_facing: u32, target: u32, ticks: u32) -> Vec<(u32, &'static str)> {
        let mut t = TopAim::new(kind, start_facing).unwrap();
        let mut log = Vec::new();
        let mut last = State::Idle;
        for tick in 1..=ticks {
            let ev = t.tick(Some(target), |n| n / 2);
            if ev.acquired {
                log.push((tick, "acquired"));
            }
            if ev.shot.is_some() {
                log.push((tick, "shot"));
            }
            if t.state != last {
                log.push((tick, match t.state { State::Idle => "idle", State::Aiming => "aiming", State::Firing => "firing", State::Cooldown => "cooldown" }));
                last = t.state;
            }
        }
        log
    }

    #[test]
    fn cannon_turns_fires_cools_down_and_fires_again() {
        // 1C: facing 0 -> 8 needs 8 turn steps, one every 4 ticks starting at tick 4: facing reaches 8 on tick 32.
        let log = run(7, 0, 8, 140);
        assert_eq!(log[0], (1, "acquired"));
        assert_eq!(log[1], (1, "aiming"));
        assert_eq!(log[2], (32, "firing")); // state changes on the tick the facing matches
        assert_eq!(log[3], (33, "shot")); // sequence tick 1
        // 2 steps x 11 ticks = 22 ticks; the wrap tick is 33 + 22 = 55
        assert_eq!(log[4], (55, "cooldown"));
        // cooldown 72 ticks counted from tick 56: back to idle on tick 127, which searches at once and re-acquires
        assert_eq!(log[5], (127, "acquired"));
        assert_eq!(log[6], (127, "aiming"));
    }

    #[test]
    fn already_aimed_top_fires_after_acquiring() {
        let log = run(7, 8, 8, 40);
        assert_eq!(&log[..4], &[(1, "acquired"), (1, "aiming"), (2, "firing"), (3, "shot")]);
    }

    #[test]
    fn jitter_changes_the_cooldown() {
        // 1AA: reload 120, jitter 24 -> counter = 120 - 12 + random(24); a deterministic random of 0 gives 108.
        let mut t = TopAim::new(1, 0).unwrap();
        t.tick(Some(0), |_| 0);
        assert_eq!(t.cooldown, 108);
        let mut t = TopAim::new(1, 0).unwrap();
        t.tick(Some(0), |_| 23);
        assert_eq!(t.cooldown, 131);
    }

    #[test]
    fn losing_the_target_returns_to_idle_and_poll_is_every_30_ticks() {
        let mut t = TopAim::new(7, 0).unwrap();
        assert!(!t.tick(None, |_| 0).acquired);
        for _ in 0..29 {
            assert!(!t.tick(Some(8), |_| 0).acquired, "no search between polls");
        }
        assert!(t.tick(Some(8), |_| 0).acquired); // tick 31: the counter, reloaded to 30 on tick 1, is back to 0
        // losing the target while aiming goes back to idle
        let mut t = TopAim::new(7, 0).unwrap();
        t.tick(Some(8), |_| 0);
        assert_eq!(t.state, State::Aiming);
        t.tick(None, |_| 0);
        assert_eq!(t.state, State::Idle);
    }

    #[test]
    fn target_facing_is_only_settable_while_idle() {
        let mut t = TopAim::new(7, 0).unwrap();
        t.set_target_facing(5);
        for _ in 0..20 {
            t.tick(None, |_| 0);
        }
        assert_eq!(t.facing, 5, "an idle top follows the facing it was given, one step per 4 ticks");
        let mut t = TopAim::new(7, 0).unwrap();
        t.tick(Some(8), |_| 0); // the first tick searches
        assert_eq!(t.state, State::Aiming);
        t.set_target_facing(20);
        assert_eq!(t.target_facing, 8, "ignored while aiming");
    }

    #[test]
    fn parameters() {
        assert_eq!(aim_params(7), Some(p(0, 6, 72, 0, 32)));
        assert_eq!(aim_params(44).unwrap().steps, 16);
        assert_eq!(aim_params(40).map(|p| (p.max_range, p.reload)), Some((10, 180))); // the Advancer gun top (0x47a8af)
        assert!(TopAim::new(100, 0).is_none());
    }
}
