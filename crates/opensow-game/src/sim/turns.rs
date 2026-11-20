//! The exe's per-player turn (`0x42da61..0x42ddfe`, docs/ai.md "Update order"): for player 0, 1 and 2 in turn, the input or the
//! computer (`0x42dc22`), then that player's buildings (`0x40d9a0`, `0x40de60`: heal-in, destruction animation, production,
//! the turret's gun unit), aircraft (`0x467b50`, table `0x5ba910`), units and turret guns (`0x46df40`, table `0x5b62c0`; a dead
//! unit is removed at its turn, `0x43a7e0`) and bullets (`0x411220`, table `0x5b91a0`). Every table is walked in slot order
//! and a new object takes the first free slot, so the lockstep draws come in the exe's order.

use super::*;

/// A unit-table entry: a unit, or the gun unit of a turret (by structure index).
#[derive(Clone, Copy)]
enum Mover {
    Unit(u32),
    Gun(usize),
    /// A dead unit that still holds its slot (by index in `World::corpses`).
    Corpse(usize),
}

/// The table index (0 player, 1 computer, 2 neutral) of an owner.
pub(super) fn owner_index(o: Owner) -> usize {
    match o {
        Owner::Player => 0,
        Owner::Enemy => 1,
        _ => 2,
    }
}

/// The lowest slot not in `used`.
fn first_free(used: impl Iterator<Item = u32>) -> u32 {
    let mut used: Vec<u32> = used.filter(|&s| s != u32::MAX).collect();
    used.sort_unstable();
    used.dedup();
    used.iter().enumerate().find(|&(n, &s)| n as u32 != s).map_or(used.len() as u32, |(n, _)| n as u32)
}

impl World {
    /// The first free slot of player `p`'s unit table (`0x43a50c`), which its units and turret guns share.
    pub(super) fn free_unit_slot(&self, p: usize) -> u32 {
        let units = self.units.iter().filter(|u| u.player.clamp(0, 2) as usize == p).map(|u| u.slot);
        let guns = self.gun_slot.iter().filter(|(k, _)| self.structures.get(**k).is_some_and(|s| owner_index(s.owner) == p)).map(|(_, &s)| s);
        let corpses = self.corpses.iter().filter(|c| c.player.clamp(0, 2) as usize == p).map(|c| c.slot);
        first_free(units.chain(guns).chain(corpses))
    }

    /// One player's turn of a game tick.
    pub(super) fn player_turn(&mut self, p: usize, index: &HashMap<(i32, i32), Vec<Target>>) {
        let frozen = self.cheats.freeze_enemy && p == 1;
        if self.ai && !self.cheats.freeze_enemy {
            if p == 1 {
                self.step_ai(1);
            } else if p == 0 && self.spectate {
                self.step_ai_spectate();
            }
        }
        // buildings, in the owner's slot order
        let mut order: Vec<usize> = self.slot_tables[p].iter().flatten().copied().collect();
        let rest: Vec<usize> = (0..self.structures.len()).filter(|k| owner_index(self.structures[*k].owner) == p && !order.contains(k)).collect();
        order.extend(rest);
        for k in order {
            if owner_index(self.structures[k].owner) != p {
                continue; // captured earlier in this turn
            }
            // the pending heal is paid out at 150 hp per tick (`0x40de60`)
            let s = &mut self.structures[k];
            if s.heal > 0 && s.hp > 0 {
                let h = s.heal.min(HEAL_PER_TICK);
                s.hp += h;
                s.heal -= h;
            }
            // the destruction animation: the exe runs `DESTRUCTION_PASSES` passes per tick
            for _ in 0..DESTRUCTION_PASSES {
                if self.structures[k].dying.is_none() {
                    break;
                }
                self.destruction_pass(k);
            }
            if !frozen {
                self.step_production(k);
            }
            self.create_gun(k);
            // satellite protection (`0x409f50`, called at `0x40e38e`)
            self.satellite_check(k);
        }
        self.aircraft_turn(p);
        // units and turret guns, in slot order
        // units without a slot yet (a loaded snapshot) take theirs now, in creation order
        for i in 0..self.units.len() {
            if self.units[i].slot == u32::MAX {
                self.units[i].slot = self.free_unit_slot(self.units[i].player.clamp(0, 2) as usize);
            }
        }
        let mut movers: Vec<(u32, Mover)> = self.units.iter().filter(|u| u.player.clamp(0, 2) as usize == p).map(|u| (u.slot, Mover::Unit(u.id))).collect();
        movers.extend(
            self.gun_slot.iter().filter(|(k, _)| self.structures.get(**k).is_some_and(|s| owner_index(s.owner) == p)).map(|(&k, &s)| (s, Mover::Gun(k))),
        );
        for n in 0..self.corpses.len() {
            if self.corpses[n].slot == u32::MAX && self.corpses[n].player.clamp(0, 2) as usize == p {
                self.corpses[n].slot = self.free_unit_slot(p);
            }
        }
        movers.extend(self.corpses.iter().enumerate().filter(|(_, c)| c.player.clamp(0, 2) as usize == p).map(|(n, c)| (c.slot, Mover::Corpse(n))));
        movers.sort_by_key(|m| m.0);
        let corpses_before = self.corpses.len();
        for (_, m) in movers {
            match m {
                Mover::Unit(id) => {
                    let Some(i) = self.units.iter().position(|u| u.id == id) else { continue };
                    if self.units[i].hp <= 0 {
                        self.unit_death(i);
                        self.units.remove(i);
                    } else {
                        if !frozen {
                            self.step_unit(i, index);
                        }
                        // the Advancer's beam (its chassis tick `0x46f210`; every unit's beam cooldown member ticks)
                        self.advancer_action(i);
                    }
                }
                Mover::Gun(k) => {
                    if !frozen {
                        self.step_structure(k, index);
                    }
                }
                Mover::Corpse(n) => self.step_corpse(n),
            }
        }
        // a unit that died this turn became a corpse at once; it ticks in this turn as well, as before
        for n in corpses_before..self.corpses.len() {
            self.step_corpse(n);
        }
        // a corpse whose time is up frees its slot (`0x43a7e0`) and clears its cell in the static grid (`0x46a230`)
        let mut freed = Vec::new();
        self.corpses.retain(|c| {
            let keep = c.player.clamp(0, 2) as usize != p || c.left >= 0;
            if !keep {
                freed.push(*c);
            }
            keep
        });
        if let Some(occ) = self.occ.as_mut() {
            for c in freed {
                if let Some(at) = movement::grid_index((c.x.div_euclid(CELL), c.y.div_euclid(CELL))) {
                    if c.air {
                        occ.air_stat[at] = 0
                    } else {
                        occ.stat[at] = 0
                    }
                }
            }
        }
        self.bullet_turn(p);
    }

    /// Player `p`'s aircraft, in slot order (a new one takes the first free slot of the side's table).
    fn aircraft_turn(&mut self, p: usize) {
        let all = std::mem::take(&mut self.aircraft);
        let (mut mine, others): (Vec<_>, Vec<_>) = all.into_iter().partition(|a| a.side.clamp(0, 2) as usize == p);
        for n in 0..mine.len() {
            if mine[n].slot == u32::MAX {
                mine[n].slot = first_free(mine.iter().map(|a| a.slot));
            }
        }
        mine.sort_by_key(|a| a.slot);
        self.aircraft = mine;
        self.step_aircraft();
        let mut after = std::mem::replace(&mut self.aircraft, others);
        self.aircraft.append(&mut after);
    }

    /// The first free slot of owner `p`'s bullet table (`0x43a147`), which its bullets, Advancer shots and bomb events share.
    fn free_bullet_slot(&self, p: usize) -> u32 {
        let bullets = self.bullets.iter().filter(|b| b.owner.clamp(0, 2) as usize == p).map(|b| b.slot);
        let beams = self.beams.iter().filter(|b| b.side.min(2) == p).map(|b| b.slot);
        let nukes = self.nukes.iter().filter(|n| n.owner.clamp(0, 2) as usize == p).map(|n| n.slot);
        first_free(bullets.chain(beams).chain(nukes))
    }

    /// Adds a bullet in the first free slot of its owner's table.
    pub fn push_bullet(&mut self, mut b: Bullet) {
        b.slot = self.free_bullet_slot(b.owner.clamp(0, 2) as usize);
        self.bullets.push(b);
    }

    /// Adds an Advancer shot (a bullet of type 19..21) in the first free slot of its side's table.
    pub(super) fn push_beam(&mut self, mut b: BeamShot) {
        b.slot = self.free_bullet_slot(b.side.min(2));
        self.beams.push(b);
    }

    /// Adds a bomb event (a bullet of type 35) in the first free slot of its owner's table.
    pub(super) fn push_nuke(&mut self, mut n: Nuke) {
        n.slot = self.free_bullet_slot(n.owner.clamp(0, 2) as usize);
        self.nukes.push(n);
    }

    /// Player `p`'s bullet table, in slot order (`0x411220` per object): every bullet and Advancer shot flies, and the ones at
    /// the end of their path land, and every bomb event takes its step, in slot order. Flights draw only from the visual
    /// generator, so the flights run first and the landings and bomb steps follow in slot order.
    fn bullet_turn(&mut self, p: usize) {
        // objects without a slot yet (a loaded snapshot) take theirs now
        for n in 0..self.bullets.len() {
            if self.bullets[n].slot == u32::MAX && self.bullets[n].owner.clamp(0, 2) as usize == p {
                self.bullets[n].slot = self.free_bullet_slot(p);
            }
        }
        for n in 0..self.beams.len() {
            if self.beams[n].slot == u32::MAX && self.beams[n].side.min(2) == p {
                self.beams[n].slot = self.free_bullet_slot(p);
            }
        }
        for n in 0..self.nukes.len() {
            if self.nukes[n].slot == u32::MAX && self.nukes[n].owner.clamp(0, 2) as usize == p {
                self.nukes[n].slot = self.free_bullet_slot(p);
            }
        }
        enum Ev {
            Land(Landed),
            Beam(Box<BeamShot>),
            Nuke(usize),
        }
        let mut evs: Vec<(u32, Ev)> = Vec::new();
        evs.extend(self.fly_bullets(p).into_iter().map(|(s, l)| (s, Ev::Land(l))));
        evs.extend(self.fly_beams(p).into_iter().map(|(s, b)| (s, Ev::Beam(Box::new(b)))));
        evs.extend(self.nukes.iter().enumerate().filter(|(_, n)| n.owner.clamp(0, 2) as usize == p).map(|(i, n)| (n.slot, Ev::Nuke(i))));
        evs.sort_by_key(|e| e.0);
        let mut ended = Vec::new();
        for (_, e) in evs {
            match e {
                Ev::Land(l) => self.land_bullet(l),
                Ev::Beam(b) => self.beam_lands(*b),
                Ev::Nuke(i) => {
                    if !self.step_nuke(i) {
                        ended.push(i);
                    }
                }
            }
        }
        for i in ended.into_iter().rev() {
            self.nukes.remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::first_free;

    #[test]
    fn a_new_object_takes_the_first_free_slot() {
        assert_eq!(first_free([].into_iter()), 0);
        assert_eq!(first_free([0, 1, 2].into_iter()), 3);
        assert_eq!(first_free([0, 2, 3].into_iter()), 1);
        assert_eq!(first_free([1, u32::MAX].into_iter()), 0);
    }
}
