//! Mission rules: headquarters and convoy checks, win and lose, the mission scripts, hints and reinforcements.

use super::*;

impl World {
    /// A headquarters whose Advancer is gone builds a new one on the cell diagonally next to it (`0x42e38d`), unless the level
    /// header forbids it for that side (words 34 / 35). In mission 7 the player's waits for the end of the countdown
    /// (`0x42e36c`, `[0x5baa99]`): its Advancer comes with the reinforcements.
    pub(super) fn step_hq(&mut self) {
        let held_back = self.mission_no == Some(7) && !self.timer_expired;
        for k in 0..self.structures.len() {
            let s = &self.structures[k];
            if s.kind_id != kinds::HEADQUARTERS || !s.alive() {
                continue;
            }
            let Some(side) = side_of(s.owner) else { continue };
            if self.no_advancer_respawn[side] || self.hq_without_advancer.contains(&k) || (side == 0 && held_back) {
                continue;
            }
            let alive = s.advancer.is_some_and(|id| self.unit(id).is_some_and(|u| u.hp > 0));
            if !alive {
                let c = (s.cell.0 + 1, s.cell.1 + 1);
                let id = self.add_unit(kinds::ADVANCER, side as i32, c.0 * CELL + CELL / 2, c.1 * CELL + CELL / 2);
                self.structures[k].advancer = Some(id);
            }
        }
    }

    /// Missions 3 and 8 drive the player's Rogon pods by script (`0x42e580`): in mission 3 every pod is sent to (176, 2352);
    /// in mission 8 a pod west of x = 2880 and south of y = 352 is sent to (2960, 272).
    pub(super) fn step_convoy(&mut self) {
        if self.convoy.is_some() {
            return self.step_custom_convoy();
        }
        let goal = |u: &Unit| match self.mission_no {
            Some(3) => Some((176, 2352)),
            Some(8) if u.px().0 < 0xb40 && u.px().1 >= 0x180 => Some((0xb90, 0x110)),
            _ => None,
        };
        let orders: Vec<(u32, (i32, i32))> = self
            .units
            .iter()
            .filter(|u| u.hp > 0 && u.player == 0 && u.kind == kinds::POD && u.path.is_empty())
            .filter_map(|u| goal(u).map(|g| (u.id, g)))
            .collect();
        for (id, (x, y)) in orders {
            self.order_move(id, (x.div_euclid(CELL), y.div_euclid(CELL)), Some((x, y)));
        }
    }

    /// A custom escort: each of the human's pods walks to its next route cell; within a cell of it, it goes on to the one
    /// after, and at the goal it stops. Like the missions' script it is re-ordered whenever its path has run out.
    pub(super) fn step_custom_convoy(&mut self) {
        let Some(mut c) = self.convoy.take() else { return };
        c.next.retain(|(id, _)| self.units.iter().any(|u| u.id == *id && u.hp > 0));
        let mut orders = Vec::new();
        for u in self.units.iter().filter(|u| u.hp > 0 && u.player == 0 && u.kind == kinds::POD) {
            let k = match c.next.iter().position(|e| e.0 == u.id) {
                Some(k) => k,
                None => {
                    c.next.push((u.id, 0));
                    c.next.len() - 1
                }
            };
            let cell = u.cell();
            let mut i = c.next[k].1;
            while i + 1 < c.route.len() && (c.route[i].0 - cell.0).abs() <= 1 && (c.route[i].1 - cell.1).abs() <= 1 {
                i += 1;
            }
            c.next[k].1 = i;
            let Some(&to) = c.route.get(i) else { continue };
            let arrived = i + 1 == c.route.len() && c.in_goal(cell);
            if u.path.is_empty() && !arrived {
                orders.push((u.id, to));
            }
        }
        self.convoy = Some(c);
        for (id, to) in orders {
            self.order_move(id, to, Some((to.0 * CELL + CELL / 2, to.1 * CELL + CELL / 2)));
        }
    }

    /// Win and lose rules (`0x4212f0`): the player loses without a headquarters; the enemy loses when it has no headquarters
    /// (missions 4, 8, 13, 16, 21 and 22: no units and no buildings at all). A countdown that runs out ends the game by its
    /// mode (header word 67: 0 victory, 1 defeat, 2 only reinforcements).
    pub(super) fn check_outcome(&mut self) {
        if self.tutorial.is_some() {
            return; // the tutorial has no HQ rules (`0x42e306`): it ends through its own script
        }
        let hq = |o: Owner| self.structures.iter().any(|s| s.kind_id == kinds::HEADQUARTERS && s.owner == o && s.present());
        let had_player_hq = self.structures.iter().any(|s| s.kind_id == kinds::HEADQUARTERS && s.owner == Owner::Player);
        let destroy_all = self.objective.destroy_all || matches!(self.mission_no, Some(4 | 8 | 13 | 16 | 21 | 22));
        // a custom map's rule leaves out Advancers: one whose headquarters fell cannot fight and may be out of every gun's reach
        let custom = self.objective.destroy_all;
        let enemy_left = self.units.iter().any(|u| u.player != 0 && u.hp > 0 && !(custom && u.kind == kinds::ADVANCER))
            || self.structures.iter().any(|s| s.present() && s.owner == Owner::Enemy);
        if self.timer_expired {
            match self.timer.map(|t| t.2) {
                Some(0) => self.outcome = Some(Outcome::Victory),
                Some(1) => self.outcome = Some(Outcome::Defeat),
                _ => {}
            }
            if self.outcome.is_some() {
                return;
            }
        }
        // a custom escort: a pod in the goal zone wins, losing every pod loses; so does losing the last headquarters (a map
        // without a blue one only through its pods); the enemy's headquarters do not decide it
        if let Some(c) = &self.convoy {
            let pods: Vec<(i32, i32)> = self.units.iter().filter(|u| u.hp > 0 && u.player == 0 && u.kind == kinds::POD).map(|u| u.cell()).collect();
            if pods.is_empty() && c.had_pods {
                self.outcome = Some(Outcome::Defeat);
            } else if pods.iter().any(|&p| c.in_goal(p)) {
                self.outcome = Some(Outcome::Victory);
            } else if had_player_hq && !hq(Owner::Player) {
                self.outcome = Some(Outcome::Defeat);
            }
            return;
        }
        // missions 3 and 8: the convoy must survive; mission 3 is won when a pod stands at cells x 4..=6, y 72..=74
        if matches!(self.mission_no, Some(3 | 8)) {
            let pods = self.units.iter().filter(|u| u.hp > 0 && u.player == 0 && u.kind == kinds::POD).count();
            if pods == 0 {
                self.outcome = Some(Outcome::Defeat);
                return;
            }
            if self.mission_no == Some(3)
                && self
                    .units
                    .iter()
                    .any(|u| u.hp > 0 && u.player == 0 && u.kind == kinds::POD && (4..=6).contains(&u.cell().0) && (72..=74).contains(&u.cell().1))
            {
                self.outcome = Some(Outcome::Victory);
                return;
            }
        }
        if self.mission_no == Some(3) {
            return; // mission 3 ends only through its pods (0x42147e)
        }
        if had_player_hq && !hq(Owner::Player) {
            self.outcome = Some(Outcome::Defeat);
        } else if self.enemy_at_start == Some(true)
            && ((destroy_all && !enemy_left) || (!destroy_all && self.has_enemy_hq && !hq(Owner::Enemy)) || (!destroy_all && !self.has_enemy_hq && !enemy_left))
        {
            self.outcome = Some(Outcome::Victory);
        }
    }

    /// Per-second mission logic (`0x42ebd0`, `0x42e580`): the countdown, its reinforcements and the mission-1 reward.
    pub(super) fn step_mission(&mut self) {
        if !self.tick.is_multiple_of(60) {
            return;
        }
        // `0x42ebd0` looks at the clock first (the end flag `[0x5baa99]` once it shows 0:00, the 20/10/5/2 minute messages at
        // seconds == 1, the rewards in the call that sets the flag) and only then counts one second off (`0x42f1ff`)
        if let Some((min, sec, mode)) = self.timer {
            if !self.timer_expired && min <= 0 && sec <= 0 {
                self.timer_expired = true;
                if mode == 2 {
                    self.reinforcements();
                }
            }
            if mode == 2 && sec == 1 {
                match min {
                    20 => self.say(0, 14),
                    10 => self.say(0, 15),
                    5 => self.say(0, 16),
                    2 => self.say(0, 17),
                    _ => {}
                }
            }
            let (mut m, mut s) = (min, sec - 1);
            if s < 0 {
                m -= 1;
                s = 59;
            }
            if m < 0 {
                m = 0;
                s = 0;
            }
            self.timer = Some((m, s, mode));
        }
        // mission 1: raising both tech-levels of the medium factory pays out the air forces (8 fighters, 3 carryalls)
        if self.mission_no == Some(1) && !self.timer_expired {
            let done = self.structures.iter().any(|s| {
                s.owner == Owner::Player
                    && s.kind_id == kinds::MEDIUM_FACTORY
                    && s.slots.len() >= 2
                    && s.slots[0].level >= s.slots[0].max_level
                    && s.slots[1].level >= s.slots[1].max_level
            }); // no `max > 0` test in `0x42e73c`
            if done {
                self.say(0, msg::REINFORCEMENTS);
                self.air_stock[0][4] += 8;
                self.air_stock[0][2] += 3;
                self.timer_expired = true;
            }
        }
        self.update_hint();
    }

    /// The hint boxes of missions 1 and 6 (`0x4303e0`, `0x430770`): once the player has 500 research points (`[0x517b04]`)
    /// "Now increase tech-level in your factory." is shown while the medium factory's tech-levels are not both at their
    /// limit (mission 6: while none is). Mission 1 then shows "Now use fighters to destroy A.A. turrets on the island."
    /// from the moment the 8 fighters are in stock until fewer than 8 remain.
    pub(super) fn update_hint(&mut self) {
        self.hint = None;
        let maxed = |s: &Structure, k: usize| {
            let (l, m) = s.slots.get(k).map_or((0, 0), |sl| (sl.level, sl.max_level));
            l >= m
        };
        let factories: Vec<&Structure> = self.structures.iter().filter(|s| s.owner == Owner::Player && s.kind_id == kinds::MEDIUM_FACTORY).collect();
        let research = self.research[0] >= 500;
        match self.mission_no {
            Some(1) => {
                // the first medium factory: number of slots (0 and 1) at their limit
                let state = factories.first().map(|s| maxed(s, 0) as i32 + maxed(s, 1) as i32);
                if research {
                    match state {
                        Some(0) => self.hint = Some(Hint { text: 280, half: 0x94, text_dx: 0x89 }),
                        Some(1) => self.hint = Some(Hint { text: 280, half: 0xbc, text_dx: 0xb1 }),
                        _ => {}
                    }
                }
                let fighters = self.air_stock[0][4];
                if self.hint_phase == 1 && fighters < 8 {
                    self.hint_phase = 2;
                }
                if state == Some(2) && fighters >= 8 && self.hint_phase != 2 {
                    self.hint_phase = 1;
                    self.hint = Some(Hint { text: 281, half: 0xc1, text_dx: 0xb6 });
                }
            }
            Some(6) => {
                // nothing maxed yet in any medium factory (and at least one exists)
                let count: Option<i32> = (!factories.is_empty()).then(|| factories.iter().map(|s| maxed(s, 0) as i32 + maxed(s, 1) as i32).sum());
                if research && count == Some(0) {
                    self.hint = Some(Hint { text: 280, half: 0x94, text_dx: 0x89 });
                }
            }
            _ => {}
        }
    }

    /// The reinforcements of missions 7, 12 and 17 when their countdown ends (`0x42ec3a..0x42f1fa`): credits, research,
    /// air forces and carryalls that land at fixed map points.
    pub(super) fn reinforcements(&mut self) {
        // (credits, research, air stock in HEADER_ORDER, carryall drops, their squad `[+0x88]` set after the launch)
        type Payment = (i32, i32, [u32; 5], &'static [(i32, i32)], usize);
        let (credits, research, stock, drops, squad_no): Payment = match self.mission_no {
            Some(7) => (5000, 0, [0; 5], &[(0x8c0, 0x600), (0x9a0, 0x480)], 11),
            // 0x42ef3a..: six asteroids (push 0x22 x 6, `0x453e80`) and three bombers
            Some(12) => (10_000, 2500, [3, 6, 0, 0, 0], &[(0x600, 0x4c0), (0x640, 0x420), (0x600, 0x2e0)], 16),
            Some(17) => (5000, 5000, [0; 5], &[(0x9a0, 0x560), (0xb40, 0x660)], 10),
            _ => return self.custom_reinforcements(),
        };
        // the exe adds the rewards without a cap (mines cap at 99999) and announces them: 13 always, 11 "5000 credits arrived" in
        // missions 7 and 17, 12 "5000 research points arrived" in mission 17 (`0x42ee0e`, `0x42f1d7`)
        self.say(0, msg::REINFORCEMENTS);
        self.credits[0] += credits;
        self.research[0] += research;
        match self.mission_no {
            Some(7) => self.say(0, msg::CREDITS_ARRIVED),
            Some(17) => {
                self.say(0, msg::CREDITS_ARRIVED);
                self.say(0, 12);
            }
            _ => {}
        }
        for g in 0..5 {
            self.air_stock[0][g] += stock[g];
        }
        for &(x, y) in drops {
            self.scripted_carryall(x, y, 0, squad_no);
        }
    }

    /// A custom map's reinforcements (not in the original): "Reinforcements have arrived", the credits and research without a
    /// cap like the campaign's, the aircraft into the stock, and a carryall to each landing cell with the squad the map's
    /// maker chose (like the campaign's scripted carryalls, which bring fixed squads; it costs no stock).
    pub(super) fn custom_reinforcements(&mut self) {
        let Some(r) = self.reinforcement.clone() else { return };
        self.say(0, msg::REINFORCEMENTS);
        self.credits[0] += r.credits;
        self.research[0] += r.research;
        for g in 0..5 {
            self.air_stock[0][g] += r.air[g];
        }
        for ((cx, cy), squad) in r.drops {
            // the chosen squad: the game's cycle counter is left alone
            let mut a = Aircraft::launch(AirForce::Carryall, cx * CELL, cy * CELL, 0, self.cells.0, self.carry_counter);
            a.id = self.new_air_id();
            a.squad = squad;
            self.stats[0][6] += 1;
            self.aircraft.push(a);
        }
    }
}
