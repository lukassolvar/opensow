//! How a unit moves each tick: path search, chasing a target, cell occupancy and local avoidance, and the unit's whole per-tick step.

use super::*;

impl World {
    /// The exe's wave search (`opensow_formats::wave`, exact vs `0x4753a0`) over the cell grid. Returns the cell centres from
    /// the cell after `start` to `goal`. A blocked goal is replaced by the nearest free cell (the exe reaches into the
    /// blocked cell and relies on the walking code to stop short).
    /// The search runs on the ground layer (`0x4753a0`, grid `0x5fac38`) or the air layer (`0x474b30`, grid `0x60ac38`, the
    /// same search over the border terrain).
    pub fn find_path_on(&self, air: bool, start: (i32, i32), goal: (i32, i32)) -> Option<Vec<(f32, f32)>> {
        let blocked = |x: i32, y: i32| if air { self.is_air_blocked(x, y) } else { self.is_blocked(x, y) };
        let goal = if blocked(goal.0, goal.1) {
            self.rings.search(0, 12).iter().map(|&(dx, dy)| (goal.0 + dx as i32, goal.1 + dy as i32)).find(|&(x, y)| !blocked(x, y))?
        } else {
            goal
        };
        if start == goal {
            return Some(vec![centre(goal)]);
        }
        let mut wave = Wave::new(blocked);
        // the exe searches from the destination to the unit (`[unit+0x5c]` is the destination, the target argument the
        // unit's own position), so the route comes back unit first
        let mut route = wave.find(goal, start, 0)?;
        route.remove(0);
        Some(route.into_iter().map(centre).collect())
    }

    /// Weapon range in cells a chasing unit tries to get within (`[chassis+0x154]`, set at `0x46c0ac`): the middle of the
    /// weapon's minimum and maximum range, 5 for anti-air weapons, 1 without a weapon.
    pub(super) fn chase_range(&self, i: usize) -> i32 {
        let u = &self.units[i];
        let Some(p) = u.aim.as_ref().map(|a| &a.params) else { return 1 };
        if kinds::ANTI_AIR_UNITS.contains(&u.kind) {
            return 5;
        }
        (((p.min_range + p.max_range) / 2) as i32).max(1)
    }

    /// `0x46eb40`: a unit with a chase target walks until it is within its range of the target (cell distance below
    /// `range`); farther away it takes the cell `range` steps before the target on the way there (`0x477630`).
    pub(super) fn chase_step(&mut self, i: usize, t: Target, tcell: (i32, i32)) {
        let range = self.chase_range(i);
        let at = self.units[i].cell();
        let dist = (((at.0 - tcell.0).pow(2) + (at.1 - tcell.1).pow(2)) as f64).sqrt() as i32;
        if dist < range {
            return;
        }
        let (handle, tcell) = match t {
            Target::Unit(_) => (0, tcell),
            Target::Structure(k) => (0x2000000 + k as u32, self.structures[k].center_cell()),
        };
        // a target of handle type 4 (an Advancer) is chased over the air layer (`0x478200`), every other target over the ground's
        let air_target = matches!(t, Target::Unit(uid) if self.unit(uid).is_some_and(|u| u.kind == kinds::ADVANCER));
        let dims = self.cells;
        let id = self.units[i].id;
        let dest = self.occupancy().and_then(|o| {
            if air_target {
                approach_air(&mut o.air_stat, &mut o.stat_base, dims, at, tcell, handle, range)
            } else {
                approach(&mut o.stat, &mut o.stat_base, dims, at, tcell, handle, range)
            }
        });
        match dest {
            Some(cell) if cell != at => self.order_move(id, cell, None),
            Some(_) => {}
            None => self.units[i].halt(),
        }
    }

    /// Unit-avoidance grids for this tick (`0x61ac38` / `0x62ac38`, static `0x5eac38`): built on first use.
    pub(super) fn occupancy(&mut self) -> Option<&mut Occupancy> {
        if self.occ.is_none() && self.cells.0 <= 128 && self.cells.1 <= 128 {
            let mut stat = vec![0u32; 16384];
            for x in 0..128 {
                for y in 0..128 {
                    if self.is_blocked(x, y) {
                        stat[(x * 128 + y) as usize] = 0xFFFFFD;
                    }
                }
            }
            for (k, s) in self.structures.iter().enumerate().filter(|(_, s)| s.alive()) {
                for (x, y) in s.blocked_cells() {
                    if (0..128).contains(&x) && (0..128).contains(&y) {
                        stat[(x * 128 + y) as usize] = 0x2000000 + k as u32;
                    }
                }
            }
            let mut air_stat = vec![0u32; 16384];
            for x in 0..128 {
                for y in 0..128 {
                    if self.is_air_blocked(x, y) {
                        air_stat[(x * 128 + y) as usize] = 0xFFFFFD;
                    }
                }
            }
            let mut ground = Wave::new(|_, _| false);
            let mut air = Wave::new(|_, _| false);
            ground.cells = stat.clone();
            air.cells = air_stat.clone();
            for u in self.units.iter().filter(|u| u.hp > 0) {
                let (x, y) = u.cell();
                if (0..128).contains(&x) && (0..128).contains(&y) {
                    let grid = if u.kind == kinds::ADVANCER { &mut air } else { &mut ground };
                    grid.cells[(x * 128 + y) as usize] = 0x3000000 + u.id;
                }
            }
            // a corpse re-stamps its cell with the terrain marker every step (0x46dfeb / 0x46e004)
            // DELIBERATE DEVIATION: a dead Advancer's corpse (air grid) does not block. The exe stamps it for 430 steps, but then the
            // spot above a building an enemy Advancer was capturing stays closed and the player's Advancer cannot start repairing
            for c in self.corpses.iter().filter(|c| !c.air) {
                let (x, y) = (c.x.div_euclid(CELL), c.y.div_euclid(CELL));
                if (0..128).contains(&x) && (0..128).contains(&y) {
                    let grid = if c.air { &mut air } else { &mut ground };
                    grid.cells[(x * 128 + y) as usize] = 0xFFFFFD;
                }
            }
            // a walking unit also holds the cell it is heading to (0x473190 stamps `[unit+0x6c/0x70]`), so two units never
            // take the same next cell
            for u in self.units.iter().filter(|u| u.hp > 0) {
                if let Some((x, y)) = u.heading_cell().filter(|&c| c != u.cell()) {
                    if (0..128).contains(&x) && (0..128).contains(&y) {
                        let grid = if u.kind == kinds::ADVANCER { &mut air } else { &mut ground };
                        let at = (x * 128 + y) as usize;
                        if grid.cells[at] == 0 {
                            grid.cells[at] = 0x3000000 + u.id;
                        }
                    }
                }
            }
            self.occ = Some(Occupancy { ground, air, stat, air_stat, stat_base: 1 });
        }
        self.occ.as_mut()
    }

    /// A unit's pixel step took it into another cell: the exe clears the old cell and stamps the new one in the same pass
    /// (`0x46f759` / `0x46f7ff` write the static grid at every pixel step), so units planning later in the same tick see the move.
    /// The unit grid holds the step cell, which is already stamped, so only the cell the unit left changes here.
    pub(super) fn occupancy_moved(&mut self, i: usize, old: (i32, i32)) {
        let (own, air, new) = (0x3000000 + self.units[i].id, self.units[i].kind == kinds::ADVANCER, self.units[i].cell());
        let heading = self.units[i].heading_cell();
        let Some(occ) = self.occ.as_mut() else { return };
        let grid = if air { &mut occ.air } else { &mut occ.ground };
        let mut set = |cell: (i32, i32), from: u32, to: u32| {
            if (0..128).contains(&cell.0) && (0..128).contains(&cell.1) {
                let at = (cell.0 * 128 + cell.1) as usize;
                if grid.cells[at] == from {
                    grid.cells[at] = to;
                }
            }
        };
        if heading != Some(old) {
            set(old, own, 0);
        }
        set(new, 0, own);
    }

    /// The planning half of the exe's walking step (`0x46f550` when the pixel line is empty): arrival, the global route search
    /// every 7th round, then the local step (`0x473190`). Sets `step`; returns `true` when the unit has to wait.
    pub(super) fn plan_step(&mut self, i: usize) -> bool {
        let u = &mut self.units[i];
        if u.step.is_some() {
            return false;
        }
        if u.straight.is_some() {
            // a type-2 order: a straight line without planning
            u.step = u.path.first().copied();
            return false;
        }
        let at = u.cell();
        let air = u.kind == kinds::ADVANCER;
        let Some(dest) = u.dest else {
            // no destination (an Advancer's straight flight, a stopped unit): follow what is left of the route
            if u.path.is_empty() {
                return false;
            }
            return self.avoid(i);
        };
        let dest_cell = (dest.0.div_euclid(CELL), dest.1.div_euclid(CELL));
        if u.path.is_empty() && at == dest_cell {
            // on the destination: the order is done (`0x46f5b1`)
            u.dest = None;
            return false;
        }
        // not there yet: the global route is searched when the cooldown `[+0x80]` runs out (every 7th round) or when it was used
        // up by detours (`[+0xa0] == 0` with the unit away from its destination)
        u.cool -= 1;
        if u.cool < 0 || u.path.is_empty() {
            u.cool = 6;
            match self.find_path_on(air, at, dest_cell) {
                Some(mut path) => {
                    if let Some(last) = path.last_mut().filter(|_| (dest.0 as f32, dest.1 as f32) != centre(dest_cell)) {
                        *last = (dest.0 as f32, dest.1 as f32);
                    }
                    self.units[i].path = path;
                }
                None => {
                    // a failed search clears the orders
                    self.units[i].halt();
                    return false;
                }
            }
        }
        self.avoid(i)
    }

    /// Re-plans the first stretch of unit `i`'s route around other units and buildings (`0x473190` -> `0x476870`): the cells
    /// up to six ahead are replaced by the route of a local wave over the occupancy grid.
    /// Returns `true` when no way around the units in front was found: the unit waits where it is.
    pub(super) fn avoid(&mut self, i: usize) -> bool {
        let (at, own, air, cells) = {
            let u = &self.units[i];
            let cells: Vec<(i32, i32)> = u.path.iter().map(|&(x, y)| ((x.floor() as i32).div_euclid(CELL), (y.floor() as i32).div_euclid(CELL))).collect();
            (u.cell(), 0x3000000 + u.id, u.kind == kinds::ADVANCER, cells)
        };
        let mut route = vec![at];
        route.extend(&cells);
        let k = cells.len().min(6);
        let heading = self.units[i].heading_cell().filter(|&c| c != at);
        let Some(occ) = self.occupancy() else { return false };
        let Occupancy { ground, air: agrid, stat, air_stat, stat_base } = occ;
        // 0x473190 first clears the unit's old step cell
        if let Some((x, y)) = heading.filter(|&(x, y)| (0..128).contains(&x) && (0..128).contains(&y)) {
            let grid = if air { &mut *agrid } else { &mut *ground };
            if grid.cells[(x * 128 + y) as usize] == own {
                grid.cells[(x * 128 + y) as usize] = 0;
            }
        }
        let r = if air { agrid.local_step(air_stat, *stat_base, own, &route, at) } else { ground.local_step(stat, *stat_base, own, &route, at) };
        let u = &mut self.units[i];
        if r.stop {
            u.halt();
        } else if let Some(next) = r.next {
            // the step cell is the local search's first cell; the global route is not touched (the exe pops its head cell when the
            // segment is done and re-searches it every 7th round), so detours around units never rewrite the route
            u.step = Some(match u.path.first() {
                Some(&p) if ((p.0.floor() as i32).div_euclid(CELL), (p.1.floor() as i32).div_euclid(CELL)) == next => p,
                _ => centre(next),
            });
        } else {
            // no local route (0x476870 failed): the exe then looks for a free cell near the destination (0x4774c0) and keeps
            // its previous step cell; here the unit waits and tries again on the next tick
            let waiting = route.len() > 1 && route[k] != at;
            if waiting {
                self.retarget_to_free_cell(i);
            } else {
                // nothing to avoid (the route's last cell is the unit's own): walk straight to the first cell
                u.step = u.path.first().copied();
            }
            return waiting;
        }
        // the new step cell is stamped at once, so units planning later this tick see it taken
        if let Some((x, y)) = self.units[i].heading_cell().filter(|&c| c != at) {
            let air = self.units[i].kind == kinds::ADVANCER;
            let handle = 0x3000000 + self.units[i].id;
            if let Some(occ) = self.occupancy().filter(|_| (0..128).contains(&x) && (0..128).contains(&y)) {
                let grid = if air { &mut occ.air } else { &mut occ.ground };
                if grid.cells[(x * 128 + y) as usize] == 0 {
                    grid.cells[(x * 128 + y) as usize] = handle;
                }
            }
        }
        false
    }

    /// `0x4774c0`, the fallback of the planners when the local search fails: the destination moves to the free cell nearest to
    /// it that the unit can reach (ground: a flood of the unit grid from the unit's cell marks the reachable region and the
    /// range-ring list around the destination picks the first cell of it; Advancers take the first free ring cell). The exe
    /// keeps the old step cell and only searches a new global route a few steps later (cooldown `[+0x80]`); here the route is
    /// searched at once.
    pub(super) fn retarget_to_free_cell(&mut self, i: usize) {
        let (air, own, at, at_px) = {
            let u = &self.units[i];
            (u.kind == kinds::ADVANCER, 0x3000000 + u.id, u.cell(), (u.x.round() as i32, u.y.round() as i32))
        };
        let Some(&(lx, ly)) = self.units[i].path.last() else { return };
        let dest = (lx.round() as i32, ly.round() as i32);
        let map = self.cells;
        let rings = std::mem::replace(&mut self.rings, Rings { offsets: Vec::new(), ends: Vec::new() });
        let found = self.occupancy().and_then(|occ| {
            let grid = if air { &mut occ.air } else { &mut occ.ground };
            free_cell_near(grid, &rings, own, dest, at_px, air, map).destination
        });
        self.rings = rings;
        let Some((px, py)) = found else { return };
        let goal = (px.div_euclid(CELL), py.div_euclid(CELL));
        if goal == (dest.0.div_euclid(CELL), dest.1.div_euclid(CELL)) {
            return;
        }
        if let Some(path) = self.find_path_on(air, at, goal) {
            self.units[i].path = path;
            self.units[i].dest = Some((px, py));
            self.units[i].line = None;
        }
    }

    /// One tick of unit `i`: orders, walking, aiming and firing.
    pub(super) fn step_unit(&mut self, i: usize, index: &HashMap<(i32, i32), Vec<Target>>) {
        let (kind, player, cell) = (self.units[i].kind, self.units[i].player, self.units[i].cell());
        // idle look-around (`0x46e114`): a unit without orders turns its idle top to a random facing every 240..1139 ticks
        // (the counter runs while the chassis has no order, `0x46e11b`; a unit without a top, the pod, still draws the `rand(900)`
        // reload, `0x46e147`, but no facing, `0x46e16f`)
        if self.units[i].kind != kinds::ADVANCER && self.units[i].path.is_empty() && self.units[i].straight.is_none() {
            self.units[i].look -= 1;
            if self.units[i].look <= 0 {
                let wait = 240 + self.random(900) as i32;
                self.units[i].look = wait;
                if let Some(steps) = self.units[i].aim.as_ref().map(|a| a.params.steps) {
                    let face = self.random(steps);
                    let u = &mut self.units[i];
                    if u.target.is_none() {
                        u.aim.as_mut().unwrap().set_target_facing(face);
                    }
                }
            }
        }
        self.step_unit_weapon(i, index, kind, player, cell);
        // a Rogon bomb (kind 46) blows up when its target's handle is in the 3 x 3 cells around it (0x46df40, `rogon::touches`)
        if kind == kinds::POD && self.pod_reaches_target(i, cell) {
            return;
        }
        // chasing (`0x46eb40`): the route to the target is replanned every 7 ticks. The weapon firing at something does not
        // stop the chassis (the exe updates top and chassis independently every tick); a unit stops where its route ends
        if let Some(t) = self.units[i].chase {
            match self.target_pos(t) {
                Some((tx, ty)) if self.hostile(player, t) => {
                    if self.units[i].path.is_empty() || self.tick % 7 == (i as u64 % 7) {
                        self.chase_step(i, t, (tx.div_euclid(CELL), ty.div_euclid(CELL)));
                    }
                }
                _ => self.units[i].chase = None,
            }
        }
        // walking: `steps_at(speed, tick % 60)` single-pixel moves per tick (0x46f550 + table 0x436df0); the unit only
        // starts once its chassis is within 4 turn steps of the wanted facing
        // (every kind plans around other units: the movement code has no branch on kind 46, docs/walking.md)
        let ox = (self.units[i].x, self.units[i].y);
        self.step_unit_walk(i);
        let u = &mut self.units[i];
        u.moved = (u.x, u.y) != ox;
        // the chassis tick (0x46f906) advances the walk phase while the unit's position differs from the goal pixel of its current
        // order (`[chassis+0x5c/0x60]`), i.e. while it still has a route: a unit waiting for a free cell keeps stepping its legs
        let moved = !u.path.is_empty() || u.line.is_some() || u.straight.is_some();
        let then = if u.path.is_empty() { u.straight.take() } else { None };
        // the queued rally order starts when the exit orders are done (the order list's next entry, `0x470480`)
        let rally = if u.path.is_empty() && u.straight.is_none() && then.is_none() && u.chase.is_none() { u.queued.take() } else { None };
        if let Some(c) = u.chassis.as_mut() {
            // every chassis turn step hands the chassis's target facing to the weapon top (`0x479980`, docs/art.md)
            if c.tick(moved) {
                let target = c.target;
                if let Some(a) = u.aim.as_mut() {
                    a.set_target_facing(target);
                }
            }
        }
        if let Some(then) = then {
            let id = self.units[i].id;
            self.order_move(id, then, None);
        } else if let Some((x, y)) = rally {
            let id = self.units[i].id;
            self.order_move(id, (x.div_euclid(CELL), y.div_euclid(CELL)), Some((x, y)));
        }
    }

    /// The weapon part of a unit's tick: the top looks for a target, turns to it and fires (the exe updates the top and the
    /// chassis independently every tick). The Advancer's gun has its own target search.
    fn step_unit_weapon(&mut self, i: usize, index: &HashMap<(i32, i32), Vec<Target>>, kind: i32, player: i32, cell: (i32, i32)) {
        let mut spawn: Vec<((i32, i32), (i32, i32))> = Vec::new();
        if kind == kinds::ADVANCER && self.units[i].aim.is_some() {
            // the Advancer's gun (top case 0x47a8af, bullet 31, strength 64): the flyer search 0x47aed0 finds ground anti-air
            // units (handle type 5: kinds 1, 2, 3, 42; not the anti-air turret), then 0x47ad30 enemy Advancers. It keeps
            // searching while it flies to a building and while the beam hovers over it; only a type-2 (straight) order blocks it.
            let (min, max, steps) = {
                let p = &self.units[i].aim.as_ref().unwrap().params;
                (p.min_range, p.max_range, p.steps)
            };
            // only a type-2 order blocks the search (`0x46e1cc` -> 5th argument of `0x479c60`); the Advancer's go-to onto a building is type 1
            let approaching = self.units[i].straight.is_some();
            let t = if approaching { None } else { self.advancer_target(index, player, cell, min, max) };
            let (ux, uy) = self.units[i].px();
            let uy = uy + opensow_formats::aim::top_dy(kind); // aim origin = position + [top+0x60] (0x4797c0)
                                                              // a flying target is aimed at (x, y + h/2) but the shot flies to its sprite (x, y + h) (`0x4797c0`)
            let tpoints = t.and_then(|id| self.unit(id)).map(|u| if u.kind == kinds::ADVANCER { u.flyer_points() } else { (u.px(), u.px()) });
            let face = tpoints.map(|(aim, _)| facing(ux, uy, aim.0, aim.1, steps));
            let tpos = tpoints.map(|(_, shot)| shot);
            // the reload jitter is drawn from the lockstep table when the search finds a target (`0x479cf3` -> `0x47c5f0`)
            let (table, state) = (&mut self.game_rng, &mut self.rng);
            let ev = self.units[i].aim.as_mut().unwrap().tick(face, |n| draw(table, state, n));
            if ev.shot.is_some() {
                if let (Some(id), Some(to), Some((bt, strength))) = (t, tpos, weapon_shot(40)) {
                    // a bullet of type 31 (invisible, speed 600); the strength comes off the unit by handle when it lands (0x4108bc..0x4109b8)
                    // the spawner starts at the shooter's sprite: unit position + hover offset `h` (0x479e0e..0x479e20, the shooter is a
                    // flyer) + the top's offset `[top+0x40]` (= `[top+0x60]`, -16, copied at 0x47abbc)
                    let (fx, fy) = self.units[i].flyer_points().1;
                    let from = (fx, fy + opensow_formats::aim::top_dy(kind));
                    // the constructor of a type 31 bullet plays `tt-fire` (id 56) at the start point (0x412240..0x412276)
                    self.sound(GUN_SOUND, from.0, from.1, true);
                    self.shoot_at(bt, strength as i32, player.min(1), from, to, HitTarget::Unit(id));
                }
            }
        } else if self.units[i].aim.is_some() && !is_anti_air(kind) {
            let (min, max, steps) = {
                let p = &self.units[i].aim.as_ref().unwrap().params;
                (p.min_range, p.max_range, p.steps)
            };
            let t = self.find_target(index, player, cell, min, max, self.units[i].chase);
            self.units[i].target = t;
            let (ux, uy) = self.units[i].px();
            let uy = uy + opensow_formats::aim::top_dy(kind);
            let tpos = t.and_then(|t| self.target_pos(t));
            let face = tpos.map(|(tx, ty)| facing(ux, uy, tx, ty, steps));
            // the reload jitter is drawn from the lockstep table when the search finds a target (`0x479cf3` -> `0x47c5f0`)
            let (table, state) = (&mut self.game_rng, &mut self.rng);
            let ev = self.units[i].aim.as_mut().unwrap().tick(face, |n| draw(table, state, n));
            if let (Some((_, shots)), Some(to)) = (ev.shot, tpos) {
                let a = self.units[i].aim.as_ref().unwrap();
                let (sin, cos) = heading_degrees(a.facing, steps).to_radians().sin_cos();
                let fallback = (ux + (sin * 14.0) as i32, uy - (cos * 14.0) as i32);
                // the spawner `0x47cf40` loops over the step's shots, shot k using `.po1` table barrel + k (offset += 0x40 words): it
                // starts at position - raw point and ends at target + (`.po2` reference point - raw point) (`0x47d0a5..0x47d0cb`), so
                // the shots of a volley fly parallel (walker 1's two barrels = two bolts). The tables are the sprite-anchor offsets,
                // so `uy` loses the aim origin's `top_dy` again. Without the tables: one point 14 px along the facing.
                let (barrel, facing_now) = (ev.shot.map_or(0, |s| s.0) as usize, a.facing as usize);
                let anchor_y = uy - opensow_formats::aim::top_dy(kind);
                let reference = self.turret_muzzles.get(&(REFERENCE_KEY + kind)).and_then(|t| t.muzzle(0, facing_now)).unwrap_or((0, 0));
                for _ in 0..opensow_formats::aim::rounds(kind) {
                    // every round of the spawner scatters the end point (0x47cfe1..0x47d03f)
                    let (jx, jy) = self.volley_scatter(kind);
                    let to = (to.0 + jx, to.1 + jy);
                    for table in barrel..barrel + shots.max(1) as usize {
                        let (from, to) = match self.turret_muzzles.get(&kind).and_then(|t| t.muzzle(table, facing_now)) {
                            Some((mx, my)) => ((ux + mx, anchor_y + my), (to.0 + mx - reference.0, to.1 + my - reference.1)),
                            None => (fallback, to),
                        };
                        spawn.push((from, to));
                    }
                }
                // the spawner plays the weapon's muzzle sound once per volley (`sfx::muzzle`)
                if let Some((id, jitter)) = opensow_formats::sfx::muzzle(kind as u8) {
                    self.sound(id, ux, uy, jitter);
                }
            }
        } else if self.units[i].aim.is_some() {
            // anti-air tops shoot aircraft: the hit takes the shot's strength off the aircraft directly (events 1, 10, 31 in 0x40f590)
            self.units[i].target = None;
            let (min, max, steps) = {
                let p = &self.units[i].aim.as_ref().unwrap().params;
                (p.min_range, p.max_range, p.steps)
            };
            let (ux, uy) = self.units[i].px();
            let polls = self.units[i].aim.as_ref().is_some_and(|a| a.polls_now() || a.state != opensow_formats::aim::State::Idle);
            let ai = if polls {
                let poll = self.units[i].aim.as_ref().unwrap().polls_now();
                self.anti_air_aircraft((false, self.units[i].id as usize), poll, player, (ux, uy), min, max)
            } else {
                None
            };
            let oy = uy + opensow_formats::aim::top_dy(kind);
            // no aircraft: the air-grid search 0x47ad30 finds an enemy Advancer
            let adv = if ai.is_none() { self.advancer_in_range(index, player, cell, min, max) } else { None };
            let apos = adv.and_then(|id| self.unit(id)).map(|u| u.flyer_points().0);
            let face = match (ai, apos) {
                (Some(a), _) => {
                    let (tx, ty) = self.aircraft_aim_point(a);
                    Some(facing(ux, oy, tx, ty, steps))
                }
                (None, Some((tx, ty))) => Some(facing(ux, oy, tx, ty, steps)),
                _ => None,
            };
            // the reload jitter is drawn from the lockstep table when the search finds a target (`0x479cf3` -> `0x47c5f0`)
            let (table, state) = (&mut self.game_rng, &mut self.rng);
            let ev = self.units[i].aim.as_mut().unwrap().tick(face, |n| draw(table, state, n));
            if ev.shot.is_some() {
                if let Some((bt, strength)) = weapon_shot(kind) {
                    // a type 1 bullet flies from the muzzle to the aircraft (or the Advancer's shot point); the hit is by handle
                    let a = self.units[i].aim.as_ref().unwrap();
                    let (sin, cos) = heading_degrees(a.facing, steps).to_radians().sin_cos();
                    let from = (ux + (sin * 14.0) as i32, uy - (cos * 14.0) as i32);
                    if let Some((id, jitter)) = opensow_formats::sfx::muzzle(kind as u8) {
                        self.sound(id, ux, uy, jitter);
                    }
                    if let Some(a) = ai {
                        let (to, id) = (self.aircraft_aim_point(a), self.aircraft[a].id);
                        self.shoot_at(bt, strength as i32, player.min(1), from, to, HitTarget::Aircraft(id));
                    } else if let Some(id) = adv {
                        if let Some(to) = self.unit(id).map(|u| u.flyer_points().1) {
                            self.shoot_at(bt, strength as i32, player.min(1), from, to, HitTarget::Unit(id));
                        }
                    }
                }
            }
        } else {
            self.units[i].target = None;
        }
        for (from, to) in spawn {
            self.fire(kind, player, from, to);
        }
    }

    /// Whether pod `i`, standing on `cell`, has reached the target it was sent against: the target's handle lies in the
    /// 3 x 3 cells around it (`0x46df40`, `rogon::touches`). The pod is then destroyed, which sets off its bomb.
    fn pod_reaches_target(&mut self, i: usize, cell: (i32, i32)) -> bool {
        if let Some(t) = self.units[i].chase {
            let cells: Vec<(i32, i32)> = match t {
                Target::Unit(id) => self
                    .unit(id)
                    .filter(|u| u.hp > 0 && opensow_formats::rogon::can_touch(u.kind))
                    .map(|u| [Some(u.cell()), u.heading_cell()].into_iter().flatten().collect())
                    .unwrap_or_default(),
                Target::Structure(k) => self.structures.get(k).filter(|s| s.alive()).map(|s| s.blocked_cells()).unwrap_or_default(),
            };
            if opensow_formats::rogon::touches(cell, cells) {
                self.units[i].hp = 0;
                return true;
            }
        }
        false
    }

    /// The walking part of a unit's tick: the Advancer's straight flight, or the pixel steps owed this tick along the
    /// planned route (`0x46f550`).
    fn step_unit_walk(&mut self, i: usize) {
        let tick = self.tick;
        if self.units[i].kind == kinds::ADVANCER {
            let u = &mut self.units[i];
            if tick.is_multiple_of(HOVER_PERIOD) {
                u.hover_phase = (u.hover_phase + 1) % HOVER_PHASES;
            }
            // the Advancer hovers: it spins all the time, whether it moves or not
            if let Some(c) = u.chassis.as_mut() {
                if tick.is_multiple_of(ADVANCER_SPIN_TICKS) {
                    c.facing = (c.facing + 1) % 32;
                }
                c.target = c.facing;
            }
        }
        // the walking step (`0x46f550`) loops over the pixel steps owed this tick; each pass plans a segment when the pixel line
        // is empty, points the chassis at the step cell, turns it one counter tick and walks one pixel once the facing is within
        // 4 turn steps of the wanted one. The chassis tail below turns once more
        let moves = steps_at(self.walk_params_of(self.units[i].kind).map_or(20, |p| p.speed), (tick % 60) as u32);
        for _ in 0..moves {
            if self.units[i].step.is_none() && self.plan_step(i) {
                break; // waiting for a free cell
            }
            let u = &mut self.units[i];
            let Some((wx, wy)) = u.step else { break };
            let steps = walk_params(u.kind).map_or(32, |p| p.steps);
            let aligned = match u.chassis.as_mut() {
                Some(c) if u.kind != kinds::ADVANCER => {
                    // the wanted facing is recomputed from the pixel to the step cell centre unless the unit stands on it
                    if (wx - u.x).abs() + (wy - u.y).abs() > 1.0 {
                        let want = facing(u.x.round() as i32, u.y.round() as i32, wx.round() as i32, wy.round() as i32, steps);
                        c.set_target(want);
                    }
                    if c.turn() {
                        let target = c.target;
                        if let Some(a) = u.aim.as_mut() {
                            a.set_target_facing(target);
                        }
                    }
                    c.within_turns(4)
                }
                _ => true,
            };
            if !aligned {
                break;
            }
            let goal = (wx.round() as i32, wy.round() as i32);
            let here = (u.x.round() as i32, u.y.round() as i32);
            if here == goal {
                // the segment is done: the next pass plans the following one (`avoid` = 0x473190 runs when the line is empty)
                if !u.path.is_empty() {
                    u.path.remove(0);
                }
                u.step = None;
                u.line = None;
                continue;
            }
            // Bresenham from the segment start: one pixel along the long axis per move, the short axis when due
            let mut line = match u.line.take() {
                Some(l) if l.goal == goal && l.rest.last().is_some_and(|p| (p.0 - here.0).abs().max((p.1 - here.1).abs()) == 1) => l,
                _ => {
                    let mut rest = opensow_formats::walk::pixel_line(here.0, here.1, goal.0, goal.1);
                    rest.remove(0); // the start point: the unit is already there
                    rest.reverse();
                    Line { goal, rest }
                }
            };
            let next = line.rest.pop().expect("a line to a goal that is not here has points");
            u.line = Some(line);
            let before = u.cell();
            u.x += (next.0 - here.0) as f32;
            u.y += (next.1 - here.1) as f32;
            if u.cell() != before {
                self.occupancy_moved(i, before);
            }
        }
    }
}
