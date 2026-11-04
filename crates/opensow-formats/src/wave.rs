//! The exe's wave pathfinder (`0x4753a0`, docs/pathfinding.md).
//!
//! Ground units find their way over a 128 x 128 grid of `u32` (`0x5fac38`, index `x * 128 + y`). A cell is *free* when its
//! value is below the running base `[0x49a364]`; terrain is `0xFFFFFD` (and building handles are large too), so they never
//! are. Instead of clearing the grid after a search the exe raises the base past every stamp it wrote.
//!
//! The search is a breadth-first wave from the unit's cell: the cells of one generation are processed in order, each
//! stamping the free neighbours it claims with a counter that goes up by one per processed cell (and by two per
//! generation). A diagonal neighbour is only tried when neither of the two orthogonal cells next to it was claimed by the
//! same cell. When the wave pops the target cell, the route is read back from the target by always stepping to the
//! neighbour with the smallest stamp that is below the current one (and at least the base): stamps grow with the wave, so
//! that walks back to the start. The route is stored target first, start last.

pub const SIDE: i32 = 128;
pub const BLOCKED: u32 = 0xFFFFFD;
/// The exe resets the grid and the base once the base passes this (`0x458230`).
pub const BASE_LIMIT: u32 = 0x1fffff;
/// A wave gives up when the counter passes `base + 0x1f40` or a generation holds more than 8192 cells.
const MAX_SPAN: u32 = 0x1f40;
const MAX_GENERATION: usize = 0x2000;
/// A route longer than this is cut (the exe stores at most 1000 steps).
const MAX_STEPS: usize = 1000;

pub struct Wave {
    pub cells: Vec<u32>,
    /// `[0x49a364]`: the first value that counts as occupied.
    pub base: u32,
}

fn idx(x: i32, y: i32) -> Option<usize> {
    let i = x * SIDE + y;
    (0..SIDE * SIDE).contains(&i).then_some(i as usize)
}

impl Wave {
    /// A grid where `blocked(x, y)` marks the cells that can never be entered, base 1 (the state after `0x458230`).
    pub fn new(blocked: impl Fn(i32, i32) -> bool) -> Wave {
        let mut cells = vec![0; (SIDE * SIDE) as usize];
        for x in 0..SIDE {
            for y in 0..SIDE {
                if blocked(x, y) {
                    cells[(x * SIDE + y) as usize] = BLOCKED;
                }
            }
        }
        Wave { cells, base: 1 }
    }

    /// Searches a route from cell `start` to cell `target`; `drop` removes that many cells from the start end of the
    /// route (the units pass 0). Returns the route target first and start last, or `None` when there is none.
    /// The start and target cells are treated as free whatever they hold, and restored afterwards.
    pub fn find(&mut self, start: (i32, i32), target: (i32, i32), drop: usize) -> Option<Vec<(i32, i32)>> {
        let (Some(si), Some(ti)) = (idx(start.0, start.1), idx(target.0, target.1)) else { return None };
        let (saved_s, saved_t) = (self.cells[si], self.cells[ti]);
        let (found, edi) = self.search(start, target);
        let route = if found { self.trace(start, target, drop) } else { None };
        self.cells[si] = saved_s;
        self.cells[ti] = saved_t;
        self.base = edi + 3;
        route
    }

    /// The wave itself: stamps the grid from `start` until `target` is popped. Leaves the start and target cells stamped
    /// (the caller restores them) and returns whether the target was reached and the final counter.
    fn search(&mut self, start: (i32, i32), target: (i32, i32)) -> (bool, u32) {
        let base = self.base;
        let (si, ti) = (idx(start.0, start.1).unwrap(), idx(target.0, target.1).unwrap());
        self.cells[si] = base;
        let mut edi = base + 2;
        self.cells[ti] = 0;
        let limit = base + MAX_SPAN;
        let mut cur = vec![start];
        let mut found = false;
        'waves: loop {
            // the exe runs two copies of the generation loop (list B -> list A, then A -> B) with the same checks between
            for _ in 0..2 {
                let mut next = Vec::new();
                for &(a, b) in &cur {
                    if (a, b) == target {
                        found = true;
                        break 'waves;
                    }
                    self.expand(a, b, &mut edi, &mut next);
                }
                edi += 2;
                if next.is_empty() || next.len() > MAX_GENERATION || edi > limit {
                    break 'waves;
                }
                cur = next;
            }
        }
        (found, edi)
    }

    /// Claims the free neighbours of `(a, b)` (`0x475520..0x4756c8`).
    fn expand(&mut self, a: i32, b: i32, edi: &mut u32, out: &mut Vec<(i32, i32)>) {
        let base = self.base;
        let mut claim = |w: &mut Wave, x: i32, y: i32, stamp: u32| -> bool {
            match idx(x, y) {
                Some(i) if w.cells[i] < base => {
                    w.cells[i] = stamp;
                    out.push((x, y));
                    true
                }
                _ => false,
            }
        };
        let mut flags = 0u8;
        for (bit, (x, y)) in [(1, (a - 1, b)), (2, (a + 1, b)), (4, (a, b - 1)), (8, (a, b + 1))] {
            if claim(self, x, y, *edi) {
                flags |= bit;
            }
        }
        *edi += 1;
        // a diagonal is only tried when neither orthogonal cell beside it was claimed just now
        for (mask, (x, y)) in [(9, (a - 1, b + 1)), (0xa, (a + 1, b + 1)), (6, (a + 1, b - 1)), (5, (a - 1, b - 1))] {
            if flags & mask == 0 {
                claim(self, x, y, *edi);
            }
        }
    }

    /// The route back from the target (`0x47598a..0x475ba6`).
    fn trace(&self, start: (i32, i32), target: (i32, i32), drop: usize) -> Option<Vec<(i32, i32)>> {
        const AROUND: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];
        let mut route = Vec::new();
        let mut cur = target;
        let mut steps = 0;
        loop {
            route.push(cur);
            if cur == start {
                break;
            }
            let here = idx(cur.0, cur.1).map_or(0, |i| self.cells[i]);
            let (mut best, mut next) = (here, (0, 0));
            for (dx, dy) in AROUND {
                if let Some(i) = idx(cur.0, cur.1).and_then(|c| (c as i32 + dx * SIDE + dy).try_into().ok()).filter(|&i: &usize| i < self.cells.len()) {
                    let v = self.cells[i];
                    if v < best && v >= self.base {
                        best = v;
                        next = (cur.0 + dx, cur.1 + dy);
                    }
                }
            }
            if next == (0, 0) {
                return None;
            }
            cur = next;
            steps += 1;
            if steps >= MAX_STEPS {
                break;
            }
        }
        if steps == 0 {
            return None;
        }
        for _ in 0..drop {
            route.pop();
        }
        Some(route)
    }
}

/// What the unit-avoidance search returns (`0x476870`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalStep {
    /// The cell the unit steps to next (`[unit+0x6c], [unit+0x70]`); `None` when there is no step.
    pub next: Option<(i32, i32)>,
    /// The local route from the unit's cell to the look-ahead cell, excluding the unit's own cell (`[unit+0xac]`).
    pub path: Vec<(i32, i32)>,
    /// The exe resets the unit's goal to its own position (the look-ahead cell next to the unit is occupied).
    pub stop: bool,
}

/// How far ahead on the global route the local search aims (`0x476909`: at most the 7th cell).
const LOOK_AHEAD: usize = 6;
const OBSTACLE_THRESHOLD: u32 = 0x1fffff;
const CORNER: u32 = 0xFFFFFD;

impl Wave {
    /// The local search of `0x476870` (mode 0). `self` is the unit grid (`0x61ac38`, base `[0x49a36c]`: terrain, buildings
    /// and other units, each stamped with a handle), `stat` the static grid (`0x5eac38`, base `stat_base` = `[0x49a360]`).
    /// `route` is the global route from the wave, the unit's own cell first; `at` the unit's cell, `own` its handle.
    ///
    /// A wave runs from the cell up to 6 steps ahead on the route back to the unit; the unit steps to the neighbour with the
    /// smallest stamp. Diagonal corners of the unit's cell are closed where a single obstacle fills both orthogonal cells.
    pub fn local_step(&mut self, stat: &[u32], stat_base: u32, own: u32, route: &[(i32, i32)], at: (i32, i32)) -> LocalStep {
        self.local_core(stat, stat_base, own, route, at, None)
    }

    /// The second-step search of `0x475c30` (ground) / `0x4732f0` (Advancer): the same code as [`Wave::local_step`], run over
    /// the **second step cell grid** (`self` = `0x63ac38`, base `[0x49a370]`; air `0x64ac38`, `[0x49a378]`) to plan the cell
    /// *after* the step cell. `at` is the centre cell of the first step cell, `first` / `first_base` the first step grid
    /// (`0x61ac38`, `[0x49a36c]`; air `0x62ac38`, `[0x49a374]`). Differences from `local_step`: when the look-ahead cell is next
    /// to `at` it is taken if it is free in the second grid **and** the first step grid (the static grid is not asked) and
    /// otherwise there is no step: the ground routine (`air == false`) just fails, the Advancer's `0x4732f0` (`air == true`)
    /// also sets `stop` like `0x476870` / `0x473f10` do. Only the networked movement `0x46fa40` -> `0x472f50` calls it
    /// (docs/pathfinding.md).
    #[allow(clippy::too_many_arguments)]
    pub fn local_step_second(&mut self, stat: &[u32], stat_base: u32, own: u32, route: &[(i32, i32)], at: (i32, i32), first: &[u32], first_base: u32, air: bool) -> LocalStep {
        self.local_core(stat, stat_base, own, route, at, Some((first, first_base, air)))
    }

    fn local_core(&mut self, stat: &[u32], stat_base: u32, own: u32, route: &[(i32, i32)], at: (i32, i32), second: Option<(&[u32], u32, bool)>) -> LocalStep {
        let none = LocalStep::default();
        if route.len() <= 1 {
            return none;
        }
        let k = (route.len() - 1).min(LOOK_AHEAD);
        let l = route[k];
        let (Some(li), Some(ti)) = (idx(l.0, l.1), idx(at.0, at.1)) else { return none };
        if l == at {
            return none;
        }
        let s_at_l = stat[li];
        if s_at_l >= OBSTACLE_THRESHOLD && s_at_l != own {
            return none;
        }
        if (l.0 - at.0).abs() <= 1 && (l.1 - at.1).abs() <= 1 {
            // next to the look-ahead cell: step there when it is free
            if let Some((first, first_base, air)) = second {
                if self.cells[li] < self.base && first[li] < first_base {
                    return LocalStep { next: Some(l), path: Vec::new(), stop: false };
                }
                return LocalStep { stop: air, ..none };
            }
            if self.cells[li] < self.base && s_at_l < stat_base {
                return LocalStep { next: Some(l), path: Vec::new(), stop: false };
            }
            return LocalStep { stop: true, ..none };
        }
        let base = self.base;
        let around = |c: usize| -> Option<[usize; 9]> {
            let c = c as i32;
            let o = [-129, -1, 127, -128, 0, 128, -127, 1, 129];
            let mut out = [0usize; 9];
            for (i, d) in o.iter().enumerate() {
                let j = c + d;
                if !(0..SIDE * SIDE).contains(&j) {
                    return None;
                }
                out[i] = j as usize;
            }
            Some(out)
        };
        let (Some(l_ring), Some(t_ring)) = (around(li), around(ti)) else { return none };
        let saved: Vec<u32> = l_ring.iter().chain(t_ring.iter()).map(|&i| self.cells[i]).collect();
        let (u, t) = (|w: &Wave, d: i32| w.cells[(ti as i32 + d) as usize], ti as i32);
        let s = |d: i32| stat[(t + d) as usize];
        let (u_w, u_e, u_s, u_n) = (u(self, -128), u(self, 128), u(self, -1), u(self, 1)); // x-1, x+1, y-1, y+1
        let blocked = |a: u32, sb: u32| a == sb && a >= base;
        if blocked(u_s, s(-128)) || blocked(u_w, s(-1)) {
            self.cells[(t - 129) as usize] = CORNER;
        }
        if blocked(u_n, s(-128)) || blocked(u_w, s(1)) {
            self.cells[(t - 127) as usize] = CORNER;
        }
        if blocked(u_s, s(128)) || blocked(u_e, s(-1)) {
            self.cells[(t + 127) as usize] = CORNER;
        }
        if blocked(u_n, s(128)) || blocked(u_e, s(1)) {
            self.cells[(t + 129) as usize] = CORNER;
        }
        let (found, edi) = self.search(l, at);
        let mut result = none;
        if found {
            result = self.local_trace(l, at);
        }
        for (&i, &v) in l_ring.iter().chain(t_ring.iter()).zip(&saved) {
            self.cells[i] = v;
        }
        self.base = edi + 3;
        result
    }

    /// The route back from the unit's cell to `l` (`0x477097..0x477296`): the same smallest-stamp walk as [`Wave::trace`], 1000
    /// iterations at most; a dead end moves to cell `(0, 0)` and goes on from there; failing only when the very first step
    /// has no cell.
    fn local_trace(&self, l: (i32, i32), at: (i32, i32)) -> LocalStep {
        const AROUND: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];
        let mut cur = at;
        let mut path = Vec::new();
        let mut first = None;
        for step in 0..MAX_STEPS {
            if cur == l {
                break;
            }
            let here = idx(cur.0, cur.1).map_or(0, |i| self.cells[i]);
            let (mut best, mut next) = (here, (0, 0));
            for (dx, dy) in AROUND {
                if let Some(i) = idx(cur.0, cur.1).and_then(|c| usize::try_from(c as i32 + dx * SIDE + dy).ok()).filter(|&i| i < self.cells.len()) {
                    let v = self.cells[i];
                    if v < best && v >= self.base {
                        best = v;
                        next = (cur.0 + dx, cur.1 + dy);
                    }
                }
            }
            cur = next;
            if step == 0 {
                first = Some(cur);
                if cur == (0, 0) {
                    return LocalStep::default();
                }
            }
            path.push(cur);
        }
        LocalStep { next: first, path, stop: false }
    }
}

/// What the target scan `0x472a90` makes of an occupied cell value (decided by the handle bits, `0x43aa80..0x43aab0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// Not a hostile object (an own object, or neither unit nor building).
    Ignore,
    Unit,
    /// A building whose type (`0x43aaa0`) is 3 or 6: the turret class.
    Turret,
    Building,
}

/// The nearest hostile objects found by [`find_targets`]: the first handle of each class the wave met (0 = none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Targets {
    pub unit: u32,
    pub turret: u32,
    pub building: u32,
}

/// The exe's reachability scan (`0x472a90`, docs/ai.md): a wave from cell `start` over the static grid `cells`
/// (`x * 128 + y`, terrain `0xFFFFFD`, objects are handles `>= 0x400000`). Free cells (below `*base`) are stamped and
/// expanded in the order x-1, x+1, y-1, y+1; an object cell is *not* expanded: `classify` says what it is, and the first
/// unit, turret and other building met are returned. The wave stops after a generation of more than 8000 cells or when
/// the stamp passes `*base + 8000`; the start cell is restored and `*base` moves past the stamps (stamp + 2).
/// Cells outside the grid count as terrain (the exe reads the border cells around the map, which are terrain).
pub fn find_targets(cells: &mut [u32], base: &mut u32, dims: (i32, i32), start: (i32, i32), classify: impl Fn(u32) -> Seen) -> Targets {
    const OBJECT: u32 = 0x400000;
    let b = *base;
    let (w, h) = dims;
    let at = |x: i32, y: i32| -> Option<usize> { ((0..SIDE).contains(&x) && (0..SIDE).contains(&y)).then(|| (x * SIDE + y) as usize) };
    let mut found = Targets::default();
    let Some(si) = at(start.0, start.1) else { return found };
    let saved = cells[si];
    cells[si] = b;
    let mut stamp = b + 1;
    let (mut cur, mut next): (Vec<(i32, i32)>, Vec<(i32, i32)>) = (vec![start], Vec::new());
    loop {
        for &(x, y) in &cur {
            let Some(i) = at(x, y) else { continue };
            let v = cells[i];
            if v >= OBJECT {
                match classify(v) {
                    Seen::Unit if found.unit == 0 => found.unit = v,
                    Seen::Turret if found.turret == 0 => found.turret = v,
                    Seen::Building if found.building == 0 => found.building = v,
                    _ => {}
                }
                continue;
            }
            // x-1, x+1, y-1, y+1 with the exe's (weak) bounds tests
            for (dx, dy, inside) in [(-1, 0, x > -1), (1, 0, x < w), (0, -1, y > -1), (0, 1, y < h)] {
                let Some(j) = at(x + dx, y + dy) else { continue };
                let n = cells[j];
                if n == BLOCKED || !inside {
                    continue;
                }
                if n < b || n >= OBJECT {
                    next.push((x + dx, y + dy));
                }
                if n < b {
                    cells[j] = stamp;
                }
            }
        }
        stamp += 1;
        if next.len() > 0x1f40 || stamp > b + MAX_SPAN || next.is_empty() {
            break;
        }
        cur = std::mem::take(&mut next);
    }
    cells[si] = saved;
    *base = stamp + 2;
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(w: i32, h: i32) -> Wave {
        Wave::new(|x, y| x < 1 || y < 1 || x >= w - 1 || y >= h - 1)
    }

    #[test]
    fn air_chase_frees_the_strip_three_cells_before_the_block() {
        // target (20, 20), range 2; handle cells at (20, 15) (y - 5: outside the block, inside the air strip) and (22, 22)
        let handle = 0x3000009;
        let fresh = || {
            let mut cells = vec![0u32; 128 * 128];
            for x in 0..40usize {
                for y in 0..40usize {
                    if x < 1 || y < 1 || x >= 39 || y >= 39 {
                        cells[x * 128 + y] = BLOCKED;
                    }
                }
            }
            cells[20 * 128 + 15] = handle;
            cells[22 * 128 + 22] = handle;
            cells
        };
        let (mut ground, mut air) = (fresh(), fresh());
        let (mut b0, mut b1) = (1u32, 1u32);
        let g = approach(&mut ground, &mut b0, (40, 40), (5, 20), (20, 20), handle, 2).unwrap();
        let a = approach_air(&mut air, &mut b1, (40, 40), (5, 20), (20, 20), handle, 2).unwrap();
        // the ground chase restores everything; the air chase leaves (20, 15) cleared and (22, 22) in place
        assert_eq!((ground[20 * 128 + 15], ground[22 * 128 + 22]), (handle, handle));
        assert_eq!((air[20 * 128 + 15], air[22 * 128 + 22]), (0, handle));
        assert_eq!(g, a, "the destination is the same on open ground");
        // a ring of the target's own cells: both chases free what they must to arrive, and restore the block afterwards
        let mut walled = fresh();
        for (dx, dy) in [(-1i32, -1i32), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            walled[((20 + dx) * 128 + 20 + dy) as usize] = handle;
        }
        let mut walled_air = walled.clone();
        let (mut b2, mut b3) = (1u32, 1u32);
        assert!(approach(&mut walled, &mut b2, (40, 40), (5, 20), (20, 20), handle, 2).is_some());
        assert!(approach_air(&mut walled_air, &mut b3, (40, 40), (5, 20), (20, 20), handle, 2).is_some());
        assert_eq!(walled_air[21 * 128 + 21], handle, "the block is restored, only the strip stays cleared");
    }

    /// Open 40 x 40 grids with a blocked border for the second-step search tests.
    fn open_grid() -> Vec<u32> {
        let mut cells = vec![0u32; 128 * 128];
        for x in 0..40usize {
            for y in 0..40usize {
                if x < 1 || y < 1 || x >= 39 || y >= 39 {
                    cells[x * 128 + y] = BLOCKED;
                }
            }
        }
        cells
    }

    #[test]
    fn second_step_search_plans_around_second_grid_stamps() {
        // unit at (10, 10) heading east along y = 10 to (16, 10); the route lists the unit's cell first
        let route: Vec<(i32, i32)> = (10..=16).map(|x| (x, 10)).collect();
        let (stat, first) = (open_grid(), open_grid());
        let mut free = Wave { cells: open_grid(), base: 1 };
        let r = free.local_step_second(&stat, 1, 0x3100001, &route, (10, 10), &first, 1, false);
        assert!(r.next.is_some() && !r.stop);
        assert_eq!(r.path.last(), Some(&(16, 10)), "the local route runs from the step cell to the look-ahead cell");
        // a reservation in the second grid on the straight way: the route bends around it
        let mut cells = open_grid();
        cells[12 * 128 + 10] = 0x3200001;
        let mut w = Wave { cells, base: 1 };
        let r = w.local_step_second(&stat, 1, 0x3100001, &route, (10, 10), &first, 1, false);
        assert!(r.next.is_some());
        assert!(!r.path.contains(&(12, 10)), "{:?}", r.path);
        // the first step grid is not consulted by the wave itself: a stamp only there does not bend the route
        let mut first2 = open_grid();
        first2[12 * 128 + 10] = 0x3200001;
        let mut w = Wave { cells: open_grid(), base: 1 };
        assert!(w.local_step_second(&stat, 1, 0x3100001, &route, (10, 10), &first2, 1, false).path.contains(&(12, 10)));
    }

    #[test]
    fn second_step_next_to_the_unit_asks_both_step_grids() {
        // the look-ahead cell (11, 10) is next to the unit's cell (10, 10)
        let route = [(10, 10), (11, 10)];
        let stat = open_grid();
        let mut w = Wave { cells: open_grid(), base: 1 };
        let ok = w.local_step_second(&stat, 1, 7, &route, (10, 10), &open_grid(), 1, false);
        assert_eq!((ok.next, ok.stop, ok.path.is_empty()), (Some((11, 10)), false, true));
        // taken in the first step grid: no step, and (unlike 0x476870) no stop request
        let mut taken = open_grid();
        taken[11 * 128 + 10] = 0x3200001;
        let mut w = Wave { cells: open_grid(), base: 1 };
        assert_eq!(w.local_step_second(&stat, 1, 7, &route, (10, 10), &taken, 1, false), LocalStep::default());
        // the Advancer's twin 0x4732f0 asks for the stop as well
        let mut w = Wave { cells: open_grid(), base: 1 };
        assert!(w.local_step_second(&stat, 1, 7, &route, (10, 10), &taken, 1, true).stop);
        // taken in the second grid
        let mut cells = open_grid();
        cells[11 * 128 + 10] = 0x3200001;
        let mut w = Wave { cells, base: 1 };
        assert_eq!(w.local_step_second(&stat, 1, 7, &route, (10, 10), &open_grid(), 1, false), LocalStep::default());
        // a stamp of the current search generation in the *static* grid only (value >= its base, < 0x1fffff) is not asked
        // here, while 0x476870 would stop
        let mut stat2 = open_grid();
        stat2[11 * 128 + 10] = 5;
        let mut w = Wave { cells: open_grid(), base: 1 };
        assert_eq!(w.local_step_second(&stat2, 1, 7, &route, (10, 10), &open_grid(), 1, false).next, Some((11, 10)));
        let mut w = Wave { cells: open_grid(), base: 1 };
        assert!(w.local_step(&stat2, 1, 7, &route, (10, 10)).stop);
    }

    #[test]
    fn open_ground_routes_are_chebyshev_short() {
        // the wave gives up after about 8000 processed cells, so these distances are the ones a 64 x 64 mission map has
        let mut w = Wave::new(|x, y| x < 1 || y < 1 || x >= 63 || y >= 63);
        for (a, b) in [((5i32, 5i32), (55i32, 40i32)), ((58, 10), (6, 50)), ((30, 5), (31, 60)), ((50, 50), (10, 10))] {
            let r = w.find(a, b, 0).unwrap();
            let want = (a.0 - b.0).abs().max((a.1 - b.1).abs()) as usize + 1;
            assert!(r.len() <= want + 4, "{a:?} -> {b:?}: {} cells for a distance of {}", r.len(), want - 1);
        }
    }

    fn walled(w: i32, h: i32) -> Vec<u32> {
        let mut c = vec![0u32; 16384];
        for x in 0..SIDE {
            for y in 0..SIDE {
                if x < 1 || y < 1 || x >= w - 1 || y >= h - 1 {
                    c[(x * SIDE + y) as usize] = BLOCKED;
                }
            }
        }
        c
    }

    #[test]
    fn a_chasing_unit_stops_range_cells_before_the_target() {
        let mut cells = walled(40, 40);
        let mut base = 1;
        let d = approach(&mut cells, &mut base, (40, 40), (5, 20), (30, 20), 0, 5).unwrap();
        assert_eq!(d.1, 20);
        assert!((d.0 - 30).abs() <= 6 && (d.0 - 30).abs() >= 4, "{d:?}");
        assert!(base > 20, "the base moved past the stamps");
        // range 1: next to the target
        let d = approach(&mut cells, &mut base, (40, 40), (5, 20), (30, 20), 0, 1).unwrap();
        assert!((d.0 - 30).abs() <= 1 && (d.1 - 20).abs() <= 1 && d != (30, 20), "{d:?}");
    }

    #[test]
    fn a_walled_in_target_cannot_be_approached_and_the_grid_is_restored() {
        let mut cells = walled(40, 40);
        for x in 25..=35 {
            for y in 15..=25 {
                if (x - 30i32).abs().max((y - 20i32).abs()) == 5 {
                    cells[(x * SIDE + y) as usize] = BLOCKED;
                }
            }
        }
        let before = cells.clone();
        let mut base = 1;
        assert_eq!(approach(&mut cells, &mut base, (40, 40), (5, 20), (30, 20), 0, 3), None);
        // only stamps below the new base differ
        assert!(cells.iter().zip(&before).all(|(a, b)| a == b || *a < base));
        assert_eq!(cells[(5 * SIDE + 20) as usize], 0);
    }

    #[test]
    fn straight_corridor() {
        let mut w = open(20, 20);
        let r = w.find((2, 5), (8, 5), 0).unwrap();
        assert_eq!(r.first(), Some(&(8, 5)));
        assert_eq!(r.last(), Some(&(2, 5)));
        assert_eq!(r.len(), 7);
        assert!(r.windows(2).all(|p| (p[0].0 - p[1].0).abs() <= 1 && (p[0].1 - p[1].1).abs() <= 1));
    }

    #[test]
    fn diagonal_route_is_diagonal() {
        let mut w = open(20, 20);
        let r = w.find((2, 2), (9, 9), 0).unwrap();
        assert_eq!(r.len(), 8);
    }

    #[test]
    fn walls_are_walked_around() {
        let mut w = Wave::new(|x, y| x < 1 || y < 1 || x >= 19 || y >= 19 || (x == 10 && y < 15));
        let r = w.find((3, 3), (17, 3), 0).unwrap();
        assert!(r.iter().all(|&(x, y)| !(x == 10 && y < 15)));
        assert!(r.iter().any(|&(_, y)| y >= 15));
    }

    #[test]
    fn closed_off_targets_fail_and_same_cell_fails() {
        let mut w = Wave::new(|x, y| x < 1 || y < 1 || x >= 19 || y >= 19 || x == 10);
        assert!(w.find((3, 3), (17, 3), 0).is_none());
        assert!(w.find((3, 3), (3, 3), 0).is_none());
    }

    #[test]
    fn blocked_target_is_reached_and_restored() {
        let mut w = Wave::new(|x, y| x < 1 || y < 1 || x >= 19 || y >= 19 || (x, y) == (10, 10));
        let r = w.find((3, 3), (10, 10), 0).unwrap();
        assert_eq!(r[0], (10, 10));
        assert_eq!(w.cells[(10 * SIDE + 10) as usize], BLOCKED);
    }

    #[test]
    fn the_base_moves_past_the_stamps_so_searches_can_repeat() {
        let mut w = open(20, 20);
        let a = w.find((2, 2), (15, 12), 0);
        let b = w.find((2, 2), (15, 12), 0);
        assert_eq!(a, b);
        assert!(w.base > 3);
    }

    #[test]
    fn drop_removes_cells_from_the_start_end() {
        let mut w = open(20, 20);
        let r = w.find((2, 5), (8, 5), 2).unwrap();
        assert_eq!(r.last(), Some(&(4, 5)));
    }
}

/// The chase search of `0x477630` (docs/walking.md): where a unit that wants to shoot at a target walks to.
///
/// A bucket wave runs over the static grid (`0x5eac38`, `base` = `[0x49a360]`) from the unit's cell until it reaches a cell
/// within one cell (Chebyshev) of the target. Orthogonal neighbours are stamped with the generation counter and join the
/// next list, diagonal ones get counter + 1 and join the list after that (so a diagonal costs two generations); nothing
/// stops a wave except the grid edge, so cells beside the grid edge read out of bounds. The destination is then found by
/// walking back from the *target* cell `range` times, each time to the neighbour with the smallest stamp that is below
/// the current cell's and not below the base: the unit stops about `range` cells before the target along its way.
///
/// The target cell and its 7 x 7 surroundings are made passable where they hold `handle` (the target building's own
/// cells) and restored afterwards, like the unit's cell. Returns `None` when the target cannot be reached; `base` moves
/// past the stamps.
pub fn approach(cells: &mut [u32], base: &mut u32, dims: (i32, i32), unit: (i32, i32), target: (i32, i32), handle: u32, range: i32) -> Option<(i32, i32)> {
    approach_shifted(cells, base, dims, unit, target, handle, range, 0)
}

/// The chase search for a target of handle type 4 (an Advancer, `0x478200`): the same search as [`approach`] over the air
/// grid (`0x60ac38`, base `[0x49a368]`), with one quirk of the exe's unrolled code: for every cell of the 7 x 7 block that
/// is saved and restored, the cell *three places earlier in the linear index* (`x * 128 + y - 3`) is the one that is freed
/// when it holds the target's handle. So the strip y-6..y beside the block is freed (and never restored: a handle there
/// stays cleared) while handle cells at y+1..y+3 are not freed at all (docs/pathfinding.md). Only walkers' handles would
/// show it and the air grid holds none besides other Advancers, so real games rarely see the quirk.
pub fn approach_air(cells: &mut [u32], base: &mut u32, dims: (i32, i32), unit: (i32, i32), target: (i32, i32), handle: u32, range: i32) -> Option<(i32, i32)> {
    approach_shifted(cells, base, dims, unit, target, handle, range, 3)
}

#[allow(clippy::too_many_arguments)]
fn approach_shifted(cells: &mut [u32], base: &mut u32, dims: (i32, i32), unit: (i32, i32), target: (i32, i32), handle: u32, range: i32, free_shift: i32) -> Option<(i32, i32)> {
    let b = *base;
    let (w, h) = dims;
    let at = |x: i32, y: i32| (x * SIDE + y) as usize;
    let (ui, ti) = (at(unit.0, unit.1), at(target.0, target.1));
    let saved_unit = cells[ui];
    // the 7 x 7 block around the target: remember it, free the target's own cells
    let mut saved = Vec::new();
    for x in target.0 - 3..=target.0 + 3 {
        for y in target.1 - 3..=target.1 + 3 {
            if x > -1 && x < w && y > -1 && y < h {
                let i = at(x, y);
                saved.push((i, cells[i]));
                // the cell that is freed: the saved one (ground chase) or three cells before it (air chase)
                let j = i as i32 - free_shift;
                if j >= 0 && (j as usize) < cells.len() && cells[j as usize] == handle {
                    cells[j as usize] = 0;
                }
            }
        }
    }
    cells[ui] = b;
    let mut stamp = b + 1;
    cells[ti] = 0xfffffe;
    let mut lists: [Vec<(i32, i32)>; 3] = [vec![unit], vec![], vec![]];
    let (mut cur, mut next, mut third) = (0usize, 1usize, 2usize);
    let mut found = false;
    let mut reached_edge_limit = false;
    loop {
        let mut gen = std::mem::take(&mut lists[cur]);
        for &(x, y) in &gen {
            if (x - target.0).abs() < 2 && (y - target.1).abs() < 2 {
                found = true;
                break;
            }
            let i = x * SIDE + y;
            let free = |c: &[u32], d: i32| {
                let j = i + d;
                (0..SIDE * SIDE).contains(&j) && c[j as usize] < b
            };
            // orthogonal neighbours: stamp `stamp`, next list
            if free(cells, -128) && x > -1 {
                cells[(i - 128) as usize] = stamp;
                lists[next].push((x - 1, y));
            }
            if free(cells, 128) && x < w {
                cells[(i + 128) as usize] = stamp;
                lists[next].push((x + 1, y));
            }
            if free(cells, -1) && y > -1 {
                cells[(i - 1) as usize] = stamp;
                lists[next].push((x, y - 1));
            }
            if free(cells, 1) && y < h {
                cells[(i + 1) as usize] = stamp;
                lists[next].push((x, y + 1));
            }
            // diagonals: stamp + 1, the list after next
            if free(cells, -129) && x > -1 && y > -1 {
                cells[(i - 129) as usize] = stamp + 1;
                lists[third].push((x - 1, y - 1));
            }
            if free(cells, 127) && x < w && y > -1 {
                cells[(i + 127) as usize] = stamp + 1;
                lists[third].push((x + 1, y - 1));
            }
            if free(cells, -127) && x > -1 && y < h {
                cells[(i - 127) as usize] = stamp + 1;
                lists[third].push((x - 1, y + 1));
            }
            if free(cells, 129) && x < w && y < h {
                cells[(i + 129) as usize] = stamp + 1;
                lists[third].push((x + 1, y + 1));
            }
        }
        gen.clear();
        if found {
            break;
        }
        stamp += 1;
        if lists[next].len() > 0x1f40 || stamp > b + MAX_SPAN || (lists[next].is_empty() && lists[third].is_empty()) {
            reached_edge_limit = true;
            break;
        }
        (cur, next, third) = match cur {
            0 => (1, 2, 0),
            1 => (2, 0, 1),
            _ => (0, 1, 2),
        };
    }
    let _ = reached_edge_limit;
    let mut result = None;
    if found {
        result = Some((0, 0));
        if range > 0 {
            const AROUND: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];
            let mut c = target;
            let mut left = range;
            while left > 0 {
                left -= 1;
                if c == unit {
                    result = Some(unit);
                    break;
                }
                let here = cells[at(c.0, c.1)];
                let (mut best, mut pick) = (here, (0, 0));
                for (dx, dy) in AROUND {
                    let j = (c.0 * SIDE + c.1) + dx * SIDE + dy;
                    if (0..SIDE * SIDE).contains(&j) {
                        let v = cells[j as usize];
                        if v < best && v >= b {
                            best = v;
                            pick = (c.0 + dx, c.1 + dy);
                        }
                    }
                }
                if pick == (0, 0) {
                    result = None;
                    break;
                }
                c = pick;
                result = Some(c);
            }
        }
    }
    cells[ui] = saved_unit;
    for (i, v) in saved {
        cells[i] = v;
    }
    *base = stamp + 2;
    result
}
