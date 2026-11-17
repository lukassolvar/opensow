//! Weapons: target search, turrets and their muzzles, firing, bullets in flight and explosions.

use super::*;

/// A landed bullet: position, type, strength, owner, target.
pub(super) type Landed = (i32, i32, u8, i32, i32, Option<HitTarget>);

impl World {
    /// The spawner's end-point scatter for one round (`0x47cfe1..0x47d03f`): per axis (x, then y) `random(2)` picks the sign
    /// (1 = plus) and `random(spread)` the size; `spread` is `aim::spread(kind)`. Single player only (remote games skip it).
    pub(super) fn volley_scatter(&mut self, kind: i32) -> (i32, i32) {
        let spread = opensow_formats::aim::spread(kind);
        let axis = |w: &mut World| {
            let plus = w.random(2) == 1;
            let m = w.random(spread) as i32;
            if plus {
                m
            } else {
                -m
            }
        };
        let jx = axis(self);
        let jy = axis(self);
        (jx, jy)
    }

    /// Every living unit and structure by the cells it covers, rebuilt once per tick for the target searches. The exe's scan
    /// (`0x47b140`) reads the static grid `0x5eac38` cell by cell (`0x47b206`), where a building's handle is stamped on every
    /// blocked cell of its footprint (`0x40e460`), so a building is in range as soon as any of its cells is (a range-1 walker
    /// next to a wall shoots it), not only its middle cell.
    pub(super) fn index(&self) -> HashMap<(i32, i32), Vec<Target>> {
        let mut m: HashMap<(i32, i32), Vec<Target>> = HashMap::new();
        for u in self.units.iter().filter(|u| u.hp > 0) {
            m.entry(u.cell()).or_default().push(Target::Unit(u.id));
        }
        for (i, s) in self.structures.iter().enumerate().filter(|(_, s)| s.alive()) {
            for c in s.blocked_cells() {
                m.entry(c).or_default().push(Target::Structure(i));
            }
        }
        m
    }

    /// Whether target `t` is an enemy of `player`'s weapons.
    pub(super) fn hostile(&self, player: i32, t: Target) -> bool {
        match t {
            Target::Unit(id) => self.unit(id).is_some_and(|u| u.hp > 0 && (u.player == 0) != (player == 0)),
            Target::Structure(i) => {
                // the exe's target search (0x47b140) takes every live building whose owner differs from the shooter's player,
                // so neutral buildings (owner 2) are valid targets for both sides
                let o = self.structures[i].owner;
                self.structures[i].alive() && if player == 0 { o != Owner::Player } else { o != Owner::Enemy }
            }
        }
    }

    /// The map pixel a weapon aims at for target `t`; `None` when the target is gone.
    pub(super) fn target_pos(&self, t: Target) -> Option<(i32, i32)> {
        match t {
            Target::Unit(id) => self.unit(id).map(|u| u.px()),
            Target::Structure(i) => Some(self.structures[i].center()),
        }
    }

    /// Target choice of the exe (`0x47b140`): cells are scanned ring by ring between the dead zone and the range; a unit wins
    /// over a turret, a turret over any other building. `order` is the unit's own order target (`[unit+0x7c]`, set by an attack
    /// order): the scan treats it as a hostile candidate (`0x47b232`: the handle equal to it counts as an enemy), and when it
    /// is among the objects in range the weapon takes it whatever else is there (`0x47b27c` / `0x47b2a2` set `bl`, `0x47b2b3..0x47b2c1`
    /// store it as the weapon target). Only an order target out of range leaves the priority rule above in charge.
    pub fn find_target(
        &self,
        index: &HashMap<(i32, i32), Vec<Target>>,
        player: i32,
        cell: (i32, i32),
        min: u32,
        max: u32,
        order: Option<Target>,
    ) -> Option<Target> {
        let (mut unit, mut turret, mut building) = (None, None, None);
        for &(dx, dy) in self.rings.target_search(min as usize, (max as usize).min(63)) {
            let Some(list) = index.get(&(cell.0 + dx as i32, cell.1 + dy as i32)) else { continue };
            for &t in list {
                if order == Some(t) && self.hostile(player, t) && !matches!(t, Target::Unit(id) if self.unit(id).is_some_and(|u| u.kind == kinds::ADVANCER)) {
                    return Some(t);
                }
                if !self.hostile(player, t) {
                    continue;
                }
                match t {
                    // flyers (the Advancer, kind 40, `[unit+0x4b]`) are stamped into the air grid 0x60ac38, which only
                    // anti-air shooters search (0x47ad30); the ground search 0x47b140 reads grid 0x5eac38 and never sees them
                    Target::Unit(id) if self.unit(id).is_some_and(|u| u.kind == kinds::ADVANCER) => {}
                    Target::Unit(_) => {
                        unit = unit.or(Some(t));
                    }
                    Target::Structure(i) if matches!(self.structures[i].kind, ObjectKind::Turret(_)) => turret = turret.or(Some(t)),
                    Target::Structure(_) => building = building.or(Some(t)),
                }
            }
            // the scan covers all rings while there is an order target to look for; otherwise the nearest unit ends it
            if unit.is_some() && order.is_none() {
                break;
            }
        }
        unit.or(turret).or(building)
    }

    /// The building pass (`0x40de60`) of a turret that has healed in creates its gun unit once (`0x40dfd0..0x40e04d`, for every
    /// owner: no owner test): it takes the first free slot of the owner's unit table and its initialiser `0x46bd30` draws the
    /// hover phase `rand(20)` (`0x46bf31`, unused by a gun) and seeds `[unit+0x8c]` = 240 + random(900) (`0x46c10c`).
    pub(super) fn create_gun(&mut self, i: usize) {
        let s = &self.structures[i];
        if !s.alive() || s.aim.is_none() || s.heal > 0 || self.turret_look.contains_key(&i) {
            return;
        }
        let slot = self.free_unit_slot(owner_index(s.owner));
        self.gun_slot.insert(i, slot);
        self.random(20);
        let seed = 240 + self.random(900) as i32;
        self.turret_look.insert(i, seed);
    }

    /// One tick of a turret's gun unit (its turn in the owner's unit pass): look around, search, aim, fire.
    pub(super) fn step_structure(&mut self, i: usize, index: &HashMap<(i32, i32), Vec<Target>>) {
        if !self.structures[i].alive() || self.structures[i].aim.is_none() || !self.turret_look.contains_key(&i) {
            // a turret gets its gun top only when it has finished healing in (0x40de60 creates it when [bld+0x68] turns 0)
            return;
        }
        let owner_player = if self.structures[i].owner == Owner::Player { 0 } else { 1 };
        // idle look-around (`0x46e114`, docs/art.md): a gun unit has no chassis, so its counter runs only while the top is idle
        // (`[top+0x48] == 0`, `0x46e12d`); at 0 it draws `rand(900)` and `rand(steps)` (`0x46e147`, `0x46e178`, both
        // unconditional) and takes the facing only when the top has no target. Neutral guns are units of player 2 and look
        // around too.
        if self.structures[i].aim.as_ref().is_some_and(|a| a.state == opensow_formats::aim::State::Idle) {
            let look = self.turret_look.get_mut(&i).expect("seeded above");
            *look -= 1;
            if *look <= 0 {
                let wait = 240 + self.random(900) as i32;
                let steps = self.structures[i].aim.as_ref().map_or(32, |a| a.params.steps);
                let face = self.random(steps);
                self.turret_look.insert(i, wait);
                if self.structures[i].target.is_none() {
                    self.structures[i].aim.as_mut().unwrap().set_target_facing(face);
                }
            }
        }
        if self.structures[i].owner == Owner::Neutral {
            return;
        }
        let (min, max, steps) = {
            let p = &self.structures[i].aim.as_ref().unwrap().params;
            (p.min_range, p.max_range, p.steps)
        };
        let cell = self.structures[i].cell;
        if self.structures[i].weapon == 110 {
            // the anti-air turret shoots aircraft, then an enemy Advancer (air-grid search)
            let (sx, sy) = self.structures[i].center();
            // the aim origin is the gun top's anchor: the turret unit's position plus its draw offset (`0x4797c0`: `[unit+0x14] + [top+0x5c]`, `[unit+0x18] + [top+0x60]`)
            let (ox, oy) = self.turret_anchor(i);
            // the search runs from the turret unit's position (its gun top's cell middle, `turret_top_pos`)
            let _ = (sx, sy);
            let pos = opensow_formats::buildings::turret_top_pos(self.structures[i].kind_id, self.structures[i].cell);
            let aim = self.structures[i].aim.as_ref().unwrap();
            let (poll, busy) = (aim.polls_now(), aim.state != opensow_formats::aim::State::Idle);
            let ai = if poll || busy { self.anti_air_aircraft((true, i), poll, owner_player, pos, min, max) } else { None };
            let adv = if ai.is_none() { self.advancer_in_range(index, owner_player, cell, min, max) } else { None };
            let apos = adv.and_then(|id| self.unit(id)).map(|u| u.flyer_points());
            let face = match (ai, apos) {
                (Some(a), _) => {
                    let (tx, ty) = self.aircraft_aim_point(a);
                    Some(facing(ox, oy, tx, ty, steps))
                }
                (None, Some(((tx, ty), _))) => Some(facing(ox, oy, tx, ty, steps)),
                _ => None,
            };
            // the reload jitter is drawn from the lockstep table when the search finds a target (`0x479cf3` -> `0x47c5f0`)
            let (table, state) = (&mut self.game_rng, &mut self.rng);
            let ev = self.structures[i].aim.as_mut().unwrap().tick(face, |n| draw(table, state, n));
            if let Some((barrel, _)) = ev.shot {
                // the rockets fly to the target (type 1, drawn as `missile`); the damage is taken off the target directly (events 1, 10, 31)
                let target = match (ai, apos) {
                    (Some(a), _) => Some(self.aircraft_aim_point(a)),
                    (None, p) => p.map(|(_, shot)| shot),
                };
                if let (Some(to), Some((bt, strength))) = (target, weapon_shot(110)) {
                    let facing_now = self.structures[i].aim.as_ref().unwrap().facing;
                    let from = self.turret_muzzle(i, 110, barrel as usize, facing_now, steps);
                    let handle = match (ai, adv) {
                        (Some(a), _) => Some(HitTarget::Aircraft(self.aircraft[a].id)),
                        (None, Some(id)) => Some(HitTarget::Unit(id)),
                        _ => None,
                    };
                    if let Some(h) = handle {
                        self.shoot_at(bt, strength as i32, owner_player, from, to, h);
                    }
                }
            }
            return;
        }
        let t = self.find_target(index, owner_player, cell, min, max, None);
        self.structures[i].target = t;
        let (sx, sy) = self.structures[i].center();
        let tpos = t.and_then(|t| self.target_pos(t));
        let (ox, oy) = self.turret_anchor(i);
        let face = tpos.map(|(tx, ty)| facing(ox, oy, tx, ty, steps));
        let weapon = self.structures[i].weapon;
        // the reload jitter is drawn from the lockstep table when the search finds a target (`0x479cf3` -> `0x47c5f0`)
        let (table, state) = (&mut self.game_rng, &mut self.rng);
        let ev = self.structures[i].aim.as_mut().unwrap().tick(face, |n| draw(table, state, n));
        if let (Some((barrel, shots)), Some(to)) = (ev.shot, tpos) {
            // each shot fires the `.po1` tables barrel..barrel+shots-1 (`recoil`), each from the muzzle point of the top's facing
            let facing_now = self.structures[i].aim.as_ref().unwrap().facing;
            for _ in 0..opensow_formats::aim::rounds(weapon) {
                // every round of the spawner scatters the end point (0x47cfe1..0x47d03f)
                let (jx, jy) = self.volley_scatter(weapon);
                let to = (to.0 + jx, to.1 + jy);
                for table in barrel as usize..barrel as usize + shots.max(1) as usize {
                    let from = self.turret_muzzle(i, weapon, table, facing_now, steps);
                    // the spawner ends the shot at target + (`.po2` point - `.po1` point) (`0x47d0a5..0x47d0cb`): the shots fly parallel
                    let anchor = self.turret_muzzle_anchor(i);
                    let to = if self.turret_muzzles.contains_key(&weapon) {
                        let reference = self
                            .turret_muzzles
                            .get(&(REFERENCE_KEY + weapon))
                            .and_then(|t| t.muzzle(0, (facing_now * 32 / steps.max(1)) as usize))
                            .unwrap_or((0, 0));
                        (to.0 + from.0 - anchor.0 - reference.0, to.1 + from.1 - anchor.1 - reference.1)
                    } else {
                        to
                    };
                    self.fire(weapon, owner_player, from, to);
                }
            }
            if let Some((id, jitter)) = opensow_formats::sfx::muzzle(weapon as u8) {
                self.sound(id, sx, sy, jitter);
            }
        }
    }

    /// The anchor of a turret's gun top: the unit's position plus the top's vertical draw offset.
    pub(super) fn turret_anchor(&self, i: usize) -> (i32, i32) {
        let st = &self.structures[i];
        let (kx, ky) = opensow_formats::buildings::turret_top_pos(st.kind_id, st.cell);
        (kx, ky + opensow_formats::buildings::turret_art(st.kind_id).map_or(0, |a| a.top_dy))
    }

    /// The gun top's anchor as [`World::turret_muzzle`] adds the `.po1` point to it.
    pub(super) fn turret_muzzle_anchor(&self, i: usize) -> (i32, i32) {
        let st = &self.structures[i];
        let (kx, ky) = opensow_formats::buildings::turret_top_pos(st.kind_id, st.cell);
        (kx, ky + opensow_formats::buildings::turret_art(st.kind_id).map_or(0, |a| a.top_dy))
    }

    /// Where a turret's shot starts: the gun top's anchor (the turret unit's position plus the draw offset `[top+0x60]`) minus the
    /// `.po1` point of the table and facing; without the tables a point 14 px along the facing.
    pub(super) fn turret_muzzle(&self, i: usize, weapon: i32, table: usize, facing_now: u32, steps: u32) -> (i32, i32) {
        let st = &self.structures[i];
        let (kx, ky) = opensow_formats::buildings::turret_top_pos(st.kind_id, st.cell);
        let dy = opensow_formats::buildings::turret_art(st.kind_id).map_or(0, |a| a.top_dy);
        if let Some((mx, my)) = self.turret_muzzles.get(&weapon).and_then(|t| t.muzzle(table, (facing_now * 32 / steps.max(1)) as usize)) {
            return (kx + mx, ky + dy + my);
        }
        let (sin, cos) = heading_degrees(facing_now, steps).to_radians().sin_cos();
        (kx + (sin * 14.0) as i32, ky - (cos * 14.0) as i32)
    }

    /// Hands out the next aircraft id.
    pub(super) fn new_air_id(&mut self) -> u32 {
        self.next_air_id += 1;
        self.next_air_id - 1
    }

    /// A targeted shot (bullet types 1 and 31, `[+0x2688]`): the bullet flies to `to` and the strength comes off `target` when it lands.
    /// A bullet with no path (start equals end) hits at once.
    pub(super) fn shoot_at(&mut self, bt: u8, strength: i32, owner: i32, from: (i32, i32), to: (i32, i32), target: HitTarget) {
        match build_path(bt, from.0, from.1, to.0, to.1, 0).filter(|p| !p.points.is_empty()) {
            Some(path) => {
                let mut b = Bullet::new(bt, strength, owner, path);
                b.target = Some(target);
                self.push_bullet(b);
            }
            None => self.hit_target(target, strength),
        }
    }

    /// `0x4108bc`: the strength comes off the unit or aircraft with the bullet's handle, if it is still there.
    pub(super) fn hit_target(&mut self, target: HitTarget, strength: i32) {
        match target {
            HitTarget::Unit(id) => {
                if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
                    u.hp -= strength;
                }
            }
            HitTarget::Aircraft(id) => {
                if let Some(a) = self.aircraft.iter_mut().find(|a| a.id == id) {
                    a.hp -= strength;
                }
            }
        }
    }

    /// Fires weapon kind `weapon` from one map pixel at another: creates its bullet.
    pub(super) fn fire(&mut self, weapon: i32, owner: i32, from: (i32, i32), to: (i32, i32)) {
        let Some((bt, strength)) = weapon_shot(weapon) else { return };
        // the constructor of some types plays a sound at the start point, once per bullet (special tiers 2/3: 8 / 9, `0x412299`,
        // `0x4121ed`); it runs before the path is built, so a shot too short for a path still sounds
        if let Some(id) = opensow_formats::effects::constructor_sound(bt) {
            self.sound(id, from.0, from.1, true);
        }
        let Some(path) = build_path(bt, from.0, from.1, to.0, to.1, 0) else { return };
        if path.points.is_empty() {
            return;
        }
        self.push_bullet(Bullet::new(bt, strength as i32, owner, path));
    }

    /// One tick of every bullet: flight, trails, impact and damage.
    /// The flight part of owner `p`'s bullets this tick (`0x411220`): they move and leave their trails; the ones at the end
    /// of their path are removed and returned with their slots for [`Self::land_bullet`]. The flight draws only from the
    /// visual generator, so flying them all before the impacts keeps the lockstep order.
    pub(super) fn fly_bullets(&mut self, p: usize) -> Vec<(u32, Landed)> {
        let t = (self.tick % 60) as u32;
        let mut landed: Vec<(u32, Landed)> = Vec::new();
        let mut puffs: Vec<(i32, i32, AnimSpec)> = Vec::new();
        let mut smoke: Vec<(i32, i32, opensow_formats::effects::Trail)> = Vec::new();
        self.bullets.retain_mut(|b| {
            if b.owner.clamp(0, 2) as usize != p {
                return true;
            }
            let steps = steps_at(b.speed, t);
            if b.bullet_type == 18 {
                b.speed += steps; // the bomb accelerates by the steps of every tick (`0x41146d`: `[+0x2664] += steps`)
            }
            for _ in 0..steps {
                b.index += 1;
                // shells of types 1, 3, 4, 24, 25 leave smoke (`effects::trail`), also not on the landing step
                if let Some(tr) = shell_trail(b.bullet_type) {
                    if b.index < b.path.points.len() {
                        b.trail -= 1;
                        if b.trail <= 0 {
                            b.trail = tr.reload;
                            let (x, y) = b.path.points[b.index - 1];
                            smoke.push((x, y, tr));
                        }
                    }
                }
                // flamer bullets (types 8, 9) and the sparks of types 27 and 31 (the Advancer gun's blue/green beam): a puff every `interval` path steps, none on the landing step (0x4118e2)
                if let Some(spec) = flame_trail(b.bullet_type).or_else(|| spark_trail(b.bullet_type)).or_else(|| gun_trail(b.bullet_type, b.owner)) {
                    if b.index < b.path.points.len() {
                        b.trail -= 1;
                        if b.trail <= 0 {
                            b.trail = bullet_type_interval(b.bullet_type);
                            // the exe stores the step's point [k] as the position, then moves its index past it: point index - 1
                            let (x, y) = b.path.points[b.index - 1];
                            puffs.push((x, y, spec));
                        }
                    }
                }
            }
            if let Some(a) = &mut b.anim {
                if !a.step(b.bullet_type, 2 * b.index as i32) {
                    return false; // a one-shot animation (types 22, 23) has played out
                }
            }
            if b.index >= b.path.points.len() {
                let (x, y) = *b.path.points.last().unwrap();
                landed.push((b.slot, (x, y, b.bullet_type, b.strength, b.owner, b.target)));
                false
            } else {
                true
            }
        });
        for (x, y, spec) in puffs {
            self.effects.push(Effect { x, y, spec, age: 0 });
        }
        for (x, y, tr) in smoke {
            // the y offset is drawn first, then x (`0x411565`, `0x41156f`)
            let dy = tr.offset + self.fx_random(tr.jitter) as i32;
            let dx = tr.offset + self.fx_random(tr.jitter) as i32;
            self.effects.push(Effect { x: x + dx, y: y + dy, spec: tr.spec, age: 0 });
        }
        landed
    }

    /// A bullet at the end of its path (`0x40f590`): impact sprites, sound, the hit or the area damage.
    pub(super) fn land_bullet(&mut self, (x, y, bt, strength, owner, target): Landed) {
        {
            if bt == 10 {
                return; // the satellite laser only hits its target handle (0x4108bc): no area damage, decal or sound
            }
            for spec in impact_anims(bt) {
                self.effects.push(Effect { x, y, spec, age: 0 });
            }
            self.impact_ground_effects(x, y, bt);
            // the impact function's sound (`0x40f590`); the bomber's bomb (18) only half of the time, the meteors with pitch jitter
            if let Some(id) = opensow_formats::effects::impact_sound(bt) {
                if bt != 18 || self.random(2) == 0 {
                    self.sound(id, x, y, matches!(bt, 24 | 25));
                }
            }
            if matches!(bt, 1 | 31) {
                // the targeted hits (0x4108bc): only the shooter's target loses the strength, nothing else is touched
                if let Some(t) = target {
                    self.hit_target(t, strength);
                }
                return;
            }
            self.explode(x, y, bt, strength, owner);
            if bt == 17 {
                // a Tripler pod: 34 blasts of 100 on an ellipse around the impact, each jittered by -5..4 px (0x40fd40..0x40ff44)
                // Every point also makes a small blast sprite (`0x405240(.., 1)`, `0x40fdae`) and, on a free cell that is not "no marks",
                // two smoke puffs (`0x40fe18..0x40ff16`); the jitter and the puff offsets come from the visual generator `0x437cd0`.
                for &(dx, dy) in &TRIPLER_RING[..TRIPLER_RING_USED] {
                    let (jx, jy) = (self.fx_random(10) as i32 - 5, self.fx_random(10) as i32 - 5);
                    let (px, py) = (x + dx + jx, y + dy + jy);
                    self.effects.push(Effect { x: px, y: py, spec: blast(1), age: 0 });
                    self.explode(px, py, 17, TRIPLER_BLAST_STRENGTH, owner);
                    let cell = (px.div_euclid(CELL), py.div_euclid(CELL));
                    if !self.is_blocked(cell.0, cell.1) && !self.is_no_marks(cell.0, cell.1) {
                        for _ in 0..2 {
                            let (oy, ox) = (self.fx_random(20) as i32 - 10, self.fx_random(20) as i32 - 10);
                            self.puff(px + ox, py + oy);
                        }
                    }
                }
            }
        }
    }

    /// Area damage (`0x42f2c0`): ground units within 32 px take the full strength (friendly fire included), a structure takes it
    /// when the blast lies in its cell.
    pub(super) fn explode(&mut self, x: i32, y: i32, bt: u8, strength: i32, owner: i32) {
        if strength <= 0 {
            return;
        }
        let refs: Vec<UnitRef> =
            self.units.iter().map(|u| UnitRef { x: u.px().0, y: u.px().1, flying: u.kind == kinds::ADVANCER, owner: u.player as u32, kind: u.kind }).collect();
        // the Advancer is the one flyer (`[unit+0x4b]`) and `0x42f30e` skips flyers
        for v in splash_victims(&refs, x, y, bt, owner as u32) {
            self.units[v].hp -= strength;
        }
        let cell = (x.div_euclid(CELL), y.div_euclid(CELL));
        let mut destroyed = Vec::new();
        // A finished turret's gun top is a ground unit (kinds 109..113): within 32 px of the blast it does not lose hit points
        // itself, the turret does, the full strength for every bullet type (`0x42f346..0x42f38d`: `0x43a910(top owner handle)`,
        // `[bld+0x74] -= strength`, tally `[bld+0x7c + owner*4] += strength`). This comes on top of the cell test below, so a hit on
        // the turret's own cell does both.
        for (i, s) in self.structures.iter_mut().enumerate() {
            if s.alive() && s.aim.is_some() && s.heal == 0 && matches!(s.kind, ObjectKind::Turret(_)) {
                let (tx, ty) = opensow_formats::buildings::turret_top_pos(s.kind_id, s.cell);
                if opensow_formats::projectile::distance(x, y, tx, ty) < opensow_formats::projectile::SPLASH_RADIUS {
                    s.hp -= strength;
                    s.tally[owner.clamp(0, 1) as usize] += strength;
                    if s.hp <= 0 && !destroyed.contains(&i) {
                        destroyed.push(i);
                    }
                }
            }
        }
        for (i, s) in self.structures.iter_mut().enumerate() {
            // no owner check, as in the exe (0x42f3cc..0x42f462): buildings take friendly fire too
            if s.alive() && s.covers(cell) {
                // half damage for the meteors and the Rogon blast, none on a Headquarters for the air-strike types (0x42f3cc..0x42f462)
                let d = opensow_formats::projectile::building_damage(bt, strength, s.kind_id);
                s.hp -= d;
                s.tally[owner.clamp(0, 1) as usize] += d;
                if s.hp <= 0 && !destroyed.contains(&i) {
                    destroyed.push(i);
                }
            }
        }
        for i in destroyed {
            self.structure_destroyed(i);
        }
    }
}
