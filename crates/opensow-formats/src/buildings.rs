//! Building footprints, decoded from `State of War.exe` (footprint function `0x409760`, jump table `0x409910`, layout masks
//! `0x486b20`, constructor `0x40a330`).
//!
//! Every building and turret has a width/height in cells and a 25-entry layout mask. The mask is read with a row stride of
//! the width: entry `i` is cell `(i % w, i / w)` and `1` means the cell is blocked by the building (the top row is always
//! open: the sprite leans back). Turret placement (`0x408be0`) needs every footprint cell free of units, buildings and
//! blocked terrain, and the footprint expanded by two cells to touch a building of the same player.

/// `(width, height, layout)` of an object kind: turrets 25-29 and buildings 100-108.
pub fn footprint(kind: i32) -> Option<(i32, i32, usize)> {
    Some(match kind {
        25 => (1, 1, 8),
        26 | 27 => (1, 1, 9),
        28 | 29 => (1, 2, 10),
        100 => (4, 4, 0),
        101 => (3, 3, 1),
        102 => (5, 4, 2),
        103 => (5, 4, 3),
        104 => (3, 4, 4),
        105 => (3, 3, 5),
        106 => (3, 3, 6),
        107 => (3, 4, 7),
        108 => (4, 3, 11),
        _ => return None,
    })
}

/// The layout masks, in the exe's order: HQ, light, medium, heavy, air base, gold, lab, wind, turret 1, turrets 2-3, turrets 4-5, walker factory.
pub const LAYOUTS: [[i32; 25]; 12] = [
    [0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
];

/// The cells (offsets from the origin cell) a building of `kind` blocks.
pub fn blocked_cells(kind: i32) -> Vec<(i32, i32)> {
    let Some((w, h, layout)) = footprint(kind) else { return Vec::new() };
    (0..(w * h) as usize).filter(|&i| LAYOUTS[layout][i] == 1).map(|i| (i as i32 % w, i as i32 / w)).collect()
}

/// Whether a turret of `kind` may stand on `cell` (`0x408be0`): all footprint cells are free (`free`) and some cell within two
/// cells of the footprint belongs to a building of the player (`own_near`).
pub fn can_place(kind: i32, cell: (i32, i32), free: impl Fn(i32, i32) -> bool, own_near: impl Fn(i32, i32) -> bool) -> bool {
    let Some((w, h, _)) = footprint(kind) else { return false };
    let near = (cell.1 - 2..cell.1 + h + 2).any(|y| (cell.0 - 2..cell.0 + w + 2).any(|x| own_near(x, y)));
    near && (0..h).all(|dy| (0..w).all(|dx| free(cell.0 + dx, cell.1 + dy)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprints() {
        assert_eq!(footprint(102), Some((5, 4, 2)));
        assert_eq!(blocked_cells(26), vec![(0, 0)]);
        assert_eq!(blocked_cells(101).len(), 5);
        assert_eq!(blocked_cells(100).len(), 12);
        assert!(blocked_cells(100).iter().all(|&(_, y)| y >= 1));
    }

    #[test]
    fn turrets_need_a_nearby_own_building() {
        let own = |x: i32, y: i32| (5..8).contains(&x) && (5..8).contains(&y);
        assert!(can_place(25, (9, 6), |_, _| true, own));
        assert!(!can_place(25, (12, 6), |_, _| true, own));
        assert!(!can_place(28, (9, 6), |x, y| (x, y) != (9, 7), own));
    }
}
