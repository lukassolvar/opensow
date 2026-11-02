//! Range rings: the cell-offset list the exe builds at start-up (`0x436230`).
//!
//! Target searches (`0x47b140`) walk this list instead of testing distances: ring `r` holds the
//! offsets `(dx, dy)` (in 32 px cells) first reached by an octagon of "radius" `r`. A search with
//! range `max` and dead zone `min` scans rings `min+1 ..= max`.
//!
//! The generator keeps 16 corner counters of a closed octagon. Every step grows it (steps cycle
//! through states 2, 3, 4; 2 and 3 close a ring, 4 only grows the octagon so its cells join the next
//! ring), then the 8 edges are walked in order, emitting every cell not emitted before. Ring 1 is
//! (0,-1), (1,0), (0,1), (-1,0); the centre `(0,0)` is ring 0 and is never marked visited.

/// Generator steps the exe performs (`0x91`).
const STEPS: usize = 145;

/// The eight neighbour offsets used when the search range is 1 (table at `0x6b3c70`).
pub const NEIGHBOURS: [(i16, i16); 8] = [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)];

#[derive(Debug, Clone)]
pub struct Rings {
    /// All offsets, ring after ring, starting with the centre.
    pub offsets: Vec<(i16, i16)>,
    /// `ends[r]` = number of offsets up to and including ring `r` (the exe stores it doubled, in words, at `0x6b3a18`).
    pub ends: Vec<usize>,
}

impl Rings {
    pub fn generate() -> Rings {
        const SIZE: usize = 300;
        const MID: i32 = 150;
        let mut visited = vec![false; SIZE * SIZE];
        let mut offsets = vec![(0i16, 0i16)];
        let mut ends = vec![1usize];
        let emit = |visited: &mut Vec<bool>, offsets: &mut Vec<(i16, i16)>, a: i32, b: i32| {
            let cell = &mut visited[(a + MID) as usize * SIZE + (b + MID) as usize];
            if !*cell {
                *cell = true;
                offsets.push((a as i16, b as i16));
            }
        };
        // Corner counters, named after their stack slots `[ebp - 0x15fXX]`.
        let (mut bc, mut c0, mut c4, mut c8, mut cc, mut b8, mut d0, mut d4) = (0, 0, 0, 0, 0, 0, 0, 0);
        let (mut d8, mut dc, mut e0, mut e4, mut ac, mut a0, mut b4, mut b0) = (0, 0, 0, 0, 0, 0, 0, 0);
        let mut state = 2;
        let mut record = Vec::new();
        for _ in 0..STEPS {
            if state != 4 {
                e0 += 1;
                d0 += 1;
                c0 -= 1;
                e4 -= 1;
                ac += 1;
                a0 -= 1;
                b4 += 1;
                b0 -= 1;
            } else {
                bc += 1;
                c4 -= 1;
                c8 += 1;
                cc += 1;
                b8 -= 1;
                d4 += 1;
                d8 -= 1;
                dc -= 1;
            }
            // Edge walks: (start, per-step delta, end); the end corner belongs to the next edge.
            let edges = [
                ((bc, c0), (1, 1), (ac, c4)),
                ((ac, c4), (0, 1), (e0, c8)),
                ((e0, c8), (-1, 1), (cc, b4)),
                ((cc, b4), (-1, 0), (b8, d0)),
                ((b8, d0), (-1, -1), (a0, d4)),
                ((a0, d4), (0, -1), (e4, d8)),
                ((e4, d8), (1, -1), (dc, b0)),
                ((dc, b0), (1, 0), (bc, c0)),
            ];
            for ((mut a, mut b), (da, db), end) in edges {
                while (a, b) != end {
                    emit(&mut visited, &mut offsets, a, b);
                    a += da;
                    b += db;
                }
            }
            match state {
                2 => {
                    record.push(offsets.len());
                    state = 3;
                }
                3 => {
                    record.push(offsets.len());
                    state = 4;
                }
                _ => state = 2,
            }
        }
        ends.extend(record);
        Rings { offsets, ends }
    }

    /// Offsets of ring `r` alone.
    pub fn ring(&self, r: usize) -> &[(i16, i16)] {
        let start = if r == 0 { 0 } else { self.ends[r - 1] };
        &self.offsets[start..self.ends[r]]
    }

    /// Offsets a search with dead zone `min` and range `max` visits, in scan order.
    pub fn search(&self, min: usize, max: usize) -> &[(i16, i16)] {
        &self.offsets[self.ends[min]..self.ends[max]]
    }

    /// The exe's range index table (`0x6b3a18`): `ends` in words.
    pub fn index_words(&self) -> Vec<usize> {
        self.ends.iter().map(|e| e * 2).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_exe_tables() {
        let r = Rings::generate();
        assert_eq!(&r.index_words()[..10], &[2, 10, 26, 74, 114, 194, 258, 370, 458, 602]);
        assert_eq!(r.index_words()[63], 28034);
        assert_eq!(r.ring(1), &[(0, -1), (1, 0), (0, 1), (-1, 0)]);
        assert_eq!(&r.ring(2)[..3], &[(0, -2), (1, -1), (2, 0)]);
        assert_eq!(r.offsets.len(), r.ends[r.ends.len() - 1]);
    }

    #[test]
    fn rings_are_symmetric_and_disjoint() {
        let r = Rings::generate();
        let mut seen = std::collections::HashSet::new();
        for &p in &r.offsets {
            assert!(seen.insert(p), "{p:?} listed twice");
        }
        for ring in 1..64 {
            let set: std::collections::HashSet<_> = r.ring(ring).iter().copied().collect();
            for &(a, b) in &set {
                assert!(set.contains(&(b, a)) && set.contains(&(-a, b)) && set.contains(&(a, -b)));
            }
        }
    }

    #[test]
    fn ring_point_counts() {
        let r = Rings::generate();
        let counts: Vec<usize> = (0..10).map(|i| r.ring(i).len()).collect();
        assert_eq!(counts, [1, 4, 8, 24, 20, 40, 32, 56, 44, 72]);
    }
}
