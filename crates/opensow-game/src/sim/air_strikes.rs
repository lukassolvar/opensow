//! Air forces in the world: launching a strike, flying the aircraft and carryall airlifts.

use super::*;

impl World {
    /// Launches an air strike of `force` for `side` at map pixel `(x, y)` from the stock (`0x467ee0`, `0x455520`).
    pub fn launch_strike(&mut self, side: usize, force: AirForce, x: i32, y: i32) -> bool {
        let Some(g) = HEADER_ORDER.iter().position(|&h| h == force) else { return false };
        if self.air_stock[side][g] == 0 {
            return false;
        }
        self.air_stock[side][g] -= 1;
        self.stats[side][6] += 1; // 0x467eef
        if side == 0 {
            self.strike_lock = STRIKE_LOCK_TICKS; // 0x467efb..0x467f03: the local player's air icons are disabled for 3 s
        }
        let mut a = Aircraft::launch(force, x, y, side as i32, self.cells.0, self.carry_counter);
        a.id = self.new_air_id();
        if force == AirForce::Carryall {
            self.carry_counter = launch_counter(self.carry_counter);
        }
        self.aircraft.push(a);
        if side == 0 {
            // the launch for the local player pings `airforce-dest` at the target (0x417221, 0x417636: the marker animation + effect 68)
            // (the exe plays it at the target, so a strike far off screen was silent; here it is a priority sound)
            self.player_sound(68);
            // ... and shows the `cross_as` marker (`0x4171d5`: 21 frames, 3 ticks each) at the clicked point, unsnapped
            if let Some(sp) = opensow_formats::effects::SPAWNS.iter().find(|s| s.site == 0x4171d5) {
                self.effects.push(Effect { x, y, spec: sp.spec, age: 0 });
            }
        }
        true
    }

    /// Flies the aircraft (`0x467b50`): their bombs, missiles, pods and meteors become bullets, carryalls land a squad.
    pub(super) fn step_aircraft(&mut self) {
        let t = (self.tick % 60) as u32;
        let mut aircraft = std::mem::take(&mut self.aircraft);
        let mut events = Vec::new();
        for a in &mut aircraft {
            // the aircraft code draws from the lockstep table (`0x467848..0x467a95`, `0x437b10`)
            let (table, state) = (&mut self.game_rng, &mut self.rng);
            let mut rand = |n: u32| draw(table, state, n) as i32;
            for e in a.step(t, &mut rand) {
                events.push((a.side, a.right, e));
            }
            for (id, x, y) in std::mem::take(&mut a.sounds) {
                if self.sounds.len() < 64 {
                    self.sounds.push((id, x, y, false));
                }
            }
        }
        for a in aircraft.iter().filter(|a| !a.alive()) {
            if a.hp <= 0 {
                self.stats[a.side.clamp(0, 2) as usize][7] += 1; // 0x467b12
            }
        }
        aircraft.retain(|a| a.alive());
        self.aircraft = aircraft;
        for (side, right, e) in events {
            match e {
                AirEvent::Shot { bullet_type, strength, from, to } => {
                    if let Some(path) = build_path(bullet_type, from.0, from.1, to.0, to.1, 0) {
                        if !path.points.is_empty() {
                            let mut b = Bullet::new(bullet_type, strength, side, path);
                            // 0x4684f2: a bomber's bomb takes the flight direction `[aircraft+0x34]` as `[+0x268c]` (bomba_l / bomba_r)
                            b.variant = (bullet_type == 18 && right) as u32;
                            self.push_bullet(b);
                        }
                    }
                }
                AirEvent::Fireball { x, y, period } => {
                    self.effects.push(Effect { x, y, spec: AnimSpec { period, ..AIR_DESTROYED }, age: 0 });
                }
                AirEvent::Unload { cell_x, cell_y, squad: n } => {
                    // mission 1 always drops the same squad (`0x468970`: `[0x49a33c] == 1`)
                    let members = if self.mission_no == Some(1) {
                        MODE1_SQUAD.iter().enumerate().map(|(i, &k)| (i as i32 / 2 - 1, i as i32 % 2, k)).collect()
                    } else {
                        squad(n).unwrap_or_default()
                    };
                    let mut landed = Vec::new();
                    for (dx, dy, kind) in members {
                        let (x, y) = (cell_x + dx, cell_y + dy);
                        if !self.is_blocked(x, y) && !self.units.iter().any(|u| u.hp > 0 && u.cell() == (x, y)) {
                            landed.push(self.add_unit(kind, side.min(1), x * CELL + CELL / 2, y * CELL + CELL / 2));
                        }
                    }
                    // skirmish: the computer sees what its carryall brought only now and sends each unit by its type
                    let side_ix = side.min(1) as usize;
                    if let Some(n) = self.airlifts.iter().position(|&(sd, c, _)| sd == side_ix && (c.0 - cell_x).abs() <= 1 && (c.1 - cell_y).abs() <= 1) {
                        self.airlifts.remove(n);
                        let snap = self.airlift_snapshot(side_ix);
                        let troops: Vec<airlift_ai::Trooper> = snap.troops.iter().filter(|t| landed.contains(&t.id)).copied().collect();
                        for (id, chase) in airlift_ai::assign(&snap, &troops, (cell_x, cell_y)) {
                            if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
                                u.chase = Some(match chase {
                                    airlift_ai::Chase::Unit(t) => Target::Unit(t),
                                    airlift_ai::Chase::Structure(k) => Target::Structure(k),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    /// A carryall the mission scripts launch (`0x467ee0` for the local player, then `[+0x88]` = the squad and optionally the
    /// start x moved by `dx`): the global squad counter still advances, the squad it would have chosen is replaced.
    pub(super) fn scripted_carryall(&mut self, x: i32, y: i32, dx: i32, squad_no: usize) {
        let mut a = Aircraft::launch(AirForce::Carryall, x, y, 0, self.cells.0, self.carry_counter);
        a.id = self.new_air_id();
        a.x += dx;
        a.squad = squad_no;
        self.carry_counter = launch_counter(self.carry_counter);
        self.stats[0][6] += 1;
        self.strike_lock = STRIKE_LOCK_TICKS; // every `0x467ee0` call of the local player
        self.aircraft.push(a);
    }

    /// The carryalls of a freshly started mission (level loader `0x41f140`, cases `0x41f3a2` and `0x41f34a`): mission 1 gets two
    /// (squad 12) at (0x620, 0x6e0) and (0x6e0, 0x720), their start x moved by -200 and +200; mission 4 one (squad 17: two
    /// walkers) at (0x360, 0xe0), moved by -300. Not run when a savegame is loaded.
    pub fn start_airlift(&mut self) {
        match self.mission_no {
            Some(1) => {
                self.scripted_carryall(0x620, 0x6e0, -200, 12);
                self.scripted_carryall(0x6e0, 0x720, 200, 12);
            }
            Some(4) => self.scripted_carryall(0x360, 0xe0, -300, 17),
            _ => {}
        }
    }
}
