//! Scatter (key X), decoded from `State of War.exe` (docs/hotkeys.md).
//!
//! The key handler (`0x4167e1`, key flag `[pointer+0x91]` = scan code 0x2d) calls `0x418dc0`: the box round the selected units is
//! taken in pixels, its middle in half cells (`px >> 4`); every selected unit then walks away from that middle along the straight
//! line through its own half cell, `n` = 6 half cells further (`0x4378d0`). When that cell is outside the map or blocked for
//! walkers (`0x6c7a4c`), the next try uses `n` = 4, 2 and 0 (0 = the unit's own cell); a unit with no free try stays. The order
//! is the ordinary go-to (`0x4706a0` type 1) to the cell's centre. The pods of missions 3 and 8 take none. In a network game
//! the same goes out as the UnitScatter command instead (`0x418ea5`).

/// DirectInput scan code of the key (X): `cmp [esi+0x91], 1` at `0x4167e1`.
pub const HOTKEY_SCAN: u8 = 0x2d;

/// The distances tried in turn, in half cells (`0x41900e`: 6, then `sub esi, 2` while not negative).
pub const TRIES: [i32; 4] = [6, 4, 2, 0];

/// The middle of the selection's pixel box in half cells (`0x418f9d`): `((max >> 4) + (min >> 4)) >> 1` per axis.
pub fn centre(min: (i32, i32), max: (i32, i32)) -> (i32, i32) {
    let h = |a: i32, b: i32| (((a as u32 >> 4) + (b as u32 >> 4)) >> 1) as i32;
    (h(min.0, max.0), h(min.1, max.1))
}

/// `0x4378d0(&x, &y, cx, cy, n)`: walks the Bresenham line from `(cx, cy)` towards `p`, and once the major axis has reached `p`'s,
/// `n` steps in all (that step included); returns where it stops. `n` outside 1..=10 leaves `p` as it is.
pub fn extend(p: (i32, i32), c: (i32, i32), n: i32) -> (i32, i32) {
    if !(1..=10).contains(&n) {
        return p;
    }
    let (dx, dy) = (p.0.wrapping_sub(c.0), p.1.wrapping_sub(c.1));
    let (sx, sy) = (if dx < 0 { -1 } else { 1 }, if dy < 0 { -1 } else { 1 });
    let (ax, ay) = (dx.wrapping_abs(), dy.wrapping_abs());
    let (mut x, mut y, mut left, mut reached) = (c.0, c.1, n, false);
    // the major axis steps every time, the minor one when the error term is not negative
    let (major, minor_d, major_d) = if ax > ay { (true, ay, ax) } else { (false, ax, ay) };
    let mut err = minor_d.wrapping_mul(2).wrapping_sub(major_d);
    loop {
        if major {
            reached |= x == p.0;
            x = x.wrapping_add(sx);
        } else {
            reached |= y == p.1;
            y = y.wrapping_add(sy);
        }
        let e = err;
        err = e.wrapping_add(2i32.wrapping_mul(if e >= 0 { minor_d.wrapping_sub(major_d) } else { minor_d }));
        if e >= 0 {
            if major {
                y = y.wrapping_add(sy);
            } else {
                x = x.wrapping_add(sx);
            }
        }
        if reached {
            left -= 1;
            if left <= 0 {
                return (x, y);
            }
        }
    }
}

/// The cell try `n` sends a unit at pixel `(x, y)` to, for the selection middle `c` (half cells): the line end halved towards
/// zero (`cdq; sub; sar 1` at `0x41903d`).
pub fn target(x: i32, y: i32, c: (i32, i32), n: i32) -> (i32, i32) {
    let (hx, hy) = extend(((x as u32 >> 4) as i32, (y as u32 >> 4) as i32), c, n);
    (hx / 2, hy / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unit_moves_away_from_the_middle() {
        // straight right: the step off the unit's own half cell is the first of the 6
        assert_eq!(extend((10, 5), (5, 5), 6), (16, 5));
        assert_eq!(extend((0, 5), (5, 5), 6), (-6, 5));
        assert_eq!(extend((5, 12), (5, 5), 4), (5, 16));
        // a diagonal keeps going diagonally
        assert_eq!(extend((8, 8), (5, 5), 2), (10, 10));
        // a unit in the middle goes down and right
        assert_eq!(extend((5, 5), (5, 5), 6), (11, 11));
        // n = 0 (and anything outside 1..=10) is the unit's own spot
        assert_eq!(extend((7, 3), (5, 5), 0), (7, 3));
        assert_eq!(extend((7, 3), (5, 5), 11), (7, 3));
    }

    #[test]
    fn the_middle_and_the_target_cell_are_in_half_cells() {
        assert_eq!(centre((32, 32), (96, 64)), (4, 3));
        // unit at pixel (100, 48) = half cell (6, 3), middle (4, 3): 6 further = (12, 3) -> cell (6, 1)
        assert_eq!(target(100, 48, (4, 3), 6), (6, 1));
        // a line end left of the map is rounded towards zero, like the exe
        assert_eq!(target(0, 48, (4, 3), 2), (-1, 1));
    }
}
