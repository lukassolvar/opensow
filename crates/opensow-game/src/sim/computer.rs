//! The computer opponent's turn: it gathers a snapshot of the world for each of its planners (`defence`, `advancer_ai`, `airlift_ai`, `air_ai`) and carries out what they decide.

use super::*;

impl World {
    /// The computer player (`0x42f490`, run for the non-local side every tick outside the tutorial): buys tech-levels,
    /// picks a strategy every 900-1500 ticks, sends idle units after the nearest target of the strategy's class, steers
    /// its Advancers (`0x42fae0`) and builds turrets next to threatened buildings (`0x431120`).
    /// Which difficulty drives `side` (both sides are the computer in a spectated game).
    pub fn difficulty_of(&self, side: usize) -> Difficulty {
        if side == 0 && self.spectate {
            self.difficulty0
        } else {
            self.difficulty
        }
    }

    /// Spectating: the computer runs side 0 as well. The strategy, strike and defence timers are one set shared by the
    /// code, so side 0 swaps its own set in for the call and back out.
    pub(super) fn step_ai_spectate(&mut self) {
        let swap = |w: &mut World| {
            let s = (w.ai_strategy, w.ai_timer, w.ai_strike, w.ai_defence);
            (w.ai_strategy, w.ai_timer, w.ai_strike, w.ai_defence) = w.spectate_ai;
            w.spectate_ai = s;
        };
        swap(self);
        self.step_ai(0);
        swap(self);
    }

    /// `0x42fae0`: every idle Advancer of `side` scores every building of the three players, `rand(3)` + bonuses - distance
    /// (`0x42fb94..0x42fd9f`), and goes for the best. The buildings are walked player by player in each owner's slot order
    /// (`0x43a8e0(player, index)`), one `rand(3)` each, so the draws match the exe's.
    fn ai_pick_buildings(&mut self, side: usize) {
        let me = if side == 0 { Owner::Player } else { Owner::Enemy };
        let mine = |o: Owner| o == me;
        let order: Vec<usize> = self.slot_tables.iter().flatten().flatten().copied().filter(|&k| self.structures.get(k).is_some_and(|s| s.alive())).collect();
        let advancers: Vec<usize> = (0..self.units.len())
            .filter(|&i| {
                let u = &self.units[i];
                u.hp > 0 && u.kind == kinds::ADVANCER && (u.player != 0) == (side != 0) && u.action.is_none()
            })
            .collect();
        for i in advancers {
            let (ux, uy) = (self.units[i].x as i32 / CELL, self.units[i].y as i32 / CELL);
            let mut best: Option<(i32, usize)> = None;
            for k in order.iter().copied() {
                let mut score = self.random(3) as i32;
                let s = &self.structures[k];
                if s.kind_id == kinds::HEADQUARTERS {
                    score = -10_000;
                }
                if mine(s.owner) {
                    if matches!(s.kind, ObjectKind::Turret(_)) || s.active >= s.slots.len() {
                        score = -10_000;
                    }
                } else {
                    if s.protected {
                        score = -10_000;
                    }
                    if matches!(s.kind, ObjectKind::Turret(_)) {
                        score += 2;
                    }
                    if s.kind_id == kinds::ANTI_AIR_TURRET {
                        score += 2;
                    }
                    score += s.active as i32;
                }
                let sc = s.center_cell();
                let d = (((sc.0 - ux).pow(2) + (sc.1 - uy).pow(2)) as f32).sqrt() as i32;
                score -= d;
                if best.is_none_or(|(b, _)| score > b) {
                    best = Some((score, k));
                }
            }
            if let Some((_, k)) = best {
                let id = self.units[i].id;
                self.order_action(id, k);
            }
        }
    }

    /// One tick of the computer opponent playing `side`.
    pub(super) fn step_ai(&mut self, side: usize) {
        self.tech_up_all(side);
        let me = if side == 0 { Owner::Player } else { Owner::Enemy };
        let mine = |o: Owner| o == me;
        let hq = self.structures.iter().position(|s| s.alive() && s.kind_id == kinds::HEADQUARTERS && !mine(s.owner));
        let enemy_turrets = self.structures.iter().filter(|s| s.alive() && !mine(s.owner) && matches!(s.kind, ObjectKind::Turret(_))).count();
        // the Advancers choose their building first (`0x42fae0` is called at `0x42f602`, before the strategy roll)
        let think = self.tick.is_multiple_of(self.difficulty_of(side).handicap().think_every.max(1) as u64);
        if !self.skirmish && think {
            self.ai_pick_buildings(side);
        }
        // strategy: re-rolled when it no longer fits the enemy's turret count (0x42f67e..0x42f6c1) or its timer expires
        if (enemy_turrets == 0 && self.ai_strategy == 2) || (enemy_turrets > 4 && self.ai_strategy != 2) || self.ai_strategy == 0 {
            self.ai_timer = 0;
        }
        self.ai_timer -= 1;
        if self.ai_timer <= 0 {
            self.ai_strategy = 1 + self.random(4);
            self.ai_timer = 900 + self.random(600) as i32;
        }
        if enemy_turrets == 0 && self.ai_strategy == 2 {
            self.ai_strategy = 4;
        }
        if enemy_turrets > 4 {
            self.ai_strategy = 2;
        }
        // idle units attack: strategy 1 the HQ, 2 turret > unit > building, 3 unit > turret > building, 4 building > turret > unit
        let enemy_adv = self.units.iter().filter(|u| u.hp > 0 && u.kind == kinds::ADVANCER && (u.player != 0) != (side != 0)).map(|u| u.id).next_back();
        // the escort missions 3 and 8 skip the strategy: the nearest enemy unit (the pods are units), else the HQ
        // (`0x42f81d..0x42f828` jump to `0x42f86e`); a custom map's escort (not in the exe) plays by the same rule
        let escort = (!self.skirmish && matches!(self.mission_no, Some(3 | 8))) || (side == 1 && self.convoy.is_some());
        let idle: Vec<usize> = (0..self.units.len())
            .filter(|&i| {
                let u = &self.units[i];
                think && u.hp > 0 && (u.player != 0) == (side != 0) && u.kind != kinds::ADVANCER && u.chase.is_none() && u.path.is_empty()
            })
            .collect();
        for i in idle {
            let (cell, kind) = (self.units[i].cell(), self.units[i].kind);
            let pick = if is_anti_air(kind) {
                // skirmish: an Advancer far away is no goal (it flies about); then the anti-air units go with the others to the
                // enemy HQ as a body shield (the fallback below)
                enemy_adv
                    .filter(|&id| {
                        !self.skirmish
                            || self.unit(id).is_some_and(|a| {
                                let (ac, d) = (a.cell(), 30);
                                (ac.0 - cell.0).abs().max((ac.1 - cell.1).abs()) <= d
                            })
                    })
                    .map(Target::Unit)
            } else {
                let (u, t, b) = self.nearest_enemy(side, cell);
                let order = match self.ai_strategy {
                    _ if escort => [u, None, None],
                    1 => [None, None, None],
                    2 => [t, u, b],
                    3 => [u, t, b],
                    _ => [b, t, u],
                };
                order.into_iter().flatten().next()
            };
            let target = pick.or(hq.map(Target::Structure));
            if (self.skirmish && !escort) || kind == kinds::POD {
                // skirmish (not in the exe) keeps the order as a chase, except against an escort, which takes the exe's
                // one-shot order below; a pod keeps its target in the exe too (0x46a147)
                self.units[i].chase = target;
            } else if let Some(t) = target {
                // the order `0x469fe0(target, 0)`: it sets `[unit+0x7c]`, plans ONE route through the chase check `0x46eb40`
                // (to the cell `range` steps before the target, nothing when already in range) and, its flag being 0, clears
                // `[unit+0x7c]` again (`0x46a141..0x46a14d`). So the unit walks that route and has no order target for its
                // weapon (the priority scan picks what to shoot); when the route ends it is idle and picks again with the
                // strategy of that moment
                if let Some((tx, ty)) = self.target_pos(t) {
                    self.chase_step(i, t, (tx.div_euclid(CELL), ty.div_euclid(CELL)));
                }
            }
        }
        // Advancers choose a building: score = random(3) + bonuses - distance (0x42fb94..0x42fd9f); skirmish: `ai_advancers`
        if self.skirmish {
            self.ai_advancers(side);
        }
        // air strikes every 600 ticks: bomber, fighter or tripler at random if in stock (0x42f930..0x42fa2a);
        // skirmish on Hard and Brutal: the stock is spent where it pays (`ai_air`)
        let proactive = self.skirmish && matches!(self.difficulty_of(side), Difficulty::Hard | Difficulty::Brutal);
        self.ai_strike -= 1;
        let due = !proactive && self.ai_strike <= 0;
        if proactive {
            self.ai_air(side);
        } else if due {
            self.ai_strike = self.difficulty_of(side).handicap().strike_period;
            let force = [AirForce::Bomber, AirForce::Fighter, AirForce::Tripler][self.random(3) as usize];
            if let Some((x, y)) = self.strike_target(side) {
                self.launch_strike(side, force, x, y);
            }
        }
        // skirmish: the player-like planner on its own timer (docs/skirmish.md); campaign: the exe's routine below
        if self.skirmish {
            self.ai_airlift(side);
            self.ai_defend(side);
            return;
        }
        // `0x42fa30`: the turret routine runs right after the air-strike pick, on the same 600-step timer (`0x42f942` jumps past
        // both while it runs; twice at most, the second time only while the credits still reach 200)
        if due && self.credits[side] >= 200 {
            self.ai_build_turret(side);
            if self.credits[side] >= 200 {
                self.ai_build_turret(side);
            }
        }
    }

    /// Skirmish only: the computer's Advancers by `advancer_ai::think` (not the exe's pick): jobs weighed by gain, time and
    /// anti-air risk, broken off under fire, failures remembered. Looks every 30 ticks (90 on Easy).
    pub(super) fn ai_advancers(&mut self, side: usize) {
        use advancer_ai::{Adv, Job, Order, Rel, Site, Threat};
        let every = advancer_ai::LOOK * if self.difficulty_of(side) == Difficulty::Easy { 3 } else { 1 };
        if !self.tick.is_multiple_of(every) {
            return;
        }
        let foe_side = |p: i32| (p != 0) != (side != 0);
        let sites: Vec<Site> = self
            .structures
            .iter()
            .enumerate()
            .filter(|(_, st)| st.alive())
            .map(|(k, st)| {
                let rel = match side_of(st.owner) {
                    Some(o) if o == side => Rel::Own,
                    Some(_) => Rel::Foe,
                    None => Rel::Neutral,
                };
                let upgrade_left = if rel == Rel::Own { st.slots.get(st.active).map_or(0, |sl| (sl.unlock_points() as i32 - st.upgrade).max(1)) } else { 0 };
                Site {
                    index: k,
                    kind_id: st.kind_id,
                    rel,
                    hp: st.hp,
                    max_hp: st.max_hp,
                    upgrade_left,
                    protected: st.protected,
                    action_cell: st.action_cell(),
                    healing: st.heal > 0,
                }
            })
            .collect();
        let mut threats: Vec<Threat> = self
            .structures
            .iter()
            .filter(|st| st.alive() && st.heal == 0 && st.kind_id == kinds::ANTI_AIR_TURRET && side_of(st.owner).is_some_and(|o| o != side))
            .map(|st| Threat { cell: st.cell, range: 9.0, dps: 0.45 })
            .collect();
        threats.extend(self.units.iter().filter(|u| u.hp > 0 && foe_side(u.player) && (is_anti_air(u.kind) || u.kind == kinds::ADVANCER)).map(|u| Threat {
            cell: u.cell(),
            range: weapon_range(u.kind).map_or(9.0, |r| r.1 as f32),
            dps: match u.kind {
                40 => 0.3,
                42 => 0.5,
                _ => 0.3,
            },
        }));
        let advs: Vec<Adv> = self
            .units
            .iter()
            .filter(|u| u.hp > 0 && u.kind == kinds::ADVANCER && !foe_side(u.player))
            .map(|u| {
                let working = u.action.is_some_and(|(_, k)| self.structures.get(k).is_some_and(|st| st.action_cell() == u.cell()));
                // its job's building is in the destruction animation (the capture follows): wait for it
                let holding = self.adv_mind[side].tasks.iter().any(|t| {
                    t.unit == u.id
                        && t.job == Job::Destroy
                        && self.structures.get(t.target).is_some_and(|st| !st.alive() && st.dying.is_some())
                        && u.cell() == self.structures[t.target].action_cell()
                });
                Adv { id: u.id, cell: u.cell(), hp: u.hp, max_hp: u.max_hp, working, idle: u.action.is_none() && u.path.is_empty(), holding }
            })
            .collect();
        let mut mind = std::mem::take(&mut self.adv_mind[side]);
        mind.tiered = true;
        mind.bold = match self.difficulty_of(side) {
            Difficulty::Hard => 1,
            Difficulty::Brutal => 2,
            _ => 0,
        };
        let orders = advancer_ai::think(&mut mind, &advs, &sites, &threats, self.rates[side], self.tick);
        self.adv_mind[side] = mind;
        for (id, order) in orders {
            match order {
                Order::Go { target, job } => {
                    let action = match job {
                        Job::Destroy => Action::Destroy,
                        Job::Repair => Action::Repair,
                        Job::Upgrade => Action::Upgrade,
                    };
                    self.order_job(id, target, action);
                }
                Order::Retreat(cell) => {
                    if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
                        u.action = None;
                    }
                    self.order_move(id, cell, None);
                }
            }
        }
    }

    /// Skirmish only: the computer calls in carryalls (`airlift_ai::decide`; the exe's AI never does). One look per second.
    /// It does not know the squad; the landed units get their goals in the unload (`airlift_ai::assign`).
    pub(super) fn ai_airlift(&mut self, side: usize) {
        if !self.tick.is_multiple_of(60) {
            return;
        }
        // a carryall shot down before it unloaded leaves its drop open again
        let flying: Vec<u32> = self.aircraft.iter().filter(|a| a.hp > 0).map(|a| a.id).collect();
        self.airlifts.retain(|a| flying.contains(&a.2));
        let snap = self.airlift_snapshot(side);
        if snap.last_launch.is_some_and(|t| self.tick < t + snap.tuning.cooldown) {
            return;
        }
        if snap.stock == 0 {
            return;
        }
        let free = |c: (i32, i32)| {
            !self.is_blocked(c.0, c.1)
                && !self.units.iter().any(|u| u.hp > 0 && u.kind != kinds::ADVANCER && u.cell() == c)
                && !self.structures.iter().any(|st| st.alive() && st.covers(c))
        };
        let Some(drop) = airlift_ai::decide(&snap, free) else { return };
        let (x, y) = (drop.cell.0 * CELL + CELL / 2, drop.cell.1 * CELL + CELL / 2);
        if self.launch_strike(side, AirForce::Carryall, x, y) {
            let id = self.aircraft.last().map_or(0, |a| a.id);
            self.airlifts.push((side, drop.cell, id));
            self.last_airlift[side] = Some(self.tick);
        }
    }

    /// What the carryall planner sees for `side` (no squad: it does not know what comes).
    pub(super) fn airlift_snapshot(&self, side: usize) -> airlift_ai::Snapshot {
        use airlift_ai::{Flak, Site, Snapshot, Trooper};
        let g = HEADER_ORDER.iter().position(|&h| h == AirForce::Carryall).unwrap();
        let foe_side = |p: i32| (p != 0) != (side != 0);
        let sites: Vec<Site> = self
            .structures
            .iter()
            .enumerate()
            .filter(|(_, st)| st.alive())
            .map(|(k, st)| {
                let size = opensow_formats::buildings::footprint(st.kind_id).map_or((1, 1), |(w, h, _)| (w, h));
                Site {
                    index: k,
                    kind_id: st.kind_id,
                    cell: st.cell,
                    size,
                    own: side_of(st.owner) == Some(side),
                    neutral: side_of(st.owner).is_none(),
                    protected: st.protected,
                }
            })
            .collect();
        let troops: Vec<Trooper> = self
            .units
            .iter()
            .filter(|u| u.hp > 0 && u.kind != kinds::ADVANCER && u.kind != kinds::POD)
            .map(|u| Trooper { id: u.id, cell: u.cell(), kind: u.kind, own: !foe_side(u.player) })
            .collect();
        let mut flak: Vec<Flak> = self
            .structures
            .iter()
            .filter(|st| st.alive() && st.heal == 0 && st.kind_id == kinds::ANTI_AIR_TURRET && side_of(st.owner).is_some_and(|o| o != side))
            .map(|st| Flak { cell: st.cell, range: 9.0, dps: 0.45 })
            .collect();
        flak.extend(self.units.iter().filter(|u| u.hp > 0 && foe_side(u.player) && airlift_ai::is_anti_air(u.kind)).map(|u| Flak {
            cell: u.cell(),
            range: weapon_range(u.kind).map_or(9.0, |r| r.1 as f32),
            dps: 0.3,
        }));
        let enemy_advancer = self.units.iter().find(|u| u.hp > 0 && u.kind == kinds::ADVANCER && foe_side(u.player)).map(|u| {
            let on = u.action.map(|(_, k)| k).filter(|&k| self.structures.get(k).is_some_and(|st| side_of(st.owner) == Some(side)));
            (u.id, u.cell(), on)
        });
        Snapshot {
            map: self.cells,
            sites,
            troops,
            flak,
            enemy_advancer,
            stock: self.air_stock[side][g],
            tick: self.tick,
            last_launch: self.last_airlift[side],
            inbound: self.airlifts.iter().filter(|a| a.0 == side).map(|a| a.1).collect(),
            tuning: match self.difficulty_of(side) {
                Difficulty::Easy => airlift_ai::Tuning::EASY,
                Difficulty::Normal => airlift_ai::Tuning::NORMAL,
                Difficulty::Hard => airlift_ai::Tuning::HARD,
                Difficulty::Brutal => airlift_ai::Tuning::BRUTAL,
            },
            opponent_hq: self.structures.iter().find(|st| st.alive() && st.kind_id == kinds::HEADQUARTERS && side_of(st.owner).is_some_and(|o| o != side)).map(
                |st| {
                    let (w, h) = opensow_formats::buildings::footprint(100).map_or((4, 4), |(w, h, _)| (w, h));
                    (st.cell.0 as f32 + w as f32 / 2.0, st.cell.1 as f32 + h as f32 / 2.0)
                },
            ),
        }
    }

    /// Like `order_action`, with the action given instead of read off the pointer (the computer does not use the human's
    /// repair toggle).
    pub(super) fn order_job(&mut self, id: u32, target: usize, action: Action) {
        let Some(u) = self.unit(id) else { return };
        if u.kind != kinds::ADVANCER || !self.action_valid(u, action, target) {
            return;
        }
        let cell = self.structures[target].action_cell();
        let c = centre(cell);
        self.order_move(id, cell, Some((c.0 as i32, c.1 as i32)));
        if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
            u.action = Some((action, target));
        }
    }

    /// Skirmish only: the computer's turrets by `defence::decide` (not the exe's routine). Looks every second while an
    /// own building is under pressure, every four seconds otherwise (half as often on Easy); at most one gun per look.
    pub(super) fn ai_defend(&mut self, side: usize) {
        use defence::{Foe, Gun, Plan, Site, Snapshot};
        self.ai_defence -= 1;
        if self.ai_defence > 0 {
            return;
        }
        let mine = |o: Owner| side_of(o) == Some(side);
        let foe_side = |p: i32| (p != 0) != (side != 0);
        let own: Vec<usize> = (0..self.structures.len())
            .filter(|&k| {
                let st = &self.structures[k];
                st.alive() && mine(st.owner) && !matches!(st.kind, ObjectKind::Turret(_))
            })
            .collect();
        let index = |k: usize| own.iter().position(|&o| o == k);
        let sites: Vec<Site> = own
            .iter()
            .map(|&k| {
                let st = &self.structures[k];
                let c = st.center_cell();
                let advancer = self.units.iter().any(|u| {
                    u.hp > 0
                        && u.kind == kinds::ADVANCER
                        && foe_side(u.player)
                        && (u.action.is_some_and(|(_, t)| t == k) || {
                            let uc = u.cell();
                            (uc.0 - c.0).abs().max((uc.1 - c.1).abs()) <= 6
                        })
                });
                Site { kind_id: st.kind_id, cell: st.cell, advancer }
            })
            .collect();
        let guns: Vec<Gun> = self
            .structures
            .iter()
            .enumerate()
            .filter(|(_, t)| t.alive() && mine(t.owner) && kinds::TURRETS.contains(&t.kind_id))
            .map(|(k, t)| Gun {
                tier: (t.kind_id - 24) as u32,
                cell: t.cell,
                attacked: self.units.iter().any(|u| u.hp > 0 && u.kind == kinds::ADVANCER && foe_side(u.player) && u.action.is_some_and(|(_, a)| a == k)),
            })
            .collect();
        let foes: Vec<Foe> = self
            .units
            .iter()
            .filter(|u| u.hp > 0 && u.kind != kinds::ADVANCER && foe_side(u.player))
            .map(|u| {
                let weight = if kinds::WALKERS.contains(&u.kind) {
                    2.5
                } else if is_anti_air(u.kind) {
                    0.3
                } else {
                    1.0
                };
                let target = match u.chase.or(u.target) {
                    Some(Target::Structure(k)) => index(k),
                    _ => None,
                };
                Foe { cell: u.cell(), weight, target }
            })
            .collect();
        let theirs = |st: &Structure| st.alive() && side_of(st.owner).is_some_and(|o| o != side);
        let enemy_base = self
            .structures
            .iter()
            .find(|st| theirs(st) && st.kind_id == kinds::HEADQUARTERS)
            .or_else(|| self.structures.iter().find(|st| theirs(st)))
            .map(|st| st.center_cell());
        let mut keep_clear = Vec::new();
        for &k in &own {
            let st = &self.structures[k];
            if let Some(e) = factory_exit(st.kind_id) {
                let at = |d: (i32, i32)| (st.cell.0 + d.0, st.cell.1 + d.1);
                let (p, a) = (at(e.primary), at(e.alternative));
                keep_clear.extend([p, (p.0, p.1 + 1), (p.0, p.1 + 2), a, (a.0, a.1 - 1), (a.0, a.1 - 2)]);
            }
            keep_clear.extend(st.rally);
        }
        let mut snap = Snapshot { sites, guns, foes, enemy_base, credits: self.credits[side], flags: self.turret_flags[side], keep_clear };
        let urgent = defence::needs(&snap).iter().any(|n| n.urgent(&snap));
        let me = if side == 0 { Owner::Player } else { Owner::Enemy };
        // as many guns as the attack needs and the credits allow (at most four per look), each seen by the next decision
        for _ in 0..4 {
            let plan = defence::decide(&snap, |tier, cell| {
                let free = |x: i32, y: i32| {
                    !self.is_blocked(x, y)
                        && !self.is_no_build(x, y)
                        && !self.units.iter().any(|u| u.hp > 0 && u.cell() == (x, y))
                        && !self.structures.iter().any(|s| s.alive() && s.covers((x, y)))
                };
                let own_near = |x: i32, y: i32| self.structures.iter().any(|s| s.alive() && s.owner == me && s.covers((x, y)));
                can_place(24 + tier as i32, cell, free, own_near)
            });
            let Plan::Build { tier, cell } = plan else { break };
            if !self.build_turret(side, tier, cell) {
                break;
            }
            snap.guns.push(Gun { tier, cell, attacked: false });
            snap.credits = self.credits[side];
        }
        let slow = if self.difficulty_of(side) == Difficulty::Easy { 2 } else { 1 };
        self.ai_defence = if urgent { 60 } else { 240 } * slow;
    }

    /// The computer's turret routine (`0x431120`): the decision is `formats::turretai::plan` (exe-verified, docs/ai.md item 7);
    /// this gathers its inputs from the world and builds the result.
    pub(super) fn ai_build_turret(&mut self, side: usize) {
        use opensow_formats::turretai::{plan, Building, Slot};
        let me = side as i32;
        // the unit slots; dead units stay in theirs, and the gun tops of finished turrets are units 109..113 in the exe
        let mut slots: Vec<Slot> = self.units.iter().filter(|u| u.hp > 0).map(|u| Slot { cell: u.cell(), owner: u.player, kind: u.kind }).collect();
        slots.extend(self.corpses.iter().map(|c| Slot { cell: (c.x.div_euclid(CELL), c.y.div_euclid(CELL)), owner: c.player, kind: c.kind }));
        for t in self.structures.iter().filter(|t| t.alive() && t.heal == 0) {
            if let (ObjectKind::Turret(n), Some(o)) = (t.kind, side_of(t.owner)) {
                // the top is created at the building's cell, one row lower for the 1x2 turrets 4 and 5 (`0x40e029`, `0x40e03e`)
                slots.push(Slot { cell: (t.cell.0, t.cell.1 + (n >= 4) as i32), owner: o as i32, kind: 108 + n as i32 });
            }
        }
        // own buildings in handle-table order (ties go to the lower slot); structures the table does not know follow
        let wanted =
            |k: &usize| self.structures.get(*k).is_some_and(|s| s.alive() && side_of(s.owner) == Some(side) && !matches!(s.kind, ObjectKind::Turret(_)));
        let mut order: Vec<usize> = self.slot_tables[side].iter().flatten().copied().filter(wanted).collect();
        let known = order.clone();
        order.extend((0..self.structures.len()).filter(|k| wanted(k) && !known.contains(k)));
        let buildings: Vec<Building> = order.iter().map(|&k| Building { kind_id: self.structures[k].kind_id, cell: self.structures[k].cell }).collect();
        let flags = self.turret_flags[side];
        let credits = self.credits[side];
        let me_owner = if side == 0 { Owner::Player } else { Owner::Enemy };
        let aa = |c: (i32, i32)| self.structures.iter().any(|t| t.alive() && t.kind_id == kinds::ANTI_AIR_TURRET && t.covers(c));
        // the generator is lent to the closure (one `rand(8)` per try, as in the exe)
        let (mut table, mut state) = (self.game_rng.take(), self.rng);
        let placed = plan(
            &buildings,
            &slots,
            me,
            credits,
            &flags,
            aa,
            |n| draw(&mut table, &mut state, n),
            |x, y, w, h| {
                let free = |cx: i32, cy: i32| {
                    !self.is_blocked(cx, cy)
                        && !self.is_no_build(cx, cy)
                        && !self.units.iter().any(|u| u.hp > 0 && u.cell() == (cx, cy))
                        && !self.structures.iter().any(|s| s.alive() && s.covers((cx, cy)))
                };
                let own_near = |cx: i32, cy: i32| self.structures.iter().any(|s| s.alive() && s.owner == me_owner && s.covers((cx, cy)));
                can_place(if h == 2 && w == 1 { 28 } else { 25 }, (x, y), free, own_near)
            },
        );
        self.game_rng = table;
        self.rng = state;
        if let Some(p) = placed {
            self.build_turret(side, p.tier, p.cell);
        }
    }

    /// Skirmish, Hard and Brutal: every second the air stock is looked at (`air_ai::decide`); a launch at least every 3 s
    /// (Brutal) or 6 s (Hard).
    pub(super) fn ai_air(&mut self, side: usize) {
        use air_ai::{Flak, Force, Obj};
        let gap = if self.difficulty_of(side) == Difficulty::Brutal { 180 } else { 360 };
        if !self.tick.is_multiple_of(60) || self.tick < self.last_air[side] + gap {
            return;
        }
        let mut stock = [0u32; 3];
        for (n, f) in [AirForce::Bomber, AirForce::Fighter, AirForce::Tripler].into_iter().enumerate() {
            stock[n] = HEADER_ORDER.iter().position(|&h| h == f).map_or(0, |g| self.air_stock[side][g]);
        }
        if stock == [0; 3] {
            return;
        }
        let foe = |p: i32| (p != 0) != (side != 0);
        let mut foes: Vec<Obj> = self
            .units
            .iter()
            .filter(|u| u.hp > 0 && foe(u.player) && u.kind != kinds::ADVANCER)
            .map(|u| Obj { cell: u.cell(), w: if kinds::WALKERS.contains(&u.kind) { 2.5 } else { 1.0 }, aa: is_anti_air(u.kind) })
            .collect();
        let mut own: Vec<(i32, i32)> = self.units.iter().filter(|u| u.hp > 0 && !foe(u.player) && u.kind != kinds::ADVANCER).map(|u| u.cell()).collect();
        let mut flak: Vec<Flak> = self
            .units
            .iter()
            .filter(|u| u.hp > 0 && foe(u.player) && is_anti_air(u.kind))
            .map(|u| Flak { cell: u.cell(), range: weapon_range(u.kind).map_or(9.0, |r| r.1 as f32) })
            .collect();
        for st in self.structures.iter().filter(|st| st.alive()) {
            match side_of(st.owner) {
                Some(o) if o != side => {
                    if st.kind_id == kinds::HEADQUARTERS {
                        continue; // satellite-protected
                    }
                    let aa = st.kind_id == kinds::ANTI_AIR_TURRET;
                    let w = match st.kind_id {
                        26 => 4.0,
                        103 | 108 | 105 => 4.0,
                        25 | 27..=29 | 102 | 106 => 3.0,
                        _ => 2.0,
                    };
                    foes.push(Obj { cell: st.center_cell(), w, aa });
                    if aa && st.heal == 0 {
                        flak.push(Flak { cell: st.center_cell(), range: 9.0 });
                    }
                }
                Some(_) => own.push(st.center_cell()),
                None => {}
            }
        }
        if let Some((force, cell)) = air_ai::decide(stock, &foes, &own, &flak) {
            let f = match force {
                Force::Bomber => AirForce::Bomber,
                Force::Fighter => AirForce::Fighter,
                Force::Tripler => AirForce::Tripler,
            };
            if self.launch_strike(side, f, cell.0 * CELL + CELL / 2, cell.1 * CELL + CELL / 2) {
                self.last_air[side] = self.tick;
            }
        }
    }

    /// Where the computer sends an air strike (`0x435ad0`, `aircraft::ai_strike_target`).
    pub(super) fn strike_target(&self, side: usize) -> Option<(i32, i32)> {
        let units: Vec<opensow_formats::aircraft::StrikeUnit> = self
            .units
            .iter()
            .filter(|u| u.hp > 0)
            .map(|u| opensow_formats::aircraft::StrikeUnit {
                x: u.px().0,
                y: u.px().1,
                kind: u.kind,
                own: (u.player != 0) == (side != 0),
                idle: u.path.is_empty(),
            })
            .collect();
        opensow_formats::aircraft::ai_strike_target(&units)
    }

    /// The hostile unit / turret / other building a reachability wave from `cell` meets first (`0x472a90`, exact port
    /// `wave::find_targets`): the static grid holds terrain, buildings and the ground units' handles; anything whose owner
    /// differs from `side` counts, so neutral buildings are targets too. Objects stop the wave.
    pub(super) fn nearest_enemy(&mut self, side: usize, cell: (i32, i32)) -> (Option<Target>, Option<Target>, Option<Target>) {
        let dims = self.cells;
        let Some(o) = self.occupancy() else { return (None, None, None) };
        let mut grid = o.stat.clone();
        let mut base = o.stat_base;
        for u in self.units.iter().filter(|u| u.hp > 0 && u.kind != kinds::ADVANCER) {
            let (x, y) = u.cell();
            if (0..128).contains(&x) && (0..128).contains(&y) && grid[(x * 128 + y) as usize] == 0 {
                grid[(x * 128 + y) as usize] = 0x3000000 + u.id;
            }
        }
        let classify = |h: u32| {
            if h >= 0x3000000 {
                match self.unit(h - 0x3000000) {
                    Some(u) if (u.player != 0) != (side != 0) => Seen::Unit,
                    _ => Seen::Ignore,
                }
            } else if h >= 0x2000000 {
                match self.structures.get((h - 0x2000000) as usize) {
                    Some(s) if s.alive() && side_of(s.owner) != Some(side) => {
                        if matches!(s.kind, ObjectKind::Turret(_)) {
                            Seen::Turret
                        } else {
                            Seen::Building
                        }
                    }
                    _ => Seen::Ignore,
                }
            } else {
                Seen::Ignore
            }
        };
        let t = find_targets(&mut grid, &mut base, dims, cell, classify);
        let unit = (t.unit != 0).then(|| Target::Unit(t.unit - 0x3000000));
        let structure = |h: u32| (h != 0).then(|| Target::Structure((h - 0x2000000) as usize));
        (unit, structure(t.turret), structure(t.building))
    }

    /// The nearest enemy aircraft whose ground position is within `min..=max` cells of `pos`.
    /// The Advancer's target (`0x46c990` -> `0x47aed0`, then `0x47ad30` when nothing was found): the nearest enemy ground
    /// anti-air unit (kinds 1, 2, 3, 42), else the nearest enemy Advancer.
    pub(super) fn advancer_target(&self, index: &HashMap<(i32, i32), Vec<Target>>, player: i32, cell: (i32, i32), min: u32, max: u32) -> Option<u32> {
        for &(dx, dy) in self.rings.search(min as usize, (max as usize).min(63)) {
            let Some(list) = index.get(&(cell.0 + dx as i32, cell.1 + dy as i32)) else { continue };
            for &t in list {
                if let Target::Unit(id) = t {
                    if self.hostile(player, t) && self.unit(id).is_some_and(|u| kinds::ANTI_AIR_UNITS.contains(&u.kind)) {
                        return Some(id);
                    }
                }
            }
        }
        self.advancer_in_range(index, player, cell, min, max)
    }

    /// Air-grid search (`0x47ad30`): the nearest enemy flying unit (the Advancer) in the range rings.
    pub(super) fn advancer_in_range(&self, index: &HashMap<(i32, i32), Vec<Target>>, player: i32, cell: (i32, i32), min: u32, max: u32) -> Option<u32> {
        for &(dx, dy) in self.rings.search(min as usize, (max as usize).min(63)) {
            let Some(list) = index.get(&(cell.0 + dx as i32, cell.1 + dy as i32)) else { continue };
            for &t in list {
                if let Target::Unit(id) = t {
                    if self.hostile(player, t) && self.unit(id).is_some_and(|u| u.kind == kinds::ADVANCER) {
                        return Some(id);
                    }
                }
            }
        }
        None
    }

    /// The aircraft an anti-air top of `player` at `pos` (the unit's position `[unit+0x14/0x18]`) shoots at, range `min..max` cells:
    /// on the top's poll the exe's search `0x47b3c0` (`aircraft::anti_air_target`, it may keep the last target), otherwise the
    /// kept target while it is still in the aircraft list (`0x479902..0x47996a`: aiming and firing do not search again). Returns
    /// the index into `self.aircraft`.
    pub(super) fn anti_air_aircraft(&mut self, shooter: (bool, usize), polls: bool, player: i32, pos: (i32, i32), min: u32, max: u32) -> Option<usize> {
        use opensow_formats::aircraft::{anti_air_target, AirSlot};
        let current = self.air_targets.get(&shooter).copied();
        let found = if polls {
            let mut slots: Vec<AirSlot> =
                self.aircraft.iter().map(|a| AirSlot { id: a.id, side: a.side, force: a.force, x: a.x, y: a.y, aim_dx: a.aim_dx, hp: a.hp }).collect();
            slots.sort_by_key(|a| a.side); // per-side slot arrays, scanned side by side
            anti_air_target(&slots, pos.0, pos.1, current, min, max, if player == 0 { 0 } else { 1 })
        } else {
            // DELIBERATE DEVIATION: the exe keeps the aircraft between polls without a range check, so a turret finished its volley at
            // a bomber that had already flown out of range ("shooting at a ghost"); here the kept target is dropped once out of range
            let hi = max as f64 * opensow_formats::aircraft::AIR_RANGE_STEP as f64;
            current.filter(|id| {
                self.aircraft.iter().any(|a| a.id == *id && (((pos.0 - a.aim_dx - a.x) as f64).powi(2) + ((pos.1 - a.y) as f64).powi(2)).sqrt() < hi)
            })
        };
        match found {
            Some(id) => self.air_targets.insert(shooter, id),
            None => self.air_targets.remove(&shooter),
        };
        found.and_then(|id| self.aircraft.iter().position(|a| a.id == id))
    }

    /// Where anti-air aims at an aircraft and its rockets fly to (`0x479953..0x479962`): `x + [+0x18]`, `y - altitude`.
    pub(super) fn aircraft_aim_point(&self, a: usize) -> (i32, i32) {
        let a = &self.aircraft[a];
        (a.x + a.aim_dx, a.y - a.altitude)
    }
}
