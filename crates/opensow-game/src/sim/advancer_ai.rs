//! The computer's Advancer for skirmishes (not in the exe; campaign missions keep the decoded pick `0x42fae0`, docs/ai.md
//! rule 5). docs/skirmish.md "The Advancer like a player".
//!
//! The exe's pick scores every building once, with enemy anti-air turrets as a *bonus*, and never looks back: an Advancer that
//! is shot down respawns at the HQ and flies to the same target again, forever. A player instead:
//! - **weighs gain against time and risk**: what the job is worth (capturing a gold mine or factory, upgrading a mine,
//!   repairing a badly hurt building), how long the flight and the beam work take (30 px/s, one shot per 10 ticks) and how
//!   much fire it would take on the way and while hovering (anti-air turrets and units, enemy Advancers in range);
//!   a job whose expected damage would cost most of the remaining hit points is not started;
//! - **watches the job**: looks again every half second; when the target is gone or done it picks the next one, when it
//!   is losing hit points under fire it **breaks off and flies home** (to the nearest own building out of enemy reach);
//! - **remembers**: a target where it was shot down or driven off is avoided for a minute (longer after each failure),
//!   unless the anti-air around it is gone meanwhile; a target it cannot reach (no route, no progress) is dropped too;
//! - **switches** to a clearly better job while still flying, never in the middle of the beam work;
//! - **brute-forces** when nothing safe is left: beam damage and the capture tally stay when it is shot down and a new
//!   Advancer comes for free, so it pushes the target where one life gets the most work done ([`forced_job`]) and keeps
//!   at it until it dies or a safe job turns up.
//!
//! Pure: the caller describes the Advancers, the buildings and the anti-air threats; no random numbers are drawn.

use opensow_formats::kinds;
use serde::{Deserialize, Serialize};

/// Flight speed: 30 px/s (`anim::walk_params(40)`) = one cell per 64 ticks.
pub const TICKS_PER_CELL: f32 = 64.0;
/// One beam shot per 10 ticks (docs/advancer-actions.md).
pub const BEAM_PERIOD: f32 = 10.0;
/// How often the Advancers are reconsidered (ticks).
pub const LOOK: u64 = 30;
/// A job whose expected damage exceeds this share of the remaining hit points is not started.
pub const MAX_RISK: f32 = 0.8;
/// Losing this share of the maximum hit points under fire on one job breaks it off.
pub const BREAK_OFF: f32 = 0.45;
/// Captures this many cells away (from the Advancer or the HQ, whichever is nearer) count half.
pub const NEAR_HALF: f32 = 16.0;
/// Neutral buildings this close (cells) to the Advancer are taken before the opponent's.
pub const NEARBY: f32 = 20.0;
/// Base time a denied target is avoided; doubles with every failure (up to 8x).
pub const DENY: u64 = 3600;
/// No step closer for this long while not working: the target counts as unreachable.
pub const STUCK: u64 = 480;

/// What the building is to the Advancer's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rel {
    Own,
    Foe,
    Neutral,
}

/// What an Advancer can be sent to do at a building or turret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Job {
    Destroy,
    Repair,
    Upgrade,
}

/// A building or turret on the map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Site {
    /// Index in the world's structure list.
    pub index: usize,
    /// Object kind number.
    pub kind_id: i32,
    /// Whose it is, seen from the side that plans.
    pub rel: Rel,
    /// Hit points now.
    pub hp: i32,
    /// Hit points when undamaged.
    pub max_hp: i32,
    /// Upgrade points still missing for the next slot (0 when it cannot be upgraded).
    pub upgrade_left: i32,
    /// Satellite protection: the Advancer is shot down when it beams it (the exe's pick skips these too).
    pub protected: bool,
    /// The cell the Advancer must hover over.
    pub action_cell: (i32, i32),
    /// Still healing in after a capture (150 hp per tick): no repair needed.
    pub healing: bool,
}

impl Site {
    fn is_turret(&self) -> bool {
        kinds::TURRETS.contains(&self.kind_id)
    }

    /// What the building is worth to own (capture) or to improve.
    pub fn worth(&self) -> f32 {
        match self.kind_id {
            100 => 10.0,         // HQ (own: repair; enemy HQs are satellite-protected)
            105 => 9.0,          // gold mine: credits
            103 | 108 => 8.0,    // heavy and walker factories
            102 => 7.0,          // medium factory
            106 => 6.0,          // research lab
            101 | 104 => 5.0,    // light factory, air base
            107 => 4.0,          // windtrap
            26 => 4.0,           // anti-air turret: opens the sky
            25 | 27..=29 => 3.0, // other turrets
            _ => 2.0,
        }
    }
}

/// Something that shoots the Advancer: an anti-air turret or unit, or an enemy Advancer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Threat {
    /// Where it stands.
    pub cell: (i32, i32),
    /// Reach in cells.
    pub range: f32,
    /// Damage per tick.
    pub dps: f32,
}

/// One of the computer's Advancers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adv {
    /// The unit's id.
    pub id: u32,
    /// The cell it is over.
    pub cell: (i32, i32),
    /// Hit points now.
    pub hp: i32,
    /// Hit points when undamaged.
    pub max_hp: i32,
    /// Hovering over its job's cell (beaming).
    pub working: bool,
    /// Not moving and without a job.
    pub idle: bool,
    /// Its building is going down (the destruction animation runs, the capture comes at its end): it waits where it is
    /// instead of choosing again, so it is still over the building when that becomes its own and can upgrade it at once.
    pub holding: bool,
}

/// A job in progress.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// The Advancer's unit id.
    pub unit: u32,
    /// The site (structure index) it works on.
    pub target: usize,
    /// What it does there.
    pub job: Job,
    /// The Advancer's hit points when it took the job (to notice a losing fight).
    pub start_hp: i32,
    /// The nearest it came to the job's cell (cells) and when.
    pub best: f32,
    /// The tick when it last came nearer (see `best`).
    pub best_tick: u64,
    /// The planner's score of the job when it was chosen.
    pub score: f32,
    /// Taken although too risky, because nothing safe was left ([`forced_job`]): it is not broken off under fire.
    #[serde(default)]
    pub forced: bool,
}

/// A target to stay away from until `until`; `danger` is the fire there when it was given up.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Denied {
    /// The site (structure index) to avoid.
    pub target: usize,
    /// The tick the ban ends.
    pub until: u64,
    /// How many times this target has been given up.
    pub strikes: u32,
    /// The fire there when it was given up.
    pub danger: f32,
}

/// The computer's memory about its Advancers (per side, it survives their death).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Mind {
    /// The jobs in progress.
    pub tasks: Vec<Task>,
    /// The targets to stay away from for now.
    pub denied: Vec<Denied>,
    /// Strict priorities (Easy and Normal, [`ranked`]) instead of weighing gain against time.
    #[serde(default)]
    pub tiered: bool,
    /// With `tiered`: 0 Easy/Normal, 1 Hard, 2 Brutal. The harder ones also take valuable buildings of the opponent
    /// among the nearby captures ([`ranked`]).
    #[serde(default)]
    pub bold: u8,
}

/// What the world should do with an Advancer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Order {
    /// Fly to `target` and do `job` there.
    Go { target: usize, job: Job },
    /// Drop the job and fly to this cell.
    Retreat((i32, i32)),
}

fn dist(a: (i32, i32), b: (i32, i32)) -> f32 {
    (((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)) as f32).sqrt()
}

/// Fire per tick at a cell.
pub fn danger(threats: &[Threat], c: (i32, i32)) -> f32 {
    threats.iter().filter(|t| dist(t.cell, c) <= t.range + 0.5).map(|t| t.dps).sum()
}

/// Expected damage of flying from `from` to `to` in a straight line (the air route is nearly straight).
pub fn route_damage(threats: &[Threat], from: (i32, i32), to: (i32, i32)) -> f32 {
    let n = dist(from, to).ceil() as i32;
    (1..=n)
        .map(|i| {
            let f = i as f32 / n as f32;
            let c = ((from.0 as f32 + (to.0 - from.0) as f32 * f).round() as i32, (from.1 as f32 + (to.1 - from.1) as f32 * f).round() as i32);
            danger(threats, c) * TICKS_PER_CELL
        })
        .sum()
}

/// The job an Advancer can do on a site, if any.
pub fn job_for(s: &Site) -> Option<Job> {
    match s.rel {
        Rel::Foe | Rel::Neutral if !s.protected => Some(Job::Destroy),
        Rel::Own if s.hp * 4 < s.max_hp * 3 && !s.healing => Some(Job::Repair),
        Rel::Own if s.upgrade_left > 0 => Some(Job::Upgrade),
        _ => None,
    }
}

/// With strict priorities the Advancer does not repair turrets: they are replaced (the turret planner has the money) and a
/// beam kept on two turrets under fire is a job that never ends while better targets wait.
fn skip_repair(mind: &Mind, s: &Site, job: Job) -> bool {
    mind.tiered && job == Job::Repair && s.is_turret()
}

/// Whether the job still makes sense.
fn still_valid(s: &Site, job: Job) -> bool {
    match job {
        Job::Destroy => s.rel != Rel::Own && s.hp > 0,
        Job::Repair => s.rel == Rel::Own && s.hp < s.max_hp,
        Job::Upgrade => s.rel == Rel::Own && s.upgrade_left > 0,
    }
}

/// Ticks of beam work for the job.
pub fn work_ticks(s: &Site, job: Job, rate: i32) -> f32 {
    let rate = rate.max(1) as f32;
    match job {
        Job::Destroy => s.hp.max(0) as f32 / (2.0 * rate) * BEAM_PERIOD,
        Job::Repair => (s.max_hp - s.hp).max(0) as f32 / (2.0 * rate) * BEAM_PERIOD,
        Job::Upgrade => s.upgrade_left.max(0) as f32 / rate * BEAM_PERIOD,
    }
}

/// Gain per second of the job for Advancer `a`, or None when it is too dangerous. `home` is the own HQ's cell.
/// Upgrades are worth 1.5x the building (every level alike), captures 3x (4.2x from the enemy) and fall off with the distance from the Advancer or the HQ (whichever is
/// nearer: a near building is quicker to take and easier to hold), repairs only count by the missing hit points (and
/// [`best_job`] takes them only when nothing else is left).
pub fn score(a: &Adv, s: &Site, job: Job, threats: &[Threat], rate: i32, home: Option<(i32, i32)>) -> Option<f32> {
    let value = match job {
        // capturing takes the building from the enemy as well; a turret is only removed
        // a captured building is a whole building (with its levels if it was neutral): worth 3 upgrade levels
        Job::Destroy => {
            s.worth()
                * if s.is_turret() {
                    0.7
                } else if s.rel == Rel::Foe {
                    4.2
                } else {
                    3.0
                }
                * nearness(a, s, home)
        }
        Job::Repair => s.worth() * (1.0 - s.hp as f32 / s.max_hp.max(1) as f32),
        Job::Upgrade => s.worth() * 1.5,
    };
    let fly = dist(a.cell, s.action_cell) * TICKS_PER_CELL;
    let work = work_ticks(s, job, rate);
    let hurt = route_damage(threats, a.cell, s.action_cell) + danger(threats, s.action_cell) * work;
    let share = hurt / a.hp.max(1) as f32;
    if share > MAX_RISK {
        return None;
    }
    Some(value * 60.0 / ((fly + work) / 60.0 + 20.0) * (1.0 - share))
}

/// 1 next to the Advancer or the HQ, 1/2 at [`NEAR_HALF`] cells, 1/3 at twice that.
fn nearness(a: &Adv, s: &Site, home: Option<(i32, i32)>) -> f32 {
    let d = home.map_or(f32::MAX, |h| dist(h, s.action_cell)).min(dist(a.cell, s.action_cell));
    1.0 / (1.0 + d / NEAR_HALF)
}

/// Repairs come last: any capture or upgrade beats them. Above all, an Advancer hovering over an own building that can still
/// be upgraded stays and finishes every level (also right after capturing it).
fn rank(a: &Adv, s: &Site, job: Job) -> u8 {
    match job {
        Job::Repair => 0,
        Job::Upgrade if a.cell == s.action_cell => 2,
        _ => 1,
    }
}

/// The best job for Advancer `a`; `busy` are targets other Advancers of the side already work on.
pub fn best_job(
    mind: &Mind,
    a: &Adv,
    sites: &[Site],
    threats: &[Threat],
    rate: i32,
    tick: u64,
    busy: &[usize],
    home: Option<(i32, i32)>,
) -> Option<(usize, Job, f32)> {
    let mut best: Option<(usize, Job, f32)> = None;
    for s in sites {
        let Some(job) = job_for(s) else { continue };
        if busy.contains(&s.index) || skip_repair(mind, s, job) {
            continue;
        }
        // remembered failures: skip unless the fire there has dropped to half since
        // (a failure without known fire, such as no route, holds for the whole time)
        if mind.denied.iter().any(|d| d.target == s.index && d.until > tick && (d.danger <= 0.01 || danger(threats, s.action_cell) >= d.danger * 0.5)) {
            continue;
        }
        let Some(v) = score(a, s, job, threats, rate, home) else { continue };
        let v = ranked(a, s, job, v, mind, home);
        if best.is_none_or(|(_, _, b)| v > b + 1e-4) {
            best = Some((s.index, job, v));
        }
    }
    best
}

/// A score with its [`rank`] on top (scores stay far below 1000), so comparisons respect the ranks.
///
/// `tiered` (Easy and Normal) makes the order strict instead: 1. upgrades of own buildings (also one it just captured), the
/// nearest first; 2. neutral buildings within [`NEARBY`] cells of where it is now, the nearest first; 3. the opponent's
/// buildings (and far neutral ones) by the usual gain (Hard and Brutal: its valuable ones already in step 2); 4. turrets; repairs last. Risk still rules a job out (score is None).
pub fn ranked(a: &Adv, s: &Site, job: Job, v: f32, mind: &Mind, home: Option<(i32, i32)>) -> f32 {
    if !mind.tiered {
        return v + 1000.0 * rank(a, s, job) as f32;
    }
    let d = dist(a.cell, s.action_cell);
    let near = s.worth() * 10.0 / (1.0 + d);
    match job {
        Job::Repair => v,
        Job::Upgrade => (if d == 0.0 { 5000.0 } else { 4000.0 }) + near,
        Job::Destroy if s.is_turret() => 1000.0 + v,
        // a building the opponent took next to our HQ comes back before anything else is captured
        Job::Destroy if s.rel == Rel::Foe && home.is_some_and(|h| dist(h, s.action_cell) <= NEARBY) => 3500.0 + near,
        Job::Destroy if s.rel == Rel::Neutral && d <= NEARBY + 10.0 * mind.bold.saturating_sub(1) as f32 => 3000.0 + near,
        // Hard: the opponent's gold mine and heavy factories count as nearby captures too; Brutal: any factory, lab, mine
        Job::Destroy if s.rel == Rel::Foe && mind.bold > 0 && s.worth() >= if mind.bold > 1 { 6.0 } else { 8.0 } => 3000.0 + near,
        Job::Destroy => 2000.0 + v,
    }
}

/// Nothing safe is left: the job where one Advancer life gets the most done. Beam damage and the capture tally stay when
/// the Advancer is shot down and a new one comes for free, so a player keeps pushing the best target: per job the share of
/// the work that can be done before the hit points run out (after the flight there), times its worth; a target it cannot
/// even reach alive, a satellite-protected one and one remembered as unreachable (no route) are left out.
pub fn forced_job(
    mind: &Mind,
    a: &Adv,
    sites: &[Site],
    threats: &[Threat],
    rate: i32,
    tick: u64,
    busy: &[usize],
    home: Option<(i32, i32)>,
) -> Option<(usize, Job, f32)> {
    let mut best: Option<(usize, Job, f32)> = None;
    for s in sites {
        let Some(job) = job_for(s) else { continue };
        if busy.contains(&s.index) || skip_repair(mind, s, job) || mind.denied.iter().any(|d| d.target == s.index && d.until > tick && d.danger <= 0.01) {
            continue;
        }
        let left = a.hp as f32 - route_damage(threats, a.cell, s.action_cell);
        if left <= 0.0 {
            continue;
        }
        let work = work_ticks(s, job, rate).max(1.0);
        let fire = danger(threats, s.action_cell);
        let done = if fire > 0.0 { (left / fire / work).min(1.0) } else { 1.0 };
        let fly = dist(a.cell, s.action_cell) * TICKS_PER_CELL;
        // every recent failure there makes it a little less attractive, so equal targets take turns
        let strikes = mind.denied.iter().find(|d| d.target == s.index && d.until > tick).map_or(0, |d| d.strikes);
        let near = if job == Job::Destroy { nearness(a, s, home) } else { 1.0 };
        let v = s.worth() * near * done * 60.0 / ((fly + work.min(left / fire.max(1e-6))) / 60.0 + 20.0) / (1.0 + 0.5 * strikes as f32);
        if best.is_none_or(|(_, _, b)| v > b + 1e-4) {
            best = Some((s.index, job, v));
        }
    }
    best
}

/// The nearest own building out of enemy reach: where a hurt Advancer waits.
pub fn shelter(a: &Adv, sites: &[Site], threats: &[Threat]) -> Option<(i32, i32)> {
    sites
        .iter()
        .filter(|s| s.rel == Rel::Own && !s.is_turret() && danger(threats, s.action_cell) == 0.0)
        .min_by(|x, y| dist(a.cell, x.action_cell).partial_cmp(&dist(a.cell, y.action_cell)).unwrap())
        .map(|s| s.action_cell)
}

fn deny(mind: &mut Mind, target: usize, tick: u64, danger: f32) {
    let strikes = mind.denied.iter().find(|d| d.target == target).map_or(0, |d| d.strikes) + 1;
    mind.denied.retain(|d| d.target != target);
    mind.denied.push(Denied { target, until: tick + DENY * (1 << (strikes - 1).min(3)), strikes, danger });
}

/// One look at every Advancer of the side. Returns the orders to give; Advancers not listed keep what they do.
pub fn think(mind: &mut Mind, advs: &[Adv], sites: &[Site], threats: &[Threat], rate: i32, tick: u64) -> Vec<(u32, Order)> {
    let site = |k: usize| sites.iter().find(|s| s.index == k);
    let home = sites.iter().find(|s| s.rel == Rel::Own && s.kind_id == kinds::HEADQUARTERS).map(|s| s.action_cell);
    // shot down on the job: remember the place
    let lost: Vec<Task> = mind.tasks.iter().copied().filter(|t| !advs.iter().any(|a| a.id == t.unit)).collect();
    for t in lost {
        let d = site(t.target).map_or(0.0, |s| danger(threats, s.action_cell)).max(0.01);
        deny(mind, t.target, tick, d);
    }
    mind.tasks.retain(|t| advs.iter().any(|a| a.id == t.unit));
    mind.denied.retain(|d| d.until > tick || d.strikes > 1 && d.until + DENY * 4 > tick);
    let mut orders = Vec::new();
    for a in advs {
        if a.holding {
            continue;
        }
        let here = danger(threats, a.cell);
        let task = mind.tasks.iter().position(|t| t.unit == a.id);
        if let Some(n) = task {
            let t = mind.tasks[n];
            let target = site(t.target).filter(|s| still_valid(s, t.job));
            let d = target.map_or(f32::MAX, |s| dist(a.cell, s.action_cell));
            if d + 0.5 < t.best {
                mind.tasks[n].best = d;
                mind.tasks[n].best_tick = tick;
            }
            match target {
                None => {
                    mind.tasks.remove(n); // done or gone: choose again below
                }
                Some(s) => {
                    if !t.forced && here > 0.0 && (t.start_hp - a.hp) as f32 > BREAK_OFF * a.max_hp as f32 {
                        // losing the fight: break off, remember, go home
                        deny(mind, t.target, tick, danger(threats, s.action_cell).max(here));
                        mind.tasks.remove(n);
                        if let Some(c) = shelter(a, sites, threats) {
                            orders.push((a.id, Order::Retreat(c)));
                        }
                        continue;
                    } else if !a.working && tick.saturating_sub(mind.tasks[n].best_tick) > STUCK {
                        // no step closer for a long time: unreachable for now
                        deny(mind, t.target, tick, 0.0);
                        mind.tasks.remove(n);
                    } else if a.working {
                        continue; // never switch in the middle of the beam work
                    } else if t.forced {
                        // a forced push gives way to any safe job that turned up
                        let busy: Vec<usize> = mind.tasks.iter().filter(|o| o.unit != a.id).map(|o| o.target).collect();
                        if let Some((k, job, b)) = best_job(mind, a, sites, threats, rate, tick, &busy, home) {
                            mind.tasks.remove(n);
                            mind.tasks.push(Task { unit: a.id, target: k, job, start_hp: a.hp, best: f32::MAX, best_tick: tick, score: b, forced: false });
                            orders.push((a.id, Order::Go { target: k, job }));
                        }
                        continue;
                    } else {
                        // still flying: switch only to a clearly better job
                        let busy: Vec<usize> = mind.tasks.iter().filter(|o| o.unit != a.id).map(|o| o.target).collect();
                        let now = score(a, s, t.job, threats, rate, home).map(|v| ranked(a, s, t.job, v, mind, home));
                        match (now, best_job(mind, a, sites, threats, rate, tick, &busy, home)) {
                            (None, _) => {
                                // the way got too dangerous: give it up
                                deny(mind, t.target, tick, danger(threats, s.action_cell).max(0.01));
                                mind.tasks.remove(n);
                            }
                            (Some(v), Some((k, job, b))) if k != t.target && b > 2.0 * v => {
                                mind.tasks.remove(n);
                                mind.tasks.push(Task { unit: a.id, target: k, job, start_hp: a.hp, best: f32::MAX, best_tick: tick, score: b, forced: false });
                                orders.push((a.id, Order::Go { target: k, job }));
                                continue;
                            }
                            _ => continue,
                        }
                    }
                }
            }
        }
        let busy: Vec<usize> = mind.tasks.iter().map(|o| o.target).collect();
        if let Some((k, job, v)) = best_job(mind, a, sites, threats, rate, tick, &busy, home) {
            mind.tasks.push(Task { unit: a.id, target: k, job, start_hp: a.hp, best: f32::MAX, best_tick: tick, score: v, forced: false });
            orders.push((a.id, Order::Go { target: k, job }));
        } else if let Some((k, job, v)) = forced_job(mind, a, sites, threats, rate, tick, &busy, home) {
            // nothing safe left: push the target where a life gets the most done
            mind.tasks.push(Task { unit: a.id, target: k, job, start_hp: a.hp, best: f32::MAX, best_tick: tick, score: v, forced: true });
            orders.push((a.id, Order::Go { target: k, job }));
        } else if let Some(c) = shelter(a, sites, threats) {
            // nothing it could even reach: wait at home, out of reach
            if here > 0.0 || (a.idle && dist(a.cell, c) > 4.0) {
                orders.push((a.id, Order::Retreat(c)));
            }
        }
    }
    orders
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(index: usize, kind_id: i32, rel: Rel, cell: (i32, i32)) -> Site {
        let max_hp = if kinds::TURRETS.contains(&kind_id) { 900 } else { 13536 };
        let hp = if rel == Rel::Neutral { max_hp / 2 } else { max_hp };
        Site { index, kind_id, rel, hp, max_hp, upgrade_left: 0, protected: kind_id == kinds::HEADQUARTERS, action_cell: cell, healing: false }
    }

    fn adv(cell: (i32, i32)) -> Adv {
        Adv { id: 7, cell, hp: 800, max_hp: 800, working: false, idle: false, holding: false }
    }

    fn aa(cell: (i32, i32)) -> Threat {
        Threat { cell, range: 9.0, dps: 0.45 }
    }

    fn go(o: &[(u32, Order)]) -> Option<usize> {
        o.iter().find_map(|(_, o)| match o {
            Order::Go { target, .. } => Some(*target),
            _ => None,
        })
    }

    #[test]
    fn it_never_starts_a_capture_under_anti_air_fire() {
        let sites = [site(0, 100, Rel::Own, (10, 10)), site(1, 105, Rel::Foe, (40, 10)), site(2, 107, Rel::Neutral, (12, 30))];
        let threats = [aa((41, 12))];
        let mut m = Mind::default();
        let o = think(&mut m, &[adv((11, 11))], &sites, &threats, 10, 0);
        assert_eq!(go(&o), Some(2), "the guarded gold mine is too dangerous, the open windtrap is not: {o:?}");
        // without the anti-air the gold mine of the enemy is the better catch
        let mut m = Mind::default();
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &sites, &[], 10, 0)), Some(1));
    }

    #[test]
    fn shot_down_it_does_not_fly_to_the_same_target_again() {
        let sites = [site(0, 100, Rel::Own, (10, 10)), site(1, 105, Rel::Foe, (30, 10)), site(2, 107, Rel::Neutral, (10, 30))];
        let mut m = Mind::default();
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &sites, &[], 10, 0)), Some(1));
        // an enemy anti-air unit arrived and the Advancer died; the new one must not go back while the unit stays
        let threats = [Threat { cell: (29, 11), range: 9.0, dps: 0.1 }];
        let o = think(&mut m, &[Adv { id: 8, ..adv((11, 11)) }], &sites, &threats, 10, 600);
        assert_eq!(go(&o), Some(2), "{o:?} {m:?}");
        // a minute later with the unit gone, it may try again
        let a = Adv { id: 8, ..adv((11, 11)) };
        m.tasks.clear();
        assert_eq!(go(&think(&mut m, &[a], &sites, &[], 10, 600 + DENY + 30)), Some(1));
    }

    #[test]
    fn with_nothing_safe_left_it_pushes_the_best_target_and_keeps_at_it() {
        // every target is covered by two A.A. turrets: too risky for a normal job
        let sites = [site(0, 100, Rel::Own, (10, 10)), site(1, 105, Rel::Foe, (30, 10)), site(2, 107, Rel::Neutral, (10, 30))];
        let threats = [aa((31, 11)), aa((29, 9)), aa((11, 31)), aa((9, 29))];
        let mut m = Mind::default();
        let o = think(&mut m, &[adv((11, 11))], &sites, &threats, 10, 0);
        let k = go(&o).expect("it does not sit at home");
        assert!(m.tasks[0].forced);
        // under fire on the forced job it does not break off
        let a = Adv { hp: 300, working: true, ..adv(sites[k].action_cell) };
        assert!(think(&mut m, &[a], &sites, &threats, 10, 900).is_empty(), "{m:?}");
        // shot down: the new Advancer pushes again (the damage done stays)
        let o = think(&mut m, &[Adv { id: 8, ..adv((11, 11)) }], &sites, &threats, 10, 1200);
        assert!(go(&o).is_some(), "{o:?}");
        // a safe job turning up takes over while it flies
        let mut more = sites.to_vec();
        more.push(site(3, 101, Rel::Neutral, (14, 14)));
        let o = think(&mut m, &[Adv { id: 8, ..adv((12, 12)) }], &more, &threats, 10, 1230);
        assert_eq!(go(&o), Some(3), "{o:?}");
        assert!(!m.tasks[0].forced);
    }

    #[test]
    fn on_top_of_a_building_it_finishes_every_upgrade_level_first() {
        let hq = site(0, 100, Rel::Own, (10, 10));
        // it just captured this windtrap (still healing in) and hovers over it; a rich enemy factory waits nearby
        let mut wind = site(1, 107, Rel::Own, (30, 10));
        wind.upgrade_left = 2000;
        wind.healing = true;
        wind.hp = 2000;
        let factory = site(2, 103, Rel::Foe, (34, 12));
        let mut m = Mind::default();
        let o = think(&mut m, &[adv((30, 10))], &[hq, wind, factory], &[], 10, 0);
        assert_eq!(o, vec![(7, Order::Go { target: 1, job: Job::Upgrade })]);
        // level after level: while it works there it stays, and a new level keeps it there
        let a = Adv { working: true, ..adv((30, 10)) };
        assert!(think(&mut m, &[a], &[hq, wind, factory], &[], 10, 600).is_empty());
        // every level is worth the same: a higher level is not skipped for a capture
        let fresh = Adv { id: 9, ..adv((30, 10)) };
        let mut m = Mind::default();
        assert_eq!(go(&think(&mut m, &[fresh], &[hq, wind, factory], &[], 10, 900)), Some(1));
        // done: then the factory
        wind.upgrade_left = 0;
        let mut m = Mind::default();
        assert_eq!(go(&think(&mut m, &[adv((30, 10))], &[hq, wind, factory], &[], 10, 1200)), Some(2));
    }

    #[test]
    fn it_waits_over_a_building_that_is_going_down_and_upgrades_it_once_captured() {
        let hq = site(0, 100, Rel::Own, (10, 10));
        let mut prize = site(1, 103, Rel::Foe, (30, 10));
        let other = site(2, 105, Rel::Neutral, (10, 60));
        let mut m = Mind::default();
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &[hq, prize, other], &[], 10, 0)), Some(1));
        // the building is dying: it is no site any more, but the Advancer holds its task and stays
        let a = Adv { holding: true, ..adv((30, 10)) };
        assert!(think(&mut m, &[a], &[hq, other], &[], 10, 30).is_empty());
        assert_eq!(m.tasks.len(), 1);
        // captured: it upgrades on the spot
        prize.rel = Rel::Own;
        prize.upgrade_left = 2000;
        prize.healing = true;
        assert_eq!(go(&think(&mut m, &[adv((30, 10))], &[hq, prize, other], &[], 10, 60)), Some(1));
    }

    #[test]
    fn tiered_upgrades_first_then_nearby_neutrals_then_the_opponent() {
        let hq = site(0, 100, Rel::Own, (10, 10));
        let mut mine = site(1, 105, Rel::Own, (50, 10));
        mine.upgrade_left = 2000;
        let near = site(2, 107, Rel::Neutral, (20, 10));
        let far_rich = site(3, 105, Rel::Foe, (35, 12));
        let mut m = Mind { tiered: true, ..Default::default() };
        // the upgrade 40 cells away still beats a windtrap next door and the enemy mine
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &[hq, mine, near, far_rich], &[], 10, 0)), Some(1));
        // nothing to upgrade: the neutral building nearby beats the (richer) opponent's one
        mine.upgrade_left = 0;
        let mut m = Mind { tiered: true, ..Default::default() };
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &[hq, mine, near, far_rich], &[], 10, 0)), Some(2));
        // none nearby: the opponent's
        let mut m = Mind { tiered: true, ..Default::default() };
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &[hq, mine, far_rich], &[], 10, 0)), Some(3));
        // of two neutral buildings the one nearest to where it is now
        let near2 = site(4, 106, Rel::Neutral, (14, 10));
        let mut m = Mind { tiered: true, ..Default::default() };
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &[hq, near, near2], &[], 10, 0)), Some(4));
    }

    #[test]
    fn hard_takes_valuable_enemy_buildings_among_the_nearby_captures() {
        let hq = site(0, 100, Rel::Own, (10, 10));
        let near = site(2, 107, Rel::Neutral, (20, 10));
        let mine = site(3, 105, Rel::Foe, (30, 12));
        let sites = [hq, near, mine];
        let mut m = Mind { tiered: true, bold: 0, ..Default::default() };
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &sites, &[], 10, 0)), Some(2), "Normal: the neutral one first");
        let mut m = Mind { tiered: true, bold: 1, ..Default::default() };
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &sites, &[], 10, 0)), Some(3), "Hard: same tier, the richer one weighs more");
        let mut m = Mind { tiered: true, bold: 1, ..Default::default() };
        assert_eq!(go(&think(&mut m, &[adv((28, 11))], &sites, &[], 10, 0)), Some(3), "Hard: and more so when it is nearest");
    }

    #[test]
    fn tiered_it_retakes_a_lost_building_at_home_and_does_not_repair_turrets() {
        let hq = site(0, 100, Rel::Own, (10, 10));
        let lost = site(1, 103, Rel::Foe, (16, 12));
        let neutral = site(2, 107, Rel::Neutral, (12, 14));
        let mut m = Mind { tiered: true, ..Default::default() };
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &[hq, lost, neutral], &[], 10, 0)), Some(1), "the lost factory first");
        // two hurt own turrets are no job on their own
        let mut t1 = site(3, 27, Rel::Own, (14, 10));
        t1.hp = 100;
        let mut t2 = site(4, 27, Rel::Own, (6, 10));
        t2.hp = 100;
        let mut m = Mind { tiered: true, ..Default::default() };
        assert!(go(&think(&mut m, &[adv((11, 11))], &[hq, t1, t2], &[], 10, 0)).is_none());
        let mut m = Mind::default();
        assert!(go(&think(&mut m, &[adv((11, 11))], &[hq, t1, t2], &[], 10, 0)).is_some(), "untiered still repairs");
    }

    #[test]
    fn captures_near_the_advancer_or_the_hq_come_first() {
        // two equal neutral windtraps: one 6 cells from the HQ, one 30 cells away
        let sites = [site(0, 100, Rel::Own, (10, 10)), site(1, 107, Rel::Neutral, (40, 10)), site(2, 107, Rel::Neutral, (10, 16))];
        let mut m = Mind::default();
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &sites, &[], 10, 0)), Some(2));
        // the Advancer already stands by the far one: that one is nearest now
        let mut m = Mind::default();
        assert_eq!(go(&think(&mut m, &[adv((39, 10))], &sites, &[], 10, 0)), Some(1));
        // a far, richer target loses to a near one of a little less worth (gold mine 9 at 40 cells vs lab 6 next door)
        let sites = [site(0, 100, Rel::Own, (10, 10)), site(1, 105, Rel::Neutral, (50, 10)), site(2, 106, Rel::Neutral, (10, 16))];
        let mut m = Mind::default();
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &sites, &[], 10, 0)), Some(2));
    }

    #[test]
    fn a_target_it_cannot_reach_alive_is_not_forced() {
        let sites = [site(0, 100, Rel::Own, (10, 10)), site(1, 105, Rel::Foe, (40, 10))];
        // a wall of anti-air along the whole way
        let threats: Vec<Threat> = (14..40).step_by(2).map(|x| Threat { cell: (x, 10), range: 9.0, dps: 2.0 }).collect();
        let mut m = Mind::default();
        let o = think(&mut m, &[adv((11, 11))], &sites, &threats, 10, 0);
        assert!(go(&o).is_none(), "{o:?}");
    }

    #[test]
    fn under_fire_it_breaks_off_and_flies_home() {
        let sites = [site(0, 100, Rel::Own, (10, 10)), site(1, 101, Rel::Neutral, (30, 10))];
        let mut m = Mind::default();
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &sites, &[], 10, 0)), Some(1));
        // anti-air units appeared at the target; it has lost 400 hp there
        let threats = [Threat { cell: (31, 11), range: 9.0, dps: 0.4 }];
        let o = think(&mut m, &[Adv { hp: 400, working: true, ..adv((30, 10)) }], &sites, &threats, 10, 900);
        assert_eq!(o, vec![(7, Order::Retreat((10, 10)))]);
        assert!(m.denied.iter().any(|d| d.target == 1));
    }

    #[test]
    fn an_unreachable_target_is_dropped() {
        let sites = [site(0, 100, Rel::Own, (10, 10)), site(1, 101, Rel::Neutral, (30, 10)), site(2, 107, Rel::Neutral, (10, 40))];
        let mut m = Mind::default();
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &sites, &[], 10, 0)), Some(1));
        // it never gets closer than where it started
        for t in (30..=STUCK + 60).step_by(30) {
            let o = think(&mut m, &[adv((11, 11))], &sites, &[], 10, t);
            if let Some(k) = go(&o) {
                assert_eq!(k, 2, "it gives up the unreachable one");
                return;
            }
        }
        panic!("still stuck: {m:?}");
    }

    #[test]
    fn hurt_buildings_get_repaired_and_mines_upgraded_at_home() {
        let mut hq = site(0, 100, Rel::Own, (10, 10));
        let mut mine = site(1, 105, Rel::Own, (14, 10));
        mine.upgrade_left = 2000;
        let mut m = Mind::default();
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &[hq, mine], &[], 10, 0)), Some(1), "upgrade the mine");
        // repairs come last: the badly hurt HQ waits while the mine can still be upgraded
        hq.hp = 4000;
        let mut m = Mind::default();
        assert_eq!(go(&think(&mut m, &[adv((11, 11))], &[hq, mine], &[], 10, 0)), Some(1));
        mine.upgrade_left = 0;
        let mut m = Mind::default();
        let o = think(&mut m, &[adv((11, 11))], &[hq, mine], &[], 10, 0);
        assert_eq!(o, vec![(7, Order::Go { target: 0, job: Job::Repair })], "nothing else left");
        // a capture turning up takes over from a repair while it flies
        let near = site(2, 107, Rel::Neutral, (40, 10));
        let o = think(&mut m, &[adv((12, 11))], &[hq, mine, near], &[], 10, 30);
        assert_eq!(go(&o), Some(2));
    }

    #[test]
    fn it_does_not_switch_during_beam_work_and_two_advancers_split_the_jobs() {
        let sites = [site(0, 100, Rel::Own, (10, 10)), site(1, 107, Rel::Neutral, (12, 14)), site(2, 105, Rel::Neutral, (20, 10))];
        let mut m = Mind::default();
        let o = think(&mut m, &[adv((11, 11)), Adv { id: 9, ..adv((11, 12)) }], &sites, &[], 10, 0);
        let mut targets: Vec<usize> = o.iter().filter_map(|(_, o)| if let Order::Go { target, .. } = o { Some(*target) } else { None }).collect();
        targets.sort();
        assert_eq!(targets, [1, 2]);
        let t = m.tasks[0];
        let me = Adv { id: t.unit, working: true, ..adv(sites[t.target].action_cell) };
        let other = m.tasks[1].unit;
        assert!(think(&mut m, &[me, Adv { id: other, working: true, ..adv((0, 0)) }], &sites, &[], 10, 60).is_empty());
    }
}
