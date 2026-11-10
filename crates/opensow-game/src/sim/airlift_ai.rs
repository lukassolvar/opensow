//! The computer's carryalls for skirmishes (not in the exe: the original AI never launches a carryall, only bombers,
//! fighters and triplers, docs/ai.md item 6). docs/skirmish.md "Carryalls".
//!
//! A carryall always flies in from the right map edge along the row of its target, slows down to almost nothing over it,
//! drops the next squad of the global cycle (`aircraft::squad`) on the 4x2 cells at `(x-1..x+2, y..y+1)` and flies on to
//! the left edge. Like a player, the computer **does not know which squad comes**; it only knows what a carryall brings on
//! average. So it plays in two steps:
//! 1. **Call it** ([`decide`]) where units are needed: an own building losing a ground fight, the enemy Advancer at an own
//!    building, or a weakly held foreign building an average squad would outgun (raids keep one carryall back). It lands on
//!    the **nearest spot out of anti-air reach** (no A.A. turret or anti-air unit covers the hover) with room to land, and
//!    not at all when the flight along the row would cost the carryall most of its hit points.
//! 2. **Use what came** ([`assign`]), after the unload, by unit type: anti-air units go for the enemy Advancer; the ground
//!    units help the nearest own building that is losing a fight, else take a nearby foreign building they outgun (shooting
//!    it down captures it), else the nearest enemy unit; otherwise the normal AI takes them over.
//!
//! How hard it pushes depends on the difficulty ([`Tuning`]): Easy only defends, Normal raids the player's buildings only
//! after 5 minutes and never near the player's HQ, one carryall at a time; Hard and Brutal push harder. Pure: the caller describes the map; no random numbers are drawn.

/// Carryall hit points (`Aircraft::launch`).
pub const CARRYALL_HP: f32 = 360.0;
/// Cruising speed 120 px/s = 16 ticks per cell; the slow approach and the unload take about this long over the target.
pub const TICKS_PER_CELL: f32 = 16.0;
pub const HOVER_TICKS: f32 = 200.0;
/// A drop whose expected damage exceeds this share of the carryall's hit points is not flown.
pub const MAX_RISK: f32 = 0.7;
/// A raid with only one carryall in stock waits this long after the last launch (where [`Tuning::quiet_raids`] allows it).
pub const QUIET: u64 = 3600;

/// How hard the computer pushes its carryalls (per skirmish difficulty, `World::ai_airlift`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// Ticks between two launches.
    pub cooldown: u64,
    /// Raids at all (Easy: only defence and the enemy Advancer).
    pub raids: bool,
    /// Raids on the opponent's own buildings only from this tick on (neutral ones at any time).
    pub raid_opponent_after: u64,
    /// No raid on the opponent's buildings within this many cells of its HQ (0: anywhere).
    pub hq_safe: f32,
    /// The last carryall may raid after a quiet minute (otherwise raids always keep one back).
    pub quiet_raids: bool,
    /// Carryalls of the side in the air at once.
    pub max_inbound: usize,
}

impl Tuning {
    pub const EASY: Tuning = Tuning { cooldown: 1800, raids: false, raid_opponent_after: u64::MAX, hq_safe: 0.0, quiet_raids: false, max_inbound: 1 };
    /// Normal: one carryall at a time, 30 s apart, raids on the player's buildings only after 5 minutes and never into
    /// the 16 cells around the player's HQ, always one carryall kept back.
    pub const NORMAL: Tuning = Tuning { cooldown: 1800, raids: true, raid_opponent_after: 5 * 3600, hq_safe: 16.0, quiet_raids: false, max_inbound: 1 };
    pub const HARD: Tuning = Tuning { cooldown: 900, raids: true, raid_opponent_after: 2 * 3600, hq_safe: 8.0, quiet_raids: true, max_inbound: 2 };
    pub const BRUTAL: Tuning = Tuning { cooldown: 600, raids: true, raid_opponent_after: 0, hq_safe: 0.0, quiet_raids: true, max_inbound: 3 };
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning::BRUTAL
    }
}
/// Units within this many cells of a building press on it or defend it.
const NEAR: f32 = 9.0;

/// Anti-air kinds (they shoot only flyers: the Advancer and aircraft).
pub fn is_anti_air(kind: i32) -> bool {
    matches!(kind, 1 | 2 | 3 | 42)
}

/// How much a ground unit counts in a fight.
pub fn weight(kind: i32) -> f32 {
    if (41..=45).contains(&kind) {
        2.5
    } else if is_anti_air(kind) {
        0.3
    } else {
        1.0
    }
}

/// A building or turret.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Site {
    pub index: usize,
    pub kind_id: i32,
    pub cell: (i32, i32),
    pub size: (i32, i32),
    pub own: bool,
    /// Owned by nobody (neutral) rather than the enemy.
    pub neutral: bool,
    pub protected: bool,
}

impl Site {
    pub fn centre(&self) -> (f32, f32) {
        (self.cell.0 as f32 + self.size.0 as f32 / 2.0, self.cell.1 as f32 + self.size.1 as f32 / 2.0)
    }

    fn worth(&self) -> f32 {
        match self.kind_id {
            100 => 10.0,
            105 => 9.0,
            103 | 108 => 8.0,
            102 => 7.0,
            106 => 6.0,
            101 | 104 => 5.0,
            26 => 5.0,
            107 => 4.0,
            _ => 3.0,
        }
    }

    fn is_turret(&self) -> bool {
        (25..=29).contains(&self.kind_id)
    }

    /// Ground firepower of a turret (tier points as in `defence.rs`; the A.A. turret has none).
    fn ground_power(&self) -> f32 {
        match self.kind_id {
            25 => 2.0,
            27 => 3.0,
            28 | 29 => 4.0,
            _ => 0.0,
        }
    }
}

/// A ground unit (either side).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trooper {
    pub id: u32,
    pub cell: (i32, i32),
    pub kind: i32,
    pub own: bool,
}

/// Anti-air fire: an A.A. turret or unit of the enemy (cell, reach in cells, damage per tick).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flak {
    pub cell: (i32, i32),
    pub range: f32,
    pub dps: f32,
}

/// What the planner sees.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub map: (i32, i32),
    pub sites: Vec<Site>,
    pub troops: Vec<Trooper>,
    pub flak: Vec<Flak>,
    /// The enemy Advancer: id, cell, and the own building (site index) it works on, if any.
    pub enemy_advancer: Option<(u32, (i32, i32), Option<usize>)>,
    pub stock: u32,
    pub tick: u64,
    /// When the side last launched a carryall.
    pub last_launch: Option<u64>,
    /// Drop cells of the side's carryalls still in the air: a following one lands beside them.
    pub inbound: Vec<(i32, i32)>,
    pub tuning: Tuning,
    /// Centre of the opponent's HQ (the [`Tuning::hq_safe`] zone).
    pub opponent_hq: Option<(f32, f32)>,
}

/// What a landed unit goes for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chase {
    Unit(u32),
    Structure(usize),
}

/// Why a carryall is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Own building (site index) losing a ground fight.
    Reinforce(usize),
    /// The enemy Advancer (unit id) at an own building.
    Advancer(u32),
    /// A weakly held foreign building (site index).
    Raid(usize),
}

/// A launch: the carryall drops whatever squad comes at `cell` (out of anti-air reach) for `need`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drop {
    pub cell: (i32, i32),
    pub need: Need,
    pub value: f32,
}

/// The ground strength a carryall brings on average: the mean over the ten squads of the launch cycle (a player knows the
/// usual loads, not the next one).
pub fn expected_strength() -> f32 {
    let total: f32 = (0..10).filter_map(opensow_formats::aircraft::squad).map(|m| m.iter().map(|&(_, _, k)| weight(k)).sum::<f32>()).sum();
    total / 10.0
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn mid(c: (i32, i32)) -> (f32, f32) {
    (c.0 as f32 + 0.5, c.1 as f32 + 0.5)
}

fn flak_at(s: &Snapshot, c: (i32, i32)) -> f32 {
    s.flak.iter().filter(|f| dist(mid(f.cell), mid(c)) <= f.range + 0.5).map(|f| f.dps).sum()
}

/// Expected damage to a carryall dropping at `c`: the flight along row `c.1` from the right edge to the target, the slow
/// hover over it and the way on to the left edge.
pub fn risk(s: &Snapshot, c: (i32, i32)) -> f32 {
    let row: f32 = (0..s.map.0).map(|x| flak_at(s, (x, c.1)) * TICKS_PER_CELL).sum();
    row + flak_at(s, c) * HOVER_TICKS
}

/// Room to land at `c`: a squad fills the 4x2 block `(x-1..x+2, y..y+1)` column by column from the left, so a small squad
/// (four units) uses only the two left columns; those must be free and at least two more cells of the block.
fn can_land(c: (i32, i32), free: &impl Fn((i32, i32)) -> bool) -> bool {
    let left = (0..2).all(|dy| (-1..1).all(|dx| free((c.0 + dx, c.1 + dy))));
    left && (0..2).flat_map(|dy| (1..3).map(move |dx| (c.0 + dx, c.1 + dy))).filter(|&p| free(p)).count() >= 2
}

/// Another carryall of the side is already landing at `other`: the blocks must not overlap (one cell apart).
fn clashes(c: (i32, i32), other: (i32, i32)) -> bool {
    (c.0 - other.0).abs() < 5 && (c.1 - other.1).abs() < 3
}

/// The drop cell nearest to `want` (within 10 cells) that no anti-air covers, with room for a squad to land, a flight
/// along its row the carryall survives and no overlap with a carryall of the side still on its way (the next one in a
/// sequence lands beside it).
pub fn drop_cell(s: &Snapshot, want: (f32, f32), free: &impl Fn((i32, i32)) -> bool) -> Option<(i32, i32)> {
    let (wx, wy) = (want.0.floor() as i32, want.1.floor() as i32);
    let mut best: Option<(f32, (i32, i32))> = None;
    for y in wy - 10..=wy + 10 {
        for x in wx - 10..=wx + 10 {
            let d = dist(mid((x, y)), want);
            if d > 10.5 || best.is_some_and(|(b, _)| d >= b) {
                continue;
            }
            if x < 1 || y < 0 || x + 2 >= s.map.0 || y + 1 >= s.map.1 || !can_land((x, y), free) || s.inbound.iter().any(|&o| clashes((x, y), o)) {
                continue;
            }
            if flak_at(s, (x, y)) > 0.0 || risk(s, (x, y)) > MAX_RISK * CARRYALL_HP {
                continue;
            }
            best = Some((d, (x, y)));
        }
    }
    best.map(|(_, c)| c)
}

/// Ground pressure on and defence of a point: enemy units' weight, and own units' weight plus own turrets' power / 2.
fn balance(s: &Snapshot, at: (f32, f32), own_side: bool) -> (f32, f32) {
    let (mut press, mut hold) = (0.0, 0.0);
    for t in &s.troops {
        if dist(mid(t.cell), at) <= NEAR {
            if t.own == own_side {
                hold += weight(t.kind);
            } else {
                press += weight(t.kind);
            }
        }
    }
    for b in s.sites.iter().filter(|b| b.own == own_side && b.is_turret()) {
        if dist(b.centre(), at) <= NEAR {
            hold += b.ground_power() / 2.0;
        }
    }
    (press, hold)
}

/// The next carryall launch, if one is worth it now. It does not know the squad.
pub fn decide(s: &Snapshot, free: impl Fn((i32, i32)) -> bool) -> Option<Drop> {
    if s.stock == 0 || s.last_launch.is_some_and(|t| s.tick < t + s.tuning.cooldown) || s.inbound.len() >= s.tuning.max_inbound {
        return None;
    }
    let strength = expected_strength();
    let mut best: Option<Drop> = None;
    let mut consider = |d: Drop| {
        if best.map_or(true, |b| d.value > b.value) {
            best = Some(d);
        }
    };
    // 1. an own building losing a ground fight: land between it and the attackers
    for b in s.sites.iter().filter(|b| b.own && !b.is_turret()) {
        let (press, hold) = balance(s, b.centre(), true);
        if press < 2.0 || press <= hold {
            continue;
        }
        let foes: Vec<&Trooper> = s.troops.iter().filter(|t| !t.own && dist(mid(t.cell), b.centre()) <= NEAR).collect();
        let (fx, fy) = foes.iter().fold((0.0, 0.0), |(x, y), t| (x + mid(t.cell).0, y + mid(t.cell).1));
        let n = foes.len().max(1) as f32;
        let c = b.centre();
        let want = (c.0 + (fx / n - c.0) * 0.4, c.1 + (fy / n - c.1) * 0.4);
        if let Some(cell) = drop_cell(s, want, &free) {
            consider(Drop { cell, need: Need::Reinforce(b.index), value: b.worth() * (press - hold).min(strength) });
        }
    }
    // 2. the enemy Advancer at an own building: whatever comes may hold anti-air units
    if let Some((id, cell, on)) = s.enemy_advancer {
        let at = on.and_then(|k| s.sites.iter().find(|b| b.index == k)).or_else(|| {
            s.sites.iter().filter(|b| b.own && !b.is_turret()).find(|b| dist(b.centre(), mid(cell)) <= 6.0)
        });
        if let Some(b) = at {
            if let Some(c) = drop_cell(s, mid((cell.0 + 2, cell.1)), &free) {
                consider(Drop { cell: c, need: Need::Advancer(id), value: b.worth() });
            }
        }
    }
    // 3. raid a foreign building an average squad outguns (keeps one carryall back for defence)
    let quiet = s.tuning.quiet_raids && s.last_launch.map_or(s.tick >= QUIET, |t| s.tick >= t + QUIET);
    if s.tuning.raids && (s.stock >= 2 || quiet) {
        for b in s.sites.iter().filter(|b| raidable(s, b)) {
            let (_, hold) = balance(s, b.centre(), false); // seen from its owner: its units and guns hold
            if strength < 1.5 * hold + 2.0 {
                continue;
            }
            let c = b.centre();
            if let Some(cell) = drop_cell(s, (c.0 - 3.0, c.1), &free) {
                let worth = b.worth() * if b.neutral { 1.0 } else { 1.4 };
                consider(Drop { cell, need: Need::Raid(b.index), value: worth * strength / (hold + 2.0) });
            }
        }
    }
    best.filter(|d| d.value >= 4.0)
}

/// A foreign building a raid may go for: not satellite-protected, of the turrets only the A.A. turret, and the opponent's
/// own buildings only as the [`Tuning`] allows (time, distance from its HQ).
fn raidable(s: &Snapshot, b: &Site) -> bool {
    if b.own || b.protected || (b.is_turret() && b.kind_id != 26) {
        return false;
    }
    b.neutral || (s.tick >= s.tuning.raid_opponent_after && s.opponent_hq.map_or(true, |h| dist(b.centre(), h) > s.tuning.hq_safe))
}

/// After the unload: what each landed unit goes for, by its type (units without a goal are left to the normal AI).
/// `landed` are the new units; they do not count as defenders of the fights they are judging.
pub fn assign(s: &Snapshot, landed: &[Trooper], at: (i32, i32)) -> Vec<(u32, Chase)> {
    let ids: Vec<u32> = landed.iter().map(|t| t.id).collect();
    let mut s2 = s.clone();
    s2.troops.retain(|t| !ids.contains(&t.id));
    let s = &s2;
    let here = mid(at);
    let mut out = Vec::new();
    // anti-air units: the enemy Advancer if it is around
    let adv = s.enemy_advancer.filter(|(_, c, _)| dist(mid(*c), here) <= 25.0).map(|(id, _, _)| id);
    for t in landed.iter().filter(|t| is_anti_air(t.kind)) {
        if let Some(id) = adv {
            out.push((t.id, Chase::Unit(id)));
        }
    }
    let ground: Vec<&Trooper> = landed.iter().filter(|t| !is_anti_air(t.kind)).collect();
    let g: f32 = ground.iter().map(|t| weight(t.kind)).sum();
    if ground.is_empty() {
        return out;
    }
    let nearest_foe = |p: (f32, f32), r: f32| {
        s.troops.iter().filter(|t| !t.own && dist(mid(t.cell), p) <= r).min_by(|a, b| dist(mid(a.cell), p).partial_cmp(&dist(mid(b.cell), p)).unwrap())
    };
    // a) the nearest own building losing a fight within 14 cells: its nearest attacker
    let losing = s.sites.iter().filter(|b| b.own && !b.is_turret() && dist(b.centre(), here) <= 14.0).filter(|b| {
        let (press, hold) = balance(s, b.centre(), true);
        press >= 1.0 && press > hold
    }).min_by(|a, b| dist(a.centre(), here).partial_cmp(&dist(b.centre(), here)).unwrap());
    let goal = if let Some(f) = losing.and_then(|b| nearest_foe(b.centre(), NEAR)) {
        Some(Chase::Unit(f.id))
    } else if let Some(b) = s.sites.iter().filter(|b| raidable(s, b) && dist(b.centre(), here) <= 14.0)
        .filter(|b| 1.5 * balance(s, b.centre(), false).1 + 1.0 <= g)
        .min_by(|a, b| dist(a.centre(), here).partial_cmp(&dist(b.centre(), here)).unwrap())
    {
        // b) a foreign building they outgun: shoot it down (captures it)
        Some(Chase::Structure(b.index))
    } else {
        // c) the nearest enemy unit close by
        nearest_foe(here, 10.0).map(|f| Chase::Unit(f.id))
    };
    if let Some(goal) = goal {
        out.extend(ground.iter().map(|t| (t.id, goal)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(index: usize, kind_id: i32, cell: (i32, i32), own: bool) -> Site {
        let size = if (25..=29).contains(&kind_id) { (1, 1) } else { (3, 3) };
        Site { index, kind_id, cell, size, own, neutral: false, protected: kind_id == 100 }
    }

    fn snap() -> Snapshot {
        Snapshot {
            map: (64, 64),
            sites: vec![site(0, 100, (50, 50), true), site(1, 105, (40, 50), true), site(2, 100, (5, 5), false)],
            stock: 1,
            tick: 10_000,
            ..Default::default()
        }
    }

    fn open(_: (i32, i32)) -> bool {
        true
    }

    fn foe(id: u32, cell: (i32, i32)) -> Trooper {
        Trooper { id, cell, kind: 7, own: false }
    }

    fn mine(id: u32, cell: (i32, i32), kind: i32) -> Trooper {
        Trooper { id, cell, kind, own: true }
    }

    #[test]
    fn an_average_carryall_brings_a_few_tanks() {
        let e = expected_strength();
        assert!(e > 3.0 && e < 7.0, "{e}");
    }

    #[test]
    fn nothing_to_do_keeps_the_carryall() {
        assert_eq!(decide(&snap(), open), None);
    }

    #[test]
    fn a_building_under_attack_calls_a_carryall_between_it_and_the_attackers() {
        let mut s = snap();
        s.troops = vec![foe(1, (35, 51)), foe(2, (35, 52)), foe(3, (36, 53))];
        let d = decide(&s, open).expect("reinforce the mine");
        assert_eq!(d.need, Need::Reinforce(1));
        assert!(d.cell.0 > 35 && d.cell.0 < 42, "between the attackers (x 35) and the mine (x 40..43): {d:?}");
        // a second carryall while the first is still on its way: it lands at another spot close by
        s.inbound = vec![d.cell];
        let d2 = decide(&s, open).expect("a follow-up");
        assert!(!clashes(d2.cell, d.cell) && dist(mid(d2.cell), mid(d.cell)) <= 6.0, "{d:?} {d2:?}");
        s.inbound.push(d2.cell);
        let d3 = decide(&s, open).expect("and another");
        assert!(!clashes(d3.cell, d.cell) && !clashes(d3.cell, d2.cell), "{d3:?}");
        s.inbound.clear();
        s.troops.extend((0..6).map(|i| mine(10 + i, (41, 49), 7)));
        assert_eq!(decide(&s, open), None, "enough defenders already");
    }

    #[test]
    fn it_lands_at_the_nearest_spot_out_of_anti_air_reach() {
        let mut s = snap();
        s.troops = vec![foe(1, (35, 51)), foe(2, (35, 52)), foe(3, (36, 53))];
        // an anti-air unit next to the attackers covers 6 cells around (35, 50)
        s.flak = vec![Flak { cell: (35, 50), range: 6.0, dps: 0.3 }];
        let d = decide(&s, open).unwrap();
        assert!(dist(mid(d.cell), mid((35, 50))) > 6.5, "out of its reach: {d:?}");
        assert!(dist(mid(d.cell), (39.0, 51.5)) <= 6.0, "but as near as that allows: {d:?}");
        // flak everywhere around: no drop
        s.flak = vec![Flak { cell: (38, 51), range: 12.0, dps: 0.3 }];
        assert_eq!(decide(&s, open), None);
    }

    #[test]
    fn the_enemy_advancer_at_an_own_building_calls_a_carryall() {
        let mut s = snap();
        s.enemy_advancer = Some((77, (41, 51), Some(1)));
        assert_eq!(decide(&s, open).map(|d| d.need), Some(Need::Advancer(77)));
        s.enemy_advancer = Some((77, (10, 20), None));
        assert_eq!(decide(&s, open), None, "far from the own base it is left alone");
    }

    #[test]
    fn raids_go_for_weakly_held_buildings_and_keep_a_carryall_back() {
        let mut s = snap();
        s.sites.push(Site { neutral: true, ..site(3, 105, (20, 30), false) });
        s.sites.push(site(4, 105, (20, 10), false));
        s.troops = (0..6).map(|i| foe(30 + i, (21, 11))).collect();
        s.last_launch = Some(9_000);
        assert_eq!(decide(&s, open), None, "one carryall, launched a while ago: kept for defence");
        s.stock = 2;
        let d = decide(&s, open).unwrap();
        assert_eq!(d.need, Need::Raid(3), "the unguarded neutral mine, not the guarded enemy one: {d:?}");
        s.stock = 1;
        s.last_launch = Some(10_000 - QUIET);
        assert!(decide(&s, open).is_some(), "a quiet minute later even the last one");
        s.last_launch = Some(s.tick - 100);
        s.stock = 3;
        assert_eq!(decide(&s, open), None, "cooldown");
    }

    #[test]
    fn normal_difficulty_keeps_raids_away_from_the_players_hq_and_the_opening() {
        let mut s = snap();
        s.stock = 3;
        s.opponent_hq = Some((6.5, 6.5)); // the player's HQ at (5, 5)
        s.sites.push(site(3, 105, (10, 6), false)); // a player's gold mine next to it
        s.tuning = Tuning::BRUTAL;
        assert_eq!(decide(&s, open).map(|d| d.need), Some(Need::Raid(3)), "Brutal goes for it");
        s.tuning = Tuning::NORMAL;
        s.tick = 30 * 3600;
        assert_eq!(decide(&s, open), None, "Normal: never next to the player's HQ");
        s.sites[3].cell = (40, 20); // far from the player's HQ
        assert_eq!(decide(&s, open).map(|d| d.need), Some(Need::Raid(3)));
        s.tick = 2 * 3600;
        assert_eq!(decide(&s, open), None, "Normal: not in the first five minutes");
        s.sites[3].neutral = true;
        assert_eq!(decide(&s, open).map(|d| d.need), Some(Need::Raid(3)), "neutral buildings at any time");
        s.stock = 1;
        s.last_launch = Some(0);
        assert_eq!(decide(&s, open), None, "Normal always keeps one carryall back");
        s.stock = 3;
        s.inbound = vec![(30, 40)];
        assert_eq!(decide(&s, open), None, "Normal: one carryall in the air at a time");
        s.inbound.clear();
        s.last_launch = Some(s.tick - 1000);
        assert_eq!(decide(&s, open), None, "Normal: 30 s between launches");
        s.tuning = Tuning::EASY;
        s.last_launch = None;
        assert_eq!(decide(&s, open), None, "Easy never raids");
        s.troops = vec![foe(1, (35, 51)), foe(2, (35, 52)), foe(3, (36, 53))];
        assert!(matches!(decide(&s, open).map(|d| d.need), Some(Need::Reinforce(_))), "but it defends");
    }

    #[test]
    fn landed_units_on_normal_leave_the_players_base_alone() {
        let mut s = snap();
        s.tuning = Tuning::NORMAL;
        s.tick = 30 * 3600;
        s.opponent_hq = Some((6.5, 6.5));
        s.sites.push(site(3, 105, (10, 6), false));
        let landed = [mine(100, (12, 9), 7), mine(101, (12, 10), 7)];
        assert!(assign(&s, &landed, (12, 9)).is_empty(), "no raid on the mine by the player's HQ");
        s.tuning = Tuning::BRUTAL;
        assert_eq!(assign(&s, &landed, (12, 9)).len(), 2);
    }

    #[test]
    fn it_needs_room_to_land() {
        let mut s = snap();
        s.stock = 2;
        s.sites.push(Site { neutral: true, ..site(3, 105, (20, 30), false) });
        assert_eq!(decide(&s, |_| false), None);
        // the two left columns are where a small squad lands: they must be free
        let gaps = |c: (i32, i32)| c.0 % 5 != 0;
        let d = decide(&s, gaps).unwrap();
        assert!([(-1, 0), (0, 0), (-1, 1), (0, 1)].iter().all(|&(dx, dy)| gaps((d.cell.0 + dx, d.cell.1 + dy))), "{d:?}");
    }

    #[test]
    fn after_landing_each_unit_gets_a_goal_by_its_type() {
        let mut s = snap();
        s.troops = vec![foe(1, (35, 51)), foe(2, (35, 52)), foe(3, (36, 53))];
        s.enemy_advancer = Some((77, (45, 40), None));
        let landed = [mine(100, (38, 51), 7), mine(101, (38, 52), 10), mine(102, (39, 51), 2)];
        let a = assign(&s, &landed, (38, 51));
        assert!(a.contains(&(102, Chase::Unit(77))), "the anti-air unit goes for the Advancer: {a:?}");
        assert!(a.iter().filter(|(id, _)| *id != 102).all(|(_, c)| matches!(c, Chase::Unit(1..=3))), "the tanks help the mine: {a:?}");
        // no fight around: the tanks take the nearby neutral windtrap
        let mut s = snap();
        s.sites.push(Site { neutral: true, ..site(3, 107, (30, 30), false) });
        let landed = [mine(100, (27, 31), 7), mine(101, (27, 32), 7), mine(102, (28, 31), 10)];
        assert_eq!(assign(&s, &landed, (27, 31)), vec![(100, Chase::Structure(3)), (101, Chase::Structure(3)), (102, Chase::Structure(3))]);
        // only anti-air units and no Advancer around: nothing to assign
        assert!(assign(&s, &[mine(100, (27, 31), 1)], (27, 31)).is_empty());
    }
}
