//! The Rogon bomb (unit kind 46, a bare `chN` pod) and its explosion, the event of type 35 (docs/ai.md).
//!
//! * Touch test `0x46df40` (every tick, on the chassis): the bomb's hit points are set to 0 when its target handle `[+0x7c]` is found in
//!   the **static grid `0x5eac38`** (index `cx * 128 + cy`) on any of the nine cells `cx-1..=cx+1, cy-1..=cy+1` around its own cell
//!   (nine `cmp` against `0x5eaa34 .. 0x5eae3c`). Ground units stamp their handle there, walkers (`[type+0x4b]`, kinds 41-45) use
//!   the second grid `0x60ac38`, so a bomb never touches a walker.
//! * Death `0x46a4f0` for kind 46 does nothing but create the event `0x411dc0(0x23, 0, x, y, x+20, y+20, owner)` and returns: no
//!   clouds, wreck, debris, puffs or death sound. It runs for every way the pod dies (its hit points reach 0), not only by touching.
//! * Event `0x411220`: counter 20 (`[+0x26c4]`), `[+0x26c8]` = k = 0. Each call adds the elapsed ticks to the counter; while it is above
//!   1: `k++`, counter `-1`; the event ends when `k >= ends[7]*2` (`[0x6b3a34]`, the range index table `0x6b3a18` entry 7); otherwise a
//!   blast at `(x + 16 + 32*w[k], y + 16 + 32*w[k+1])` with `w` the ring offset list `0x65bbd8` read as words, area damage 370 - k of
//!   bullet type 35, an explosion sprite, and when `k % 17 == 1` a sound (67 if `random(4) == 1`, else 19).

/// Unit kind of the bomb.
pub const KIND: i32 = crate::kinds::POD;
/// Bullet type of the explosion (HQs are immune).
pub const BULLET_TYPE: u8 = 35;
/// Starting value of the event counter (`0x411e86`).
pub const START_COUNTER: i32 = 20;
/// Damage of blast `k` is `BASE_STRENGTH - k`.
pub const BASE_STRENGTH: i32 = 370;
/// Every `SOUND_PERIOD`-th blast (`k % 17 == 1`) is accompanied by a sound.
pub const SOUND_PERIOD: usize = 17;
/// The ring whose end bounds the explosion (`[0x6b3a34]` is entry 7 of the index table).
pub const LAST_RING: usize = 7;

/// Walkers keep their handle in the second grid, which the touch test does not read.
pub fn can_touch(target_kind: i32) -> bool {
    !crate::kinds::WALKERS.contains(&target_kind)
}

/// `0x46df40`: the bomb at cell `bomb` touches a target whose handle sits on any of `target_cells` when one of them lies in the
/// 3 x 3 block around the bomb. `target_cells` are the cells carrying the handle (a unit: its cell and the cell it heads to; a
/// building: its footprint).
pub fn touches(bomb: (i32, i32), target_cells: impl IntoIterator<Item = (i32, i32)>) -> bool {
    target_cells.into_iter().any(|c| (c.0 - bomb.0).abs() <= 1 && (c.1 - bomb.1).abs() <= 1)
}

/// One blast of the explosion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blast {
    /// Offset of the blast centre from `(x + 16, y + 16)` in cells.
    pub dx: i32,
    /// Y offset of the blast centre, in cells (see `dx`).
    pub dy: i32,
    /// The blast's area damage strength.
    pub strength: i32,
    /// Whether this blast rumbles (`k % 17 == 1`).
    pub sound: bool,
}

/// The explosion state machine (`0x411220`, event type 35).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Explosion {
    counter: i32,
    k: usize,
}

impl Default for Explosion {
    fn default() -> Self {
        Explosion { counter: START_COUNTER, k: 0 }
    }
}

impl Explosion {
    /// `([bullet+0x26c4], [bullet+0x26c8])`: the counter and the step the event is at (a savegame keeps both).
    pub fn parts(&self) -> (i32, usize) {
        (self.counter, self.k)
    }

    /// An explosion with the given countdown `counter` and blast index `k`.
    pub fn from_parts(counter: i32, k: usize) -> Explosion {
        Explosion { counter, k }
    }

    /// Number of words of the offset list used: twice the offsets up to `LAST_RING` (`ends` as in `Rings::ends`).
    pub fn limit(ends: &[usize]) -> usize {
        ends.get(LAST_RING).map_or(0, |&e| 2 * e)
    }

    /// One tick (the exe adds the elapsed ticks; one at a time here). `words` is the ring offset list as `(dx, dy)` pairs flattened.
    /// Returns the blasts of this tick and whether the event is still running.
    pub fn step(&mut self, words: &[i32], limit: usize) -> (Vec<Blast>, bool) {
        let mut out = Vec::new();
        self.counter += 1;
        while self.counter > 1 {
            self.k += 1;
            self.counter -= 1;
            if self.k >= limit {
                return (out, false);
            }
            let (Some(&dx), Some(&dy)) = (words.get(self.k), words.get(self.k + 1)) else { return (out, false) };
            out.push(Blast { dx, dy, strength: BASE_STRENGTH - self.k as i32, sound: self.k % SOUND_PERIOD == 1 });
        }
        (out, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_is_the_three_by_three_block() {
        assert!(touches((5, 5), [(5, 5)]));
        assert!(touches((5, 5), [(6, 4)]));
        assert!(!touches((5, 5), [(7, 5)]));
        assert!(!touches((5, 5), [(5, 3), (3, 3)]));
        assert!(touches((5, 5), [(9, 9), (4, 6)]));
        assert!(!can_touch(41) && !can_touch(45) && can_touch(7) && can_touch(40));
    }

    #[test]
    fn explosion_starts_with_nineteen_blasts_then_one_per_tick() {
        let words: Vec<i32> = (0..200).collect();
        let mut e = Explosion::default();
        let (b, alive) = e.step(&words, 100);
        assert!(alive);
        assert_eq!(b.len(), 20, "counter 21 -> 20 steps in the first tick");
        assert_eq!(b[0], Blast { dx: 1, dy: 2, strength: 369, sound: true });
        assert_eq!(b[17], Blast { dx: 18, dy: 19, strength: 352, sound: true });
        assert!(!b[1].sound);
        let (b, _) = e.step(&words, 100);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].strength, 349);
    }

    #[test]
    fn explosion_ends_when_k_reaches_the_limit() {
        let words: Vec<i32> = vec![0; 200];
        let mut e = Explosion::default();
        let mut total = 0;
        for _ in 0..200 {
            let (b, alive) = e.step(&words, 30);
            total += b.len();
            if !alive {
                break;
            }
        }
        assert_eq!(total, 29, "k = 1..=29 blast, k = 30 ends it");
    }
}
