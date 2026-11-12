//! The sparks that run along the table lines of the Load / Save page (`0x443210`, called by the page's per-frame update `0x444660`).
//!
//! The page keeps up to ten sparks (bullet type `0x21`, list `[0x49ce38]`). Each frame, while fewer than ten exist, `rand(10) == 5` (the visual
//! generator `0x437cd0`) spawns one at a random grid vertex: column `COLUMNS[rand(4)]`, row `rand(11)`; its first segment goes to
//! [`next_vertex`]`(v, v)`. A spark makes `2 + rand(2)` segments; whenever its bullet has flown to the end of a segment, the next vertex is chosen
//! from (previous, current) and a new type-33 bullet is built for the segment with delay `0x50` (`0x411dc0(0x21, 0x50, ...)`; the delay skips
//! the first 80 path steps of segments longer than that). All positions here are relative to the screen centre.

/// Table columns, relative to the screen centre x (`0x445503..0x445521`: `cx - 0x12c`, `cx - 0x10e`, `cx - 0x2d`, `cx + 0x68`).
pub const COLUMNS: [i32; 4] = [-300, -270, -45, 104];
/// y of the first table line relative to the screen centre (`0x445538`: `cy - 0xa0`), eleven lines 32 px apart.
pub const ROW0: i32 = -160;
/// Number of table lines.
pub const ROWS: i32 = 11;
/// Pixels between two table lines.
pub const ROW_STEP: i32 = 32;
/// Sparks alive at the same time at most (a new one is only born while fewer than ten exist, `0x444b79`).
pub const MAX_SPARKS: usize = 10;
/// Delay passed to the bullet constructor for the follow-up segments.
pub const SEGMENT_DELAY: u32 = 0x50;

fn column_of(x: i32) -> i32 {
    COLUMNS.iter().position(|&c| c == x).map_or(-1, |i| i as i32)
}

fn row_of(y: i32) -> i32 {
    (0..ROWS).find(|&i| ROW0 + ROW_STEP * i == y).unwrap_or(-1)
}

/// `0x443210(prev_x, prev_y, x, y, &out_x, &out_y)`: the vertex a spark at `(x, y)` that came from `(prev_x, prev_y)` runs to next.
/// `rand(n)` is the visual generator and is called once, unless there is no choice to make. Returns `None` where the exe returns without
/// writing its outputs (the caller's variables keep their old values), `Some(current)` for a point that is not a grid vertex.
///
/// The number of choices is `(3 if the column is an inner one else 2) - (1 on the first or last row, i.e. row >= 10 or 0) + (1 when the
/// spark has not moved yet)`; the choices are taken in the order up, right, down, left, skipping the vertex it came from. Quirk kept: the
/// "down" test compares the row with 11 instead of 10, so on the last line a spark can run to a twelfth line below the table.
pub fn next_vertex(prev: (i32, i32), cur: (i32, i32), mut rand: impl FnMut(u32) -> u32) -> Option<(i32, i32)> {
    let (pc, cc) = (column_of(prev.0), column_of(cur.0));
    let (pr, cr) = (row_of(prev.1), row_of(cur.1));
    if pc == -1 || cc == -1 || pr == -1 || cr == -1 {
        return Some(cur);
    }
    let mut n = if cc == 0 || cc >= 3 { 2 } else { 3 };
    if cr == 0 || cr >= 10 {
        n -= 1;
    }
    if prev == cur {
        n += 1;
    }
    let mut r = if n == 0 { 0 } else { rand(n as u32) as i32 };
    let y_of = |row: i32| ROW0 + ROW_STEP * row;
    if cr > 0 && cr - 1 != pr {
        if r == 0 {
            return Some((COLUMNS[cc as usize], y_of(cr - 1)));
        }
        r -= 1;
    }
    if cc < 3 && cc + 1 != pc {
        if r <= 0 {
            return Some((COLUMNS[cc as usize + 1], y_of(cr)));
        }
        r -= 1;
    }
    if cr < 11 && cr + 1 != pr {
        if r <= 0 {
            return Some((COLUMNS[cc as usize], y_of(cr + 1)));
        }
        r -= 1;
    }
    if cc > 0 && cc - 1 != pc && r <= 0 {
        return Some((COLUMNS[cc as usize - 1], y_of(cr)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(prev: (i32, i32), cur: (i32, i32), r: u32) -> Option<(i32, i32)> {
        next_vertex(prev, cur, |n| r % n)
    }

    #[test]
    fn a_new_spark_may_go_anywhere_a_moving_one_does_not_turn_back() {
        let c = (COLUMNS[1], ROW0 + 32 * 4);
        // not moved yet: 4 choices up, right, down, left
        assert_eq!(pick(c, c, 0), Some((COLUMNS[1], ROW0 + 32 * 3)));
        assert_eq!(pick(c, c, 1), Some((COLUMNS[2], ROW0 + 32 * 4)));
        assert_eq!(pick(c, c, 2), Some((COLUMNS[1], ROW0 + 32 * 5)));
        assert_eq!(pick(c, c, 3), Some((COLUMNS[0], ROW0 + 32 * 4)));
        // came from above: down, right, left remain (up is skipped)
        let from_above = (COLUMNS[1], ROW0 + 32 * 3);
        assert_eq!(pick(from_above, c, 0), Some((COLUMNS[2], ROW0 + 32 * 4)));
        assert_eq!(pick(from_above, c, 1), Some((COLUMNS[1], ROW0 + 32 * 5)));
        assert_eq!(pick(from_above, c, 2), Some((COLUMNS[0], ROW0 + 32 * 4)));
    }

    #[test]
    fn off_grid_points_stay_and_the_last_line_has_the_exes_quirk() {
        assert_eq!(pick((0, 0), (1, 1), 0), Some((1, 1)));
        // top-left corner, new spark: right or down (2 choices)
        let corner = (COLUMNS[0], ROW0);
        assert_eq!(pick(corner, corner, 0), Some((COLUMNS[1], ROW0)));
        assert_eq!(pick(corner, corner, 1), Some((COLUMNS[0], ROW0 + 32)));
        // bottom line, inner column, came from above: n = 2 but candidates are right, "down" (a twelfth line), left
        let from = (COLUMNS[1], ROW0 + 32 * 9);
        let bottom = (COLUMNS[1], ROW0 + 32 * 10);
        assert_eq!(pick(from, bottom, 0), Some((COLUMNS[2], ROW0 + 32 * 10)));
        assert_eq!(pick(from, bottom, 1), Some((COLUMNS[1], ROW0 + 32 * 11)));
    }
}
