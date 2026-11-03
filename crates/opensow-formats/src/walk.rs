//! Ground-unit walking helpers decoded from the exe (docs/walking.md).

/// The pixel line a unit walks from `(x0, y0)` to `(x1, y1)` (`0x46ee80`): integer Bresenham with the error term
/// `2*minor - major`, the start point included, one point per pixel along the long axis (a tie counts as y-major).
pub fn pixel_line(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let (sx, sy) = (if dx < 0 { -1 } else { 1 }, if dy < 0 { -1 } else { 1 });
    let (adx, ady) = (dx.abs(), dy.abs());
    let mut out = vec![(x0, y0)];
    let (mut x, mut y) = (x0, y0);
    if adx > ady {
        let mut d = 2 * ady - adx;
        while x != x1 {
            x += sx;
            if d >= 0 {
                y += sy;
                d += 2 * (ady - adx);
            } else {
                d += 2 * ady;
            }
            out.push((x, y));
        }
    } else {
        let mut d = 2 * adx - ady;
        while y != y1 {
            y += sy;
            if d >= 0 {
                x += sx;
                d += 2 * (adx - ady);
            } else {
                d += 2 * adx;
            }
            out.push((x, y));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_have_one_point_per_long_axis_pixel() {
        assert_eq!(pixel_line(0, 0, 0, 0), vec![(0, 0)]);
        assert_eq!(pixel_line(0, 0, 4, 0).len(), 5);
        let l = pixel_line(0, 0, 10, 4);
        assert_eq!(l.len(), 11);
        assert_eq!(l.last(), Some(&(10, 4)));
        let l = pixel_line(5, 5, 2, -4);
        assert_eq!(l.len(), 10);
        assert!(l.windows(2).all(|p| (p[1].0 - p[0].0).abs() <= 1 && (p[1].1 - p[0].1).abs() == 1));
    }

    #[test]
    fn diagonal_ties_step_both_axes() {
        assert_eq!(pixel_line(0, 0, 3, 3), vec![(0, 0), (1, 1), (2, 2), (3, 3)]);
    }
}
