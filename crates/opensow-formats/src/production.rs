//! Factory production, decoded from `State of War.exe` (slot initialiser `0x40e8f0`, slot tick `0x40e640`, building
//! update `0x40de60`, spawn `0x408740`, level loader `0x41da00`).
//!
//! A building owns up to five production *slots* (stride 0x28 from `[bld+0x15c]`). Each slot has a kind (a unit kind
//! 1-15 / 41-46, an air force 30-34, or an upgrade level 16-24), a progress counter and a target. Production is automatic
//! and free ("Production of ground units is automatic and costs nothing", tutorial string). Per tick a slot gains
//! `projectile::steps_at(row, tick % 60)`; when the progress reaches the target the unit is spawned at a free cell next to
//! the building (`0x408740`; if none is free the spawn is retried every tick and, after 240 ticks, the player is told "No
//! space for units"). Air-force slots (the air base) add to the player's stock instead of spawning ("<force> ready").
//!
//! The building record in a level file stores the slots: raw word 2 = number of *active* slots (`[bld+0x144]`, raised
//! by the tech-level upgrade), words 3-7 = slot kinds, words 8-12 = a per-slot value (`[slot+0x24]`, only read by mission
//! scripts so far).

use crate::level::Object;

pub const SLOTS: usize = 5;
/// Default slot rate `[slot+0x14]` (set by the slot initialiser for every kind).
pub const BASE_RATE: u32 = 30;
/// Extra rate per windtrap level of the owner, multiplayer only (`0x40e640`: row = rate + 12 * `[0x5baaa0 + player*4]`,
/// which `0x40d9a0` fills with the windtrap's level field `[bld+0x144]`).
pub const WINDTRAP_RATE: u32 = 12;

/// Progress a slot needs before it yields a unit (`[slot+0xc]`): 2000 for ground units, walkers and the upgrade levels
/// 16-24, 4000 for air forces and the pod; `None` for kinds the initialiser rejects ("Unknown unit": turrets 25-29, 35-40).
pub fn slot_target(kind: i32) -> Option<u32> {
    match kind {
        1..=24 | 41..=45 => Some(2000),
        30..=34 | 46 => Some(4000),
        _ => None,
    }
}

/// `[slot+0x1c]`: 500 for ground units, 200 for everything else (probably the research-point price of the tech level that
/// unlocks the slot; the tutorial says "increase tech-level for 500").
pub fn slot_unlock_cost(kind: i32) -> Option<u32> {
    match kind {
        1..=15 => Some(500),
        16..=24 | 30..=34 | 41..=46 => Some(200),
        _ => None,
    }
}

/// The kind a slot becomes when its tech-level is raised (`0x40e730`, jump table `0x40e854`): the next tier inside each
/// triple of classes 1-15 (anti-air, artillery, cannon, flamer, special) and the upgrade levels 16-24 (gold, research,
/// energy); `None` at the last tier of a triple and for every other kind.
pub fn next_tier(kind: i32) -> Option<i32> {
    match kind {
        1..=24 if kind % 3 != 0 => Some(kind + 1),
        _ => None,
    }
}

/// Credits a turret costs (`0x4172ea..0x41732a`): 200 / 400 / 600 / 800 / 1000 for turrets 1-5 (kinds 25-29).
pub fn turret_price(tier: u32) -> Option<i32> {
    (1..=5).contains(&tier).then(|| 200 * tier as i32)
}

/// Income per tick of a gold mine (credits) or research lab (research points) with `active` upgrade levels unlocked
/// (`0x42d6b0` loop: jump tables `0x42e56c` / `0x42e558`): `active + 1`, nothing at 0. Credits cap at 99999, research at 9999.
pub fn income_per_tick(active: usize) -> i32 {
    if (1..=5).contains(&active) { active as i32 + 1 } else { 0 }
}

pub const CREDITS_CAP: i32 = 99_999;
pub const RESEARCH_CAP: i32 = 9_999;

/// A factory's two exits as cell offsets from the building's origin cell (constructor `0x40a330`: `[bld+0x124/0x128]`
/// spawn, `[bld+0x12c/0x130]` primary walk-to, `[bld+0x134/0x138]` alternative walk-to; spawn routine `0x408740`).
///
/// The unit always appears at `spawn` (inside the footprint) and then walks out through one of two cells: the primary one
/// at the bottom right, matching the model (light factory (3,3), the others (4,3)), or, only when that cell is occupied
/// (blocked terrain, a building or any unit), the opposite one at the top left (light (-1,-1), the others (1,0)). Both
/// occupied: nothing is spawned and the slot retries every tick (see [`NO_SPACE_TICKS`]). After the walk-to cell the unit
/// continues two cells further out: down from the primary one (`y + 0x50` px), up from the alternative (`y - 0x30` px).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactoryExit {
    pub spawn: (i32, i32),
    pub primary: (i32, i32),
    pub alternative: (i32, i32),
}

/// Ticks a finished unit may wait for a free exit before "No space for units" is said (`0x40e2ee`: counter reaches `0xf0`).
pub const NO_SPACE_TICKS: u32 = 240;

pub fn factory_exit(kind: i32) -> Option<FactoryExit> {
    match kind {
        101 => Some(FactoryExit { spawn: (1, 1), primary: (3, 3), alternative: (-1, -1) }),
        102 | 103 | 108 => Some(FactoryExit { spawn: (2, 1), primary: (4, 3), alternative: (1, 0) }),
        _ => None,
    }
}

/// Rate row of a slot for `steps_at`. `tutorial_step` is `[0x70f2cc]`: the step of the tutorial script (`0x432d6e`,
/// 0..=0x1b) or `None` for -1, which is every other mission and multiplayer. Normal games use `rate + 12 * windtrap
/// levels` (30 steps/s without windtrap, so a ground unit takes 67 s); the tutorial uses `3 * rate` = 90 (x4 on steps 15-17).
pub fn rate_row(tutorial_step: Option<u32>, windtrap_levels: u32) -> u32 {
    match tutorial_step {
        Some(step) => 3 * BASE_RATE * if matches!(step, 15..=17) { 4 } else { 1 },
        None => BASE_RATE + WINDTRAP_RATE * windtrap_levels,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slots {
    pub active: u32,
    pub kinds: [i32; SLOTS],
    pub values: [i32; SLOTS],
}

impl Slots {
    /// The slots of a level building record (all-zero kinds for buildings without production).
    pub fn of(o: &Object) -> Slots {
        let mut s = Slots { active: o.raw[2].max(0) as u32, kinds: [0; SLOTS], values: [0; SLOTS] };
        for i in 0..SLOTS {
            s.kinds[i] = o.raw[3 + i];
            s.values[i] = o.raw[8 + i];
        }
        s
    }
}

/// Game ticks a slot needs from empty to done at `steps_per_second` (the sum of `steps_at` over 60 ticks).
pub fn seconds_to_build(kind: i32, steps_per_second: u32) -> Option<f32> {
    Some(slot_target(kind)? as f32 / steps_per_second.max(1) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_by_kind() {
        assert_eq!(slot_target(7), Some(2000));
        assert_eq!(slot_target(31), Some(4000));
        assert_eq!(slot_target(43), Some(2000));
        assert_eq!(slot_target(25), None);
        assert_eq!(slot_unlock_cost(7), Some(500));
        assert_eq!(slot_unlock_cost(32), Some(200));
    }

    #[test]
    fn tech_tiers_and_prices() {
        assert_eq!(next_tier(7), Some(8));
        assert_eq!(next_tier(9), None);
        assert_eq!(next_tier(22), Some(23));
        assert_eq!(next_tier(24), None);
        assert_eq!(next_tier(31), None);
        assert_eq!(turret_price(3), Some(600));
        assert_eq!(income_per_tick(1), 2);
        assert_eq!(income_per_tick(0), 0);
    }

    #[test]
    fn tutorial_light_unit_takes_about_22_seconds_and_normal_games_67() {
        let t = seconds_to_build(7, rate_row(Some(1), 0)).unwrap();
        assert!((t - 22.2).abs() < 0.1, "{t}");
        assert!((seconds_to_build(7, rate_row(None, 0)).unwrap() - 66.7).abs() < 0.1);
        assert_eq!(rate_row(None, 2), 54);
        assert_eq!(rate_row(Some(16), 0), 360);
    }
}
