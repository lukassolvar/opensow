//! Fire / recoil sequences of weapon tops, decoded from `State of War.exe` (top constructor `0x47a000..0x47ab5f`,
//! entry helper `0x479760`, state machine `0x479c60`, flash draw `0x479a20`, top draw `0x479a80`).
//!
//! A top keeps a list of *steps* (`[obj+0xc4]` = current step, `[obj+0xc8]` = count). Step 0 is an implicit rest step
//! (phase 0, no flash, no shot) created by the constructor prologue; the per-kind code appends the steps listed here.
//! Each step is four numbers: `phase` (which animation phase of the top sprite to show: `frame = phase * steps + facing`),
//! `flash_group` (which group of 32 facings in the top's `_fire` sprite set to draw, -1 = none), `barrel` (first
//! `.po1` table used for the shots of this step, -1 = no shot) and `shots` (how many consecutive tables fire at once).
//!
//! Timeline once the top is aimed (state 3 -> 4; the step timer `[obj+0x20c]` starts at 0): step *k* (0-based among the
//! listed steps) starts on tick `k * (reload + 1) + 1` and lasts `reload + 1` ticks (`[obj+0x210]`, default 10); shots of
//! a step fire on its first tick. After step N-1 the index wraps to the rest step, the flash/phase override ends
//! (`[obj+0x214] = 0`) and the state becomes 6. While no sequence runs the top shows phase 0 (the idle phase array
//! `[obj+0x94]` has a single zero entry for every top).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FireStep {
    pub phase: u8,
    pub flash_group: i8,
    pub barrel: i8,
    pub shots: u8,
}

const fn s(phase: u8, flash_group: i8, barrel: i8, shots: u8) -> FireStep {
    FireStep { phase, flash_group, barrel, shots }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FireSequence {
    /// Index into [`FLASH_SETS`] (`[obj+0x58]`), `None` for tops without muzzle-flash sprites.
    pub flash_set: Option<u8>,
    /// Counter reload `[obj+0x210]`: each step lasts `reload + 1` ticks.
    pub reload: u32,
    pub steps: &'static [FireStep],
}

/// Tops that have a `units\{name}_fire.ps6` muzzle-flash sprite set, in the exe's table order (`flash_set` index).
pub const FLASH_SETS: [&str; 9] = ["1A", "1C", "2A", "2C", "3A", "3C", "tur_01", "tur_02", "5W4"];

static SEQ_1AA: [FireStep; 2] = [s(0, -1, 0, 1), s(0, -1, 1, 1)];
static SEQ_2AA: [FireStep; 4] = [s(0, -1, 0, 1), s(0, -1, 2, 1), s(0, -1, 1, 1), s(0, -1, 3, 1)];
static SEQ_3AA: [FireStep; 6] = [s(0, -1, 0, 1), s(0, -1, 1, 1), s(0, -1, 2, 1), s(0, -1, 3, 1), s(0, -1, 4, 1), s(0, -1, 5, 1)];
static SEQ_A1: [FireStep; 2] = [s(0, 0, 0, 1), s(0, 1, -1, 1)];
static SEQ_A2: [FireStep; 2] = [s(1, 0, 0, 1), s(1, 1, -1, 1)];
static SEQ_A3: [FireStep; 5] = [s(1, 0, 0, 1), s(1, 1, -1, 1), s(0, -1, -1, 1), s(2, 2, 1, 1), s(2, 3, -1, 1)];
static SEQ_C1: [FireStep; 2] = [s(0, 0, 0, 1), s(0, 1, -1, 1)];
static SEQ_C2: [FireStep; 2] = [s(1, 0, 0, 2), s(1, 1, -1, 1)];
static SEQ_C3: [FireStep; 2] = [s(1, 0, 0, 3), s(1, 1, -1, 1)];
static SEQ_F1: [FireStep; 1] = [s(0, -1, 0, 1)];
static SEQ_F3: [FireStep; 1] = [s(0, -1, 0, 2)];
static SEQ_S3: [FireStep; 3] = [s(0, -1, 0, 1), s(0, -1, 0, 1), s(0, -1, 0, 1)];
static SEQ_W1: [FireStep; 1] = [s(0, -1, 0, 2)];
static SEQ_W2: [FireStep; 4] = [s(0, -1, 0, 1), s(0, -1, 1, 1), s(0, -1, 2, 1), s(0, -1, 3, 1)];
static SEQ_W4: [FireStep; 3] = [s(0, 0, 0, 1), s(1, 0, -1, 1), s(2, 1, -1, 1)];

static SEQ_T4: [FireStep; 3] = [s(0, 0, 0, 1), s(1, 1, -1, 1), s(2, 2, -1, 1)];
static SEQ_T3: [FireStep; 2] = [s(0, -1, 0, 1), s(0, -1, -1, 1)];
static SEQ_T5: [FireStep; 3] = [s(0, -1, 0, 1), s(0, -1, 1, 1), s(0, -1, 2, 1)];

macro_rules! seq {
    ($set:expr, $reload:expr, $steps:expr) => {
        FireSequence { flash_set: $set, reload: $reload, steps: &$steps }
    };
}

/// Fire sequence of a unit kind's weapon top (ground kinds 1..=15, walkers 41..=45).
pub fn fire_sequence(kind: i32) -> Option<FireSequence> {
    Some(match kind {
        1 => seq!(None, 15, SEQ_1AA),
        2 => seq!(None, 15, SEQ_2AA),
        3 => seq!(None, 15, SEQ_3AA),
        4 => seq!(Some(0), 5, SEQ_A1),
        5 => seq!(Some(2), 10, SEQ_A2),
        6 => seq!(Some(4), 10, SEQ_A3),
        7 => seq!(Some(1), 10, SEQ_C1),
        8 => seq!(Some(3), 10, SEQ_C2),
        9 => seq!(Some(5), 10, SEQ_C3),
        10 | 11 => seq!(None, 8, SEQ_F1),
        12 => seq!(None, 8, SEQ_F3),
        13 | 14 => seq!(None, 10, SEQ_F1),
        15 => seq!(None, 4, SEQ_S3),
        41 => seq!(None, 4, SEQ_W1),
        42 => seq!(None, 4, SEQ_W2),
        43 => seq!(None, 8, SEQ_W2),
        44 => seq!(Some(8), 2, SEQ_W4),
        45 => seq!(None, 4, SEQ_F1),
        109 => seq!(Some(7), 10, SEQ_C1),
        110 => seq!(None, 15, SEQ_2AA),
        111 => seq!(None, 10, SEQ_T3),
        112 => seq!(Some(6), 5, SEQ_T4),
        113 => seq!(None, 1, SEQ_T5),
        _ => return None,
    })
}

/// What a top does on a given tick of its fire sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    pub step: usize,
    pub phase: u8,
    pub flash_group: Option<u8>,
    /// Tables fired on this very tick: `(first barrel, shots)`.
    pub shot: Option<(u8, u8)>,
}

impl FireSequence {
    /// Length of the sequence in ticks.
    pub fn duration(&self) -> u32 {
        self.steps.len() as u32 * (self.reload + 1)
    }

    /// State on `tick` (1 = first tick after the top starts firing); `None` before and after the sequence.
    pub fn at(&self, tick: u32) -> Option<Moment> {
        if tick == 0 || tick > self.duration() {
            return None;
        }
        let len = self.reload + 1;
        let step = ((tick - 1) / len) as usize;
        let st = self.steps[step];
        let first = (tick - 1) % len == 0;
        Some(Moment {
            step,
            phase: st.phase,
            flash_group: u8::try_from(st.flash_group).ok(),
            shot: (first && st.barrel >= 0).then_some((st.barrel as u8, st.shots)),
        })
    }

    /// Frame of the top sprite for a facing and tick: `phase * steps + facing` (phase 0 outside the sequence).
    pub fn top_frame(&self, tick: u32, facing: u32, steps_per_phase: u32) -> u32 {
        let phase = self.at(tick).map_or(0, |m| m.phase as u32);
        phase * steps_per_phase + facing
    }

    /// Frame of the muzzle-flash sprite (`_fire` set) at this tick, if one is shown.
    pub fn flash_frame(&self, tick: u32, facing: u32, steps_per_phase: u32) -> Option<u32> {
        let group = self.at(tick)?.flash_group? as u32;
        self.flash_set?;
        Some(group * steps_per_phase + facing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_of_a_cannon() {
        let c = fire_sequence(8).unwrap(); // 2C: 2 steps, reload 10 -> 11 ticks each
        assert_eq!(c.duration(), 22);
        assert_eq!(c.at(0), None);
        assert_eq!(c.at(1), Some(Moment { step: 0, phase: 1, flash_group: Some(0), shot: Some((0, 2)) }));
        assert_eq!(c.at(2).unwrap().shot, None);
        assert_eq!(c.at(11).unwrap().step, 0);
        assert_eq!(c.at(12), Some(Moment { step: 1, phase: 1, flash_group: Some(1), shot: None }));
        assert_eq!(c.at(22).unwrap().step, 1);
        assert_eq!(c.at(23), None);
        assert_eq!(c.top_frame(1, 8, 32), 40);
        assert_eq!(c.top_frame(30, 8, 32), 8);
        assert_eq!(c.flash_frame(1, 8, 32), Some(8));
        assert_eq!(c.flash_frame(12, 8, 32), Some(40));
        assert_eq!(c.flash_frame(23, 8, 32), None);
    }

    #[test]
    fn twin_aa_alternates_barrels_without_a_flash() {
        let a = fire_sequence(1).unwrap();
        assert_eq!(a.at(1).unwrap().shot, Some((0, 1)));
        assert_eq!(a.at(17).unwrap().shot, Some((1, 1)));
        assert_eq!(a.flash_frame(1, 0, 32), None);
        assert_eq!(a.duration(), 32);
    }

    #[test]
    fn heavy_artillery_has_four_flash_groups_and_three_phases() {
        let a = fire_sequence(6).unwrap();
        let groups: Vec<_> = a.steps.iter().filter_map(|s| u8::try_from(s.flash_group).ok()).collect();
        assert_eq!(groups, [0, 1, 2, 3]);
        assert_eq!(a.steps.iter().map(|s| s.phase).max(), Some(2));
        assert_eq!(a.at(a.reload + 2 + 11).unwrap().phase, 0); // the third step is the pause between volleys
        assert_eq!(FLASH_SETS[a.flash_set.unwrap() as usize], "3A");
    }

    #[test]
    fn unknown_kinds_have_no_sequence() {
        assert!(fire_sequence(40).is_none());
        assert!(fire_sequence(100).is_none());
    }
}
