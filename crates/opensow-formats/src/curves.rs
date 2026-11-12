//! Random-curve bullets (types 13 and 14), ported from `State of War.exe`.
//!
//! The curve table is built every time a game starts (`0x422c13` -> `0x40f2c0`, right after the lockstep RNG index is reset to
//! 0), from the lockstep generator ([`GameRng`], `rand(n)` = `0x437b10`), so it is the same in every game and every
//! multiplayer peer. For each of the 200 curves it draws
//!
//! 1. `dx = rand(300) - 150`, `dy = rand(300) - 150`: the end point relative to the start;
//! 2. the ground track = Bresenham line to `(dx, dy)` (same rule as [`build_path`]: start excluded, end included, so
//!    `max(|dx|, |dy|)` points);
//! 3. `rand(3)`: 0 -> lift `h(i) = i - i*i/n`, otherwise `h(i) = 2i - 2*i*i/n` (`n` = number of points, `i` = 0..n,
//!    divisions truncate); `y -= h(i)`. The lift is stored per point too.
//!
//! That is 600 `rand` calls, so the RNG index is 600 higher afterwards (plus whatever was drawn before it).
//!
//! A bullet of type 13 or 14 is built by `0x411dc0` (type 13: `[+0x268c] = rand(2)`, speed `30 + rand(60)`; type 14: speed
//! `60 + rand(30)`; both in that order, before the path) and then `0x40ef60` draws `rand(200)` and copies that curve, offset
//! by the start point `(x1, y1)`; the end point passed in is ignored. Both are fired by burning buildings (`0x409d90`: 3 of each,
//! strength 80). Bullets of these types reach the end of the curve and are then removed; the removal does the generic area
//! damage (`0x42f2c0`, strength 80 at the last point, as for every type that has no special case).

use crate::projectile::{build_path, Path};
use crate::rng::GameRng;

/// Number of precomputed curves (`0x4ec098` has 200 counts).
pub const CURVES: usize = 200;

/// One curve: points relative to the start point (lift already subtracted from `y`) and the lift per point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Curve {
    /// The points, relative to the start point.
    pub points: Vec<(i32, i32)>,
    /// The lift at each point, in pixels.
    pub lift: Vec<i32>,
}

/// The 200 curves (`0x4ec228` lift, `0x49de98` points, `0x4ec098` lengths).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveTable {
    /// The curves, in table order.
    pub curves: Vec<Curve>,
}

impl CurveTable {
    /// `0x40f2c0`: draws 600 numbers from `rng`.
    pub fn generate(rng: &mut GameRng) -> CurveTable {
        let mut curves = Vec::with_capacity(CURVES);
        for _ in 0..CURVES {
            let dx = rng.next(300) as i32 - 150;
            let dy = rng.next(300) as i32 - 150;
            let mut points = build_path(5, 0, 0, dx, dy, 0).expect("straight type").points;
            let n = points.len() as i32;
            let shallow = rng.next(3) == 0;
            let mut lift = vec![0; points.len()];
            for (i, p) in points.iter_mut().enumerate() {
                let i = i as i32;
                let h = if shallow { i - i * i / n } else { 2 * i - 2 * i * i / n };
                lift[i as usize] = h;
                p.1 -= h;
            }
            curves.push(Curve { points, lift });
        }
        CurveTable { curves }
    }

    /// Path of curve `index` for a bullet starting at `(x1, y1)` (`0x40ef60`, types 13/14, after the bullet drew
    /// `index = rand(200)`). `delay` is 0 for these bullets.
    pub fn path(&self, index: usize, x1: i32, y1: i32, delay: u32) -> Path {
        let c = &self.curves[index];
        let skip = delay as usize * 2;
        let first_step = if c.points.len() * 2 <= skip { 0 } else { skip } / 2;
        Path { points: c.points.iter().map(|&(x, y)| (x + x1, y + y1)).collect(), lift: c.lift.clone(), first_step, facing: None }
    }
}

/// What the constructor and the path builder draw for a bullet of type 13 or 14, in that order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomBullet {
    /// Path steps per 60 ticks (`[+0x2664]`).
    pub speed: u32,
    /// Type 13 only: `rand(2)` stored at `[+0x268c]`.
    pub flag: Option<u32>,
    /// Curve number (`rand(200)`).
    pub curve: usize,
}

impl RandomBullet {
    /// Draws the random numbers of a new bullet of `bullet_type` (13 or 14); `None` for other types.
    pub fn draw(bullet_type: u8, rng: &mut GameRng) -> Option<RandomBullet> {
        let (flag, speed) = match bullet_type {
            13 => {
                let f = rng.next(2) as u32;
                (Some(f), 30 + rng.next(60) as u32)
            }
            14 => (None, 60 + rng.next(30) as u32),
            _ => return None,
        };
        Some(RandomBullet { speed, flag, curve: rng.next(CURVES as u16) as usize })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::TABLE_LEN;

    fn rng() -> GameRng {
        let data: Vec<u8> = (0..TABLE_LEN as u32).flat_map(|i| ((i.wrapping_mul(2654435761) >> 7) as u16).to_le_bytes()).collect();
        GameRng::from_file(&data).unwrap()
    }

    #[test]
    fn table_has_200_curves_and_uses_600_numbers() {
        let mut r = rng();
        let t = CurveTable::generate(&mut r);
        assert_eq!(t.curves.len(), CURVES);
        assert_eq!(r.index(), 600);
        for c in &t.curves {
            assert!(c.points.len() <= 150);
            assert_eq!(c.points.len(), c.lift.len());
        }
    }

    #[test]
    fn lift_follows_the_two_formulas() {
        let t = CurveTable::generate(&mut rng());
        for c in t.curves.iter().filter(|c| c.points.len() > 4) {
            let n = c.points.len() as i32;
            let a = c.lift.iter().enumerate().all(|(i, &h)| h == i as i32 - (i * i) as i32 / n);
            let b = c.lift.iter().enumerate().all(|(i, &h)| h == 2 * i as i32 - 2 * (i * i) as i32 / n);
            assert!(a || b);
        }
    }

    #[test]
    fn path_is_offset_by_the_start() {
        let t = CurveTable::generate(&mut rng());
        let c = &t.curves[5];
        let p = t.path(5, 1000, 2000, 0);
        assert_eq!(p.points[0], (c.points[0].0 + 1000, c.points[0].1 + 2000));
        assert_eq!(p.first_step, 0);
    }

    #[test]
    fn bullet_draws_in_constructor_order() {
        let mut a = rng();
        let b13 = RandomBullet::draw(13, &mut a).unwrap();
        assert_eq!(a.index(), 3);
        assert!((30..90).contains(&b13.speed) && b13.flag.unwrap() < 2 && b13.curve < 200);
        let b14 = RandomBullet::draw(14, &mut a).unwrap();
        assert_eq!(a.index(), 5);
        assert!((60..90).contains(&b14.speed) && b14.flag.is_none());
        assert!(RandomBullet::draw(3, &mut a).is_none());
    }
}
