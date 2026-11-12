//! What is left when something dies: building destruction bursts, corpses, wrecks and nuclear blasts.

use super::*;

impl World {
    /// Test/headless helper: the structure covering `cell` loses all its hit points.
    pub fn debug_destroy_at(&mut self, cx: i32, cy: i32) {
        if let Some(i) = self.structures.iter().position(|s| s.alive() && s.covers((cx, cy))) {
            self.structures[i].hp = 0;
            self.structure_destroyed(i);
        }
    }

    /// One burst of a dying building's destruction animation (`0x409b60`): a blast at a random point of the footprint with area
    /// damage, the sound, and three bullets of type 14 and three of type 13 (strength 80) that fly off on random curves. Draw order:
    /// the visual `rand(7)` first (sound 24 once in seven), then lockstep `rand(15)`, x, y; the bullets draw their own numbers
    /// (`curves::RandomBullet::draw`), and the last lockstep draw `rand(15)` sets the passes until the next burst (returned).
    pub(super) fn destruction_burst(&mut self, i: usize, turret: bool) -> i32 {
        let (cell, (w, h), owner) = (self.structures[i].cell, self.structures[i].size(), self.structures[i].owner);
        let owner = side(owner) as i32;
        let (cx, cy) = (cell.0 * CELL, cell.1 * CELL);
        if self.fx_random(7) == 2 {
            self.sound(24, cx, cy, false);
        }
        let pick = self.random(15);
        let x = self.random((w * CELL - 20).max(1) as u32) as i32 + cx + 10;
        let y = self.random((h * CELL - 20).max(1) as u32) as i32 + cy + 10;
        // turrets: strength 50 and `e_turret`; others 80 for a pick of 5 or less, else 50, and `e_bubg`
        let strength = if turret || pick > 5 { 50 } else { 80 };
        self.explode(x, y, 0, strength, owner);
        self.effects.push(Effect { x, y, spec: if turret { TURRET_DESTROYED } else { BUILDING_DESTROYED }, age: 0 });
        self.sound(25, cx, cy, false);
        for bt in [14u8, 14, 14, 13, 13, 13] {
            let Some(rb) = self.game_rng.as_mut().and_then(|r| RandomBullet::draw(bt, r)) else { break };
            let Some(path) = self.curves.as_ref().map(|c| c.path(rb.curve, x, y, 0)).filter(|p| !p.points.is_empty()) else { break };
            let mut b = Bullet::new(bt, 80, owner, path);
            b.speed = rb.speed;
            b.variant = rb.flag.unwrap_or(0);
            self.bullets.push(b);
        }
        BURST_GAP + self.random(BURST_JITTER) as i32
    }

    /// One pass of the destruction animation (`0x409b60` loop body): count the debris counter down; at 0 the animation counter
    /// goes down too, and either the next burst is thrown or the animation ends.
    pub(super) fn destruction_pass(&mut self, i: usize) {
        let Some((anim, next)) = self.structures[i].dying else { return };
        if next > 1 {
            self.structures[i].dying = Some((anim, next - 1));
        } else if anim <= 1 {
            self.finish_destruction(i);
        } else {
            let turret = matches!(self.structures[i].kind, ObjectKind::Turret(_));
            let gap = self.destruction_burst(i, turret);
            self.structures[i].dying = Some((anim - 1, gap));
        }
    }

    /// The animations of all dying buildings: the exe runs [`DESTRUCTION_PASSES`] passes per tick.
    pub(super) fn step_destruction(&mut self) {
        for i in 0..self.structures.len() {
            for _ in 0..DESTRUCTION_PASSES {
                if self.structures[i].dying.is_none() {
                    break;
                }
                self.destruction_pass(i);
            }
        }
    }

    /// A structure ran out of hit points (`0x409130`): its destruction animation starts (the first burst comes at once), the
    /// building stays on the map, dead, until the animation ends ([`Self::finish_destruction`]).
    pub(super) fn structure_destroyed(&mut self, i: usize) {
        if self.structures[i].dying.is_some() {
            return;
        }
        let owner = side(self.structures[i].owner);
        let turret = matches!(self.structures[i].kind, ObjectKind::Turret(_));
        let (cx, cy) = self.structures[i].center();
        self.quake.start(if turret { QUAKE_TURRET } else { QUAKE_BUILDING }, cx, cy);
        self.structures[i].dying = Some((destruction_bursts(self.structures[i].kind_id), 1));
        self.destruction_pass(i);
        // `0x409270` (lost / turret destroyed) passes the building's centre
        self.say_at(owner, if turret { msg::TURRET_DESTROYED } else { msg::BUILDING_LOST }, cx, cy);
    }

    /// The end of the destruction animation (`0x409ead`, then the player loop `0x42dc60` -> `0x43ab40`): turrets and
    /// headquarters are removed, every other building is rebuilt for the player with the largest foreign damage tally, starting
    /// at 1 hit point and healing up to its maximum.
    pub(super) fn finish_destruction(&mut self, i: usize) {
        self.structures[i].dying = None;
        let s = &self.structures[i];
        let owner = side(s.owner);
        // 0x43ab40..0x43ac38: with nobody else's damage on it the building goes to player 1 (single player: `[0x575720] == 0`)
        let winner = if removed_when_destroyed(s.kind_id) { None } else { Some(capture_winner(s.tally, owner).unwrap_or(1)) };
        let turret = matches!(self.structures[i].kind, ObjectKind::Turret(_));
        self.stats[owner][if turret { 5 } else { 1 }] += 1; // 0x409ee8 / 0x409ef0
        if let (Some(p), false) = (winner, turret) {
            self.stats[p.min(2)][0] += 1; // the capture builds the building again for its new owner (0x43a068)
        }
        let (cx, cy) = self.structures[i].center();
        // `0x43ad46` (captured) passes the building's centre
        if let Some(p) = winner {
            self.say_at(p, msg::BUILDING_CAPTURED, cx, cy);
        }
        // the rebuild (`0x43ab40`) frees the old handle slot and takes the first free one of the new owner's table
        self.slot_free(owner, i);
        match winner {
            Some(p) => {
                self.slot_alloc(p.min(2), i);
                let s = &mut self.structures[i];
                s.owner = match p {
                    0 => Owner::Player,
                    1 => Owner::Enemy,
                    _ => Owner::Neutral,
                };
                s.hp = 1;
                s.heal = s.max_hp - 1;
                s.tally = [0; 3];
                s.upgrade = 0;
                // the rebuild (`0x43ab40` -> constructor `0x40d870`) starts every slot from its base kind (`[slot+8]`) with no bought
                // tech-levels and the building at its default upgrade level 1 (`0x40a3d1`); only a neutral building's level is
                // copied back (`0x43ad00`: old owner 2), so capturing from either side drops the upgrades
                if owner != 2 {
                    s.active = s.active.min(1);
                }
                for sl in s.slots.iter_mut() {
                    sl.kind -= sl.level as i32;
                    sl.level = 0;
                    if let Some(t) = slot_target(sl.kind) {
                        sl.target = t;
                    }
                }
                s.target = None;
                s.rally = None; // the exe rebuilds the building (`0x43ab40`): the constructor clears the point
                // the new object's constructor (`0x439e30` -> `0x40a330`) zeroes the satellite word `[b+0x114]` (`0x40a3ef`; only
                // an HQ gets 1, and HQs are removed, not captured) and nothing copies it back: a captured building is unprotected
                s.protected = false;
                s.attacked = false;
                s.sat_cd = 0;
                s.slots.iter_mut().for_each(|sl| sl.progress = 0);
            }
            None => {
                for c in self.structures[i].blocked_cells() {
                    if let Some(b) = self.cell_index(c.0, c.1) {
                        self.blocked[b] = false;
                    }
                }
            }
        }
    }

    /// Corpses (`0x46b600`): the timer runs down; below [`CORPSE_BLAST_BELOW`] the final blasts go off once; below 0 the slot is freed.
    pub(super) fn step_corpses(&mut self) {
        let mut corpses = std::mem::take(&mut self.corpses);
        for c in corpses.iter_mut() {
            c.left -= 1;
            if c.left < CORPSE_BLAST_BELOW && !c.blasted {
                c.blasted = true;
                for &size in corpse_blasts(c.variant) {
                    let (dy, dx) = (self.random(30) as i32 - 15, self.random(30) as i32 - 15);
                    self.effects.push(Effect { x: c.x + dx, y: c.y + dy, spec: blast(size), age: 0 });
                }
            }
        }
        corpses.retain(|c| c.left >= 0);
        self.corpses = corpses;
        self.occ = None;
    }

    /// Burning wrecks (`0x45b0a0`): remove at 0 columns left, else count down and throw a rising smoke bullet every 20 steps.
    pub(super) fn step_wrecks(&mut self) {
        let mut wrecks = std::mem::take(&mut self.wrecks);
        wrecks.retain_mut(|w| {
            if w.left <= 0 {
                return false;
            }
            w.timer -= 1;
            if w.timer <= 0 {
                w.left -= 1;
                w.timer = WRECK_SMOKE_PERIOD;
                let (dx, dy) = (80 + self.fx_random(80) as i32, 80 + self.fx_random(40) as i32);
                if let Some(path) = build_path(WRECK_SMOKE_TYPE, w.x, w.y, w.x + dx, w.y - dy, 0) {
                    if !path.points.is_empty() {
                        self.bullets.push(Bullet::new(WRECK_SMOKE_TYPE, 0, 0, path));
                    }
                }
            }
            true
        });
        self.wrecks = wrecks;
    }

    /// Advances the explosions (`0x411220`, type 35).
    pub(super) fn step_nukes(&mut self) {
        use opensow_formats::rogon::Explosion;
        let words: Vec<i32> = self.rings.offsets.iter().flat_map(|&(dx, dy)| [dx as i32, dy as i32]).collect();
        // the list used goes up to ring 7: `[0x6b3a34]` = 2 * (offsets up to and including ring 7)
        let limit = Explosion::limit(&self.rings.ends);
        let mut blasts: Vec<(i32, i32, i32, i32, bool)> = Vec::new();
        self.nukes.retain_mut(|n| {
            let (list, alive) = n.ex.step(&words, limit);
            for b in list {
                blasts.push((n.x + 16 + b.dx * CELL, n.y + 16 + b.dy * CELL, b.strength, n.owner, b.sound));
            }
            alive
        });
        // per blast, in the exe's order (0x411310..0x411442): the rumble every 17th blast (the walker nuke 19, now and then the
        // fighter missile's boom 67), then a `e_unit2` cloud (the unit-death cloud, not a blast sprite) at the centre moved by
        // rand(32) - 16 on y (first draw) and x, then the area damage
        for (x, y, strength, owner, sound) in blasts {
            if sound {
                let id = if self.random(4) == 1 { 67 } else { 19 };
                self.sound(id, x, y, true);
            }
            let (dy, dx) = (self.random(32) as i32 - 16, self.random(32) as i32 - 16);
            self.effects.push(Effect { x: x + dx, y: y + dy, spec: UNIT_CLOUD, age: 0 });
            self.explode(x, y, 35, strength, owner);
        }
    }
}
