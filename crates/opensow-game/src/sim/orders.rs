//! The player's commands: selecting units, go-to and attack orders, rally points.

use super::*;

impl World {
    /// Orders the selected units to walk to `(x, y)` (map pixels); a group spreads over neighbouring free cells.
    pub fn order_move_selected(&mut self, x: i32, y: i32) {
        // the pods of missions 3 and 8 take no go-to orders either (`0x418a0c` / `0x419079`): the script drives them
        let scripted = self.pods_scripted();
        if self.units.iter().any(|u| u.selected && u.player == 0 && u.kind == kinds::ADVANCER) {
            if let Some(t) = &mut self.tutorial {
                t.advancer_moved = true; // step 5: the selected Advancer got a go-to order
            }
        }
        for u in self.units.iter_mut().filter(|u| u.selected && u.player == 0 && !(scripted && u.kind == kinds::POD)) {
            u.chase = None;
            u.queued = None; // a go-to replaces the whole order list (`0x4706a0`)
        }
        let goal = (x.div_euclid(CELL), y.div_euclid(CELL));
        let mut slots = std::iter::once((0i16, 0i16)).chain(self.rings.search(0, 8).iter().copied()).collect::<Vec<_>>();
        slots.dedup();
        let mut next = 0usize;
        let ids: Vec<u32> = self.units.iter().filter(|u| u.selected && u.player == 0 && !(scripted && u.kind == kinds::POD)).map(|u| u.id).collect();
        for id in ids {
            let mut cell = goal;
            while next < slots.len() {
                let c = (goal.0 + slots[next].0 as i32, goal.1 + slots[next].1 as i32);
                next += 1;
                if !self.is_blocked(c.0, c.1) {
                    cell = c;
                    break;
                }
            }
            self.order_move(id, cell, (cell == goal).then_some((x, y)));
        }
    }

    /// Key X (`0x418dc0`, `formats::scatter`): every selected unit walks away from the middle of the selection's box, 6 half cells
    /// past its own spot along the line from the middle, else 4, 2 or 0 when that cell is off the map or blocked for walkers.
    /// Returns whether any unit got an order.
    pub fn scatter_selected(&mut self) -> bool {
        use opensow_formats::scatter;
        let sel: Vec<(u32, i32, i32, i32)> =
            self.units.iter().filter(|u| u.selected && u.player == 0 && u.hp > 0).map(|u| (u.id, u.x as i32, u.y as i32, u.kind)).collect();
        let Some(&(_, x0, y0, _)) = sel.first() else { return false };
        let (min, max) = sel.iter().fold(((x0, y0), (x0, y0)), |(lo, hi), &(_, x, y, _)| ((lo.0.min(x), lo.1.min(y)), (hi.0.max(x), hi.1.max(y))));
        let c = scatter::centre(min, max);
        // the pods of missions 3 and 8 never find a cell (`0x419079`): the script drives them
        let scripted = self.pods_scripted();
        let mut any = false;
        for (id, x, y, kind) in sel {
            if scripted && kind == kinds::POD {
                continue;
            }
            let Some(cell) = scatter::TRIES.iter().map(|&n| scatter::target(x, y, c, n)).find(|&(cx, cy)| !self.is_blocked(cx, cy)) else { continue };
            if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
                u.chase = None;
                u.queued = None; // a go-to replaces the whole order list (`0x4706a0`)
            }
            self.order_move(id, cell, None);
            any = true;
        }
        any
    }

    /// Orders the selected units to attack `target`.
    pub fn order_attack_selected(&mut self, target: Target) {
        // the pods of missions 3 and 8 take no attack orders (0x46a10b: they are driven by the script)
        let scripted = self.pods_scripted();
        for u in self.units.iter_mut().filter(|u| u.selected && u.player == 0 && !(scripted && u.kind == kinds::POD)) {
            u.chase = Some(target);
            u.queued = None;
        }
    }

    /// Can the pointer over `t` show the attack pointer while an Advancer is selected? The exe (`0x417a8c`) shows go-to over an
    /// enemy unit that is neither a flyer (`[unit+0x4b]`, the Advancer) nor anti-air (`[unit+0x4c]`).
    pub fn advancer_can_attack(&self, t: Target) -> bool {
        matches!(t, Target::Unit(id) if self.unit(id).is_some_and(|u| u.kind == kinds::ADVANCER || is_anti_air(u.kind)))
    }

    /// The hostile unit or structure under map pixel `(x, y)` for the player, if any.
    pub fn enemy_at(&self, x: i32, y: i32) -> Option<Target> {
        let near = self
            .units
            .iter()
            .filter(|u| u.hp > 0 && u.player != 0)
            .find(|u| ((u.x - x as f32).powi(2) + (u.sprite_y() - y as f32).powi(2)).sqrt() < 20.0)
            .map(|u| Target::Unit(u.id));
        near.or_else(|| self.structure_at(x, y).filter(|&k| self.structures[k].owner != Owner::Player).map(Target::Structure))
    }

    /// Walks unit `id` to `cell`, ending at the pixel `exact` when given and reachable.
    pub fn order_move(&mut self, id: u32, cell: (i32, i32), exact: Option<(i32, i32)>) {
        if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
            u.straight = None;
        }
        let Some(u) = self.unit(id) else { return };
        // a unit in the middle of a segment finishes its pixel line first (a type-1 order does not touch it, `0x470480`); the
        // route is searched from the step cell it is walking to, which is where the exe's next search starts
        let (start, air, flying_to) = (u.heading_cell().unwrap_or(u.cell()), u.kind == kinds::ADVANCER, u.step);
        // a type-1 order (`0x470480`): the wave search of the unit's own layer (`0x4753a0` mode 0, or `0x474b30` for the Advancer)
        let mut path = match self.find_path_on(air, start, cell) {
            Some(p) => p,
            None => return,
        };
        if let (Some((ex, ey)), Some(last)) = (exact, path.last_mut()) {
            *last = (ex as f32, ey as f32);
        }
        if let Some(from) = flying_to {
            // `path[0]` stands for the segment in flight and is dropped when the unit gets there
            path.insert(0, from);
        }
        let dest = exact.unwrap_or_else(|| {
            let c = centre(cell);
            (c.0 as i32, c.1 as i32)
        });
        let u = self.units.iter_mut().find(|u| u.id == id).unwrap();
        u.path = path;
        u.dest = Some(dest);
        // the search just done counts as the first of the exe's every-7th-round searches (`[+0x80]` is cleared by the order)
        u.cool = 7;
        if flying_to.is_none() {
            // no pixel line yet: the next step plans it (0x473190), avoiding units
            u.line = None;
        }
    }

    /// A type-2 order (`0x470480`) for the Advancer: it flies the straight line to `cell` without path finding (the beam
    /// approach of `0x408890`).
    #[cfg(test)]
    pub fn order_fly_straight(&mut self, id: u32, cell: (i32, i32)) {
        if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
            u.straight = None;
            u.line = None;
            // the pixel route is dropped for a straight line to the cell, which is also the step cell (`0x470480` type 2)
            u.step = Some(centre(cell));
            u.dest = None;
            u.path = vec![centre(cell)];
        }
    }

    /// Advancers (40) and nuclear vehicles (46) are never part of a group: they are selected alone, by clicking them.
    pub fn is_single_select(kind: i32) -> bool {
        matches!(kind, kinds::ADVANCER | kinds::POD)
    }

    /// Deselects every unit.
    pub fn clear_selection(&mut self) {
        self.units.iter_mut().for_each(|u| u.selected = false);
    }

    /// Selects the units inside the pixel rectangle; returns how many are selected afterwards. Like the exe (`0x417700..0x41776a` runs
    /// `0x46c820` over the unit lists of all three players, with no owner test) the box also takes enemy and neutral units: they
    /// get the local selection flag and so their health bar (`0x46a420`), but every order loop walks the player's own list only.
    pub fn select_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, additive: bool) -> usize {
        if !additive {
            self.clear_selection();
        }
        let (lx, hx, ly, hy) = (x0.min(x1) as f32, x0.max(x1) as f32, y0.min(y1) as f32, y0.max(y1) as f32);
        // a box never takes Advancers or nuclear vehicles, and drops one that was selected alone
        if self.units.iter().any(|u| u.selected && Self::is_single_select(u.kind)) {
            self.clear_selection();
        }
        for u in self.units.iter_mut().filter(|u| u.hp > 0 && !Self::is_single_select(u.kind)) {
            if u.x >= lx && u.x <= hx && u.sprite_y() >= ly && u.sprite_y() <= hy {
                u.selected = true;
            }
        }
        self.units.iter().filter(|u| u.selected).count()
    }

    /// Selects the player's unit nearest to `(x, y)` within `radius` pixels.
    pub fn select_at(&mut self, x: i32, y: i32, radius: f32, additive: bool) -> usize {
        if !additive {
            self.clear_selection();
        }
        let best = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.player == 0)
            .map(|(i, u)| (i, (u.x - x as f32).hypot(u.sprite_y() - y as f32)))
            .filter(|&(_, d)| d <= radius)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = best {
            // a single-select unit replaces the selection (even with shift); nothing joins one that is selected
            if Self::is_single_select(self.units[i].kind) || self.units.iter().any(|u| u.selected && Self::is_single_select(u.kind)) {
                self.clear_selection();
            }
            self.units[i].selected = true;
        }
        self.units.iter().filter(|u| u.selected).count()
    }

    /// Selects all of the player's regular units (not the ones that are only ever selected alone).
    pub fn select_all(&mut self) {
        self.units.iter_mut().for_each(|u| u.selected = u.player == 0 && !Self::is_single_select(u.kind));
    }

    /// The factory exit orders (`0x408740`): a type-2 order walks the unit in a straight line to the centre of `first` (it comes
    /// out of the building through the wall), the queued type-1 order then plans a route to `then`.
    pub(super) fn order_straight_then(&mut self, id: u32, first: (i32, i32), then: (i32, i32)) {
        if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
            u.path = vec![centre(first)];
            u.line = None;
            u.step = None;
            u.dest = None;
            u.straight = Some(then);
        }
    }

    /// Sets the rally point of own factory `b` to the centre of the cell holding map pixel `(x, y)` (`0x417489..0x4175a1`).
    /// Returns false when `b` is not a living factory of `player`'s.
    pub fn set_rally(&mut self, b: usize, player: usize, x: i32, y: i32) -> bool {
        let me = if player == 0 { Owner::Player } else { Owner::Enemy };
        match self.structures.get_mut(b) {
            Some(s) if s.alive() && s.owner == me && opensow_formats::rally::has_rally(s.kind_id) => {
                s.rally = Some(opensow_formats::rally::snap(x, y));
                true
            }
            _ => false,
        }
    }
}
