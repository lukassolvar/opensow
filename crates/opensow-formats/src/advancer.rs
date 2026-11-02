//! Advancer building actions, decoded from `State of War.exe` (docs/advancer-actions.md).
//!
//! The Advancer (unit kind 40) takes one of three orders on a building or turret: *destroy* (hit points go down, the
//! building changes hands when they reach zero), *repair* and *upgrade* (unlock the next production slot). An order
//! stores a mode, the target cell centre and the target id in the Advancer's top (`[top+0xfc]` 1/2/3, `+0xf4/+0xf8`,
//! `+0x100`; setters `0x46e5c0/0x46e620/0x46e680`) and flies there. Once the Advancer sits on that cell and the target is
//! still valid, every beam shot (`0x46f440`) creates an action event (code 0x15/0x13/0x14) executed by `0x40f590`.

use crate::level::Level;

/// The three Advancer orders; the numbers are the action-event codes (`[event+0x2658]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Repair = 0x13,
    Upgrade = 0x14,
    Destroy = 0x15,
}

/// Maximum hit points of a building kind (constructor `0x40a330`, jump table `0x40afb8`): turrets 25-29, every
/// building 100-108 (headquarters included) 13536, kinds 109-113 and the rest.
pub fn building_max_hp(kind: i32) -> i32 {
    match kind {
        25 => 900,
        26 => 1250,
        27 => 1540,
        28 => 2100,
        29 => 2600,
        100..=108 => 13536,
        109..=113 => 200, // docs/levels.md: constructed at runtime only
        _ => 50,
    }
}

/// A new building starts with 1 hit point and a pending heal of its maximum (`[bld+0x6c]`), paid out by the building
/// update (`0x40de60`) at up to this many points per tick.
pub const HEAL_PER_TICK: i32 = 150;

pub fn is_turret(kind: i32) -> bool {
    (25..=29).contains(&kind)
}

/// The per-player Advancer strength `[0x5baaac + player*4]` (`0x41d7b4`): level header words 31 (player 0) and 32
/// (player 1), usually 10 and ranging 6..15 per mission. The loader shifts them left by two (`rate`) unless a flag is set.
pub fn rate(level: &Level, player: usize) -> i32 {
    level.header.get(31 + player).copied().unwrap_or(10) * 4
}

/// Hit points a destroy (`-`) or repair (`+`) event moves: `2 * rate` (`0x40ffe9`, `0x4102df`).
pub fn hp_per_event(rate: i32) -> i32 {
    2 * rate
}

/// Upgrade points one upgrade event adds (`0x40e3d0`: `[bld+0x148] += rate`); the next slot unlocks when they reach that
/// slot's target (2000 ground, 4000 air; `production::slot_target`).
pub fn upgrade_per_event(rate: i32) -> i32 {
    rate
}

/// Who owns a destroyed building afterwards (`0x43ab40` with the owner argument 0): the player with the largest
/// damage tally `[bld+0x7c + player*4]` among those who are not the current owner. `None` means nobody dealt damage
/// (the exe then picks a random player). Turrets and headquarters are removed instead of captured.
pub fn capture_winner(tally: [i32; 3], owner: usize) -> Option<usize> {
    let mut best = 0;
    let mut winner = None;
    for p in 0..3 {
        if tally[p] > best && p != owner {
            best = tally[p];
            winner = Some(p);
        }
    }
    winner
}

/// Whether destroying a building of this kind removes it (turrets and the HQ) instead of handing it over.
pub fn removed_when_destroyed(kind: i32) -> bool {
    is_turret(kind) || kind == 100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_points_by_kind() {
        assert_eq!(building_max_hp(27), 1540);
        assert_eq!(building_max_hp(100), 13536);
        assert_eq!(building_max_hp(103), 13536);
    }

    #[test]
    fn the_biggest_foreign_tally_takes_the_building() {
        assert_eq!(capture_winner([500, 0, 0], 2), Some(0));
        assert_eq!(capture_winner([500, 900, 100], 1), Some(0));
        assert_eq!(capture_winner([0, 0, 0], 2), None);
        assert_eq!(capture_winner([0, 300, 0], 1), None);
    }

    #[test]
    fn event_sizes_follow_the_rate() {
        assert_eq!(hp_per_event(40), 80);
        assert_eq!(upgrade_per_event(40), 40);
    }
}
