//! The computer's turret decision (`0x431120`), ported from `State of War.exe` (docs/ai.md item 7).
//!
//! Pure: the caller supplies the unit slots, the buildings, the credits, the level's tier flags, an anti-air test and the random
//! and placement-test callbacks. `scripts/verify_against_exe.py` runs the exe's `0x431120` (and the constructor part that writes
//! the eight spots, `0x40aa04..0x40ace6`) in an emulator and compares it with [`plan`] / [`Building::spots`].

use crate::buildings::{advancer_offset, blocked_cells, footprint};
use crate::kinds;

/// A unit slot of the exe's handle table: the cell of its pixel position, its owner `[+0x44]` and kind `[+0x40]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// The cell of the unit's pixel position.
    pub cell: (i32, i32),
    /// `[+0x44]`: the owner.
    pub owner: i32,
    /// `[+0x40]`: the unit kind.
    pub kind: i32,
}

/// An own building that is not a turret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Building {
    /// The building's kind.
    pub kind_id: i32,
    /// The building's origin cell.
    pub cell: (i32, i32),
}

impl Building {
    /// Footprint in cells (`[+0x14]`, `[+0x18]`).
    pub fn size(&self) -> (i32, i32) {
        footprint(self.kind_id).map_or((1, 1), |(w, h, _)| (w, h))
    }

    /// The cell an Advancer hovers over (`0x409ab0` / `0x409ac0`).
    pub fn action_cell(&self) -> (i32, i32) {
        let (dx, dy) = advancer_offset(self.kind_id);
        (self.cell.0 + dx, self.cell.1 + dy)
    }

    /// The eight turret spots `[+0x224 + 4i]`, `[+0x244 + 4i]` the constructor stores (`0x40aac9..0x40acdd`); `r0` is the row of
    /// the first blocked cell of the layout (the height when none is).
    pub fn spots(&self) -> [(i32, i32); 8] {
        let (w, h) = self.size();
        let (x, y) = self.cell;
        let r0 = blocked_cells(self.kind_id).iter().map(|&(_, dy)| dy).min().unwrap_or(h);
        [
            (x + w - 1, y + r0 - 2),
            (x + w + 1, y + r0),
            (x + w + 1, y + h - 1),
            (x + w - 1, y + h + 1),
            (x, y + h + 1),
            (x - 2, y + h - 1),
            (x - 2, y + r0),
            (x, y + r0 - 2),
        ]
    }
}

/// `[b+0x264]`: the units in the rectangle `x-4 ..= x+w+3` by `y-4 ..= y+h+3` (own -1, others +1, walkers 41-45 and gun tops
/// 111-113 one more).
pub fn threat(b: &Building, slots: &[Slot], me: i32) -> i32 {
    let (w, h) = b.size();
    let (x, y) = b.cell;
    let mut t = 0;
    for s in slots.iter().filter(|s| s.cell.0 >= x - 4 && s.cell.0 <= x + w + 3 && s.cell.1 >= y - 4 && s.cell.1 <= y + h + 3) {
        if s.owner == me {
            t -= 1;
        } else {
            t += 1;
            if kinds::WALKERS.contains(&s.kind) || (111..=113).contains(&s.kind) {
                t += 1;
            }
        }
    }
    t
}

/// `[b+0x268]`: with 400 credits, 6 when another side's Advancer stands on the action cell, minus 2 for each spot that holds
/// an anti-air turret (`aa`); otherwise 0.
pub fn free(b: &Building, slots: &[Slot], me: i32, credits: i32, aa: impl Fn((i32, i32)) -> bool) -> i32 {
    if credits < 400 {
        return 0;
    }
    let a = b.action_cell();
    if !slots.iter().any(|s| s.owner != me && s.kind == kinds::ADVANCER && s.cell == a) {
        return 0;
    }
    6 - 2 * b.spots().iter().filter(|&&c| aa(c)).count() as i32
}

/// The kind the credit band and the two values ask for (0x19..=0x1d = tier 1..=5, -1 = nothing).
pub fn wanted(credits: i32, t: i32, f: i32) -> i32 {
    if credits <= 1000 {
        if t > f {
            match t {
                ..=2 => -1,
                3..=4 => 0x19,
                5 => 0x1b,
                6 => 0x1c,
                _ => 0x1d,
            }
        } else if f >= 4 {
            0x1a
        } else {
            -1
        }
    } else if credits <= 1500 {
        if t > f {
            match t {
                ..=1 => -1,
                2..=3 => 0x19,
                4 => 0x1b,
                5 => 0x1c,
                _ => 0x1d,
            }
        } else if f >= 3 {
            0x1a
        } else {
            -1
        }
    } else if t > f {
        if t >= 4 {
            0x1d
        } else if t >= 2 {
            0x1b
        } else {
            -1
        }
    } else if f >= 2 {
        0x1a
    } else {
        -1
    }
}

/// The step-down (`0x4316c5..0x43177f`): a tier is built when the credits reach its price and its flag is set; otherwise
/// 5 -> 4 -> 3 -> 1; tiers 1 and 2 do not step down.
pub fn step_down(mut kind: i32, credits: i32, flags: &[bool; 5]) -> Option<u32> {
    loop {
        let tier = kind - 0x18;
        match kind {
            0x19..=0x1d if credits >= 200 * tier && flags[(tier - 1) as usize] => return Some(tier as u32),
            0x1d => kind = 0x1c,
            0x1c => kind = 0x1b,
            0x1b => kind = 0x19,
            _ => return None,
        }
    }
}

/// A chosen turret: tier and the cell passed to the placement (`0x439e30`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// Which turret (its tier) to build.
    pub tier: u32,
    /// The cell to build it on.
    pub cell: (i32, i32),
}

/// The whole routine. `rand(n)` is the lockstep generator, `ok(x, y, w, h)` the placement test `0x408be0` for a turret footprint.
pub fn plan(
    buildings: &[Building],
    slots: &[Slot],
    me: i32,
    credits: i32,
    flags: &[bool; 5],
    aa: impl Fn((i32, i32)) -> bool,
    mut rand: impl FnMut(u32) -> u32,
    mut ok: impl FnMut(i32, i32, i32, i32) -> bool,
) -> Option<Placement> {
    if buildings.is_empty() {
        return None;
    }
    let t: Vec<i32> = buildings.iter().map(|b| threat(b, slots, me)).collect();
    let f: Vec<i32> = buildings.iter().map(|b| free(b, slots, me, credits, &aa)).collect();
    let best = (0..buildings.len()).fold(0, |m, n| m.max(t[n]).max(f[n]));
    let n = (0..buildings.len()).find(|&n| best <= t[n] || best <= f[n]).unwrap_or(buildings.len() - 1);
    let kind = wanted(credits, t[n], f[n]);
    let tier = step_down(kind, credits, flags)?;
    let turret = 0x18 + tier as i32;
    let (w, h, _) = footprint(turret)?;
    let spots = buildings[n].spots();
    for _ in 0..8 {
        let (sx, sy) = spots[rand(8) as usize];
        let sy = if turret >= 0x1c { sy - 1 } else { sy }; // the 1x2 turrets stand one row higher
        if ok(sx, sy, w, h) {
            return Some(Placement { tier, cell: (sx, sy) });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_and_step_down() {
        assert_eq!(wanted(500, 2, 0), -1);
        assert_eq!(wanted(500, 3, 0), 0x19);
        assert_eq!(wanted(900, 7, 0), 0x1d);
        assert_eq!(wanted(1200, 4, 0), 0x1b);
        assert_eq!(wanted(3000, 2, 0), 0x1b);
        assert_eq!(wanted(3000, 0, 2), 0x1a);
        assert_eq!(step_down(0x1b, 5000, &[true, true, false, false, false]), Some(1));
        assert_eq!(step_down(0x1a, 5000, &[true, false, true, true, true]), None);
        assert_eq!(step_down(0x1d, 900, &[true, true, true, true, true]), Some(4));
    }

    #[test]
    fn a_light_factory_has_its_eight_spots_around_it() {
        let b = Building { kind_id: 101, cell: (8, 8) };
        assert_eq!(b.spots(), [(10, 7), (12, 9), (12, 10), (10, 12), (8, 12), (6, 10), (6, 9), (8, 7)]);
    }
}
