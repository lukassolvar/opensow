//! The Advancer's beam work: capture, repair and upgrade orders, the beam shots and the satellites that guard a protected building.

use super::*;

impl World {
    /// The pointer over structure `target` while an Advancer of `player` is selected (`0x417b8c..0x417d0a`): destroy for
    /// enemy and neutral buildings, repair / upgrade for own ones (docs/cursor.md).
    pub fn advancer_pointer(&self, player: i32, target: usize) -> Pointer {
        let Some(s) = self.structures.get(target).filter(|s| s.alive()) else { return Pointer::Arrow };
        let me = if player == 0 { Owner::Player } else { Owner::Enemy };
        pointer_over_building(s.owner == me, s.kind_id, s.hp < s.max_hp, s.active < s.slots.len(), self.repair_mode)
    }

    /// What the Advancer does when it is ordered onto structure `target`: the action of the pointer it would show.
    pub fn action_for(&self, unit: &Unit, target: usize) -> Option<Action> {
        action_for_pointer(self.advancer_pointer(unit.player, target))
    }

    /// The exe re-checks the target before every shot (`0x46f3a6`): repair needs missing hit points, upgrade a free slot.
    pub(super) fn action_valid(&self, unit: &Unit, action: Action, target: usize) -> bool {
        let Some(s) = self.structures.get(target) else { return false };
        let me = if unit.player == 0 { Owner::Player } else { Owner::Enemy };
        s.alive()
            && match action {
                // no owner test (`0x46f3a6`): Ctrl or Alt order it onto an own building; a capture ends it (`finish_destruction`)
                Action::Destroy => true,
                Action::Repair => s.owner == me && s.hp < s.max_hp,
                Action::Upgrade => s.owner == me && s.active < s.slots.len(),
            }
    }

    /// Orders the Advancer `id` onto structure `target`: it flies to the building's cell and shoots its beam there.
    pub fn order_action(&mut self, id: u32, target: usize) -> bool {
        let Some(action) = self.unit(id).and_then(|u| self.action_for(u, target)) else { return false };
        self.order_action_with(id, target, action)
    }

    /// Orders the Advancer `id` to do `action` on structure `target` (the pointer's action: destroy on an own building with
    /// Ctrl or Alt held, `0x417cdd`).
    pub fn order_action_with(&mut self, id: u32, target: usize, action: Action) -> bool {
        if self.unit(id).is_none_or(|u| u.kind != kinds::ADVANCER) || self.structures.get(target).is_none_or(|s| !s.alive()) {
            return false;
        }
        let cell = self.structures[target].action_cell();
        // `0x46b8b0` calls `0x4706a0` with order type 1 (a normal go-to, `0x46b9c6`), not the straight type 2 of the factory
        // exits (`0x408890`): the wave route and the local avoidance apply, so an Advancer already hovering over the cell blocks
        // it and the second one stops next to it, out of beam range (the original does the same)
        let c = centre(cell);
        self.order_move(id, cell, Some((c.0 as i32, c.1 as i32)));
        let u = self.units.iter_mut().find(|u| u.id == id).unwrap();
        u.action = Some((action, target));
        true
    }

    /// The structure under map pixel `(x, y)`, if any.
    /// Like [`structure_at`], also counting the art above the footprint; the footprint wins.
    pub fn structure_by_art(&self, x: i32, y: i32) -> Option<usize> {
        let c = (x.div_euclid(CELL), y.div_euclid(CELL));
        self.structure_at(x, y).or_else(|| self.structures.iter().position(|s| s.alive() && s.covers_art(c)))
    }

    /// Index of the living structure whose footprint covers a map pixel.
    pub fn structure_at(&self, x: i32, y: i32) -> Option<usize> {
        let c = (x.div_euclid(CELL), y.div_euclid(CELL));
        self.structures.iter().position(|s| s.alive() && s.covers(c))
    }

    /// Satellite protection (`0x409f50`): on a protected building a recently attacked flag plus an expired 240-tick timer
    /// destroys every foreign Advancer above the building; the flag is cleared on every pass.
    /// One building's check, in its owner's building pass (`0x40e38e` in `0x40de60`).
    pub(super) fn satellite_check(&mut self, i: usize) {
        {
            let s = &mut self.structures[i];
            if !s.protected || !s.alive() {
                return;
            }
            if s.sat_cd > 0 {
                s.sat_cd -= 1;
            }
            let strike = s.attacked && s.sat_cd <= 0;
            if strike {
                s.sat_cd = SATELLITE_TICKS;
            }
            s.attacked = false;
            if strike {
                let owner = side(s.owner);
                let s = self.structures[i].clone_footprint();
                let mut shots = Vec::new();
                for u in self
                    .units
                    .iter_mut()
                    .filter(|u| u.kind == kinds::ADVANCER && u.hp > 0 && s.covers(u.cell()) && (if u.player == 0 { 0 } else { 1 }) != owner)
                {
                    u.hp = 0;
                    shots.push((u.px(), u.player.min(1)));
                }
                // the laser bullet (type 10, `effects::SATLASER_*`): purely visual, the Advancer is already dead
                for ((x, y), player) in shots {
                    let lift = opensow_formats::anim::HOVER_ALTITUDE;
                    if let Some(path) = build_path(10, x, y + lift + SATLASER_START_DY, x, y + lift + SATLASER_END_DY, 0) {
                        if !path.points.is_empty() {
                            self.push_bullet(Bullet::new(10, SATLASER_STRENGTH, player, path));
                        }
                    }
                }
            }
        }
    }

    /// Advancer beams: once the Advancer hovers over the target cell it shoots every `ADVANCER_BEAM_TICKS`; each shot
    /// moves `2 * rate` hit points (destroy / repair, `0x40ffe9`) or adds `rate` upgrade points (`0x40e3d0`). New
    /// buildings heal in at `HEAL_PER_TICK`.
    /// One unit's part of it, at the end of its turn (the Advancer's chassis tick `0x46f210..0x46f321`).
    pub(super) fn advancer_action(&mut self, i: usize) {
        {
            if self.units[i].kind == kinds::ADVANCER && self.units[i].player == 0 {
                let u = &mut self.units[i];
                if u.action.is_none() && u.path.is_empty() && u.hp > 0 {
                    u.idle += 1;
                    if u.idle >= 420 {
                        u.idle = -840;
                        self.last_event = Some((u.x as i32, u.y as i32)); // `0x46f30a`
                        self.log.push((self.tick, msg::ADVANCER_DISENGAGED));
                    }
                } else {
                    u.idle = 0;
                }
            }
            // `0x46f210` ticks the cooldown member first, every tick, whatever the unit does (`0x46f321`)
            self.units[i].beam.tick();
            let Some((action, t)) = self.units[i].action else { return };
            let u = &self.units[i];
            let side_u = if u.player == 0 { 0 } else { 1 };
            let still_valid = self.action_valid(u, action, t);
            let target_cell = self.structures.get(t).map(|s| s.action_cell());
            // a new move order (path set elsewhere) or an invalid target cancels the action
            if !still_valid || target_cell.is_none() || (u.path.is_empty() && u.cell() != target_cell.unwrap()) {
                self.units[i].action = None;
                return;
            }
            if u.cell() != target_cell.unwrap() || !u.path.is_empty() {
                return;
            }
            if !self.units[i].beam.ready() {
                return;
            }
            self.units[i].beam.fire();
            let rate = self.rates[side_u];
            // the beam's sound (`0x4111b0`: destroy 53, upgrade 58, repair 57) on every third shot
            self.units[i].beam_shots += 1;
            if self.units[i].beam_shots % 3 == 1 {
                let id = match action {
                    Action::Destroy => 53,
                    Action::Upgrade => 58,
                    Action::Repair => 57,
                };
                let (bx, by) = self.structures[t].center();
                self.sound(id, bx, by, false);
            }
            // the shot: a bullet of type 19 / 20 / 21 from the hovering Advancer straight down to the building's cell (`0x47cf40`)
            let (sx, sy) = (self.units[i].x as i32, self.units[i].sprite_y() as i32);
            let (cx, cy) = self.structures[t].action_cell();
            let (ex, ey) = (cx * CELL + CELL / 2, cy * CELL + CELL / 2);
            if let Some(path) = build_path(action as u8, sx, sy, ex, ey, 0) {
                let mut frags = Vec::new();
                for k in 0..fragment_count(action) {
                    let mut rnd = |n: u32| self.random(n);
                    frags.push(fragment(action, k, &mut rnd));
                }
                self.push_beam(BeamShot { action, target: t, side: side_u, rate, path, index: 0, trail: 2, frags, slot: u32::MAX });
            }
        }
    }

    /// Flies the Advancers' beam bullets (speed 60: one step per tick). The upgrade bullet drops two yellow sparks every
    /// second step (`0x411993`); on landing the action happens (`0x40f590`).
    /// Side `p`'s shots this tick; the ones that arrived are removed and returned with their slots for [`Self::beam_lands`].
    pub(super) fn fly_beams(&mut self, p: usize) -> Vec<(u32, BeamShot)> {
        let mut landed = Vec::new();
        let mut trails = Vec::new();
        for (n, b) in self.beams.iter_mut().enumerate() {
            if b.side.min(2) != p {
                continue;
            }
            let (x, y) = b.path.points[b.index.min(b.path.points.len() - 1)];
            if b.action == Action::Upgrade {
                b.trail -= 1;
                if b.trail <= 0 {
                    b.trail = 2;
                    for &(fx, fy, frame) in &b.frags {
                        trails.push(Effect {
                            x: x + fx,
                            y: y + fy,
                            spec: AnimSpec { array: BODKY_Y, from: frame, to: frame + 1, period: 11, repeat: 1, mode: 6 },
                            age: 0,
                        });
                    }
                }
            }
            b.index += 1;
            if b.index >= b.path.points.len() {
                landed.push(n);
            }
        }
        self.effects.extend(trails);
        let mut out: Vec<(u32, BeamShot)> = landed.iter().rev().map(|&n| self.beams.remove(n)).map(|b| (b.slot, b)).collect();
        out.reverse();
        out
    }

    /// An Advancer shot arrives at its building: repairs, upgrades or damages it.
    pub(super) fn beam_lands(&mut self, b: BeamShot) {
        let (x, y) = *b.path.points.last().unwrap();
        let t = b.target;
        // the target is looked up by its id (`[bullet+0x2688]`): a building that changed hands meanwhile is a new object
        let me = if b.side == 0 { Owner::Player } else { Owner::Enemy };
        let valid = self.structures.get(t).is_some_and(|s| s.alive() && (b.action == Action::Destroy || s.owner == me));
        if !valid {
            return;
        }
        let (mut upgraded, mut repaired) = (false, false);
        let s = &mut self.structures[t];
        match b.action {
            Action::Destroy => {
                s.attacked = true;
                s.hp -= hp_per_event(b.rate);
                s.tally[b.side] += hp_per_event(b.rate);
                // one red spark animation at the landing point (`0x410342`)
                self.effects.push(Effect { x, y, spec: AnimSpec { array: BODKY_R, from: 6, to: 16, period: 2, repeat: 1, mode: 6 }, age: 0 });
                if self.structures[t].hp <= 0 {
                    self.structure_destroyed(t);
                }
            }
            Action::Repair => {
                // beams still in flight when the building reaches full hit points land on a full building: one message only
                let was_full = s.hp >= s.max_hp;
                s.hp = (s.hp + hp_per_event(b.rate)).min(s.max_hp);
                s.tally[b.side] = (s.tally[b.side] - hp_per_event(b.rate)).max(0);
                repaired = !was_full && s.hp >= s.max_hp;
                // a blue spark per fragment (`0x410063`)
                for &(fx, fy, _) in &b.frags {
                    self.effects.push(Effect {
                        x: x + fx,
                        y: y + fy,
                        spec: AnimSpec { array: BODKY_B, from: 6, to: 9, period: 4, repeat: 1, mode: 6 },
                        age: 0,
                    });
                }
            }
            Action::Upgrade => {
                if s.active < s.slots.len() {
                    s.upgrade += upgrade_per_event(b.rate);
                    if let Some(slot) = s.slots.get(s.active) {
                        if s.upgrade >= slot.unlock_points() as i32 {
                            s.active += 1;
                            s.upgrade = 0;
                            upgraded = true;
                        }
                    }
                }
            }
        }
        let turret = matches!(self.structures[t].kind, ObjectKind::Turret(_));
        if upgraded {
            let (x, y) = self.structures[t].center(); // `0x40e445`
            self.say_at(b.side, msg::BUILDING_UPGRADED, x, y);
        }
        if repaired {
            // `0x410150` passes the Advancer's y (`[unit+0x18]`) for both x and y
            let y = self.units.iter().find(|u| u.kind == kinds::ADVANCER && u.player as usize == b.side).map_or(0, |u| u.y as i32);
            self.say_at(b.side, if turret { msg::TURRET_REPAIRED } else { msg::BUILDING_REPAIRED }, y, y);
        }
    }
}
