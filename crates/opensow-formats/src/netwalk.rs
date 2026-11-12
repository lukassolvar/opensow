//! The networked chassis step `0x46fa40` (docs/pathfinding.md, "The networked step"). The dispatcher `0x46f140` calls it
//! instead of `0x46f550` when `[0x575720] >= 1` (a multiplayer session) and the unit is not on its destination.
//!
//! Unlike the single-player step, which takes `steps_at(speed, tick)` pixels of the pixel line per tick, the networked step
//! computes the unit's position **from the game clock**: `(a, b)` = (seconds, tick in the second), the same clock for every
//! machine. The chassis remembers the clock of the start of the current line (`[+0x168]`, `[+0x16c]`); the pixel steps owed
//! since then are the sum of the speed row over the ticks in between, and the unit stands on `line[steps]` (the line keeps
//! all its points; it is not consumed). When the steps reach the end of the line the unit snaps to the last point, the line is
//! freed, the clock restarts at `(a, b)` and the next cell is planned: by the planner `0x472f50` (the player that is not
//! player 0 does not plan: `0x41f020(local player)` is `[0x575720] > 0 && player != 0`; it takes over the step cell that the
//! other machine's planner reserved, `step1 = step2`, and draws the line to it). The facing has no gradual turn: it is set
//! to the heading from the line's first cell to the step cell at once (no `0x46a190`). Ported with the planner calls left to a
//! [`NetHost`]; emulation-verified (`scripts/verify_against_exe.py` section 12, `opensow netstep`).

use crate::facing::facing;

/// Ticks per speed-table row (the exe's `0x5524a0` rows are 60 bytes).
pub const ROW: usize = 60;

/// The fields of the chassis (`this`) the step reads and writes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetChassis {
    /// The unit's pixel position (`[unit+0x14/0x18]`, passed by pointer).
    pub pos: (i32, i32),
    /// `[+0x5c/0x60]`.
    pub dest: (i32, i32),
    /// `[+0x168]`, `[+0x16c]`: the clock (seconds, tick) when the current line started.
    pub since: (i32, i32),
    /// The speed row of `[+0x18]` (steps per tick of the second, `0x5524a0 + row * 60`).
    pub row: Vec<u8>,
    /// The pixel line `[+0x9c/0xa0]`, start first.
    pub line: Vec<(i32, i32)>,
    /// `[+0x14]`: the unit's handle (stamped into the static grid).
    pub handle: u32,
    /// `[unit+0x4b]`: the Advancer (air grid) flag.
    pub air: bool,
    /// The unit is kind 40: its sprite frame is not set.
    pub kind40: bool,
    /// `[+0x64/0x68]`: the cell the line started at (heading start).
    pub prev: (i32, i32),
    /// `[+0x6c/0x70]` and `[+0x74/0x78]`: the first and second step cell.
    pub step1: (i32, i32),
    /// `[+0x74/0x78]`: the second step cell.
    pub step2: (i32, i32),
    /// `[+0x30]`, `[+0x34]`: current and wanted facing (both set at once here).
    pub facing: i32,
    /// `[+0x34]`: the wanted facing.
    pub want: i32,
    /// `[+8]`: facing steps (32, 16, 8).
    pub turn_steps: i32,
    /// `[+0x80]`: route re-search cooldown.
    pub cooldown: i32,
    /// Animation: `[+0x4c]` counter, `[+0x50]` reload, `[+0x44]` phase, `[+0x48]` phase count, `[+0x40]` frame.
    pub anim_count: i32,
    /// `[+0x50]`: the value the animation counter restarts from.
    pub anim_reload: i32,
    /// `[+0x44]`: the animation phase.
    pub phase: i32,
    /// `[+0x48]`: the number of animation phases.
    pub phases: i32,
    /// `[+0x40]`: the sprite frame.
    pub frame: i32,
    /// `[+0x8c]`: length of the global route (`[+0x88]`), cleared when the unit has arrived.
    pub route_len: i32,
}

/// What the step asks the rest of the game to do (the exe's calls, in order).
pub trait NetHost {
    /// `0x41f020([0x575730])`: networked game and the local player is not player 0.
    fn is_guest(&self) -> bool;
    /// `0x46eb40` (target check) with `[+0x168]`, `[+0x16c]` as its two arguments.
    fn chase(&mut self, c: &mut NetChassis);
    /// `0x4753a0` (global route to the unit's position); false clears the orders.
    fn route(&mut self, c: &mut NetChassis) -> bool;
    /// `0x472f50(x, y)`: plans the two step cells and the line (docs/pathfinding.md).
    fn plan(&mut self, c: &mut NetChassis);
    /// `0x46ee80`: builds the pixel line between two pixel points into `c.line`.
    fn build_line(&mut self, c: &mut NetChassis, from: (i32, i32), to: (i32, i32));
    /// `0x470810(0, 1)`: removes the head order (the arrival).
    fn pop_order(&mut self);
    /// `0x470480`: starts the next order.
    fn start_order(&mut self);
    /// Frees the whole order list.
    fn clear_orders(&mut self);
}

fn centre(c: (i32, i32)) -> (i32, i32) {
    (c.0 * 32 + 16, c.1 * 32 + 16)
}

fn cell_index(p: (i32, i32)) -> Option<usize> {
    let (x, y) = ((p.0 as u32 >> 5) as i32, (p.1 as u32 >> 5) as i32);
    ((0..128).contains(&x) && (0..128).contains(&y)).then(|| (x * 128 + y) as usize)
}

/// The pixel steps owed between the clocks `from` and `to` (the loop at `0x46fa90`): the sum of the row over the ticks, a
/// clock being compared as `seconds * 64 + tick`.
pub fn steps_owed(row: &[u8], from: (i32, i32), to: (i32, i32)) -> i32 {
    let (mut sec, mut tick) = from;
    let target = to.0 * 64 + to.1;
    let mut steps = 0;
    while sec * 64 + tick < target {
        steps += row.get(tick as usize).copied().unwrap_or(0) as i32;
        tick += 1;
        if tick >= ROW as i32 {
            sec += tick / 60;
            tick %= 60;
        }
    }
    steps
}

impl NetChassis {
    /// `0x46fa40(this, &x, &y, a, b)`. `stat` is the static grid of the unit's layer (`0x5eac38`, air `0x60ac38`): the cell
    /// the unit leaves is cleared and the one it enters stamped with its handle while it follows the line.
    pub fn step(&mut self, now: (i32, i32), stat: &mut [u32], host: &mut impl NetHost) {
        let steps = steps_owed(&self.row, self.since, now);
        let n = self.line.len() as i32;
        if steps < n - 1 {
            if let Some(i) = cell_index(self.pos) {
                stat[i] = 0;
            }
            self.pos = self.line[steps as usize];
            if let Some(i) = cell_index(self.pos) {
                stat[i] = self.handle;
            }
        } else {
            if let Some(&last) = self.line.last() {
                self.pos = last;
                self.line.clear();
            }
            self.since = now;
            if host.is_guest() {
                self.prev = self.step1;
                self.step1 = self.step2;
                if self.prev != self.step1 {
                    let (from, to) = (centre(self.prev), centre(self.step1));
                    host.build_line(self, from, to);
                }
            } else {
                if self.pos == self.dest {
                    let cell = ((self.dest.0 as u32 >> 5) as i32, (self.dest.1 as u32 >> 5) as i32);
                    self.step1 = cell;
                    self.step2 = cell;
                    self.route_len = 0;
                    host.pop_order();
                    host.start_order();
                    return;
                }
                self.cooldown -= 1;
                if self.cooldown < 0 {
                    self.cooldown = 6;
                    host.chase(self);
                    if !host.route(self) {
                        host.clear_orders();
                        host.start_order();
                        return;
                    }
                }
                host.plan(self);
            }
        }
        // the heading: from the cell the line started in to the step cell, no gradual turn
        self.prev = match self.line.first() {
            Some(&(x, y)) => (x / 32, y / 32),
            None => ((self.pos.0 as u32 >> 5) as i32, (self.pos.1 as u32 >> 5) as i32),
        };
        if self.prev != self.step1 {
            let (a, b) = (centre(self.prev), centre(self.step1));
            self.facing = facing(a.0, a.1, b.0, b.1, self.turn_steps as u32) as i32;
            self.want = self.facing;
        }
        self.anim_count -= 1;
        if self.anim_count < 0 {
            self.anim_count = self.anim_reload;
            self.phase += 1;
            if self.phase >= self.phases {
                self.phase = 0;
            }
        }
        if !self.kind40 {
            self.frame = self.phase * self.turn_steps + self.facing;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Log {
        calls: Vec<String>,
        guest: bool,
        route_ok: bool,
    }

    impl NetHost for Log {
        fn is_guest(&self) -> bool {
            self.guest
        }
        fn chase(&mut self, c: &mut NetChassis) {
            self.calls.push(format!("chase {} {}", c.since.0, c.since.1));
        }
        fn route(&mut self, _: &mut NetChassis) -> bool {
            self.calls.push("route".into());
            self.route_ok
        }
        fn plan(&mut self, c: &mut NetChassis) {
            self.calls.push("plan".into());
            c.step1 = (11, 10);
            c.line = vec![(336, 336), (336, 352), (336, 368)];
        }
        fn build_line(&mut self, c: &mut NetChassis, from: (i32, i32), to: (i32, i32)) {
            self.calls.push(format!("line {from:?} {to:?}"));
            c.line = vec![from, to];
        }
        fn pop_order(&mut self) {
            self.calls.push("pop".into());
        }
        fn start_order(&mut self) {
            self.calls.push("start".into());
        }
        fn clear_orders(&mut self) {
            self.calls.push("clear".into());
        }
    }

    fn chassis() -> NetChassis {
        NetChassis {
            pos: (336, 336),
            dest: (3 * 32 + 16, 3 * 32 + 16),
            row: vec![1; ROW],
            handle: 9,
            turn_steps: 32,
            phases: 4,
            anim_reload: 2,
            ..Default::default()
        }
    }

    #[test]
    fn steps_follow_the_clock_and_wrap_the_second() {
        let row: Vec<u8> = (0..60).map(|i| (i % 3) as u8).collect();
        assert_eq!(steps_owed(&row, (5, 0), (5, 0)), 0);
        assert_eq!(steps_owed(&row, (5, 0), (5, 4)), (1 + 2));
        // across the end of a second: ticks 58, 59 of second 5, tick 0 of second 6 (the clock is seconds * 64 + tick)
        assert_eq!(steps_owed(&row, (5, 58), (6, 1)), row[58] as i32 + row[59] as i32 + row[0] as i32);
        assert_eq!(steps_owed(&row, (6, 1), (5, 58)), 0);
    }

    #[test]
    fn the_unit_stands_on_the_line_point_for_the_steps_owed() {
        let mut c = chassis();
        c.line = (0..10).map(|i| (336, 336 + i)).collect();
        c.since = (0, 0);
        let mut stat = vec![0u32; 128 * 128];
        stat[10 * 128 + 10] = 9;
        c.step((0, 4), &mut stat, &mut Log::default());
        assert_eq!(c.pos, (336, 340));
        assert_eq!(c.line.len(), 10, "the line is kept");
        assert_eq!(c.since, (0, 0), "the clock stays at the start of the line");
    }

    #[test]
    fn the_end_of_the_line_frees_it_restarts_the_clock_and_plans() {
        let mut c = chassis();
        c.line = vec![(336, 336), (336, 337)];
        let mut log = Log { route_ok: true, ..Default::default() };
        c.step((1, 5), &mut vec![0; 128 * 128], &mut log);
        assert_eq!(c.pos, (336, 337));
        assert_eq!(c.since, (1, 5));
        assert_eq!(log.calls, ["chase 1 5", "route", "plan"]);
        assert_eq!(c.cooldown, 6);
        assert_eq!(c.prev, (10, 10));
        assert_eq!(c.facing, 8, "from cell (10,10) to (11,10): east = 8 of 32");
        assert_eq!((c.want, c.frame), (8, 40), "the animation advanced one phase: 1 * 32 + 8");
    }

    #[test]
    fn arrival_pops_the_order_and_a_failed_route_clears_them() {
        let mut c = chassis();
        c.pos = c.dest;
        c.route_len = 5;
        let mut log = Log::default();
        c.step((0, 1), &mut vec![0; 128 * 128], &mut log);
        assert_eq!(log.calls, ["pop", "start"]);
        assert_eq!((c.step1, c.step2, c.route_len), ((3, 3), (3, 3), 0));
        let mut c = chassis();
        let mut log = Log::default();
        c.step((0, 1), &mut vec![0; 128 * 128], &mut log);
        assert_eq!(log.calls, ["chase 0 1", "route", "clear", "start"]);
    }

    #[test]
    fn the_guest_takes_the_other_machines_second_step_cell() {
        let mut c = chassis();
        c.step1 = (10, 10);
        c.step2 = (11, 10);
        let mut log = Log { guest: true, ..Default::default() };
        c.step((0, 1), &mut vec![0; 128 * 128], &mut log);
        assert_eq!(log.calls, [format!("line {:?} {:?}", (336, 336), (368, 336))]);
        assert_eq!(c.step1, (11, 10));
    }
}
