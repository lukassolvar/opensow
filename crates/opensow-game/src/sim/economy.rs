//! Production and money: factory slots, income, tech levels and building turrets.

use super::*;

impl World {
    /// Factory production: each active slot gains `steps_at(row, tick % 60)` per tick and spawns its unit at a free cell
    /// next to the building once full (0x40e640 + 0x408740). Neutral buildings do not produce.
    pub(super) fn step_production(&mut self, i: usize) {
        let s = &self.structures[i];
        if !s.alive() || s.owner == Owner::Neutral || s.slots.is_empty() || !(s.kind.produces_units() || s.kind == ObjectKind::AirBase) || s.heal > 0 {
            return;
        }
        let (player, cell, active) = (if s.owner == Owner::Player { 0 } else { 1 }, s.cell, s.active.min(s.slots.len()));
        let free = if self.ai && (player == 1 || self.spectate) { self.difficulty_of(player as usize).handicap().free_windtrap_levels } else { 0 };
        let rate = rate_row(self.tutorial_step, self.wind[player as usize] + free);
        let gain = steps_at(rate, (self.tick % 60) as u32);
        for j in 0..active {
            let slot = &mut self.structures[i].slots[j];
            slot.progress = (slot.progress + gain).min(slot.target);
            if slot.progress < slot.target {
                continue;
            }
            let kind = slot.kind;
            if let Some(g) = AirForce::from_code(kind).and_then(|f| HEADER_ORDER.iter().position(|&h| h == f)) {
                // air bases add the force to the stock (0x453e80) and start over
                self.air_stock[player as usize][g] += 1;
                self.structures[i].slots[j].progress = 0;
                if let Some(id) = msg::force_ready(kind) {
                    self.say(player as usize, id); // `0x453ef4` passes -1, -1
                }
                continue;
            }
            let held = self.units.iter().filter(|u| u.player == player && u.hp > 0).count() + self.corpses.iter().filter(|c| c.player == player).count();
            if held >= UNIT_SLOTS {
                continue;
            }
            // `0x408740`: the unit appears at the spawn cell (inside the footprint) and
            // leaves through the primary exit, or the opposite one when that is occupied; with both occupied it waits
            let kind_id = self.structures[i].kind_id;
            let e = factory_exit(kind_id).unwrap_or(FactoryExit { spawn: (1, 1), primary: (3, 3), alternative: (-1, -1) });
            let at = |o: (i32, i32)| (cell.0 + o.0, cell.1 + o.1);
            // the exits are tested in the static grid `0x5eac38` (terrain and buildings only): units never block them
            let occupied = |w: &World, c: (i32, i32)| w.is_blocked(c.0, c.1);
            let spawn = at(e.spawn);
            let exit = if !occupied(self, at(e.primary)) {
                Some((at(e.primary), (0, 2)))
            } else if !occupied(self, at(e.alternative)) {
                Some((at(e.alternative), (0, -2)))
            } else {
                None
            };
            match exit {
                Some((walk, beyond)) => {
                    let id = self.add_unit(kind, player, spawn.0 * CELL + CELL / 2, spawn.1 * CELL + CELL / 2);
                    self.stats[player.clamp(0, 2) as usize][2] += 1; // 0x4087ed / 0x408a11
                    self.order_straight_then(id, walk, (walk.0 + beyond.0, walk.1 + beyond.1));
                    // the third order (`0x4089a6`): on to the rally point when it lies inside the map (player factories only: the
                    // exe's AI never sets one)
                    let rally = self.structures[i].rally.filter(|&p| player == 0 && opensow_formats::rally::usable(p, self.cells));
                    if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
                        u.queued = rally;
                    }
                    let slot = &mut self.structures[i].slots[j];
                    slot.progress = 0;
                    slot.waiting = 0;
                }
                None => {
                    let slot = &mut self.structures[i].slots[j];
                    slot.waiting += 1;
                    if slot.waiting == NO_SPACE_TICKS {
                        let (x, y) = self.structures[i].center(); // `0x40e323`
                        self.say_at(player as usize, msg::NO_SPACE, x, y);
                    }
                }
            }
        }
    }

    /// Income (`0x42d6b0`): every finished gold mine adds `active + 1` credits per second to its owner, every research lab
    /// `active + 1` research points per second; buildings still healing in (`[bld+0x68]`) and destroyed ones give nothing.
    pub(super) fn step_economy(&mut self) {
        self.wind = [0; 2];
        for s in self.structures.iter().filter(|s| s.alive() && s.heal == 0 && s.kind == ObjectKind::Wind) {
            if let Some(side) = side_of(s.owner) {
                self.wind[side] += s.active as u32;
            }
        }
        if !self.tick.is_multiple_of(60) {
            return; // the exe's income loop runs on the first tick of each second ([0x517b10 + i*4] == 0)
        }
        let before = (self.credits[0], self.research[0]);
        for s in self.structures.iter().filter(|s| s.alive() && s.heal == 0) {
            let side = match s.owner {
                Owner::Player => 0,
                Owner::Enemy => 1,
                _ => continue,
            };
            let mut gain = income_per_tick(s.active);
            let pct = if self.ai && (side == 1 || self.spectate) { self.difficulty_of(side).handicap().income_pct } else { 100 };
            let which = match s.kind {
                ObjectKind::GoldMine => 0,
                ObjectKind::ResearchLab => 1,
                _ => continue,
            };
            if pct != 100 {
                // the scaled yield pays out whole points; the hundredths carry over to the next second
                let hundredths = gain as u32 * pct + self.income_rest[side][which];
                self.income_rest[side][which] = hundredths % 100;
                gain = (hundredths / 100) as _;
            }
            if which == 0 {
                self.credits[side] = (self.credits[side] + gain).min(CREDITS_CAP);
            } else {
                self.research[side] = (self.research[side] + gain).min(RESEARCH_CAP);
            }
        }
        // crossing the price (200..1000) of an available turret tier or 500 research tells the player (0x42d88c, 0x42d8f5..0x42d98a)
        if (1..=5).any(|t| self.turret_flags[0][t - 1] && before.0 < 200 * t as i32 && self.credits[0] >= 200 * t as i32) {
            self.say(0, msg::NEW_TURRET);
        }
        if before.1 < 500 && self.research[0] >= 500 {
            self.say(0, msg::NEW_TECH_LEVEL);
        }
    }

    /// Buys the next tech-level of slot `slot` of structure `s` for `side` (`0x454c19` + `0x40e730`): the slot must be
    /// active, below its level limit and affordable in research points; its kind becomes the next tier, progress is kept.
    pub fn tech_up(&mut self, s: usize, slot: usize, side: usize) -> bool {
        let Some(st) = self.structures.get_mut(s) else { return false };
        if !st.alive() || side_of(st.owner) != Some(side) || slot >= st.active {
            return false;
        }
        let Some(sl) = st.slots.get_mut(slot) else { return false };
        let (Some(next), Some(price)) = (next_tier(sl.kind), slot_unlock_cost(sl.kind)) else { return false };
        if sl.level >= sl.max_level || self.research[side] < price as i32 {
            return false;
        }
        self.research[side] -= price as i32;
        sl.kind = next;
        sl.level += 1;
        if let Some(t) = slot_target(next) {
            sl.target = t;
        }
        true
    }

    /// Raises every affordable tech-level of every factory of `side` (the exe's AI does this in `0x42f490`).
    pub fn tech_up_all(&mut self, side: usize) -> usize {
        let mut n = 0;
        for s in 0..self.structures.len() {
            for slot in 0..5 {
                n += self.tech_up(s, slot, side) as usize;
            }
        }
        n
    }

    /// The placement test per footprint cell (`0x408be0`, results `[pointer+0x170..]`, row by row from the top-left `cell`): a cell is
    /// good (state 2) when it is free (units, terrain, buildings, no-build layer) and some cell within two cells of the footprint
    /// belongs to one of `side`'s buildings; otherwise state 1 and the whole spot is refused.
    pub fn turret_cells(&self, side: usize, tier: u32, cell: (i32, i32)) -> Vec<bool> {
        let me = if side == 0 { Owner::Player } else { Owner::Enemy };
        let (w, h) = footprint(24 + tier as i32).map_or((1, 1), |(w, h, _)| (w, h));
        let own_near = |x: i32, y: i32| self.structures.iter().any(|s| s.alive() && s.owner == me && s.covers((x, y)));
        let near = (cell.1 - 2..cell.1 + h + 2).any(|y| (cell.0 - 2..cell.0 + w + 2).any(|x| own_near(x, y)));
        let free = |x: i32, y: i32| {
            !self.is_blocked(x, y)
                && !self.is_no_build(x, y)
                && !self.units.iter().any(|u| u.hp > 0 && u.cell() == (x, y))
                && !self.structures.iter().any(|s| s.alive() && s.covers((x, y)))
        };
        (0..h).flat_map(|dy| (0..w).map(move |dx| (dx, dy))).map(|(dx, dy)| near && free(cell.0 + dx, cell.1 + dy)).collect()
    }

    /// Dev tools: puts a turret (25..=29) or building (100..=108) of `side` on `cell` (its top-left footprint cell) with full hit
    /// points, without cost, range or terrain tests. Refused (false) when the footprint leaves the map or overlaps a living
    /// structure. Buildings get no production slots (the level scripts supply them); an HQ links the side's first free Advancer.
    pub fn dev_place_structure(&mut self, kind_id: i32, side: usize, cell: (i32, i32)) -> bool {
        let kind = ObjectKind::from_id(kind_id);
        let Some((w, h, _)) = footprint(kind_id).filter(|_| !matches!(kind, ObjectKind::Unknown(_))) else { return false };
        let cells: Vec<(i32, i32)> = (0..h).flat_map(|dy| (0..w).map(move |dx| (cell.0 + dx, cell.1 + dy))).collect();
        if cells.iter().any(|&(x, y)| x < 0 || y < 0 || x >= self.cells.0 || y >= self.cells.1 || self.structures.iter().any(|s| s.alive() && s.covers((x, y))))
        {
            return false;
        }
        let owner = if side == 0 { Owner::Player } else { Owner::Enemy };
        let (weapon, aim) = match kind {
            ObjectKind::Turret(n) => (turret_weapon(n as u32), TopAim::new(turret_weapon(n as u32), 0)),
            _ => (0, None),
        };
        let max_hp = building_max_hp(kind_id);
        self.stats[side][4] += 1;
        self.add_structure(Structure {
            object: usize::MAX,
            kind,
            owner,
            cell,
            hp: max_hp,
            max_hp,
            aim,
            weapon,
            target: None,
            slots: Vec::new(),
            active: 0,
            kind_id,
            tally: [0; 3],
            heal: 0,
            upgrade: 0,
            protected: kind_id == kinds::HEADQUARTERS,
            attacked: false,
            sat_cd: 0,
            advancer: None,
            rally: None,
            dying: None,
        });
        if kind_id == kinds::HEADQUARTERS {
            let k = self.structures.len() - 1;
            let taken: Vec<u32> = self.structures.iter().filter_map(|s| s.advancer).collect();
            self.structures[k].advancer =
                self.units.iter().find(|u| u.kind == kinds::ADVANCER && u.hp > 0 && (u.player != 0) == (side != 0) && !taken.contains(&u.id)).map(|u| u.id);
        }
        self.sound(72, cell.0 * CELL, cell.1 * CELL, false);
        true
    }

    /// Builds a turret of `tier` (1-5) for `side` on `cell` (`0x417335`..`0x4173da`): costs 200 * tier credits, the new
    /// turret starts at 1 hit point and heals in. The placement test is `0x408be0` (docs/ai.md): free of units, terrain, buildings and the map's no-build layer, and an own building within two cells.
    pub fn place_turret(&mut self, side: usize, tier: u32, cell: (i32, i32)) -> bool {
        if !self.build_turret(side, tier, cell) {
            return false;
        }
        // `0x4173f9`: sound 0x48 at the cell's top-left pixel; the player's own placement is a priority sound here, heard anywhere
        if side == 0 {
            self.player_sound(72);
        } else {
            self.sound(72, cell.0 * CELL, cell.1 * CELL, false);
        }
        true
    }

    /// Places a turret and pays for it (`0x439e30`; the computer's `0x431120` calls it without the click sound).
    pub(super) fn build_turret(&mut self, side: usize, tier: u32, cell: (i32, i32)) -> bool {
        let Some(price) = turret_price(tier) else { return false };
        let me = if side == 0 { Owner::Player } else { Owner::Enemy };
        let kind_id = 24 + tier as i32;
        let free = |x: i32, y: i32| {
            !self.is_blocked(x, y)
                && !self.is_no_build(x, y)
                && !self.units.iter().any(|u| u.hp > 0 && u.cell() == (x, y))
                && !self.structures.iter().any(|s| s.alive() && s.covers((x, y)))
        };
        let own_near = |x: i32, y: i32| self.structures.iter().any(|s| s.alive() && s.owner == me && s.covers((x, y)));
        if self.credits[side] < price || !can_place(kind_id, cell, free, own_near) {
            return false;
        }
        self.credits[side] -= price;
        self.stats[side][4] += 1; // 0x43a04e
        let weapon = turret_weapon(tier);
        let aim = TopAim::new(weapon, 0);
        let max_hp = building_max_hp(24 + tier as i32);
        let owner = if side == 0 { Owner::Player } else { Owner::Enemy };
        self.add_structure(Structure {
            object: usize::MAX,
            kind: ObjectKind::Turret(tier as u8),
            owner,
            cell,
            hp: 1,
            max_hp,
            aim,
            weapon,
            target: None,
            slots: Vec::new(),
            active: 0,
            kind_id: 24 + tier as i32,
            tally: [0; 3],
            heal: max_hp - 1,
            upgrade: 0,
            protected: false,
            attacked: false,
            sat_cd: 0,
            advancer: None,
            rally: None,
            dying: None,
        });
        true
    }
}
