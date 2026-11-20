//! Building a `World`: an empty one, one from a level file, custom-map extras, snapshots, and adding structures and units.

use super::*;

impl World {
    /// An empty world of `cells` (width, height) with the given blocked-cell grid; tests start from this.
    pub fn new(cells: (i32, i32), blocked: Vec<bool>) -> World {
        assert_eq!(blocked.len(), (cells.0 * cells.1) as usize);
        // without a map the Advancers are only kept inside the outer ring
        let air_blocked = (0..cells.0 * cells.1)
            .map(|i| {
                let (x, y) = (i % cells.0, i / cells.0);
                x == 0 || y == 0 || x == cells.0 - 1 || y == cells.1 - 1
            })
            .collect();
        World {
            cheats: Cheats::default(),
            sheet_sizes: Default::default(),
            cells,
            blocked,
            air_blocked,
            no_marks: Vec::new(),
            no_build: Vec::new(),
            units: Vec::new(),
            structures: Vec::new(),
            bullets: Vec::new(),
            beams: Vec::new(),
            blinks: HashMap::new(),
            turret_look: HashMap::new(),
            gun_slot: HashMap::new(),
            air_targets: HashMap::new(),
            turret_muzzles: HashMap::new(),
            effects: Vec::new(),
            sounds: Vec::new(),
            player_sounds: Vec::new(),
            decals: Vec::new(),
            fx_rng: 0x9E3779B9,
            sky: Default::default(),
            occ: None,
            tick: 0,
            outcome: None,
            stats: [[0; 8]; 3],
            next_id: 1,
            rng: 0x2545_f491,
            game_rng: None,
            curves: None,
            slot_tables: Default::default(),
            rng_index: 0,
            rings: Rings::generate(),
            has_enemy_hq: false,
            objective: Objective::default(),
            reinforcement: None,
            convoy: None,
            rates: [40; 3],
            repair_mode: false,
            credits: [0; 2],
            research: [0; 2],
            tutorial_step: None,
            tutorial: None,
            camera_request: None,
            ai: false,
            air_stock: [[0; 5]; 2],
            aircraft: Vec::new(),
            carry_counter: 0,
            strike_lock: 0,
            next_air_id: 1,
            nukes: Vec::new(),
            wrecks: Vec::new(),
            corpses: Vec::new(),
            log: Vec::new(),
            chat: Vec::new(),
            last_event: None,
            no_advancer_respawn: [false; 2],
            hq_without_advancer: Vec::new(),
            weather: 0,
            quake: Quake::default(),
            mission_no: None,
            timer: None,
            turret_flags: [[false; 5]; 2],
            timer_expired: false,
            ai_strategy: 0,
            ai_timer: 0,
            ai_strike: 600,
            airlifts: Vec::new(),
            last_airlift: [None; 2],
            last_air: [0; 2],
            difficulty: Difficulty::Normal,
            spectate: false,
            difficulty0: Difficulty::Normal,
            spectate_ai: (0, 0, 600, 0),
            skirmish: false,
            income_rest: [[0; 2]; 2],
            wind: [0; 2],
            enemy_at_start: None,
            hint_phase: 0,
            hint: None,
        }
    }

    /// Builds the world of a loaded level: blocked cells from the map, structures from the objects, units from the markers.
    #[cfg(test)]
    pub fn from_level(map: &opensow_formats::map::Map, level: &Level) -> World {
        Self::from_level_with_rng(map, level, None)
    }

    /// `from_level` with the exe's random table installed first. A fresh mission start (`0x422b00`, state 11) puts the table
    /// back to index 0 (`0x422b19`), builds the debris curves (`0x422c13`, 600 numbers) and only then loads the level
    /// (`0x41f140`), whose units each draw `rand(20)` and `rand(900)` from it (`0x46bf31`, `0x46c111`). Every later draw (the
    /// computer's first strategy roll among them) depends on that order.
    pub fn from_level_with_rng(map: &opensow_formats::map::Map, level: &Level, rng: Option<GameRng>) -> World {
        let (w, h) = (map.width as i32, map.height as i32);
        let mut blocked = vec![false; (w * h) as usize];
        for c in map.cells.iter().filter(|c| c.blocked) {
            blocked[c.y as usize * w as usize + c.x as usize] = true;
        }
        let mut world = World::new((w, h), blocked);
        if let Some(r) = rng {
            world.set_game_rng(r);
        }
        // the air layer: the map's second terrain grid (`+0x180cc`), which the .map file stores as the "border" layer
        for c in map.cells.iter() {
            world.air_blocked[c.y as usize * w as usize + c.x as usize] = c.border;
        }
        if map.cells.iter().any(|c| c.no_build) {
            world.no_build = vec![false; (w * h) as usize];
            for c in map.cells.iter().filter(|c| c.no_build) {
                world.no_build[c.y as usize * w as usize + c.x as usize] = true;
            }
        }
        if map.cells.iter().any(|c| c.no_marks) {
            world.no_marks = vec![false; (w * h) as usize];
            for c in map.cells.iter().filter(|c| c.no_marks) {
                world.no_marks[c.y as usize * w as usize + c.x as usize] = true;
            }
        }
        world.rates = [advancer_rate(level, 0), advancer_rate(level, 1), 40];
        world.ai = true;
        world.no_advancer_respawn = [level.header[34] != 0, level.header[35] != 0];
        world.weather = level.header[64];
        if level.header[65] != 0 {
            world.timer = Some((level.header[66], 0, level.header[67]));
        }
        for (g, st) in level.air_stock().iter().enumerate() {
            world.air_stock[0][g] = st.counts[0].max(0) as u32;
            world.air_stock[1][g] = st.counts[1].max(0) as u32;
        }
        // header words 2/3 = credits, 37/38 = research points of player/enemy (`0x41d9c5..0x41d9f5`)
        world.credits = [level.header[2], level.header[3]];
        // header block bytes: `header[i]` is block word `i - 2`
        let byte = |b: usize| (level.header[b / 4 + 2] >> (8 * (b % 4))) & 0xff != 0;
        for t in 0..5 {
            world.turret_flags[0][t] = byte(84 + t);
            world.turret_flags[1][t] = byte(94 + t);
        }
        world.research = [level.header[37], level.header[38]];
        for (i, o) in level.objects.iter().enumerate() {
            let kind = o.object_kind();
            if matches!(kind, ObjectKind::Unknown(_)) || o.start_hp(building_max_hp(o.kind)) <= 0 {
                continue;
            }
            let weapon = if let ObjectKind::Turret(n) = kind { turret_weapon(n as u32) } else { 0 };
            let aim = (weapon != 0).then(|| TopAim::new(weapon, 0)).flatten();
            let (mut slots, mut active) = (Vec::new(), 0);
            if matches!(
                kind,
                ObjectKind::LightFactory
                    | ObjectKind::MediumFactory
                    | ObjectKind::HeavyFactory
                    | ObjectKind::WalkerFactory
                    | ObjectKind::GoldMine
                    | ObjectKind::ResearchLab
                    | ObjectKind::Wind
                    | ObjectKind::AirBase
            ) {
                let rec = Slots::of(o);
                slots = rec
                    .kinds
                    .iter()
                    .zip(rec.values)
                    .filter_map(|(&k, v)| {
                        slot_target(k).filter(|_| k != 0).map(|target| SlotState {
                            kind: k,
                            progress: 0,
                            target,
                            level: 0,
                            max_level: v.max(0) as u32,
                            waiting: 0,
                            unlock: 0,
                        })
                    })
                    .collect();
                active = (rec.active as usize).min(slots.len());
            }
            world.add_structure(Structure {
                object: i,
                kind,
                owner: o.owner(),
                cell: (o.cell_x, o.cell_y),
                hp: o.start_hp(building_max_hp(o.kind)),
                max_hp: building_max_hp(o.kind),
                aim,
                weapon,
                target: None,
                slots,
                active,
                kind_id: o.kind,
                tally: [0; 3],
                heal: 0,
                upgrade: 0,
                protected: o.raw[15] == 1 || o.kind == kinds::HEADQUARTERS,
                attacked: false,
                sat_cd: 0,
                advancer: None,
                rally: None,
                dying: None,
            });
        }
        for m in &level.markers {
            if let UnitKind::Unknown(_) | UnitKind::RuntimeOnly(_) = m.unit_kind() {
                continue;
            }
            world.add_unit(m.kind, m.player, m.x, m.y);
        }
        // each headquarters owns the first free Advancer of its side (0x41e030..0x41e0a7)
        for k in 0..world.structures.len() {
            if world.structures[k].kind_id != kinds::HEADQUARTERS {
                continue;
            }
            let side = side_of(world.structures[k].owner);
            let taken: Vec<u32> = world.structures.iter().filter_map(|s| s.advancer).collect();
            let id = world
                .units
                .iter()
                .find(|u| u.kind == kinds::ADVANCER && Some(if u.player == 0 { 0 } else { 1 }) == side && !taken.contains(&u.id))
                .map(|u| u.id);
            world.structures[k].advancer = id;
        }
        world
    }

    /// A skirmish's neutral buildings and turrets start at their object word 18 percent like owned ones (the exe gives every
    /// neutral one exactly half, `0x43a003`; the shipped maps hold 50 there after `skirmish::even_start_health`, custom maps what
    /// their editor set). Not in the campaign.
    pub fn neutral_start_hp_from_level(&mut self, level: &Level) {
        for s in &mut self.structures {
            let Some(o) = level.objects.get(s.object) else { continue };
            if o.raw[14] == 2 && o.raw[18] > 0 {
                s.hp = (s.max_hp * o.raw[18] / 100).clamp(1, s.max_hp);
            }
        }
    }

    /// The custom-map additions (`custommap::ObjectExt`, per object index of the level the world was built from): upgrade points
    /// per production line, and headquarters without an Advancer (their start Advancer goes to a headquarters of the side that has
    /// none yet; they never get a replacement).
    pub fn apply_custom(&mut self, ext: &[opensow_formats::custommap::ObjectExt]) {
        for k in 0..self.structures.len() {
            let Some(e) = ext.get(self.structures[k].object) else { continue };
            for (slot, &u) in self.structures[k].slots.iter_mut().zip(e.unlock.iter()) {
                slot.unlock = u.max(0) as u32;
            }
            if self.structures[k].kind_id == kinds::HEADQUARTERS && e.no_advancer && !self.hq_without_advancer.contains(&k) {
                self.hq_without_advancer.push(k);
            }
        }
        for &k in &self.hq_without_advancer.clone() {
            let Some(id) = self.structures[k].advancer.take() else { continue };
            let owner = self.structures[k].owner;
            match (0..self.structures.len()).find(|&j| {
                self.structures[j].kind_id == kinds::HEADQUARTERS
                    && self.structures[j].owner == owner
                    && self.structures[j].advancer.is_none()
                    && !self.hq_without_advancer.contains(&j)
            }) {
                Some(j) => self.structures[j].advancer = Some(id),
                // more Advancers than headquarters that want one: the extra one is not placed
                None => self.units.retain(|u| u.id != id),
            }
        }
    }

    /// The whole running state as bytes (JSON; the savegame container compresses it). Static tables (`rings`, muzzle tables,
    /// the random table, the occupancy grids) are not stored: `restore` takes them from a freshly built world of the same level.
    pub fn snapshot(&mut self) -> Vec<u8> {
        self.rng_index = self.game_rng.as_ref().map_or(0, |r| r.index());
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Replaces this (freshly built) world's state by a snapshot of the same level; `Err` when the bytes are not one or the map
    /// does not match.
    pub fn restore(&mut self, data: &[u8]) -> Result<(), String> {
        let mut saved: World = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        if saved.cells != self.cells || saved.blocked.len() != self.blocked.len() {
            return Err("the savegame is for another map".into());
        }
        saved.turret_muzzles = std::mem::take(&mut self.turret_muzzles);
        saved.game_rng = self.game_rng.take();
        saved.curves = self.curves.take();
        if let Some(r) = saved.game_rng.as_mut() {
            r.set_index(saved.rng_index);
        }
        for s in &mut saved.structures {
            if let Some(a) = s.aim.as_mut() {
                a.restore();
            }
        }
        for u in &mut saved.units {
            if let Some(a) = u.aim.as_mut() {
                a.restore();
            }
        }
        *self = saved;
        Ok(())
    }

    /// Adds a building or turret and marks the cells it stands on.
    pub fn add_structure(&mut self, s: Structure) {
        for (cx, cy) in s.blocked_cells() {
            if let Some(b) = self.cell_index(cx, cy) {
                self.blocked[b] = true;
            }
        }
        if s.kind == ObjectKind::Headquarters && s.owner == Owner::Enemy {
            self.has_enemy_hq = true;
        }
        let owner = side(s.owner);
        self.structures.push(s);
        let i = self.structures.len() - 1;
        self.slot_alloc(owner, i);
    }

    /// Gives structure `i` a slot in `owner`'s slot table.
    pub(super) fn slot_alloc(&mut self, owner: usize, i: usize) {
        let t = &mut self.slot_tables[owner];
        match t.iter().position(Option::is_none) {
            Some(p) => t[p] = Some(i),
            None => t.push(Some(i)),
        }
    }

    /// Frees the slot of structure `i` in `owner`'s slot table.
    pub(super) fn slot_free(&mut self, owner: usize, i: usize) {
        if let Some(e) = self.slot_tables[owner].iter_mut().find(|e| **e == Some(i)) {
            *e = None;
        }
    }

    /// Walk parameters of a kind in the running mission (the pods of missions 3 and 8 are slow, `anim::walk_params_for_mode`;
    /// so are a custom escort's).
    pub(super) fn walk_params_of(&self, kind: i32) -> Option<opensow_formats::anim::WalkParams> {
        let mode = if kind == kinds::POD && self.convoy.is_some() { 3 } else { self.mission_no.unwrap_or(u32::MAX) };
        opensow_formats::anim::walk_params_for_mode(kind, mode)
    }

    /// The human's pods walk by script and take no orders: missions 3 and 8 (`0x418a0c`, `0x46a10b`) and custom escorts.
    pub(super) fn pods_scripted(&self) -> bool {
        matches!(self.mission_no, Some(3 | 8)) || self.convoy.is_some()
    }

    /// Starts a custom map's escort: the human's pods walk `route` (waypoints, then the goal) at the slow convoy pace.
    pub fn set_convoy(&mut self, route: Vec<(i32, i32)>) {
        let had_pods = self.units.iter().any(|u| u.hp > 0 && u.player == 0 && u.kind == kinds::POD);
        self.convoy = Some(Convoy { route, next: Vec::new(), had_pods });
        for i in 0..self.units.len() {
            if self.units[i].kind == kinds::POD {
                self.units[i].chassis = self.walk_params_of(46).map(|p| ChassisAnim::new(p, p.steps, 0));
            }
        }
    }

    /// Sets the mission number and gives units that already exist the mission's walk parameters.
    pub fn set_mission(&mut self, n: Option<u32>) {
        self.mission_no = n;
        for i in 0..self.units.len() {
            let kind = self.units[i].kind;
            if kind == kinds::POD {
                // spawned before the mission was known: rebuild the chassis with the mission's parameters
                self.units[i].chassis = self.walk_params_of(kind).map(|p| ChassisAnim::new(p, p.steps, 0));
            }
        }
    }

    /// Adds a unit of `kind` for `player` at a map pixel and returns its id.
    pub fn add_unit(&mut self, kind: i32, player: i32, x: i32, y: i32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let params = self.walk_params_of(kind);
        // the initialisers set chassis facing `[+0x30]` = 8 with target `[+0x34]` = 0 (`0x470361`, `0x470397`) and top facing
        // `[+0x6c]` = 15 with target `[+0x70]` = 0 (`0x47abb5`, `0x479f02`): a new unit turns to north right after it appears
        let chassis = params.map(|p| {
            let mut c = ChassisAnim::new(p, p.steps, 8 % p.steps.max(1));
            c.set_target(0);
            c
        });
        let aim = TopAim::new(kind, 15);
        let hover_phase = self.random(20);
        // look-around countdown `[unit+0x8c]` = 240 + random(900) (`0x46c10c..0x46c121`)
        let look = 240 + self.random(900) as i32;
        let max_hp = UnitKind::max_hp(kind).unwrap_or(300);
        let slot = self.free_unit_slot(player.clamp(0, 2) as usize);
        self.units.push(Unit {
            id,
            kind,
            player,
            x: x as f32,
            y: y as f32,
            hp: max_hp,
            max_hp,
            chassis,
            aim,
            path: Vec::new(),
            target: None,
            selected: false,
            moved: false,
            line: None,
            step: None,
            dest: None,
            cool: 0,
            straight: None,
            queued: None,
            hover_phase,
            action: None,
            beam: BeamTimer::default(),
            beam_shots: 0,
            chase: None,
            idle: 0,
            look,
            slot,
        });
        // the unit constructor stamps the new unit into the ground static grid, whatever its layer (`0x46c0b9`)
        if let (Some(occ), Some(at)) = (self.occ.as_mut(), movement::grid_index((x.div_euclid(CELL), y.div_euclid(CELL)))) {
            occ.stat[at] = 0x3000000 + id;
        }
        id
    }

    /// Adds a unit on the free cell nearest to the pixel position (used for the starter squad).
    pub fn add_unit_near(&mut self, kind: i32, player: i32, x: i32, y: i32) -> Option<u32> {
        let cell = self.nearest_free((x.div_euclid(CELL), y.div_euclid(CELL)))?;
        let taken = |w: &World, c: (i32, i32)| w.units.iter().any(|u| u.cell() == c);
        let cell = std::iter::once(cell)
            .chain(self.rings.search(0, 12).iter().map(|&(dx, dy)| (x.div_euclid(CELL) + dx as i32, y.div_euclid(CELL) + dy as i32)))
            .find(|&c| !self.is_blocked(c.0, c.1) && !taken(self, c))?;
        Some(self.add_unit(kind, player, cell.0 * CELL + CELL / 2, cell.1 * CELL + CELL / 2))
    }

    /// Places `kinds` for `side` in blocks of six (three columns of two, filled column by column like a carryall squad,
    /// so each column is a pair of one kind): the 12 Goldmine Valley units make two 3x2 blocks. Each block goes to the
    /// nearest spot 6 to 18 cells from the HQ's middle (by its middle), out in the open: every cell free (not blocked, no
    /// building, no unit, not by a factory exit, not `hidden` under a tree or rock top) and no building or unit on the ring
    /// of cells around it, and reachable on foot from the HQ, nearest by walking distance; so the blocks stand in the open,
    /// apart from the base and from each other, not on a plateau behind a cliff. Without an HQ it goes by the side's first building; with no such
    /// spot the block's units are placed one by one on the nearest free cells of that range.
    pub fn add_start_units(&mut self, side: usize, kinds: &[i32], hidden: impl Fn((i32, i32)) -> bool) {
        let mine = |o: Owner| side_of(o) == Some(side);
        let Some(home) = self
            .structures
            .iter()
            .find(|s| s.alive() && mine(s.owner) && s.kind_id == kinds::HEADQUARTERS)
            .or_else(|| self.structures.iter().find(|s| s.alive() && mine(s.owner)))
            .map(|s| s.center_cell())
        else {
            return;
        };
        let exits: Vec<(i32, i32)> = self
            .structures
            .iter()
            .filter(|s| s.alive())
            .filter_map(|s| factory_exit(s.kind_id).map(|e| (s.cell.0 + e.primary.0, s.cell.1 + e.primary.1)))
            .collect();
        let inside = |w: &World, c: (i32, i32)| c.0 >= 0 && c.1 >= 0 && c.0 < w.cells.0 && c.1 < w.cells.1;
        let building = |w: &World, c: (i32, i32)| w.structures.iter().any(|s| s.alive() && s.covers(c));
        let unit = |w: &World, c: (i32, i32)| w.units.iter().any(|u| u.hp > 0 && u.cell() == c);
        let free = |w: &World, c: (i32, i32)| {
            inside(w, c) && !w.is_blocked(c.0, c.1) && !building(w, c) && !unit(w, c) && !exits.iter().any(|e| (e.0 - c.0).abs() <= 1 && (e.1 - c.1).abs() <= 2)
        };
        let (near, far) = (6, 18);
        // walking distance from the HQ over unblocked cells without buildings (a breadth-first fill within the search range):
        // blocks go to the nearest ground on foot, not to a plateau that is close as the crow flies but behind a cliff
        let mut steps: HashMap<(i32, i32), i32> = HashMap::new();
        let mut queue: std::collections::VecDeque<(i32, i32)> = (-3..=3)
            .flat_map(|dy| (-3..=3).map(move |dx| (home.0 + dx, home.1 + dy)))
            .filter(|&c| inside(self, c) && !self.is_blocked(c.0, c.1) && !building(self, c))
            .collect();
        for &c in &queue {
            steps.insert(c, 0);
        }
        while let Some(c) = queue.pop_front() {
            let d = steps[&c];
            for n in [(c.0 + 1, c.1), (c.0 - 1, c.1), (c.0, c.1 + 1), (c.0, c.1 - 1)] {
                if (n.0 - home.0).abs() <= far + 6
                    && (n.1 - home.1).abs() <= far + 6
                    && inside(self, n)
                    && !self.is_blocked(n.0, n.1)
                    && !building(self, n)
                    && !steps.contains_key(&n)
                {
                    steps.insert(n, d + 1);
                    queue.push_back(n);
                }
            }
        }
        for chunk in kinds.chunks(6) {
            let cols = (chunk.len() as i32 + 1) / 2;
            let block = |x: i32, y: i32| (0..chunk.len() as i32).map(move |i| (x + i / 2, y + i % 2));
            // twice the offset of the block's middle from the HQ's middle
            let off = |x: i32, y: i32| (x * 2 + cols - 1 - home.0 * 2, y * 2 + 1 - home.1 * 2);
            let mut corners: Vec<(i32, i32)> = (-far - cols..=far)
                .flat_map(|dy| (-far - cols..=far).map(move |dx| (home.0 + dx, home.1 + dy)))
                .filter(|&(x, y)| {
                    let (mx, my) = off(x, y);
                    (near..=far).contains(&((mx.abs().max(my.abs()) + 1) / 2))
                })
                .collect();
            // nearest on foot (the block's farthest cell), then as the crow flies
            corners.sort_by_key(|&(x, y)| {
                let (mx, my) = off(x, y);
                (block(x, y).map(|c| steps.get(&c).copied().unwrap_or(i32::MAX)).max().unwrap_or(i32::MAX), mx * mx + my * my, y, x)
            });
            let open = |w: &World, x: i32, y: i32| {
                block(x, y).all(|c| free(w, c) && !hidden(c) && steps.contains_key(&c))
                    && (y - 1..=y + 2).all(|cy| (x - 1..=x + cols).all(|cx| !building(w, (cx, cy)) && !unit(w, (cx, cy))))
            };
            match corners.iter().copied().find(|&(x, y)| open(self, x, y)) {
                Some((x, y)) => {
                    for (c, &kind) in block(x, y).zip(chunk) {
                        self.add_unit(kind, side as i32, c.0 * CELL + CELL / 2, c.1 * CELL + CELL / 2);
                    }
                }
                None => {
                    let mut cells: Vec<(i32, i32)> = (-far..=far)
                        .flat_map(|dy| (-far..=far).map(move |dx| (home.0 + dx, home.1 + dy)))
                        .filter(|&(x, y)| (near..=far).contains(&(x - home.0).abs().max((y - home.1).abs())))
                        .collect();
                    cells.sort_by_key(|&(x, y)| ((x - home.0).pow(2) + (y - home.1).pow(2), y, x));
                    let mut left = chunk.iter();
                    for c in cells {
                        if !free(self, c) || !steps.contains_key(&c) {
                            continue;
                        }
                        let Some(&kind) = left.next() else { break };
                        self.add_unit(kind, side as i32, c.0 * CELL + CELL / 2, c.1 * CELL + CELL / 2);
                    }
                }
            }
        }
    }

    /// The nearest cell around `c` that ground units can enter.
    pub(super) fn nearest_free(&self, c: (i32, i32)) -> Option<(i32, i32)> {
        self.rings.search(0, 12).iter().map(|&(dx, dy)| (c.0 + dx as i32, c.1 + dy as i32)).find(|&(x, y)| !self.is_blocked(x, y))
    }
}
