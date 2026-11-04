//! The spark loops of the Advancer beam bullets (types 19, 20, 21) and the frame count of the dim effect (type 23),
//! decoded from the constructor `0x411dc0` (cases at `0x412408`, `0x4123aa`, `0x412333` and `0x411ee0`).
//!
//! The constructor fills three arrays of ints in the bullet object: `A[i]` at `[+0x0c + 4i]` (x offset), `B[i]` at
//! `[+0x48 + 4i]` (y offset) and `C[i]` at `[+0x84 + 4i]` (frame), `i < [+0x268c]`. The random numbers come from `0x437cd0`
//! (the *visual* generator, a table of C `rand()` values: not reproducible, not part of the lockstep), so the functions here take
//! the generator as a closure `rand(n) -> 0..n`. The draw order is the exe's: per fragment `x`, then `y`, then the frame.
//!
//! | type | action | `[+0x2664]` speed | `[+0x264c]` | fragments | `A[i]` | `B[i]` | `C[i]` |
//! |---|---|---|---|---|---|---|---|
//! | 19 | repair | 60 | 100 | 5 | `rand(30) - 15` | `rand(20) - 10` | `rand(3)` |
//! | 20 | upgrade | 60 | 2 | 2 | `rand(20) - 10` | `rand(20) - 10` | 0 |
//! | 21 | destroy | 60 | 100 | 12 | 0 | `-i` | `rand(2)`; fragments 9, 10, 11 are then overwritten with 0, 4, 0 |
//!
//! Type 21 also sets the start delay `[+0x26ac]` to 5 (steps skipped at the start of the path). Its `-i` comes from reusing the
//! constructor's owner argument slot (`[ebp+0x20]`, already copied to `[+0x266c]`) as a down counter.

/// Fragments of one beam bullet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sparks {
    /// `(A[i], B[i], C[i])` per fragment: x offset, y offset, frame.
    pub fragments: Vec<(i32, i32, i32)>,
    /// `[+0x26ac]`: start delay of the path (5 for type 21, else 0).
    pub delay: u32,
}

impl Sparks {
    /// Draws the fragments of a new bullet of `bullet_type` (19, 20 or 21); `None` for other types.
    pub fn draw(bullet_type: u8, mut rand: impl FnMut(u16) -> u16) -> Option<Sparks> {
        let mut r = |n: u16, off: i32| rand(n) as i32 - off;
        Some(match bullet_type {
            19 => Sparks { fragments: (0..5).map(|_| { let x = r(30, 15); let y = r(20, 10); (x, y, r(3, 0)) }).collect(), delay: 0 },
            20 => Sparks { fragments: (0..2).map(|_| { let x = r(20, 10); let y = r(20, 10); (x, y, 0) }).collect(), delay: 0 },
            21 => {
                let mut f: Vec<(i32, i32, i32)> = (0..12).map(|i| (0, -i, r(2, 0))).collect();
                for (k, frame) in [(9, 0), (10, 4), (11, 0)] {
                    f[k].2 = frame;
                }
                Sparks { fragments: f, delay: 5 }
            }
            _ => return None,
        })
    }
}

/// Type 23 (the dim effect, sprites `dim1..dim4` of 21 frames): `[+0x2674]`, the frame at which it ends, from the variant
/// `[+0x2680]` (`ebx` in the constructor): 16 for variants 0 and 2, otherwise 20. Speed 60, `[+0x2678] = 1`, `[+0x267c] = 3`,
/// `[+0x2670] = 0` (see `effects::BulletAnim`).
pub fn dim_end_frame(variant: i32) -> i32 {
    if variant == 0 || variant == 2 {
        16
    } else {
        20
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_destroy_upgrade_shapes() {
        let mut k = 0u16;
        let mut seq = |n: u16| {
            k += 7;
            k % n
        };
        let s = Sparks::draw(19, &mut seq).unwrap();
        assert_eq!(s.fragments.len(), 5);
        assert!(s.fragments.iter().all(|&(x, y, c)| (-15..15).contains(&x) && (-10..10).contains(&y) && (0..3).contains(&c)));
        let s = Sparks::draw(20, &mut seq).unwrap();
        assert_eq!(s.fragments.len(), 2);
        assert!(s.fragments.iter().all(|&(x, y, c)| (-10..10).contains(&x) && (-10..10).contains(&y) && c == 0));
        let s = Sparks::draw(21, &mut seq).unwrap();
        assert_eq!(s.delay, 5);
        assert_eq!(s.fragments.len(), 12);
        for (i, f) in s.fragments.iter().enumerate() {
            assert_eq!((f.0, f.1), (0, -(i as i32)));
        }
        assert_eq!(s.fragments[9..].iter().map(|f| f.2).collect::<Vec<_>>(), [0, 4, 0]);
        assert!(Sparks::draw(22, &mut seq).is_none());
    }

    #[test]
    fn dim_end_frames() {
        assert_eq!([0, 1, 2, 3].map(dim_end_frame), [16, 20, 16, 20]);
    }
}
