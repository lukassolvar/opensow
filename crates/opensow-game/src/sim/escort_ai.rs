//! The computer against a custom map's escort (not in the exe, whose convoy missions 3 and 8 only use its unit-then-
//! headquarters pick). docs/map-editor-ingame.md "Missions".
//!
//! The computer knows the convoy's route and goal, as the player does, and plays to stop it:
//! - **Strike**: a unit with a pod within its strike radius attacks the nearest pod (goal guards only close to the goal).
//! - **Goal guard**: a share of its ground units holds the goal zone (a last line the convoy must break through).
//! - **Intercept**: every other unit walks to the first cell on the lead pod's remaining way that it reaches well before the
//!   pod does (straight-line distance with a detour allowance, its own walking speed against the convoy's 7 px/s), and waits
//!   there; one that cannot get ahead of the pod anywhere attacks the lead pod from behind.
//!
//! Per difficulty ([`Tuning`]): Easy only strikes at pods that come close and otherwise waits where it stands (its usual
//! strategy would send every unit at the nearest player unit, often a pod, which plays harder than Normal);
//! Normal adds the goal guard and interception; Hard and Brutal strike from further away, and Brutal also calls back units
//! that are busy with the player's buildings.
//!
//! Pure: the caller fills the pods, the route and each unit; no random numbers are drawn.

use super::Difficulty;

/// Px per second of a pod on a convoy (`anim::walk_params_for_mode(46, 3)`).
pub const POD_SPEED: f32 = 7.0;
/// Distances are straight lines; a unit's real way is longer by about this much.
const DETOUR: f32 = 1.35;
/// Route samples every this many cells.
const STEP: f32 = 2.0;
/// Goal guards only leave the goal for a pod this close (cells).
const GUARD_REACH: i32 = 8;
/// A unit this close to its post (cells) holds there.
pub const AT_POST: i32 = 2;

/// The difficulty's settings for playing against an escort.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// A pod within this many cells (Chebyshev) is attacked.
    pub strike: i32,
    /// Share of the ground units that hold the goal.
    pub goal_share: f32,
    /// Send the others ahead of the convoy (off: they wait where they stand).
    pub intercept: bool,
    /// Seconds a unit must be at a post before the lead pod.
    pub margin: f32,
    /// Units busy with the player's buildings are called back to the convoy.
    pub pull_busy: bool,
}

/// The settings of a difficulty level.
pub fn tuning(d: Difficulty) -> Tuning {
    match d {
        Difficulty::Easy => Tuning { strike: 12, goal_share: 0.0, intercept: false, margin: 0.0, pull_busy: false },
        Difficulty::Normal => Tuning { strike: 18, goal_share: 0.25, intercept: true, margin: 10.0, pull_busy: false },
        Difficulty::Hard => Tuning { strike: 24, goal_share: 0.3, intercept: true, margin: 6.0, pull_busy: false },
        Difficulty::Brutal => Tuning { strike: 30, goal_share: 0.3, intercept: true, margin: 4.0, pull_busy: true },
    }
}

/// What a unit was given to do (kept between looks: the goal guard's size is counted from it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Role {
    Strike,
    Goal,
    Intercept,
    /// Easy: waits where it stands for a pod to come close.
    Wait,
}

/// One of the player's pods: its cell and the index of the route cell it walks to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pod {
    /// The pod's unit id.
    pub id: u32,
    /// The cell it stands on.
    pub cell: (i32, i32),
    /// Index of the route cell it walks to.
    pub next: usize,
}

/// One of the computer's ground units (not anti-air, not the Advancer).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unit {
    /// The cell it stands on.
    pub cell: (i32, i32),
    /// Px per second.
    pub speed: f32,
    /// What it was told to do last time, if anything.
    pub role: Option<Role>,
}

/// What a unit is told to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Order {
    Attack(u32),
    /// Walk to the cell (the caller spreads units over free cells near it).
    Post((i32, i32)),
    /// Stay: already at its post.
    Hold,
}

fn chebyshev(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// The pod furthest along: the highest route index, then the nearest to that route cell.
pub fn lead(pods: &[Pod], route: &[(i32, i32)]) -> Option<Pod> {
    pods.iter().copied().max_by(|a, b| {
        let left = |p: &Pod| route.get(p.next).map_or(0.0, |&r| dist((p.cell.0 as f32, p.cell.1 as f32), (r.0 as f32, r.1 as f32)));
        a.next.cmp(&b.next).then(left(b).total_cmp(&left(a)))
    })
}

/// The lead pod's remaining way, sampled every [`STEP`] cells: each cell with the seconds the pod needs to get there.
pub fn ahead(route: &[(i32, i32)], lead: Pod) -> Vec<((i32, i32), f32)> {
    let mut out = Vec::new();
    let mut at = (lead.cell.0 as f32, lead.cell.1 as f32);
    let mut walked = 0.0;
    for &r in route.iter().skip(lead.next) {
        let to = (r.0 as f32, r.1 as f32);
        let len = dist(at, to);
        let mut s = STEP - walked % STEP;
        while s <= len {
            let f = s / len;
            let p = (at.0 + (to.0 - at.0) * f, at.1 + (to.1 - at.1) * f);
            out.push(((p.0.round() as i32, p.1.round() as i32), (walked + s) * 32.0 / POD_SPEED));
            s += STEP;
        }
        walked += len;
        at = to;
    }
    if let Some(&g) = route.last() {
        if out.last().is_none_or(|l| l.0 != g) {
            out.push((g, walked * 32.0 / POD_SPEED));
        }
    }
    out
}

/// What an idle unit does (`None`: no convoy left, the usual strategy). `guard`: it is to join the goal guard.
pub fn decide(t: &Tuning, route: &[(i32, i32)], pods: &[Pod], way: &[((i32, i32), f32)], u: &Unit, guard: bool) -> Option<(Order, Role)> {
    let goal = *route.last()?;
    let lead = lead(pods, route)?;
    let reach = if u.role == Some(Role::Goal) || guard { t.strike.min(GUARD_REACH) } else { t.strike };
    if let Some(p) = pods.iter().filter(|p| chebyshev(p.cell, u.cell) <= reach).min_by_key(|p| chebyshev(p.cell, u.cell)) {
        return Some((Order::Attack(p.id), u.role.filter(|&r| r == Role::Goal).unwrap_or(Role::Strike)));
    }
    let post = |c: (i32, i32)| if chebyshev(c, u.cell) <= AT_POST { Order::Hold } else { Order::Post(c) };
    if guard || u.role == Some(Role::Goal) {
        return Some((post(goal), Role::Goal));
    }
    if !t.intercept {
        return Some((Order::Hold, Role::Wait));
    }
    // posted on the convoy's way: wait there for it (re-planning would walk it a little nearer every look)
    if u.role == Some(Role::Intercept) && way.iter().any(|&(c, _)| chebyshev(c, u.cell) <= AT_POST) {
        return Some((Order::Hold, Role::Intercept));
    }
    let here = (u.cell.0 as f32, u.cell.1 as f32);
    let first = way.iter().find(|&&(c, pod_t)| dist(here, (c.0 as f32, c.1 as f32)) * DETOUR * 32.0 / u.speed.max(1.0) + t.margin <= pod_t);
    Some(match first {
        Some(&(c, _)) => (post(c), Role::Intercept),
        None => (Order::Attack(lead.id), Role::Strike),
    })
}

/// How many units the goal guard wants out of `total` ground units.
pub fn guard_size(t: &Tuning, total: usize) -> usize {
    (t.goal_share * total as f32).ceil() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTE: [(i32, i32); 3] = [(10, 0), (10, 20), (30, 20)];

    fn pod(id: u32, cell: (i32, i32), next: usize) -> Pod {
        Pod { id, cell, next }
    }

    fn unit(cell: (i32, i32)) -> Unit {
        Unit { cell, speed: 20.0, role: None }
    }

    #[test]
    fn the_lead_pod_is_the_one_furthest_along_and_its_way_is_timed() {
        let pods = [pod(1, (10, 5), 1), pod(2, (10, 12), 1), pod(3, (2, 0), 0)];
        assert_eq!(lead(&pods, &ROUTE).unwrap().id, 2);
        let way = ahead(&ROUTE, pods[1]);
        // 8 cells down, then 20 east: samples every 2 cells, the goal last
        assert_eq!(way.first().unwrap().0, (10, 14));
        assert_eq!(way.last().unwrap().0, (30, 20));
        assert!((way.last().unwrap().1 - 28.0 * 32.0 / POD_SPEED).abs() < 0.01);
        assert!(way.windows(2).all(|w| w[0].1 < w[1].1));
        assert!(way.contains(&((10, 20), 8.0 * 32.0 / POD_SPEED)), "the corner itself");
    }

    #[test]
    fn units_strike_close_pods_and_get_ahead_of_the_others() {
        let t = tuning(Difficulty::Normal);
        let pods = [pod(7, (10, 5), 1)];
        let way = ahead(&ROUTE, pods[0]);
        // close: attack
        assert_eq!(decide(&t, &ROUTE, &pods, &way, &unit((14, 10)), false), Some((Order::Attack(7), Role::Strike)));
        // near the end of the route: wait there, at the first cell it reaches with time to spare
        let (o, r) = decide(&t, &ROUTE, &pods, &way, &unit((30, 30)), false).unwrap();
        let Order::Post(c) = o else { panic!("{o:?}") };
        assert_eq!(r, Role::Intercept);
        let pod_t = way.iter().find(|w| w.0 == c).unwrap().1;
        let unit_t = (((c.0 - 30) as f32).powi(2) + ((c.1 - 30) as f32).powi(2)).sqrt() * DETOUR * 32.0 / 20.0;
        assert!(unit_t + t.margin <= pod_t, "{c:?}: {unit_t} + margin vs {pod_t}");
        // and no earlier cell of the way would have done
        let k = way.iter().position(|w| w.0 == c).unwrap();
        let arrive = |w: &((i32, i32), f32)| (((w.0 .0 - 30) as f32).powi(2) + ((w.0 .1 - 30) as f32).powi(2)).sqrt() * DETOUR * 32.0 / 20.0 + t.margin;
        assert!(k > 0 && way[..k].iter().all(|w| arrive(w) > w.1), "the earliest one");
        // a post that close to the pod is in strike reach: the unit at it attacks; at a far post it holds
        let at = Unit { cell: (c.0 + 1, c.1), ..unit((0, 0)) };
        assert_eq!(decide(&t, &ROUTE, &pods, &way, &at, false).unwrap().0, Order::Attack(7));
        let long = [(10, 0), (10, 60), (60, 60)];
        let start = [pod(7, (10, 1), 1)];
        let long_way = ahead(&long, start[0]);
        let Some((Order::Post(p), Role::Intercept)) = decide(&t, &long, &start, &long_way, &unit((60, 70)), false) else { panic!() };
        assert!(chebyshev(p, (10, 1)) > t.strike, "{p:?}");
        let there = Unit { cell: (p.0, p.1 + 1), role: Some(Role::Intercept), ..unit((0, 0)) };
        assert_eq!(decide(&t, &long, &start, &long_way, &there, false), Some((Order::Hold, Role::Intercept)), "posted: it waits");
        // too slow to get ahead anywhere: follow the lead pod
        let slow = Unit { speed: 1.0, ..unit((60, 60)) };
        assert_eq!(decide(&t, &ROUTE, &pods, &way, &slow, false), Some((Order::Attack(7), Role::Strike)));
    }

    #[test]
    fn the_goal_guard_holds_the_goal_and_only_meets_pods_close_to_it() {
        let t = tuning(Difficulty::Brutal);
        let pods = [pod(7, (10, 5), 1)];
        let way = ahead(&ROUTE, pods[0]);
        assert_eq!(decide(&t, &ROUTE, &pods, &way, &unit((40, 40)), true), Some((Order::Post((30, 20)), Role::Goal)));
        let guard = Unit { role: Some(Role::Goal), ..unit((31, 21)) };
        assert_eq!(decide(&t, &ROUTE, &pods, &way, &guard, false), Some((Order::Hold, Role::Goal)));
        // a pod 20 cells away: an ordinary unit on Brutal goes for it, the guard stays
        let near = [pod(7, (11, 20), 2)];
        let way = ahead(&ROUTE, near[0]);
        assert_eq!(decide(&t, &ROUTE, &near, &way, &guard, false).unwrap().0, Order::Hold);
        assert_eq!(decide(&t, &ROUTE, &near, &way, &unit((31, 21)), false).unwrap().0, Order::Attack(7));
        let closer = [pod(7, (25, 20), 2)];
        let way = ahead(&ROUTE, closer[0]);
        assert_eq!(decide(&t, &ROUTE, &closer, &way, &guard, false), Some((Order::Attack(7), Role::Goal)));
        assert_eq!(guard_size(&t, 10), 3);
        assert_eq!(guard_size(&tuning(Difficulty::Easy), 10), 0);
    }

    #[test]
    fn easy_only_strikes_close_pods_and_waits_otherwise() {
        let t = tuning(Difficulty::Easy);
        let pods = [pod(7, (10, 5), 1)];
        let way = ahead(&ROUTE, pods[0]);
        assert_eq!(decide(&t, &ROUTE, &pods, &way, &unit((30, 30)), false), Some((Order::Hold, Role::Wait)));
        assert_eq!(decide(&t, &ROUTE, &pods, &way, &unit((15, 12)), false).unwrap().0, Order::Attack(7));
        let difficulties = Difficulty::ALL.map(tuning);
        assert!(difficulties.windows(2).all(|w| w[0].strike < w[1].strike), "the reach grows with the level");
    }
}
