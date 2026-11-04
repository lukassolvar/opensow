//! The fallback every step planner tries after the local search fails: `0x4774c0(this=chassis, x, y)` finds a cell near the
//! unit's destination from the range-ring list (`rings.rs`) and moves the destination to its centre (docs/pathfinding.md,
//! "The fallback `0x4774c0`"). Callers: `0x473190` (single player), `0x472f50` (twice, multiplayer); `x, y` is the unit's own
//! pixel position, the destination is `[chassis+0x5c/0x60]`.
//!
//! * **Ground** (`[unit type + 0x4b] == 0`): `0x4726f0(x, y)` first floods the unit grid (`0x61ac38`, counter `[0x49a36c]`)
//!   from the unit's cell: every free cell reachable from it (8-way, the wave's diagonal rule, a generation of more than 8192
//!   cells ends the flood) is stamped with the id `base + 1`, and the counter becomes `base + 4`. Then the ring list around the
//!   destination cell is scanned (centre, ring 1, ... up to ring 32) for the first cell whose unit-grid value is the unit's own
//!   handle or that id: **the nearest cell to the destination that the unit can reach**.
//! * **Air** (Advancer, `[type + 0x4b] != 0`): no flood and the arguments are ignored; the first cell of the ring scan whose
//!   value in the air unit grid (`0x62ac38`) is the unit's own handle or below `0x11170` is taken.
//!
//! The found cell becomes the destination (its centre `cell * 32 + 16`) when it lies inside the map (`[0x71cec0]` x
//! `[0x71cec4]` cells); the return value is 1 even when the map test fails (destination unchanged), 0 only when the scan
//! found nothing in 32 rings. The exe has no bounds checks (its maps keep a blocked border); the port skips cells outside the
//! 128 x 128 array.

use crate::rings::Rings;
use crate::wave::{Wave, SIDE};

/// Rings searched (`[0x6b3a98]` = entry 32 of the ring index table `0x6b3a18`).
pub const SEARCH_RINGS: usize = 32;
/// The air scan takes any value below this (`cmp eax, 0x11170` at `0x477518`).
pub const AIR_FREE_BELOW: u32 = 0x11170;
/// A flood generation of more than this many cells ends the flood (`cmp eax, 0x4000` on the dword count).
const MAX_GENERATION: usize = 0x2000;

fn idx(x: i32, y: i32) -> Option<usize> {
    let i = x * SIDE + y;
    ((0..SIDE).contains(&x) && (0..SIDE).contains(&y)).then_some(i as usize)
}

impl Wave {
    /// `0x4726f0(x, y)` (pixels, on the unit grid `0x61ac38`): stamps every free cell reachable from the cell of `(x, y)` with
    /// `base + 1`, sets the base to `base + 4` and returns that stamp (the region id).
    /// Cells claimed per processed cell in the order W, E, S, N, then the diagonals NW, NE, SE, SW (only when none of the two
    /// orthogonal cells beside it was claimed by this cell: masks 9, 0xa, 6, 5), like the wave.
    pub fn flood_region(&mut self, from_px: (i32, i32)) -> u32 {
        let base = self.base;
        let stamp = base + 1;
        let start = (from_px.0 / 32, from_px.1 / 32); // the exe rounds toward zero
        let mut cur = vec![start];
        loop {
            let mut next: Vec<(i32, i32)> = Vec::new();
            for &(x, y) in &cur {
                let mut claim = |cx: i32, cy: i32, cells: &mut Vec<u32>| -> bool {
                    match idx(cx, cy) {
                        Some(i) if cells[i] < base => {
                            cells[i] = stamp;
                            next.push((cx, cy));
                            true
                        }
                        _ => false,
                    }
                };
                let mut mask = 0u8;
                if claim(x - 1, y, &mut self.cells) {
                    mask |= 1;
                }
                if claim(x + 1, y, &mut self.cells) {
                    mask |= 2;
                }
                if claim(x, y - 1, &mut self.cells) {
                    mask |= 4;
                }
                if claim(x, y + 1, &mut self.cells) {
                    mask |= 8;
                }
                if mask & 9 == 0 {
                    claim(x - 1, y + 1, &mut self.cells);
                }
                if mask & 0xa == 0 {
                    claim(x + 1, y + 1, &mut self.cells);
                }
                if mask & 6 == 0 {
                    claim(x + 1, y - 1, &mut self.cells);
                }
                if mask & 5 == 0 {
                    claim(x - 1, y - 1, &mut self.cells);
                }
            }
            if next.is_empty() || next.len() > MAX_GENERATION {
                break;
            }
            cur = next;
        }
        self.base = stamp + 3;
        stamp
    }
}

/// What `0x4774c0` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreeCell {
    /// The routine's return value (the planner then keeps its step): false only when the ring scan found no cell.
    pub found: bool,
    /// The new destination in pixels (`[chassis+0x5c/0x60]`), when it was written.
    pub destination: Option<(i32, i32)>,
}

/// `0x4774c0`. `grid` is the unit grid of the unit's layer (ground `0x61ac38` / air `0x62ac38`), `own` its handle
/// (`[chassis+0x14]`), `dest` the current destination in pixels, `at_px` the unit's pixel position (ground only), `map` the
/// map size in cells.
pub fn free_cell_near(grid: &mut Wave, rings: &Rings, own: u32, dest: (i32, i32), at_px: (i32, i32), air: bool, map: (i32, i32)) -> FreeCell {
    let region = if air { None } else { Some(grid.flood_region(at_px)) };
    let (cx, cy) = ((dest.0 as u32 >> 5) as i32, (dest.1 as u32 >> 5) as i32);
    for &(dx, dy) in &rings.offsets[..rings.ends[SEARCH_RINGS]] {
        let (x, y) = (cx + dx as i32, cy + dy as i32);
        let Some(i) = idx(x, y) else { continue };
        let v = grid.cells[i];
        let hit = match region {
            Some(id) => v == own || v == id,
            None => v == own || v < AIR_FREE_BELOW,
        };
        if hit {
            let (px, py) = (x * 32 + 16, y * 32 + 16);
            let inside = (px as u32) < (map.0 as u32) << 5 && (py as u32) < (map.1 as u32) << 5;
            return FreeCell { found: true, destination: inside.then_some((px, py)) };
        }
    }
    FreeCell { found: false, destination: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_grid() -> Wave {
        Wave::new(|x, y| x < 1 || y < 1 || x >= 60 || y >= 60)
    }

    #[test]
    fn flood_stamps_the_connected_region_only() {
        let mut w = open_grid();
        for y in 0..61 {
            w.cells[30 * 128 + y] = 0xFFFFFD; // a wall splitting the room
        }
        let id = w.flood_region((10 * 32 + 16, 10 * 32 + 16));
        assert_eq!(id, 2);
        assert_eq!(w.base, 5);
        assert_eq!(w.cells[11 * 128 + 10], id);
        assert_eq!(w.cells[10 * 128 + 10], id, "the start cell is claimed back by its neighbours");
        assert_eq!(w.cells[40 * 128 + 10], 0, "the other side of the wall is not reached");
        assert_eq!(w.cells[30 * 128 + 10], 0xFFFFFD);
    }

    #[test]
    fn ground_takes_the_nearest_reachable_cell() {
        let rings = Rings::generate();
        let mut w = open_grid();
        for y in 0..61 {
            w.cells[30 * 128 + y] = 0xFFFFFD;
        }
        // the destination is inside a pocket on the far side of the wall: the nearest reachable cell is next to the wall
        let r = free_cell_near(&mut w, &rings, 0x3000001, (31 * 32 + 5, 20 * 32 + 5), (10 * 32, 20 * 32), false, (64, 64));
        assert!(r.found);
        assert_eq!(r.destination, Some((29 * 32 + 16, 20 * 32 + 16)));
    }

    #[test]
    fn own_handle_counts_and_the_map_edge_keeps_the_destination() {
        let rings = Rings::generate();
        let mut w = open_grid();
        w.cells[20 * 128 + 20] = 0x3000001; // my own step cell, occupied for the flood but accepted by the scan
        let r = free_cell_near(&mut w, &rings, 0x3000001, (20 * 32, 20 * 32), (5 * 32, 5 * 32), false, (64, 64));
        assert_eq!(r.destination, Some((20 * 32 + 16, 20 * 32 + 16)));
        let mut w = open_grid();
        let r = free_cell_near(&mut w, &rings, 1, (20 * 32, 20 * 32), (5 * 32, 5 * 32), false, (10, 10));
        assert!(r.found && r.destination.is_none(), "outside the 10 x 10 map: returns 1, destination untouched");
    }

    #[test]
    fn air_takes_free_or_own_cells_and_fails_after_32_rings() {
        let rings = Rings::generate();
        let mut w = open_grid();
        w.cells.iter_mut().for_each(|c| *c = 0x2000000);
        w.cells[22 * 128 + 20] = 0x11170 - 1;
        let r = free_cell_near(&mut w, &rings, 7, (20 * 32, 20 * 32), (0, 0), true, (64, 64));
        assert_eq!(r.destination, Some((22 * 32 + 16, 20 * 32 + 16)));
        w.cells[22 * 128 + 20] = 0x2000000;
        let r = free_cell_near(&mut w, &rings, 7, (20 * 32, 20 * 32), (0, 0), true, (64, 64));
        assert_eq!(r, FreeCell { found: false, destination: None });
    }
}
