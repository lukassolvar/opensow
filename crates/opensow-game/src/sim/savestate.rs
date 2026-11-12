//! The bridge between the simulation and the original's savegame state stream (`opensow_formats::save::State`, docs/savegame.md
//! "The sim bridge"). `World::to_state` writes what the exe's store branch would write for the running mission, `World::apply_state`
//! overwrites a freshly built world of the same level with a loaded stream, like the exe's loader does (level first, then the saved
//! objects). Fields without a counterpart in the sim keep the value a freshly constructed object holds; sim state with no field in
//! the stream (AI timers, the tutorial script, the visual RNG, bullets and effects in flight) is not carried: see the doc.

use super::*;
use opensow_formats::config::PassedLevels;
use opensow_formats::kinds;
use opensow_formats::save::{CameraState, MapCell, MapState, State, StateHead, SLOTS};
use opensow_formats::saveobj::{Class, Object};

/// A handle as the exe's allocator `0x43a9b0` builds it: `slot | generation << 10 | player << 16 | type << 19 | class << 22`
/// (class 1 unit, 2 building, 4 aircraft squad, 5 bullet (built inline at `0x43a194`, the bridge leaves it 0); `type` is 0 for all
/// but units here, see docs/savegame.md Handles for the exe's building types).
pub fn handle(class: u32, kind: u32, player: u32, generation: u32, slot: u32) -> u32 {
    slot | generation << 10 | player << 16 | kind << 19 | class << 22
}

/// `(slot, generation, player, type, class)` of a handle.
pub fn split_handle(h: u32) -> (u32, u32, u32, u32, u32) {
    (h & 0x3ff, h >> 10 & 0x3f, h >> 16 & 7, h >> 19 & 7, h >> 22)
}

/// The handle type the unit initialiser `0x46bd30` gives a unit of `kind` (jump table `0x46c1b4`): 5 anti-air walkers (1, 2, 3, 42),
/// 4 the Advancer, 3 the turret guns 109, 111-113, 6 the anti-air turret gun 110, else 1.
pub fn unit_handle_type(kind: i32) -> u32 {
    match kind {
        1 | 2 | 3 | 42 => 5,
        40 => 4,
        109 | 111 | 112 | 113 => 3,
        110 => 6,
        _ => 1,
    }
}

fn is_anti_air(kind: i32) -> bool {
    kinds::ANTI_AIR_UNITS.contains(&kind) || kind == kinds::ANTI_AIR_TURRET_TOP
}

fn player_slot(p: i32) -> usize {
    p.clamp(0, 2) as usize
}

/// The nine numpad camera bookmarks a new input object starts with (`0x414a70`): pixel x, y pairs.
const BOOKMARKS: [u32; 18] = [0x1c0, 0x640, 0x400, 0x640, 0x6c0, 0x640, 0x1c0, 0x440, 0x400, 0x440, 0x6c0, 0x440, 0x1c0, 0x240, 0x400, 0x240, 0x6c0, 0x240];

impl World {
    /// The building handle of structure `i` (its slot in the owner's table, generation 0), if it has one.
    fn building_handle(&self, i: usize) -> Option<u32> {
        let owner = side(self.structures.get(i)?.owner);
        let slot = self.slot_tables[owner].iter().position(|e| *e == Some(i))?;
        Some(handle(2, 0, owner as u32, 0, slot as u32))
    }

    fn target_handle(&self, t: &Option<Target>, unit_handles: &HashMap<u32, u32>) -> u32 {
        match t {
            Some(Target::Unit(id)) => unit_handles.get(id).copied().unwrap_or(0),
            Some(Target::Structure(i)) => self.building_handle(*i).unwrap_or(0),
            None => 0,
        }
    }

    /// The state stream of the running mission. `level` is the mission number (`[0x49a33c]`), `progress` the campaign record that
    /// the exe's save embeds, `scroll` player 0's view in pixels.
    pub fn to_state(&self, level: u32, progress: &PassedLevels, scroll: (i32, i32)) -> State {
        let (w, h) = self.cells;
        // ---- handles -------------------------------------------------------------------------------------------------------
        let mut counters: [Vec<u8>; 4] = [vec![0; 3 * SLOTS[0]], vec![0; 3 * SLOTS[1]], vec![0; 3 * SLOTS[2]], vec![0; 3 * SLOTS[3]]];
        let mut unit_slot = [0usize; 3];
        let mut unit_handles: HashMap<u32, u32> = HashMap::new();
        let mut unit_slots: Vec<(usize, usize)> = Vec::new(); // (player, slot) per world.units index
        for u in &self.units {
            let p = player_slot(u.player);
            let slot = unit_slot[p];
            unit_slot[p] += 1;
            unit_handles.insert(u.id, handle(1, unit_handle_type(u.kind), p as u32, 0, slot as u32));
            unit_slots.push((p, slot));
        }
        // turret guns (kinds 109..113) are units of their own in the exe: one per living turret building
        let mut gun_slots: Vec<(usize, usize, usize)> = Vec::new(); // (structure, player, slot)
        for (i, s) in self.structures.iter().enumerate() {
            if let (ObjectKind::Turret(_), true) = (s.kind, s.alive()) {
                let p = side(s.owner);
                gun_slots.push((i, p, unit_slot[p]));
                unit_slot[p] += 1;
            }
        }
        for &(p, slot) in unit_slots.iter().chain(gun_slots.iter().map(|(_, p, s)| (*p, *s)).collect::<Vec<_>>().iter()) {
            counters[0][p * SLOTS[0] + slot] = 1;
        }
        // ---- the map object ------------------------------------------------------------------------------------------------
        let mut cells = Vec::with_capacity((w * h) as usize);
        let mut cover: HashMap<(i32, i32), usize> = HashMap::new();
        for (i, s) in self.structures.iter().enumerate().filter(|(_, s)| s.present()) {
            for c in s.blocked_cells() {
                cover.insert(c, i);
            }
        }
        let mut footprint: HashMap<(i32, i32), usize> = HashMap::new();
        for (i, s) in self.structures.iter().enumerate().filter(|(_, s)| s.present()) {
            let (fw, fh) = s.size();
            for dy in 0..fh {
                for dx in 0..fw {
                    footprint.insert((s.cell.0 + dx, s.cell.1 + dy), i);
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize;
                let ground = match cover.get(&(x, y)) {
                    Some(&i) => self.building_handle(i).unwrap_or(0xFFFFFD),
                    None if self.blocked[idx] => 0xFFFFFD,
                    None => 0,
                };
                let building = footprint.get(&(x, y)).and_then(|&i| self.building_handle(i)).unwrap_or(0);
                cells.push(MapCell {
                    coord: ((x as u16) << 8) | y as u16,
                    ground,
                    air: if self.air_blocked.get(idx).copied().unwrap_or(false) { 0xFFFFFD } else { 0 },
                    reserved: 0,
                    no_marks: self.no_marks.get(idx).copied().unwrap_or(false) as u8,
                    // a building's footprint sets the layer (`0x40ad69`) and the exe re-stamps it on load
                    no_build: (self.no_build.get(idx).copied().unwrap_or(false) || cover.contains_key(&(x, y))) as u8,
                    building,
                });
            }
        }
        let cameras = (0..3)
            .map(|k| CameraState {
                scroll_to: false,
                target_x: 0,
                target_y: 0,
                quake_left: if k == 0 { self.quake.ticks.max(0) as u32 } else { 0 },
                quake_timer: 1,
                quake_x: if k == 0 { self.quake.at.0 as u32 } else { 0 },
                quake_y: if k == 0 { self.quake.at.1 as u32 } else { 0 },
            })
            .collect();
        let map = MapState {
            version: "VER7".into(),
            scroll_x: scroll.0 as u32,
            scroll_y: scroll.1 as u32,
            width: w as u32,
            height: h as u32,
            cells,
            cameras,
            items: Vec::new(),
        };

        // ---- head, players, input, params ----------------------------------------------------------------------------------
        let rng_index = self.game_rng.as_ref().map_or(self.rng_index, |r| r.index()) as u32;
        let (minutes, seconds) = self.timer.map_or((0, 0), |t| (t.0.max(0) as u32, t.1.max(0) as u32));
        let head = StateHead {
            level,
            rng_index,
            v_5baafc: 1,
            flag_49a339: false,
            v_5baad8: 0,
            v_5baadc: 0,
            v_5baac4: minutes,
            v_5baac8: seconds,
            flag_5baa99: false,
            v_89cf50: self.carry_counter as u32,
            len: 0,
        };
        let mut player_words = Vec::new();
        for p in 0..3 {
            let (credits, research) = if p < 2 { (self.credits[p].max(0) as u32, self.research[p].max(0) as u32) } else { (0, 0) };
            player_words.push([credits, research, if p < 2 { self.wind[p] } else { 0 }]);
        }
        let mut players = Vec::new();
        let mut stocks = Vec::new();
        let mut params = Vec::new();
        for p in 0..3 {
            let mut ps = Object::blank(Class::PlayerState);
            if p == 0 {
                ps.set(0x40, self.strike_lock.max(0) as u32);
            }
            if p == 1 {
                // the computer's strategy and its timer (`[0x70ba88]`, `[0x70ba8c]`, docs/ai.md)
                ps.set(0x38, self.ai_strategy);
                ps.set(0x3c, self.ai_timer as u32);
            }
            players.push(ps);
            let mut st = Object::blank(Class::Stock);
            st.set(0x32c, u32::MAX);
            st.set(0x330, u32::MAX);
            for (k, v) in BOOKMARKS.iter().enumerate() {
                st.set(0x2e4 + 4 * k as u32, *v);
            }
            stocks.push(st);
            let mut pr = Object::blank(Class::Params);
            for k in 0..8 {
                pr.set(4 + 4 * k as u32, self.stats[p][k]);
            }
            params.push(pr);
        }
        let mut speech = Object::blank(Class::Speech);
        let (ex, ey) = self.last_event.unwrap_or((-1, -1));
        speech.set(0x4, ex as u32);
        speech.set(0x8, ey as u32);

        // ---- decals --------------------------------------------------------------------------------------------------------
        let mut decal_flags = vec![0u8; 128 * 128];
        let mut decals = Vec::new();
        for d in &self.decals {
            decal_flags[(d.cell.1 as usize & 127) * 128 + (d.cell.0 as usize & 127)] = 1;
            let mut o = Object::blank(Class::Decal);
            o.set(0x4, d.x as u32);
            o.set(0x8, d.y as u32);
            o.set(0xc, d.frame as u32);
            o.set(0x10, 1);
            decals.push(o);
        }

        // ---- buildings -----------------------------------------------------------------------------------------------------
        let mut buildings: Vec<(u32, Vec<(usize, Object)>)> = vec![(0, Vec::new()), (0, Vec::new()), (0, Vec::new())];
        for (i, s) in self.structures.iter().enumerate().filter(|(_, s)| s.present()) {
            let Some(hd) = self.building_handle(i) else { continue };
            let (slot, _, owner, _, _) = split_handle(hd);
            let (fw, fh) = s.size();
            let mut b = Object::blank(Class::Building);
            // what the exe's constructor `0x40a330` leaves in a new building of this kind (layout mask, hotspot, animation
            // bounds, exits, turret spots, ...); the sim's own state overwrites it below
            if let Some((_, fields)) = super::building_defaults::BUILDING_DEFAULTS.iter().find(|(k, _)| *k == s.kind_id) {
                for &(off, v) in *fields {
                    b.set(off, v);
                }
            }
            b.set(0x4, s.cell.0 as u32);
            b.set(0x8, s.cell.1 as u32);
            b.set(0xc, (s.cell.0 + fw) as u32);
            b.set(0x10, (s.cell.1 + fh) as u32);
            b.set(0x14, fw as u32);
            b.set(0x18, fh as u32);
            b.set(0x68, 0); // finished: the sim has no heal-in phase
            b.set(0x69, (s.hp <= 0) as u32);
            b.set(0x120, (s.cell.1 * CELL) as u32);
            // animation state (docs/art.md): 3 working loop, 4 damaged (hp <= max / 2), 5 destroyed, 6 neutral
            let state = if s.hp <= 0 {
                5
            } else if s.owner == Owner::Neutral && opensow_formats::buildings::anim(s.kind_id).is_some_and(|a| a.bounds[7] > a.bounds[6]) {
                6
            } else if s.hp * 2 <= s.max_hp {
                4
            } else {
                3
            };
            b.set(0x48, state);
            b.set(0x6c, s.heal.max(0) as u32);
            b.set(0x70, s.max_hp as u32);
            b.set(0x74, s.hp.max(0) as u32);
            for k in 0..3 {
                b.set(0x7c + 4 * k, s.tally[k as usize].max(0) as u32);
            }
            b.set(0x8c, owner);
            b.set(0x90, hd);
            b.set(0x94, s.kind_id as u32);
            b.set(0xfc, s.advancer.and_then(|id| unit_handles.get(&id).copied()).unwrap_or(0));
            if let Some((counter, passes)) = s.dying {
                b.set(0x104, 2);
                b.set(0x108, if matches!(s.kind, ObjectKind::Turret(_)) { 2 } else { 5 });
                b.set(0x10c, counter as u32);
                b.set(0x110, passes as u32);
            }
            b.set(0x114, s.protected as u32);
            b.set(0x118, s.attacked as u32);
            b.set(0x11c, s.sat_cd.max(0) as u32);
            b.set(0x140, s.slots.len() as u32);
            b.set(0x144, s.active as u32);
            b.set(0x148, s.upgrade.max(0) as u32);
            let (rx, ry) = s.rally.unwrap_or((-1, -1));
            b.set(0x2c0, rx as u32);
            b.set(0x2c4, ry as u32);
            for (k, slot_state) in s.slots.iter().take(5).enumerate() {
                let mut part = Object::blank(Class::BuildingPart);
                part.set(0x4, slot_state.kind as u32);
                part.set(0xc, slot_state.target);
                part.set(0x10, slot_state.progress);
                part.set(0x14, 30);
                part.set(0x18, (slot_state.progress >= slot_state.target && slot_state.target > 0) as u32);
                part.set(0x1c, slot_unlock_cost(slot_state.kind).unwrap_or(0)); // the unlock price (0x40e730); `waiting` is sim-only
                part.set(0x20, slot_state.level);
                part.set(0x24, slot_state.max_level);
                b.children.push((0x15c + 0x28 * k as u32, part));
            }
            let g = &mut buildings[owner as usize];
            g.0 = g.0.max(slot + 1);
            g.1.push((slot as usize, b));
            counters[1][owner as usize * SLOTS[1] + slot as usize] = 1;
        }
        for g in &mut buildings {
            g.1.sort_by_key(|x| x.0);
        }

        // ---- units (and the turret guns) -----------------------------------------------------------------------------------
        let mut units: Vec<(u32, Vec<(usize, Object)>)> = vec![(0, Vec::new()), (0, Vec::new()), (0, Vec::new())];
        for (n, u) in self.units.iter().enumerate() {
            let (p, slot) = unit_slots[n];
            let o = self.unit_record(u, unit_handles[&u.id], &unit_handles);
            units[p].0 = units[p].0.max(slot as u32 + 1);
            units[p].1.push((slot, o));
        }
        for &(i, p, slot) in &gun_slots {
            let s = &self.structures[i];
            let kind = 108
                + match s.kind {
                    ObjectKind::Turret(n) => n as i32,
                    _ => 1,
                };
            let (cx, cy) = s.center();
            let mut o = self.new_unit(kind, p as i32, true);
            let hd = handle(1, unit_handle_type(kind), p as u32, 0, slot as u32);
            self.fill_unit(&mut o, hd, kind, p as i32, cx, cy, s.hp, s.max_hp);
            o.set(0x68, self.building_handle(i).unwrap_or(0));
            o.set(0x7c, self.target_handle(&s.target, &unit_handles));
            if let (Some(a), Some(top)) = (&s.aim, o.child_mut(4)) {
                set_top(top, a, self.target_handle(&s.target, &unit_handles));
            }
            units[p].0 = units[p].0.max(slot as u32 + 1);
            units[p].1.push((slot, o));
        }
        // dead units keep their slot while the corpse lasts: dead flag `+0x2c`, corpse timer / variant / blasts thrown `+0x90..0x98`
        for c in &self.corpses {
            let p = player_slot(c.player);
            let slot = unit_slot[p];
            if slot >= SLOTS[0] {
                continue;
            }
            unit_slot[p] += 1;
            counters[0][p * SLOTS[0] + slot] = 1;
            let mut o = self.new_unit(c.kind, c.player, false);
            let hd = handle(1, unit_handle_type(c.kind), p as u32, 0, slot as u32);
            self.fill_unit(&mut o, hd, c.kind, c.player, c.x, c.y, 0, 0);
            o.set(0x2c, 1);
            o.set(0x4b, c.air as u32);
            o.set(0x90, c.left.max(0) as u32);
            o.set(0x94, c.variant);
            o.set(0x98, c.blasted as u32);
            units[p].0 = units[p].0.max(slot as u32 + 1);
            units[p].1.push((slot, o));
        }
        for g in &mut units {
            g.1.sort_by_key(|x| x.0);
        }
        // the turret guns' look-around timers (`[unit+0x8c]`)
        for &(i, p, slot) in &gun_slots {
            if let (Some(&t), Some(o)) = (self.turret_look.get(&i), units[p].1.iter_mut().find(|x| x.0 == slot)) {
                o.1.set(0x8c, t.max(0) as u32);
            }
        }
        // buildings that blink after being given as a target (`[b+0x14c..0x158]`)
        for (&i, bl) in &self.blinks {
            if let Some(hd) = self.building_handle(i) {
                let (slot, _, owner, _, _) = split_handle(hd);
                if let Some(o) = buildings[owner as usize].1.iter_mut().find(|x| x.0 == slot as usize) {
                    o.1.set(0x14c, 1);
                    o.1.set(0x150, bl.timer.max(0) as u32);
                    o.1.set(0x154, bl.on as u32);
                    o.1.set(0x158, bl.left.max(0) as u32);
                }
            }
        }
        // ---- bullets (counters[2]) and aircraft squads (counters[3]) ---------------------------------------------------------
        // an aircraft's handle in the stream (side, squad slot): bullets aimed at it store it
        let mut next_slot = [0usize; 3];
        let mut air_handles: HashMap<u32, u32> = HashMap::new();
        for a in &self.aircraft {
            let p = player_slot(a.side);
            if next_slot[p] < SLOTS[3] {
                air_handles.insert(a.id, handle(4, 0, p as u32, 0, next_slot[p] as u32));
                next_slot[p] += 1;
            }
        }
        let mut bullets: Vec<(u32, Vec<(usize, Object)>)> = vec![(0, Vec::new()), (0, Vec::new()), (0, Vec::new())];
        let mut next_slot = [0usize; 3];
        for b in &self.bullets {
            let p = player_slot(b.owner);
            let slot = next_slot[p];
            if slot >= SLOTS[2] {
                continue;
            }
            next_slot[p] += 1;
            bullets[p].0 = slot as u32 + 1;
            bullets[p].1.push((slot, self.bullet_record(b, &unit_handles, &air_handles)));
            counters[2][p * SLOTS[2] + slot] = 1;
        }
        for b in &self.beams {
            let p = b.side.min(2);
            let slot = next_slot[p];
            if slot >= SLOTS[2] {
                continue;
            }
            next_slot[p] += 1;
            bullets[p].0 = slot as u32 + 1;
            bullets[p].1.push((slot, self.beam_record(b)));
            counters[2][p * SLOTS[2] + slot] = 1;
        }
        for n in &self.nukes {
            let p = player_slot(n.owner);
            let slot = next_slot[p];
            if slot >= SLOTS[2] {
                continue;
            }
            next_slot[p] += 1;
            bullets[p].0 = slot as u32 + 1;
            bullets[p].1.push((slot, nuke_record(n)));
            counters[2][p * SLOTS[2] + slot] = 1;
        }
        let mut squads: Vec<(u32, Vec<(usize, Object)>)> = vec![(0, Vec::new()), (0, Vec::new()), (0, Vec::new())];
        let mut next_slot = [0usize; 3];
        for a in &self.aircraft {
            let p = player_slot(a.side);
            let slot = next_slot[p];
            if slot >= SLOTS[3] {
                continue;
            }
            next_slot[p] += 1;
            squads[p].0 = slot as u32 + 1;
            squads[p].1.push((slot, aircraft_record(a, air_handles[&a.id])));
            counters[3][p * SLOTS[3] + slot] = 1;
        }
        // the aircraft an anti-air top keeps aiming at: `[unit+0x80]`
        for (&(is_structure, key), &air_id) in &self.air_targets {
            let Some(&ah) = air_handles.get(&air_id) else { continue };
            if is_structure {
                if let Some(&(_, p, slot)) = gun_slots.iter().find(|g| g.0 == key) {
                    if let Some(o) = units[p].1.iter_mut().find(|x| x.0 == slot) {
                        o.1.set(0x80, ah);
                    }
                }
            } else if let Some(n) = self.units.iter().position(|u| u.id == key as u32) {
                let (p, slot) = unit_slots[n];
                if let Some(o) = units[p].1.iter_mut().find(|x| x.0 == slot) {
                    o.1.set(0x80, ah);
                }
            }
        }

        let player_arrays = (0..3)
            .map(|p| {
                [
                    counters[0][p * SLOTS[0]..(p + 1) * SLOTS[0]].to_vec(),
                    counters[1][p * SLOTS[1]..(p + 1) * SLOTS[1]].to_vec(),
                    counters[2][p * SLOTS[2]..(p + 1) * SLOTS[2]].to_vec(),
                    counters[3][p * SLOTS[3]..(p + 1) * SLOTS[3]].to_vec(),
                ]
            })
            .collect();
        State {
            head,
            player_words,
            player_arrays,
            lists: vec![[Vec::new(), Vec::new()], [Vec::new(), Vec::new()], [Vec::new(), Vec::new()]],
            map,
            players,
            stocks,
            params,
            signature: progress.write(|| 0),
            speech: vec![speech],
            decal_flags,
            units,
            buildings,
            bullets,
            squads,
            decals,
            clouds: self
                .sky
                .clouds
                .iter()
                .map(|c| {
                    let mut o = Object::blank(Class::Cloud);
                    o.set(0x4, c.x as u32);
                    o.set(0x8, c.y as u32);
                    o.set(0xc, c.speed);
                    o
                })
                .collect(),
        }
    }

    /// What the exe's unit initialiser `0x46bd30` leaves in a new unit of `kind` for `player` (`UNIT_DEFAULTS`, generated by
    /// `scripts/gen_unit_defaults.py`): the Unit record and, with `parts`, its Top (with the embedded weapon) and Chassis (with the
    /// current Order and, for the Advancer, the beam weapon) records. The sprite size words come from the install's sheets
    /// (`World::sheet_sizes`, 0 / 1 when it has none). A kind the table does not know gets an empty Unit.
    fn new_unit(&self, kind: i32, player: i32, parts: bool) -> Object {
        use super::unit_defaults::{Part, UNIT_DEFAULTS};
        let mut unit = Object::blank(Class::Unit);
        let Some((_, has_top, has_chassis, rows)) = UNIT_DEFAULTS.iter().find(|r| r.0 == kind) else { return unit };
        let slot = player_slot(player) as u32;
        let part = |p: Part, o: &mut Object| {
            for &(q, off, v) in rows.iter() {
                if q == p {
                    o.set(off, v);
                }
            }
        };
        part(Part::Unit, &mut unit);
        if !parts {
            return unit;
        }
        if *has_top {
            let mut top = Object::blank(Class::Top);
            part(Part::Top, &mut top);
            let (w, h) = self.sheet_sizes.top(top.get(0x54).unwrap_or(0) as usize);
            top.set(0x64, w);
            top.set(0x68, h);
            let mut weapon = Object::blank(Class::Weapon);
            part(Part::TopWeapon, &mut weapon);
            weapon.set(0xc, slot);
            top.children.push((8, weapon));
            unit.children.push((4, top));
        }
        if *has_chassis {
            let mut ch = Object::blank(Class::Chassis);
            part(Part::Chassis, &mut ch);
            ch.set(0x10, slot);
            let (w, h) = self.sheet_sizes.chassis(ch.get(0x1c).unwrap_or(0) as usize);
            ch.set(0x20, w);
            ch.set(0x24, h);
            let mut order = Object::blank(Class::Order);
            part(Part::Order, &mut order);
            ch.children.push((0xd8, order));
            if rows.iter().any(|r| r.0 == Part::ChassisWeapon) {
                let mut weapon = Object::blank(Class::Weapon);
                part(Part::ChassisWeapon, &mut weapon);
                weapon.set(0xc, slot);
                ch.children.push((0x104, weapon));
            }
            unit.children.push((8, ch));
        }
        unit
    }

    /// The scalar fields every unit record has (position, hit points, kind, player, handle, constructor values).
    fn fill_unit(&self, o: &mut Object, hd: u32, kind: i32, player: i32, x: i32, y: i32, hp: i32, max_hp: i32) {
        o.set(0x14, x as u32);
        o.set(0x18, y as u32);
        for (off, v) in [(0x1c, -16i32), (0x20, -16), (0x24, 32), (0x28, 30)] {
            o.set(off, v as u32);
        }
        o.set(0x30, max_hp as u32);
        o.set(0x34, hp.max(0) as u32);
        o.set(0x38, if kind == kinds::ADVANCER { 20 } else { 16 });
        o.set(0x3c, hd);
        o.set(0x40, kind as u32);
        o.set(0x44, player_slot(player) as u32);
        o.set(0x4b, (kind == kinds::ADVANCER) as u32);
        o.set(0x4c, is_anti_air(kind) as u32);
        o.set(0x50, 5);
        o.set(0x64, if kind == kinds::ADVANCER { -23i32 as u32 } else { -25i32 as u32 });
        o.set(0x88, -64i32 as u32);
    }

    /// The chassis' local route `[chassis+0x9c]` (`0x46ee80`): the pixel line to the step cell as the exe keeps it. The walking step
    /// pops one point per pixel moved, so mid-segment the list holds the points still to walk; right after the segment was planned
    /// (the sim builds its line on the first move) it still holds the start point first, which is the unit's own pixel.
    fn local_route(&self, u: &Unit) -> Vec<(u32, u32)> {
        let pts: Vec<(i32, i32)> = match (&u.line, u.step) {
            (Some(l), _) => l.rest.iter().rev().copied().collect(),
            (None, Some((sx, sy))) => {
                let (here, goal) = ((u.x.round() as i32, u.y.round() as i32), (sx.round() as i32, sy.round() as i32));
                if here == goal {
                    Vec::new()
                } else {
                    opensow_formats::walk::pixel_line(here.0, here.1, goal.0, goal.1)
                }
            }
            (None, None) => Vec::new(),
        };
        pts.into_iter().map(|(x, y)| (x as u32, y as u32)).collect()
    }

    fn unit_record(&self, u: &Unit, hd: u32, unit_handles: &HashMap<u32, u32>) -> Object {
        let mut o = self.new_unit(u.kind, u.player, true);
        self.fill_unit(&mut o, hd, u.kind, u.player, u.x.floor() as i32, u.y.floor() as i32, u.hp, u.max_hp);
        o.set(0x48, u.selected as u32);
        o.set(0x7c, self.target_handle(&u.chase, unit_handles));
        o.set(0x84, u.hover_phase);
        o.set(0x8c, u.look.max(0) as u32);
        if let (Some(a), Some(top)) = (&u.aim, o.child_mut(4)) {
            set_top(top, a, self.target_handle(&u.target, unit_handles));
        }
        if let Some(c) = &u.chassis {
            // the unit initialiser's Chassis (or a blank one for a kind the table does not know)
            let mut ch = o.children.iter().position(|x| x.0 == 8).map(|i| o.children.remove(i).1).unwrap_or_else(|| Object::blank(Class::Chassis));
            let (turn, anim, steps) = c.counters();
            ch.set(0x14, hd);
            ch.set(0x8, steps);
            ch.set(0xc, u.kind as u32);
            ch.set(0x10, (u.player != 0) as u32);
            ch.set(0x30, c.facing);
            ch.set(0x34, c.target);
            ch.set(0x38, turn as u32);
            ch.set(0x44, c.phase);
            ch.set(0x4c, anim as u32);
            let (cx, cy) = u.cell();
            ch.set(0x64, cx as u32);
            ch.set(0x68, cy as u32);
            if let Some((dx, dy)) = u.dest {
                ch.set(0x5c, dx as u32);
                ch.set(0x60, dy as u32);
            }
            if let Some((sx, sy)) = u.step {
                ch.set(0x6c, (sx.floor() as i32).div_euclid(CELL) as u32);
                ch.set(0x70, (sy.floor() as i32).div_euclid(CELL) as u32);
            } else {
                ch.set(0x6c, cx as u32);
                ch.set(0x70, cy as u32);
            }
            ch.set(0x7c, u.step.is_some() as u32);
            ch.set(0x80, u.cool.max(0) as u32);
            ch.set(0x150, u.idle as u32);
            let mut cur = Object::blank(Class::Order);
            if let Some((dx, dy)) = u.dest {
                cur.set(0x4, 1);
                cur.set(0x8, 1);
                cur.set(0xc, dx as u32);
                cur.set(0x10, dy as u32);
            }
            ch.children.retain(|x| x.0 != 0xd8);
            ch.children.push((0xd8, cur));
            ch.local_route = self.local_route(u);
            ch.set(0xa0, ch.local_route.len() as u32);
            ch.route = u.path.iter().map(|&(px, py)| ((px.floor() as i32).div_euclid(CELL) as u32, (py.floor() as i32).div_euclid(CELL) as u32)).collect();
            ch.set(0x8c, ch.route.len() as u32);
            if u.kind == kinds::ADVANCER {
                let idx = ch.children.iter().position(|x| x.0 == 0x104);
                let mut w = idx.map(|i| ch.children.remove(i).1).unwrap_or_else(|| Object::blank(Class::Weapon));
                w.set(0x4, u.beam.enabled as u32);
                w.set(0x1c, u.beam.counter as u32);
                w.set(0x20, u.beam.reload as u32);
                w.set(0x40, u.beam.state as u32);
                ch.children.push((0x104, w));
                if let Some((action, si)) = u.action {
                    let st = &self.structures[si];
                    let (ax, ay) = st.action_cell();
                    ch.set(0xf4, (ax * CELL + CELL / 2) as u32);
                    ch.set(0xf8, (ay * CELL + CELL / 2) as u32);
                    ch.set(
                        0xfc,
                        match action {
                            Action::Destroy => 1,
                            Action::Repair => 2,
                            Action::Upgrade => 3,
                        },
                    );
                    ch.set(0x100, self.building_handle(si).unwrap_or(0));
                }
            }
            // the order list: a type-2 straight flight first (then the route to its cell), then the rally order (docs/rally.md)
            let order = |x: i32, y: i32| {
                let mut ord = Object::blank(Class::Order);
                ord.set(0x4, 1);
                ord.set(0x8, 1);
                ord.set(0xc, x as u32);
                ord.set(0x10, y as u32);
                ord
            };
            if let Some((cx, cy)) = u.straight {
                ch.orders.push(order(cx * CELL + CELL / 2, cy * CELL + CELL / 2));
            }
            if let Some((qx, qy)) = u.queued {
                ch.orders.push(order(qx, qy));
            }
            ch.set(0xcc, ch.orders.len() as u32);
            if u.straight.is_some() {
                let cur = ch.children.iter_mut().find(|c| c.0 == 0xd8).map(|c| &mut c.1).unwrap();
                cur.set(0x4, 1);
                cur.set(0x8, 2);
                cur.set(0xc, u.x as i32 as u32);
                cur.set(0x10, u.y as i32 as u32);
            }
            o.children.push((8, ch));
        }
        o
    }

    /// Overwrites this (freshly built, same level) world with a loaded state stream. Returns an error for a stream of another map.
    pub fn apply_state(&mut self, st: &State) -> Result<(), String> {
        if (st.map.width as i32, st.map.height as i32) != self.cells {
            return Err("the savegame is for another map".into());
        }
        self.carry_counter = st.head.v_89cf50 as usize;
        self.rng_index = st.head.rng_index as usize;
        if let Some(r) = self.game_rng.as_mut() {
            r.set_index(st.head.rng_index as usize);
        }
        for p in 0..2 {
            self.credits[p] = st.player_words[p][0] as i32;
            self.research[p] = st.player_words[p][1] as i32;
            self.wind[p] = st.player_words[p][2];
            for k in 0..8 {
                self.stats[p][k] = st.params[p].get(4 + 4 * k as u32).unwrap_or(0);
            }
        }
        for k in 0..8 {
            self.stats[2][k] = st.params[2].get(4 + 4 * k as u32).unwrap_or(0);
        }
        self.strike_lock = st.players[0].get(0x40).unwrap_or(0) as i32;
        self.ai_strategy = st.players[1].get(0x38).unwrap_or(0);
        self.ai_timer = st.players[1].get(0x3c).unwrap_or(0) as i32;
        if let Some(t) = self.timer.as_mut() {
            t.0 = st.head.v_5baac4 as i32;
            t.1 = st.head.v_5baac8 as i32;
        }
        // the exe re-rolls a cloud's picture on load and trims the list to the options (0x453060)
        self.sky.clouds = st
            .clouds
            .iter()
            .map(|o| opensow_formats::clouds::Cloud {
                x: o.get(0x4).unwrap_or(0) as i32,
                y: o.get(0x8).unwrap_or(0) as i32,
                speed: o.get(0xc).unwrap_or(10),
                picture: 0,
            })
            .collect();
        if let Some(sp) = st.speech.first() {
            let (x, y) = (sp.get(0x4).unwrap_or(u32::MAX) as i32, sp.get(0x8).unwrap_or(u32::MAX) as i32);
            self.last_event = (x != -1 || y != -1).then_some((x, y));
        }
        let (mut quake, cam) = (Quake::default(), st.map.cameras.first());
        if let Some(c) = cam {
            quake.ticks = c.quake_left as i32;
            quake.at = (c.quake_x as i32, c.quake_y as i32);
        }
        self.quake = quake;
        self.decals = st
            .decals
            .iter()
            .map(|o| {
                let (x, y) = (o.get(0x4).unwrap_or(0) as i32, o.get(0x8).unwrap_or(0) as i32);
                Decal { x, y, cell: (x.div_euclid(CELL), y.div_euclid(CELL)), frame: o.get(0xc).unwrap_or(0) as u8 }
            })
            .collect();

        // buildings: match the level's structures by cell and kind; the slot tables come from the stream's slots
        let mut tables: [Vec<Option<usize>>; 3] = Default::default();
        let mut seen = vec![false; self.structures.len()];
        let mut advancer_handles: Vec<(usize, u32)> = Vec::new();
        for (owner, (_, objs)) in st.buildings.iter().enumerate() {
            for (slot, b) in objs {
                let cell = (b.get(0x4).unwrap_or(0) as i32, b.get(0x8).unwrap_or(0) as i32);
                let kind = b.get(0x94).unwrap_or(0) as i32;
                let Some(i) = (0..self.structures.len()).find(|&i| !seen[i] && self.structures[i].cell == cell && self.structures[i].kind_id == kind) else {
                    continue;
                };
                seen[i] = true;
                if tables[owner].len() <= *slot {
                    tables[owner].resize(slot + 1, None);
                }
                tables[owner][*slot] = Some(i);
                let s = &mut self.structures[i];
                s.hp = b.get(0x74).unwrap_or(0) as i32;
                s.max_hp = b.get(0x70).unwrap_or(0) as i32;
                s.heal = b.get(0x6c).unwrap_or(0) as i32;
                s.tally = std::array::from_fn(|k| b.get(0x7c + 4 * k as u32).unwrap_or(0) as i32);
                s.upgrade = b.get(0x148).unwrap_or(0) as i32;
                s.protected = b.get(0x114).unwrap_or(0) != 0;
                s.attacked = b.get(0x118).unwrap_or(0) != 0;
                s.sat_cd = b.get(0x11c).unwrap_or(0) as i32;
                s.active = b.get(0x144).unwrap_or(0) as usize;
                let (rx, ry) = (b.get(0x2c0).unwrap_or(u32::MAX) as i32, b.get(0x2c4).unwrap_or(u32::MAX) as i32);
                s.rally = (rx != -1 || ry != -1).then_some((rx, ry));
                s.dying = (b.get(0x104) == Some(2)).then(|| (b.get(0x10c).unwrap_or(0) as i32, b.get(0x110).unwrap_or(0) as i32));
                for (k, slot_state) in s.slots.iter_mut().enumerate() {
                    if let Some(part) = b.child(0x15c + 0x28 * k as u32) {
                        slot_state.kind = part.get(0x4).unwrap_or(0) as i32;
                        slot_state.target = part.get(0xc).unwrap_or(0);
                        slot_state.progress = part.get(0x10).unwrap_or(0);
                        slot_state.level = part.get(0x20).unwrap_or(0);
                        slot_state.max_level = part.get(0x24).unwrap_or(0);
                    }
                }
                advancer_handles.push((i, b.get(0xfc).unwrap_or(0)));
            }
        }
        for (i, s) in self.structures.iter_mut().enumerate() {
            if !seen[i] {
                s.hp = 0;
                s.dying = None;
            }
        }
        self.slot_tables = tables;

        self.blinks.clear();
        for (owner, (_, objs)) in st.buildings.iter().enumerate() {
            for (slot, b) in objs {
                if b.get(0x14c) == Some(1) {
                    if let Some(Some(i)) = self.slot_tables[owner].get(*slot).copied() {
                        self.blinks.insert(
                            i,
                            Blink { timer: b.get(0x150).unwrap_or(0) as i32, on: b.get(0x154).unwrap_or(0) != 0, left: b.get(0x158).unwrap_or(0) as i32 },
                        );
                    }
                }
            }
        }
        // units: replace them all; their ids are their handles
        self.units.clear();
        self.corpses.clear();
        self.turret_look.clear();
        let mut guns: Vec<(usize, &Object)> = Vec::new();
        for (p, (_, objs)) in st.units.iter().enumerate() {
            for (slot, o) in objs {
                let kind = o.get(0x40).unwrap_or(0) as i32;
                let hd = handle(1, unit_handle_type(kind), p as u32, 0, *slot as u32);
                if o.get(0x2c).unwrap_or(0) != 0 {
                    self.corpses.push(Corpse {
                        x: o.get(0x14).unwrap_or(0) as i32,
                        y: o.get(0x18).unwrap_or(0) as i32,
                        left: o.get(0x90).unwrap_or(0) as i32,
                        variant: o.get(0x94).unwrap_or(0),
                        air: o.get(0x4b).unwrap_or(0) != 0,
                        blasted: o.get(0x98).unwrap_or(0) != 0,
                        player: p as i32,
                        kind,
                    });
                    continue;
                }
                if kinds::TURRET_TOPS.contains(&kind) {
                    if let Some(i) = self.structure_of_handle(o.get(0x68).unwrap_or(0)) {
                        self.turret_look.insert(i, o.get(0x8c).unwrap_or(0) as i32);
                        guns.push((i, o));
                    }
                    continue;
                }
                let unit = self.unit_from_record(o, hd, p as i32);
                self.units.push(unit);
            }
        }
        self.next_id = self.next_id.max(1);
        for (i, o) in guns {
            if let (Some(top), Some(a)) = (o.child(4), self.structures[i].aim.as_mut()) {
                apply_top(a, top);
            }
            self.structures[i].target = self.target_of_handle(o.get(0x7c).unwrap_or(0));
        }
        for (i, hd) in advancer_handles {
            self.structures[i].advancer = (hd != 0).then_some(hd).filter(|h| self.units.iter().any(|u| u.id == *h));
        }
        self.nukes = st
            .bullets
            .iter()
            .flat_map(|g| g.1.iter())
            .filter(|(_, o)| o.get(0x2658) == Some(opensow_formats::rogon::BULLET_TYPE as u32))
            .map(|(_, o)| Nuke {
                x: o.get(0x269c).unwrap_or(0) as i32,
                y: o.get(0x26a0).unwrap_or(0) as i32,
                owner: o.get(0x266c).unwrap_or(0) as i32,
                ex: opensow_formats::rogon::Explosion::from_parts(o.get(0x26c4).unwrap_or(0) as i32, o.get(0x26c8).unwrap_or(0) as usize),
            })
            .collect();
        self.bullets = st
            .bullets
            .iter()
            .flat_map(|g| g.1.iter())
            .filter(|(_, o)| !(19..=21).contains(&o.get(0x2658).unwrap_or(0)) && o.get(0x2658) != Some(35))
            .filter_map(|(_, o)| self.bullet_from_record(o))
            .collect();
        self.beams = st
            .bullets
            .iter()
            .flat_map(|g| g.1.iter())
            .filter(|(_, o)| (19..=21).contains(&o.get(0x2658).unwrap_or(0)))
            .filter_map(|(_, o)| self.beam_from_record(o))
            .collect();
        let mut aircraft = Vec::new();
        for (side, g) in st.squads.iter().enumerate() {
            for (slot, o) in &g.1 {
                let hd = handle(4, 0, side as u32, 0, *slot as u32);
                aircraft.extend(self.aircraft_from_record(o, side as i32, hd));
            }
        }
        self.aircraft = aircraft;
        // kept anti-air targets (`[unit+0x80]`)
        self.air_targets.clear();
        for g in &st.units {
            for (_, o) in &g.1 {
                let ah = o.get(0x80).unwrap_or(0);
                if ah == 0 || !self.aircraft.iter().any(|a| a.id == ah) {
                    continue;
                }
                let kind = o.get(0x40).unwrap_or(0) as i32;
                if kinds::TURRET_TOPS.contains(&kind) {
                    if let Some(i) = self.structure_of_handle(o.get(0x68).unwrap_or(0)) {
                        self.air_targets.insert((true, i), ah);
                    }
                } else {
                    let hd = handle(1, unit_handle_type(kind), split_handle(o.get(0x3c).unwrap_or(0)).2, 0, split_handle(o.get(0x3c).unwrap_or(0)).0);
                    self.air_targets.insert((false, hd as usize), ah);
                }
            }
        }
        self.occ = None;
        Ok(())
    }

    /// A `Bullet` record: path progress / length in coordinate words (`2 * index`, `2 * points`), the lift array, the remaining path
    /// points (the exe keeps only those it has not flown), type, strength, speed, owner, animation, variants, target handle.
    fn bullet_record(&self, b: &Bullet, unit_handles: &HashMap<u32, u32>, air_handles: &HashMap<u32, u32>) -> Object {
        let mut o = Object::blank(Class::Bullet);
        let n = b.path.points.len();
        o.set(0x2644, 2 * b.index as u32);
        o.set(0x2648, 2 * n as u32);
        let (px, py) = b.position();
        o.set(0x4, px as u32);
        o.set(0x8, py as u32);
        o.set(0x264c, b.trail as u32);
        o.set(0x2650, bullet_type_interval(b.bullet_type) as u32);
        o.set(0x2654, b.facing as u32);
        o.set(0x2658, b.bullet_type as u32);
        o.set(0x2660, b.strength as u32);
        o.set(0x2664, b.speed);
        o.set(0x266c, b.owner as u32);
        if let Some(a) = &b.anim {
            for (k, v) in a.parts().iter().enumerate() {
                o.set(0x2670 + 4 * k as u32, *v as u32);
            }
        }
        o.set(0x2680, b.variant);
        let target = match b.target {
            Some(HitTarget::Unit(id)) => unit_handles.get(&id).copied().unwrap_or(0),
            Some(HitTarget::Aircraft(id)) => air_handles.get(&id).copied().unwrap_or(0),
            None => 0,
        };
        o.set(0x2688, target);
        for (i, l) in b.path.lift.iter().enumerate().take(n) {
            o.set(0xc4 + 4 * i as u32, *l as u32);
        }
        for k in b.index..n {
            let (x, y) = b.path.points[k];
            o.set((0xd44 + 8 * k) as u32, x as u32);
            o.set((0xd44 + 8 * k + 4) as u32, y as u32);
        }
        o
    }

    /// An Advancer shot as the exe's bullet of type 19 / 20 / 21: strength is what one landing does (repair `2 * rate`, upgrade
    /// `rate`, destroy `rate`), the target handle the building's, the spark arrays `+0xc`, `+0x48`, `+0x84` (offset x, y, frame) with
    /// their count in `+0x268c`.
    fn beam_record(&self, b: &BeamShot) -> Object {
        let mut o = Object::blank(Class::Bullet);
        let n = b.path.points.len();
        o.set(0x2644, 2 * b.index as u32);
        o.set(0x2648, 2 * n as u32);
        let (px, py) = b.path.points[b.index.min(n - 1)];
        o.set(0x4, px as u32);
        o.set(0x8, py as u32);
        o.set(0x264c, b.trail as u32);
        o.set(0x2658, b.action as u32);
        o.set(0x2660, if b.action == Action::Repair { hp_per_event(b.rate) } else { b.rate } as u32);
        o.set(0x2664, 60);
        o.set(0x266c, b.side as u32);
        o.set(0x2688, self.building_handle(b.target).unwrap_or(0));
        o.set(0x268c, b.frags.len().min(15) as u32);
        for (k, &(dx, dy, frame)) in b.frags.iter().take(15).enumerate() {
            o.set(0xc + 4 * k as u32, dx as u32);
            o.set(0x48 + 4 * k as u32, dy as u32);
            o.set(0x84 + 4 * k as u32, frame as u32);
        }
        for (i, l) in b.path.lift.iter().enumerate().take(n) {
            o.set(0xc4 + 4 * i as u32, *l as u32);
        }
        for k in b.index..n {
            let (x, y) = b.path.points[k];
            o.set((0xd44 + 8 * k) as u32, x as u32);
            o.set((0xd44 + 8 * k + 4) as u32, y as u32);
        }
        o
    }

    fn beam_from_record(&self, o: &Object) -> Option<BeamShot> {
        let action = match o.get(0x2658)? {
            19 => Action::Repair,
            20 => Action::Upgrade,
            21 => Action::Destroy,
            _ => return None,
        };
        let bullet = self.bullet_from_record(o)?;
        let strength = o.get(0x2660).unwrap_or(0) as i32;
        let rate = if action == Action::Repair { strength / 2 } else { strength };
        let count = (o.get(0x268c).unwrap_or(0) as usize).min(15);
        let frags = (0..count as u32)
            .map(|k| (o.get(0xc + 4 * k).unwrap_or(0) as i32, o.get(0x48 + 4 * k).unwrap_or(0) as i32, o.get(0x84 + 4 * k).unwrap_or(0) as u16))
            .collect();
        Some(BeamShot {
            action,
            target: self.structure_of_handle(o.get(0x2688).unwrap_or(0))?,
            side: bullet.owner.max(0) as usize,
            rate,
            path: bullet.path,
            index: bullet.index,
            trail: bullet.trail,
            frags,
        })
    }

    fn structure_of_handle(&self, h: u32) -> Option<usize> {
        let (slot, _, owner, _, class) = split_handle(h);
        if class != 2 {
            return None;
        }
        self.slot_tables.get(owner as usize)?.get(slot as usize).copied().flatten()
    }

    fn target_of_handle(&self, h: u32) -> Option<Target> {
        if h == 0 {
            return None;
        }
        match split_handle(h).4 {
            1 => Some(Target::Unit(h)),
            2 => self.structure_of_handle(h).map(Target::Structure),
            _ => None,
        }
    }

    fn unit_from_record(&self, o: &Object, hd: u32, player: i32) -> Unit {
        let kind = o.get(0x40).unwrap_or(0) as i32;
        let params = self.walk_params_of(kind);
        let mut chassis = None;
        let mut path = Vec::new();
        let (mut dest, mut step, mut cool, mut idle) = (None, None, 0, 0);
        let mut line = None;
        let mut straight: Option<(i32, i32)> = None;
        let mut queued: Option<(i32, i32)> = None;
        let (mut beam, mut action): (BeamTimer, Option<(Action, usize)>) = (BeamTimer::default(), None);
        if let Some(ch) = o.child(8) {
            let steps = ch.get(0x8).unwrap_or(32).max(1);
            if let Some(p) = params {
                let mut c = ChassisAnim::new(p, steps, ch.get(0x30).unwrap_or(0));
                c.target = ch.get(0x34).unwrap_or(0);
                c.phase = ch.get(0x44).unwrap_or(0);
                c.set_counters(ch.get(0x38).unwrap_or(0) as i32, ch.get(0x4c).unwrap_or(0) as i32);
                chassis = Some(c);
            }
            let cur = ch.child(0xd8);
            let cur_type = cur.and_then(|c| if c.get(0x4) == Some(1) { c.get(0x8) } else { None });
            if cur_type == Some(1) {
                dest = Some((ch.get(0x5c).unwrap_or(0) as i32, ch.get(0x60).unwrap_or(0) as i32));
            }
            // the order list: after a type-2 current order the first entry is the route cell, the next one the rally order
            let mut list = ch.orders.iter().map(|o| (o.get(0xc).unwrap_or(0) as i32, o.get(0x10).unwrap_or(0) as i32));
            if cur_type == Some(2) {
                straight = list.next().map(|(x, y)| (x.div_euclid(CELL), y.div_euclid(CELL)));
            }
            queued = list.next();
            if kind == kinds::ADVANCER {
                if let Some(w) = ch.child(0x104) {
                    beam = BeamTimer {
                        enabled: w.get(0x4).unwrap_or(1) != 0,
                        state: w.get(0x40).unwrap_or(0) as i32,
                        counter: w.get(0x1c).unwrap_or(0) as i32,
                        reload: w.get(0x20).unwrap_or(10) as i32,
                    };
                }
                let mode = ch.get(0xfc).unwrap_or(0);
                let target = self.structure_of_handle(ch.get(0x100).unwrap_or(0));
                action = match (mode, target) {
                    (1, Some(i)) => Some((Action::Destroy, i)),
                    (2, Some(i)) => Some((Action::Repair, i)),
                    (3, Some(i)) => Some((Action::Upgrade, i)),
                    _ => None,
                };
            }
            let (sx, sy) = (ch.get(0x6c).unwrap_or(0) as i32, ch.get(0x70).unwrap_or(0) as i32);
            step = (ch.get(0x7c).unwrap_or(0) != 0).then(|| ((sx * CELL + CELL / 2) as f32, (sy * CELL + CELL / 2) as f32));
            // the pixel line: a list that starts at the unit's own pixel was just planned (the sim builds it on the first move)
            let here = (o.get(0x14).unwrap_or(0) as i32, o.get(0x18).unwrap_or(0) as i32);
            let mut pts: Vec<(i32, i32)> = ch.local_route.iter().map(|&(a, b)| (a as i32, b as i32)).collect();
            if pts.first() == Some(&here) {
                pts.clear(); // regenerated identically from the unit's pixel to the step cell on the first move
            }
            if let Some(&goal) = pts.last() {
                pts.reverse();
                line = Some(Line { goal, rest: pts });
            }
            path = ch.route.iter().map(|&(a, b)| ((a as i32 * CELL + CELL / 2) as f32, (b as i32 * CELL + CELL / 2) as f32)).collect();
            cool = ch.get(0x80).unwrap_or(0) as i32;
            idle = ch.get(0x150).unwrap_or(0) as i32;
        }
        let aim = o.child(4).and_then(|t| {
            let mut a = TopAim::new(kind, t.get(0x6c).unwrap_or(0))?;
            apply_top(&mut a, t);
            Some(a)
        });
        let chase = self.target_of_handle(o.get(0x7c).unwrap_or(0));
        let target = o.child(4).and_then(|t| self.target_of_handle(t.get(0x7c).unwrap_or(0)));
        Unit {
            id: hd,
            kind,
            player,
            x: o.get(0x14).unwrap_or(0) as i32 as f32,
            y: o.get(0x18).unwrap_or(0) as i32 as f32,
            hp: o.get(0x34).unwrap_or(0) as i32,
            max_hp: o.get(0x30).unwrap_or(0) as i32,
            chassis,
            aim,
            path,
            target,
            selected: o.get(0x48).unwrap_or(0) != 0,
            moved: false,
            line,
            step,
            dest,
            cool,
            straight,
            queued,
            hover_phase: o.get(0x84).unwrap_or(0),
            action,
            beam,
            beam_shots: 0,
            chase,
            idle,
            look: o.get(0x8c).unwrap_or(0) as i32,
        }
    }
}

/// The weapon state codes of `0x47c6b0` / `0x479c60` (aim.rs): 0 searching, 3 turning to the target, 4 firing, 6 cooling down.
fn state_code(s: &opensow_formats::aim::State) -> u32 {
    use opensow_formats::aim::State;
    match s {
        State::Idle => 0,
        State::Aiming => 3,
        State::Firing => 4,
        State::Cooldown => 6,
    }
}

fn code_state(c: u32) -> opensow_formats::aim::State {
    use opensow_formats::aim::State;
    match c {
        3 => State::Aiming,
        4 => State::Firing,
        6 => State::Cooldown,
        _ => State::Idle,
    }
}

/// Puts the aim state into a `Top` record (the unit initialiser's, `World::new_unit`): facing and target facing (`+0x6c`, `+0x70`),
/// turn counter (`+0x74`), target search counter (`+0x80`), the fire sequence tick (`+0x20c`), the target handle (`+0x7c`) and the
/// embedded weapon's state (`+0x40`) and cooldown counter (`+0x1c`, aim.rs: both live in the base class `0x47c5f0`).
fn set_top(t: &mut Object, a: &TopAim, target: u32) {
    let (turn, search, cooldown, seq_tick) = a.counters();
    t.set(0x6c, a.facing);
    t.set(0x70, a.target_facing);
    t.set(0x74, turn as u32);
    t.set(0x7c, target);
    t.set(0x80, search as u32);
    t.set(0x20c, seq_tick);
    if t.child(8).is_none() {
        t.children.push((8, Object::blank(Class::Weapon)));
    }
    let w = t.child_mut(8).unwrap();
    w.set(0x1c, cooldown as u32);
    w.set(0x40, state_code(&a.state));
}

fn apply_top(a: &mut TopAim, t: &Object) {
    a.facing = t.get(0x6c).unwrap_or(0);
    a.target_facing = t.get(0x70).unwrap_or(0);
    let w = t.child(8);
    a.state = code_state(w.and_then(|w| w.get(0x40)).unwrap_or(0));
    a.set_counters(
        t.get(0x74).unwrap_or(0) as i32,
        t.get(0x80).unwrap_or(0) as i32,
        w.and_then(|w| w.get(0x1c)).unwrap_or(0) as i32,
        t.get(0x20c).unwrap_or(0),
    );
}

/// The Rogon blast (`0x411dc0(0x23, 0, x, y, x+20, y+20, owner)`): a bullet of type 35 whose start / end coordinates are the
/// constructor's arguments (`+0x269c..+0x26a8`) and whose state machine lives in `+0x26c4` (counter) and `+0x26c8` (step k).
fn nuke_record(n: &Nuke) -> Object {
    let mut o = Object::blank(Class::Bullet);
    let (counter, k) = n.ex.parts();
    o.set(0x2658, opensow_formats::rogon::BULLET_TYPE as u32);
    o.set(0x266c, n.owner as u32);
    o.set(0x269c, n.x as u32);
    o.set(0x26a0, n.y as u32);
    o.set(0x26a4, (n.x + 20) as u32);
    o.set(0x26a8, (n.y + 20) as u32);
    o.set(0x26c4, counter as u32);
    o.set(0x26c8, k as u32);
    o
}

/// An `Aircraft` record from a squad (fields as named in `saveobj.rs`; the exhaust / glow frames `+0x50`, `+0x60`).
fn aircraft_record(a: &Aircraft, hd: u32) -> Object {
    let mut o = Object::blank(Class::Aircraft);
    o.set(0x4, a.x as u32);
    o.set(0x8, a.y as u32);
    o.set(0xc, a.target.0 as u32);
    o.set(0x10, a.target.1 as u32);
    o.set(0x14, a.altitude as u32);
    o.set(0x18, a.aim_dx as u32);
    o.set(0x1c, hd);
    o.set(0x20, a.side.clamp(0, 2) as u32);
    o.set(0x24, a.speed as u32);
    o.set(0x28, a.force.code() as u32);
    o.set(0x2c, a.hp as u32);
    o.set(0x34, a.right as u32);
    o.set(0x38, a.death_timer as u32);
    o.set(0x3c, a.cloud_timer as u32);
    o.set(0x40, a.frame as u32);
    o.set(0x44, a.frames as u32);
    o.set(0x48, a.anim_count as u32);
    o.set(0x4c, a.anim_reload as u32);
    let (rear, down) = a.glow_counts();
    o.set(0x50, a.rear_frame as u32);
    o.set(0x54, 4);
    o.set(0x58, rear as u32);
    o.set(0x5c, 2);
    o.set(0x60, a.down_frame as u32);
    o.set(0x64, 2);
    o.set(0x68, down as u32);
    o.set(0x6c, 4);
    o.set(0x70, a.counter as u32);
    o.set(0x74, a.state as u32);
    o.set(0x78, a.reach as u32);
    o.set(0x7c, a.off_x as u32);
    o.set(0x80, a.off_y as u32);
    o.set(0x84, a.armed as u32);
    o.set(0x88, a.squad as u32);
    o
}

impl World {
    fn aircraft_from_record(&self, o: &Object, side: i32, hd: u32) -> Option<Aircraft> {
        let force = AirForce::from_code(o.get(0x28)? as i32)?;
        let i = |off: u32| o.get(off).unwrap_or(0) as i32;
        let mut a = Aircraft::launch(force, i(0xc), i(0x10), side, self.cells.0, self.carry_counter);
        a.id = hd;
        a.x = i(0x4);
        a.y = i(0x8);
        a.altitude = i(0x14);
        a.aim_dx = i(0x18);
        a.speed = i(0x24);
        a.hp = i(0x2c);
        a.right = o.get(0x34).unwrap_or(0) != 0;
        a.death_timer = i(0x38);
        a.cloud_timer = i(0x3c);
        a.frame = i(0x40);
        a.frames = i(0x44);
        a.anim_count = i(0x48);
        a.anim_reload = i(0x4c);
        a.rear_frame = i(0x50);
        a.down_frame = i(0x60);
        a.set_glow_counts(i(0x58), i(0x68));
        a.counter = i(0x70);
        a.state = i(0x74);
        a.reach = i(0x78);
        a.off_x = i(0x7c);
        a.off_y = i(0x80);
        a.armed = o.get(0x84).unwrap_or(0) != 0;
        a.squad = o.get(0x88).unwrap_or(0) as usize;
        Some(a)
    }

    fn bullet_from_record(&self, o: &Object) -> Option<Bullet> {
        let ty = o.get(0x2658)? as u8;
        let len = o.get(0x2648)? as usize;
        let (n, index) = (len / 2, o.get(0x2644)? as usize / 2);
        if n == 0 || index > n {
            return None;
        }
        // the exe saved only the points still to fly: earlier ones repeat the first saved point
        let at = |k: usize| (o.get((0xd44 + 8 * k) as u32).unwrap_or(0) as i32, o.get((0xd44 + 8 * k + 4) as u32).unwrap_or(0) as i32);
        let first = if index < n { at(index) } else { at(n - 1) };
        let points: Vec<(i32, i32)> = (0..n).map(|k| if k < index { first } else { at(k) }).collect();
        let lift: Vec<i32> = (0..n).map(|k| o.get(0xc4 + 4 * k as u32).unwrap_or(0) as i32).collect();
        let path = Path { points, lift, first_step: index, facing: None };
        let mut b = Bullet::new(ty, o.get(0x2660).unwrap_or(0) as i32, o.get(0x266c).unwrap_or(0) as i32, path);
        b.index = index;
        b.trail = o.get(0x264c).unwrap_or(0) as i32;
        b.facing = o.get(0x2654).unwrap_or(0) as u8;
        b.speed = o.get(0x2664).unwrap_or(0);
        b.variant = o.get(0x2680).unwrap_or(0);
        if b.anim.is_some() {
            b.anim = Some(BulletAnim::new(std::array::from_fn(|k| o.get(0x2670 + 4 * k as u32).unwrap_or(0) as i32)));
        }
        let h = o.get(0x2688).unwrap_or(0);
        b.target = match (h, split_handle(h).4) {
            (0, _) => None,
            (_, 4) => Some(HitTarget::Aircraft(h)),
            (_, 1) => Some(HitTarget::Unit(h)),
            _ => None,
        };
        Some(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        let mut w = World::new((20, 20), vec![false; 400]);
        w.timer = Some((12, 34, 1));
        w.ai_strategy = 3;
        w.ai_timer = 777;
        w.sky.clouds = vec![
            opensow_formats::clouds::Cloud { x: -50, y: 300, speed: 17, picture: 2 },
            opensow_formats::clouds::Cloud { x: 600, y: 40, speed: 33, picture: 4 },
        ];
        w
    }

    #[test]
    fn orders_beam_ai_timer_and_clouds_survive_the_stream() {
        let mut w = world();
        let adv = w.add_unit(kinds::ADVANCER, 0, 100, 100);
        let tank = w.add_unit(7, 0, 200, 200);
        {
            let u = w.units.iter_mut().find(|u| u.id == adv).unwrap();
            u.straight = Some((5, 6));
            u.queued = Some((300, 310));
            u.beam = BeamTimer { enabled: true, state: 6, counter: 4, reload: 10 };
            u.idle = 99;
        }
        w.units.iter_mut().find(|u| u.id == tank).unwrap().queued = Some((64, 96));
        let st = w.to_state(1, &PassedLevels::initial(), (0, 0));
        let bytes = st.to_bytes().unwrap();
        dump("synthetic_orders", &bytes);
        let parsed = State::parse(&bytes, 1).unwrap();
        let mut back = world();
        back.timer = Some((0, 0, 1));
        back.sky.clouds.clear();
        back.apply_state(&parsed).unwrap();
        assert_eq!(back.timer, Some((12, 34, 1)));
        assert_eq!((back.ai_strategy, back.ai_timer), (3, 777));
        assert_eq!(back.sky.clouds.iter().map(|c| (c.x, c.y, c.speed)).collect::<Vec<_>>(), vec![(-50, 300, 17), (600, 40, 33)]);
        let a = back.units.iter().find(|u| u.kind == kinds::ADVANCER).unwrap();
        assert_eq!((a.straight, a.queued, a.beam.state, a.beam.counter, a.idle), (Some((5, 6)), Some((300, 310)), 6, 4, 99));
        let t = back.units.iter().find(|u| u.kind == 7).unwrap();
        assert_eq!((t.straight, t.queued), (None, Some((64, 96))));
        assert!(back.to_state(1, &PassedLevels::initial(), (0, 0)).to_bytes().unwrap() == bytes);
    }

    /// With `OPENSOW_DUMP_STATES=dir` the synthetic streams are written for `scripts/verify_save_load.py`.
    fn dump(name: &str, bytes: &[u8]) {
        if let Some(dir) = std::env::var_os("OPENSOW_DUMP_STATES") {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(std::path::Path::new(&dir).join(format!("{name}.state")), bytes);
        }
    }

    fn structure(kind: ObjectKind, kind_id: i32, owner: Owner, cell: (i32, i32)) -> Structure {
        let aim = if let ObjectKind::Turret(n) = kind { TopAim::new(108 + n as i32, 0) } else { None };
        Structure {
            object: 0,
            kind,
            owner,
            cell,
            hp: 500,
            max_hp: 800,
            aim,
            weapon: 0,
            target: None,
            slots: Vec::new(),
            active: 0,
            kind_id,
            tally: [0; 3],
            heal: 0,
            upgrade: 0,
            protected: false,
            attacked: false,
            sat_cd: 0,
            advancer: None,
            rally: None,
            dying: None,
        }
    }

    /// A unit in the middle of a segment: the pixel line to its step cell is the chassis' local route in the stream (the exe's
    /// `0x46ee80` list, popped one point per pixel), and a world loaded from it walks the same pixels as the original.
    #[test]
    fn a_walking_units_pixel_line_is_the_local_route() {
        let mut w = world();
        let id = w.add_unit(7, 0, 100, 100);
        w.order_move(id, (9, 4), None);
        let mut checked = 0;
        for _ in 0..200 {
            w.step();
            let u = w.units.iter().find(|u| u.id == id).unwrap();
            if u.line.is_none() {
                continue;
            }
            let bytes = w.to_state(1, &PassedLevels::initial(), (0, 0)).to_bytes().unwrap();
            dump("synthetic_walk", &bytes);
            let mut back = world();
            back.apply_state(&State::parse(&bytes, 1).unwrap()).unwrap();
            back.tick = w.tick; // the exe's frame counter is not in the stream (our extension carries it)
            assert_eq!(back.to_state(1, &PassedLevels::initial(), (0, 0)).to_bytes().unwrap(), bytes);
            let (a, b) = (u.line.as_ref().unwrap(), back.units[0].line.as_ref().unwrap());
            assert_eq!((a.goal, &a.rest), (b.goal, &b.rest));
            // the list in the record: the points to come, nearest first
            let st = State::parse(&bytes, 1).unwrap();
            let ch = st.units.iter().flat_map(|g| g.1.iter()).find_map(|(_, o)| o.child(8)).unwrap();
            assert_eq!(ch.local_route.len(), a.rest.len());
            assert_eq!(ch.local_route[0], (a.rest.last().unwrap().0 as u32, a.rest.last().unwrap().1 as u32));
            assert_eq!(ch.get(0xa0), Some(ch.local_route.len() as u32));
            // both worlds go on identically
            for _ in 0..20 {
                w.step();
                back.step();
                assert_eq!((w.units[0].x, w.units[0].y), (back.units[0].x, back.units[0].y));
            }
            checked += 1;
            break;
        }
        assert_eq!(checked, 1, "the unit was seen walking along a line");
    }

    #[test]
    fn sprite_sizes_come_from_the_installs_sheets_and_the_rest_from_the_initialiser() {
        use opensow_formats::saveobj::Class as C;
        let mut w = world();
        w.add_unit(7, 1, 100, 100);
        let rec = |w: &World| w.to_state(1, &PassedLevels::initial(), (0, 0)).units[1].1[0].1.clone();
        let (top, ch) = (rec(&w).child(4).cloned().unwrap(), rec(&w).child(8).cloned().unwrap());
        assert_eq!((top.get(0x64), top.get(0x68), ch.get(0x20), ch.get(0x24)), (Some(0), Some(0), Some(1), Some(1)), "no sheets: the exe's null sheet values");
        // the initialiser's constants: the top's weapon owner is the player, the chassis' team too, the sheet indices of kind 7
        assert_eq!(top.child(8).unwrap().get(0xc), Some(1));
        assert_eq!(ch.get(0x10), Some(1));
        let art = opensow_formats::units::unit_art(7).unwrap();
        assert_eq!(opensow_formats::units::TOPS[top.get(0x54).unwrap() as usize], art.top.unwrap());
        assert_eq!(opensow_formats::units::CHASSIS[ch.get(0x1c).unwrap_or(0) as usize], art.chassis);
        w.sheet_sizes.tops[top.get(0x54).unwrap() as usize] = Some((0x13, 0x20));
        w.sheet_sizes.chassis[ch.get(0x1c).unwrap_or(0) as usize] = Some((0x2f, 0x3f));
        let (top, ch) = (rec(&w).child(4).cloned().unwrap(), rec(&w).child(8).cloned().unwrap());
        assert_eq!((top.get(0x64), top.get(0x68), ch.get(0x20), ch.get(0x24)), (Some(0x14), Some(0x21), Some(0x31), Some(0x41)));
        let _ = C::Unit;
    }

    /// Two independent readings of the unit initialiser agree: the emulated table and the constants the bridge (`fill_unit`) and
    /// `formats::units` / `walk` carry.
    #[test]
    fn the_unit_initialiser_table_agrees_with_the_decoded_tables() {
        use super::super::unit_defaults::{Part, UNIT_DEFAULTS};
        use opensow_formats::units::{unit_art, CHASSIS, TOPS};
        let w = world();
        for &(kind, has_top, has_chassis, rows) in UNIT_DEFAULTS {
            let get = |part: Part, off: u32| rows.iter().find(|r| r.0 == part && r.1 == off).map_or(0, |r| r.2);
            let mut expect = Object::blank(Class::Unit);
            w.fill_unit(&mut expect, 0, kind, 1, 0, 0, 0, 0);
            for off in [0x1c, 0x20, 0x24, 0x28, 0x38, 0x4b, 0x4c, 0x50, 0x64, 0x88] {
                if let Some(v) = expect.get(off) {
                    if off != 0x88 && !(0x4b..=0x4c).contains(&off) || off == 0x4b || off == 0x4c {
                        assert_eq!(get(Part::Unit, off), v, "unit {kind} +{off:#x}");
                    }
                }
            }
            if kinds::GROUND_UNITS.contains(&kind) || (40..=46).contains(&kind) {
                let art = unit_art(kind).unwrap();
                assert!(has_chassis);
                assert_eq!(CHASSIS[get(Part::Chassis, 0x1c) as usize], art.chassis, "chassis sheet of {kind}");
                assert_eq!(has_top, art.top.is_some());
                if let Some(t) = art.top {
                    assert_eq!(TOPS[get(Part::Top, 0x54) as usize], t, "top sheet of {kind}");
                }
                // the chassis walking speed and facing count are the decoded walk parameters
                let p = opensow_formats::anim::walk_params(kind).unwrap();
                assert_eq!(get(Part::Chassis, 0x8), p.steps, "facing count of {kind}");
                assert_eq!(get(Part::Chassis, 0x18), p.speed as u32, "speed of {kind}");
            }
        }
    }

    #[test]
    fn corpses_blinks_look_timers_and_beam_shots_survive_the_stream() {
        let mut w = world();
        w.add_structure(structure(ObjectKind::Headquarters, 100, Owner::Player, (4, 4)));
        w.add_structure(structure(ObjectKind::Turret(1), 25, Owner::Enemy, (12, 12)));
        w.corpses.push(Corpse { x: 300, y: 340, left: 211, variant: 3, air: false, blasted: true, player: 1, kind: 7 });
        w.blinks.insert(0, Blink { timer: 3, on: false, left: 5 });
        w.turret_look.insert(1, 123);
        let path = build_path(Action::Upgrade as u8, 100, 100, 160, 160, 0).unwrap();
        let n = path.points.len();
        w.beams.push(BeamShot { action: Action::Upgrade, target: 0, side: 0, rate: 40, path, index: 7, trail: 2, frags: vec![(1, -2, 3), (4, 5, 6)] });
        let bytes = w.to_state(1, &PassedLevels::initial(), (0, 0)).to_bytes().unwrap();
        dump("synthetic_corpse_beam", &bytes);
        let parsed = State::parse(&bytes, 1).unwrap();
        let mut back = world();
        back.add_structure(structure(ObjectKind::Headquarters, 100, Owner::Player, (4, 4)));
        back.add_structure(structure(ObjectKind::Turret(1), 25, Owner::Enemy, (12, 12)));
        back.apply_state(&parsed).unwrap();
        let c = &back.corpses[0];
        assert_eq!((c.x, c.y, c.left, c.variant, c.blasted, c.player, c.kind), (300, 340, 211, 3, true, 1, 7));
        let b = back.blinks[&0];
        assert_eq!((b.timer, b.on, b.left), (3, false, 5));
        assert_eq!(back.turret_look[&1], 123);
        let s = &back.beams[0];
        assert_eq!(
            (s.action, s.target, s.rate, s.index, s.trail, s.frags.clone(), s.path.points.len()),
            (Action::Upgrade, 0, 40, 7, 2, vec![(1, -2, 3), (4, 5, 6)], n)
        );
        assert_eq!(s.path.points[7..], w.beams[0].path.points[7..]);
        assert!(back.to_state(1, &PassedLevels::initial(), (0, 0)).to_bytes().unwrap() == bytes);
    }

    #[test]
    fn a_rogon_blast_in_progress_survives_the_stream() {
        let mut w = world();
        w.nukes.push(Nuke { x: 320, y: 352, owner: 1, ex: opensow_formats::rogon::Explosion::from_parts(7, 123) });
        let bytes = w.to_state(1, &PassedLevels::initial(), (0, 0)).to_bytes().unwrap();
        dump("synthetic_nuke", &bytes);
        let parsed = State::parse(&bytes, 1).unwrap();
        let mut back = world();
        back.apply_state(&parsed).unwrap();
        assert_eq!(back.nukes.len(), 1);
        let n = &back.nukes[0];
        assert_eq!((n.x, n.y, n.owner, n.ex.parts()), (320, 352, 1, (7, 123)));
        assert!(back.bullets.is_empty());
        assert!(back.to_state(1, &PassedLevels::initial(), (0, 0)).to_bytes().unwrap() == bytes);
    }

    #[test]
    fn a_top_keeps_its_weapon_state_and_counters() {
        let mut w = world();
        let tank = w.add_unit(7, 0, 200, 200);
        {
            let u = w.units.iter_mut().find(|u| u.id == tank).unwrap();
            let a = u.aim.as_mut().expect("a tank has a top");
            a.state = opensow_formats::aim::State::Cooldown;
            a.facing = 5;
            a.target_facing = 9;
            a.set_counters(3, 12, 44, 7);
        }
        let bytes = w.to_state(1, &PassedLevels::initial(), (0, 0)).to_bytes().unwrap();
        dump("synthetic_top", &bytes);
        let mut back = world();
        back.apply_state(&State::parse(&bytes, 1).unwrap()).unwrap();
        let a = back.units[0].aim.as_ref().unwrap();
        assert_eq!((a.state, a.facing, a.target_facing, a.counters()), (opensow_formats::aim::State::Cooldown, 5, 9, (3, 12, 44, 7)));
    }

    /// The constructor table (emulated exe code) and the hand-decoded tables of `formats::buildings` are two independent readings of
    /// `0x40a330`: footprint, sprite hotspot, animation bounds and period, and the 25-cell layout mask must agree for every kind.
    #[test]
    fn the_constructor_table_agrees_with_the_decoded_building_tables() {
        use opensow_formats::buildings::{anim, art_offset, footprint, LAYOUTS};
        for (kind, fields) in super::super::building_defaults::BUILDING_DEFAULTS {
            let get = |off: u32| fields.iter().find(|f| f.0 == off).map_or(0, |f| f.1);
            let (w, h, layout) = footprint(*kind).unwrap();
            assert_eq!((get(0x14), get(0x18)), (w as u32, h as u32), "footprint of {kind}");
            let (ax, ay) = art_offset(*kind);
            assert_eq!((get(0x1c), get(0x20)), (ax as u32, ay as u32), "hotspot of {kind}");
            let a = anim(*kind).unwrap();
            let bounds: Vec<u32> = (0..8).map(|i| get(0x28 + 4 * i)).collect();
            assert_eq!(bounds, a.bounds.iter().map(|&b| b as u32).collect::<Vec<_>>(), "bounds of {kind}");
            assert_eq!(get(0x58), a.period.min(0x1770), "period of {kind}");
            let mask: Vec<u32> = (0..25).map(|i| get(0x98 + 4 * i)).collect();
            assert_eq!(mask, LAYOUTS[layout].iter().map(|&c| (c == 1) as u32).collect::<Vec<_>>(), "layout of {kind}");
        }
    }

    #[test]
    fn handles_follow_the_exes_allocator() {
        let h = handle(1, unit_handle_type(40), 1, 0, 7);
        assert_eq!(split_handle(h), (7, 0, 1, 4, 1));
        assert_eq!(h, 7 | 1 << 16 | 4 << 19 | 1 << 22);
        assert_eq!([1, 2, 3, 42, 40, 109, 110, 7].map(unit_handle_type), [5, 5, 5, 5, 4, 3, 6, 1]);
    }
}
