//! The computer's turret planner for skirmishes (not in the exe; campaign missions keep the decoded routine `0x431120`,
//! `formats::turretai`). docs/skirmish.md "Turrets".
//!
//! Purely reactive, like a player who defends what is attacked and builds nothing for fun:
//! - **Ground**: a building with enemy units pressing on it (close by, or heading for it) wants 1.5 tier points of firepower
//!   per unit of attacking weight (a tank 1, a walker 2.5), never more than five guns. Without attackers it wants nothing.
//! - **Buy** the most expensive gun available that the credits allow (artillery only behind a direct gun); when nothing is
//!   affordable it waits and saves.
//! - **Anti-air**: where an enemy Advancer works on a building or hovers by it, three anti-air turrets cover that building
//!   (it takes about three to bring an Advancer down); none otherwise.
//! - **Place** guns on the side the danger comes from, one cell clear of the walls so units can pass, spread out (no
//!   clumps), covering as many buildings as possible, never on or next to a factory exit; artillery goes behind.
//!
//! Pure: the caller fills a [`Snapshot`] and supplies the placement test; no random numbers are drawn.

use opensow_formats::buildings::footprint;

/// Reach of turret tiers 1..5 in cells (docs/ai.md "Turrets and their guns").
pub const RANGE: [f32; 5] = [6.0, 9.0, 7.0, 8.0, 10.0];
/// Ground firepower of tiers 1..5 in "tier points" (the A.A. defence shoots aircraft and the Advancer only).
pub const POWER: [f32; 5] = [2.0, 0.0, 3.0, 4.0, 4.0];
/// No building gets more guns than this.
pub const MAX_PER_SITE: usize = 5;
/// Anti-air turrets wanted at a building an enemy Advancer is at: two hold it while it works on the building itself ...
pub const AA_MIN: f32 = 2.0;
/// ... three once it starts on the turrets that cover it ...
pub const AA_UNDER_FIRE: f32 = 3.0;
/// ... and four keep the building safe for good (the Advancer is killed before it reaches the building); bought only when
/// the credits are comfortable and no building needs ground guns.
pub const AA_SOLID: f32 = 4.0;
pub const SOLID_CREDITS: i32 = 1200;
/// Enemy units within this many cells of a building's edge press on it.
const NEAR: f32 = 9.0;

fn price(tier: u32) -> i32 {
    200 * tier as i32
}

/// An own building that is not a turret.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Site {
    pub kind_id: i32,
    pub cell: (i32, i32),
    /// An enemy Advancer works on it or hovers close by.
    pub advancer: bool,
}

impl Site {
    pub fn size(&self) -> (i32, i32) {
        footprint(self.kind_id).map_or((1, 1), |(w, h, _)| (w, h))
    }

    pub fn centre(&self) -> (f32, f32) {
        let (w, h) = self.size();
        (self.cell.0 as f32 + w as f32 / 2.0, self.cell.1 as f32 + h as f32 / 2.0)
    }

    fn half(&self) -> f32 {
        let (w, h) = self.size();
        w.min(h) as f32 / 2.0
    }

    /// Chebyshev distance from a cell to the footprint (0 inside).
    fn gap(&self, c: (i32, i32)) -> i32 {
        let (w, h) = self.size();
        let dx = (self.cell.0 - c.0).max(c.0 - (self.cell.0 + w - 1)).max(0);
        let dy = (self.cell.1 - c.1).max(c.1 - (self.cell.1 + h - 1)).max(0);
        dx.max(dy)
    }

    /// What losing it would cost.
    pub fn value(&self) -> f32 {
        match self.kind_id {
            100 => 10.0,
            103 => 8.0,
            102 | 105 | 108 => 7.0,
            101 | 104 | 106 => 5.0,
            _ => 4.0,
        }
    }
}

/// An own turret (finished or still healing in).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gun {
    pub tier: u32,
    pub cell: (i32, i32),
    /// An enemy Advancer is working on this turret.
    pub attacked: bool,
}

impl Gun {
    fn centre(&self) -> (f32, f32) {
        gun_centre(self.tier, self.cell)
    }
}

fn gun_centre(tier: u32, cell: (i32, i32)) -> (f32, f32) {
    (cell.0 as f32 + 0.5, cell.1 as f32 + if tier >= 4 { 1.0 } else { 0.5 })
}

/// An enemy ground unit: its cell, how much it counts (1 per tank, more for walkers) and the building it is after.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Foe {
    pub cell: (i32, i32),
    pub weight: f32,
    pub target: Option<usize>,
}

/// What the planner sees.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub sites: Vec<Site>,
    pub guns: Vec<Gun>,
    pub foes: Vec<Foe>,
    /// Centre of the enemy base (its HQ), the direction a gun faces when no unit shows it.
    pub enemy_base: Option<(i32, i32)>,
    pub credits: i32,
    /// Turret tiers 1..5 this side may build.
    pub flags: [bool; 5],
    /// Cells that must stay free (factory exits and the cells units walk on after them).
    pub keep_clear: Vec<(i32, i32)>,
}

/// The decision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Plan {
    Build { tier: u32, cell: (i32, i32) },
    /// Something is wanted but the money is not there yet: wait.
    Save,
    Idle,
}

/// One building's situation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Need {
    pub site: usize,
    /// Enemy pressure (unit weights) on it.
    pub danger: f32,
    /// Ground firepower covering it.
    pub cover: f32,
    /// Ground firepower missing.
    pub ground: f32,
    /// Anti-air turrets missing to hold the Advancer ([`AA_MIN`], [`AA_UNDER_FIRE`]).
    pub air: f32,
    /// Anti-air turrets missing for a solid defence ([`AA_SOLID`]); 0 when no Advancer is at the building.
    pub air_solid: f32,
    /// Number of guns covering it.
    pub guns: usize,
    /// Where the danger comes from (cell coordinates), if anywhere.
    pub from: Option<(f32, f32)>,
}

impl Need {
    pub fn urgent(&self, s: &Snapshot) -> bool {
        self.danger > 0.0 || s.sites[self.site].advancer
    }
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn mid(c: (i32, i32)) -> (f32, f32) {
    (c.0 as f32 + 0.5, c.1 as f32 + 0.5)
}

/// The situation of every own building.
pub fn needs(s: &Snapshot) -> Vec<Need> {
    s.sites
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let c = b.centre();
            let reach = |t: u32| RANGE[(t - 1) as usize] + b.half();
            let covering: Vec<&Gun> = s.guns.iter().filter(|g| dist(g.centre(), c) <= reach(g.tier)).collect();
            let cover: f32 = covering.iter().map(|g| POWER[(g.tier - 1) as usize]).sum();
            let aa = covering.iter().filter(|g| g.tier == 2).count() as f32;
            let mut danger = 0.0;
            let (mut fx, mut fy) = (0.0, 0.0);
            for f in &s.foes {
                let d = dist(mid(f.cell), c) - b.half();
                if d <= NEAR || (f.target == Some(i) && d <= 2.0 * NEAR) {
                    danger += f.weight;
                    fx += mid(f.cell).0 * f.weight;
                    fy += mid(f.cell).1 * f.weight;
                }
            }
            let from = if danger > 0.0 { Some((fx / danger, fy / danger)) } else { s.enemy_base.map(mid) };
            let full = covering.iter().filter(|g| g.tier != 2).count() >= MAX_PER_SITE;
            let ground = if danger > 0.0 && !full { (1.5 * danger).min(16.0) - cover } else { 0.0 };
            let goal = if !b.advancer { 0.0 } else if covering.iter().any(|g| g.attacked) { AA_UNDER_FIRE } else { AA_MIN };
            let air = (goal - aa).max(0.0);
            let air_solid = if b.advancer { (AA_SOLID - aa).max(0.0) } else { 0.0 };
            Need { site: i, danger, cover, ground, air, air_solid, guns: covering.len(), from }
        })
        .collect()
}

/// The most expensive allowed gun the credits pay for (artillery only behind a direct gun). `Err(())`: allowed guns exist
/// but none is affordable (save up); `Ok(None)`: no gun is allowed at all.
pub fn ground_tier(need: &Need, flags: &[bool; 5], credits: i32) -> Result<Option<u32>, ()> {
    let allowed: Vec<u32> = [5u32, 4, 3, 1].into_iter().filter(|&t| flags[(t - 1) as usize] && (t != 5 || need.cover >= 2.0)).collect();
    if allowed.is_empty() {
        return Ok(None);
    }
    allowed.into_iter().find(|&t| price(t) <= credits).map(Some).ok_or(())
}

/// The best cell for a `tier` gun guarding `need.site`, or None when nothing fits.
pub fn place(s: &Snapshot, need: &Need, tier: u32, ok: impl Fn(u32, (i32, i32)) -> bool) -> Option<(i32, i32)> {
    let b = &s.sites[need.site];
    let c = b.centre();
    let (w, h) = b.size();
    let gh = if tier >= 4 { 2 } else { 1 };
    let dir = need.from.map(|f| (f.0 - c.0, f.1 - c.1)).filter(|v| v.0.abs() + v.1.abs() > 0.01).map(|v| {
        let l = (v.0 * v.0 + v.1 * v.1).sqrt();
        (v.0 / l, v.1 / l)
    });
    let blocked = |cell: (i32, i32)| (0..gh).any(|dy| s.keep_clear.iter().any(|k| (k.0 - cell.0).abs() <= 1 && (k.1 - cell.1 - dy).abs() <= 1));
    let mut best: Option<(f32, (i32, i32))> = None;
    for y in b.cell.1 - 4..b.cell.1 + h + 4 {
        for x in b.cell.0 - 4..b.cell.0 + w + 4 {
            let cell = (x, y);
            let gap = (0..gh).map(|dy| b.gap((x, y + dy))).min().unwrap_or(0);
            if gap == 0 || blocked(cell) || !ok(tier, cell) {
                continue;
            }
            let p = gun_centre(tier, cell);
            let off = (p.0 - c.0, p.1 - c.1);
            let len = (off.0 * off.0 + off.1 * off.1).sqrt().max(0.01);
            let cos = dir.map_or(0.0, |d| (off.0 * d.0 + off.1 * d.1) / len);
            let mut score = match tier {
                5 => -2.0 * cos,
                2 => 0.0,
                _ => 3.0 * cos,
            };
            // one cell clear of the wall so units still pass; artillery and anti-air may stand closer
            score -= 0.7 * (gap - if matches!(tier, 2 | 5) { 1 } else { 2 }).abs() as f32;
            for g in &s.guns {
                let d = (g.cell.0 - x).abs().max((g.cell.1 - y).abs());
                score -= match d {
                    0 | 1 => 2.5,
                    2 => 0.8,
                    _ => 0.0,
                };
            }
            // a gun that also covers other buildings is worth more (anti-air most of all)
            let others = s.sites.iter().enumerate().filter(|&(k, o)| k != need.site && dist(p, o.centre()) <= RANGE[(tier - 1) as usize] + o.half()).count();
            score += others as f32 * if tier == 2 { 1.0 } else { 0.6 };
            if best.map_or(true, |(bs, _)| score > bs + 1e-4) {
                best = Some((score, cell));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// The next move: build one gun, save up, or nothing.
pub fn decide(s: &Snapshot, ok: impl Fn(u32, (i32, i32)) -> bool) -> Plan {
    let all = needs(s);
    let solid = s.credits >= SOLID_CREDITS && !all.iter().any(|n| n.ground > 0.0);
    // (priority, need, anti-air?): the Advancer first, then the building worth most
    let mut wants: Vec<(f32, &Need, bool)> = Vec::new();
    for n in &all {
        let v = s.sites[n.site].value();
        let air = if solid { n.air_solid } else { n.air };
        if air >= 1.0 && s.flags[1] {
            wants.push((100.0 + v * air, n, true));
        }
        if n.ground > 0.0 {
            wants.push((v * n.ground, n, false));
        }
    }
    wants.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.site.cmp(&b.1.site)));
    for (_, n, air) in wants {
        let tier = if air {
            2
        } else {
            match ground_tier(n, &s.flags, s.credits) {
                Ok(Some(t)) => t,
                Ok(None) => continue,
                Err(()) => return Plan::Save,
            }
        };
        if s.credits < price(tier) {
            return Plan::Save;
        }
        if let Some(cell) = place(s, n, tier, &ok) {
            return Plan::Build { tier, cell };
        }
    }
    Plan::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [bool; 5] = [true; 5];

    fn hq() -> Site {
        Site { kind_id: 100, cell: (20, 20), advancer: false }
    }

    fn snap(sites: Vec<Site>, foes: Vec<Foe>, credits: i32) -> Snapshot {
        Snapshot { sites, foes, credits, flags: ALL, enemy_base: Some((50, 22)), ..Default::default() }
    }

    fn foe(x: i32, y: i32) -> Foe {
        Foe { cell: (x, y), weight: 1.0, target: None }
    }

    /// Any free cell outside the buildings and guns (and the 1x2 guns' second cell).
    fn free(s: &Snapshot) -> impl Fn(u32, (i32, i32)) -> bool + '_ {
        move |t, c| {
            let cells: Vec<(i32, i32)> = if t >= 4 { vec![c, (c.0, c.1 + 1)] } else { vec![c] };
            cells.iter().all(|&p| {
                p.0 >= 0 && p.1 >= 0 && p.0 < 64 && p.1 < 64
                    && !s.sites.iter().any(|b| b.gap(p) == 0)
                    && !s.guns.iter().any(|g| g.cell == p || (g.tier >= 4 && (g.cell.0, g.cell.1 + 1) == p))
            })
        }
    }

    /// Runs the planner until it stops building; returns the guns it built.
    fn run(s: &mut Snapshot) -> Vec<Gun> {
        let mut built = Vec::new();
        for _ in 0..30 {
            match decide(s, free(s)) {
                Plan::Build { tier, cell } => {
                    s.credits -= price(tier);
                    let g = Gun { tier, cell, attacked: false };
                    s.guns.push(g);
                    built.push(g);
                }
                _ => break,
            }
        }
        built
    }

    #[test]
    fn no_attackers_no_turrets_however_rich() {
        let s = snap(vec![hq()], vec![], 50_000);
        assert_eq!(decide(&s, free(&s)), Plan::Idle);
    }

    #[test]
    fn the_guns_follow_the_number_of_attackers_and_are_the_dearest_available() {
        let count = |n: i32| {
            let foes: Vec<Foe> = (0..n).map(|i| foe(27 + i % 3, 18 + i / 3 * 2)).collect();
            let mut s = snap(vec![hq()], foes, 20_000);
            run(&mut s)
        };
        let (one, wave) = (count(1), count(12));
        assert_eq!(one.len(), 1, "{one:?}");
        assert_eq!(one[0].tier, 4, "the dearest direct gun even for one tank: {one:?}");
        assert!(wave.len() > one.len() && wave.len() <= MAX_PER_SITE, "{wave:?}");
    }

    #[test]
    fn guns_face_the_danger_and_spread_out() {
        let foes: Vec<Foe> = (0..4).map(|i| foe(30, 19 + i * 2)).collect(); // east of the HQ (20..24)
        let mut s = snap(vec![hq()], foes, 4000);
        let built = run(&mut s);
        assert!(built.len() >= 2);
        for g in built.iter().filter(|g| g.tier != 5) {
            assert!(g.cell.0 >= 24, "east side: {built:?}");
        }
        for (i, a) in built.iter().enumerate() {
            for b in &built[i + 1..] {
                assert!((a.cell.0 - b.cell.0).abs().max((a.cell.1 - b.cell.1).abs()) >= 2, "no clumps: {built:?}");
            }
        }
    }

    #[test]
    fn it_buys_what_it_can_afford_and_saves_when_nothing_is() {
        let foes: Vec<Foe> = (0..4).map(|i| foe(27, 19 + i)).collect();
        let s = snap(vec![hq()], foes.clone(), 250);
        assert!(matches!(decide(&s, free(&s)), Plan::Build { tier: 1, .. }), "250 credits: a tier-1 gun now");
        let s = snap(vec![hq()], foes, 100);
        assert_eq!(decide(&s, free(&s)), Plan::Save);
    }

    #[test]
    fn anti_air_grows_with_the_advancer_two_then_three_then_four_when_comfortable() {
        let mut b = hq();
        b.advancer = true;
        // poor: two hold it
        let mut s = snap(vec![b], vec![], 900);
        assert_eq!((needs(&s)[0].air, needs(&s)[0].air_solid), (AA_MIN, AA_SOLID));
        let built = run(&mut s);
        assert_eq!(built.len(), 2, "{built:?}");
        assert!(built.iter().all(|g| g.tier == 2));
        // it starts on the turrets: three
        s.guns[0].attacked = true;
        s.credits = 900;
        assert_eq!(run(&mut s).len(), 1);
        // rich and nothing else to pay for: four in all
        let mut s2 = snap(vec![b], vec![], 5000);
        assert_eq!(run(&mut s2).iter().filter(|g| g.tier == 2).count(), 4);
        // the money is needed for ground guns first
        let mut s3 = snap(vec![b], vec![foe(27, 22)], 5000);
        let built = run(&mut s3);
        assert!(built.iter().any(|g| g.tier != 2), "{built:?}");
        // the Advancer gone: nothing more
        s2.sites[0].advancer = false;
        assert_eq!(decide(&s2, free(&s2)), Plan::Idle);
        // no A.A. allowed and no ground danger
        let mut s = snap(vec![b], vec![], 1000);
        s.flags = [true, false, true, true, true];
        assert_eq!(decide(&s, free(&s)), Plan::Idle);
    }

    #[test]
    fn factory_exits_stay_free_and_disallowed_tiers_are_never_built() {
        let fac = Site { kind_id: 102, cell: (20, 20), advancer: false }; // medium factory 5x4, exit (24, 23)
        let foes: Vec<Foe> = (0..6).map(|i| foe(27, 20 + i)).collect();
        let mut s = snap(vec![fac], foes, 9000);
        s.flags = [true, true, true, false, false];
        s.keep_clear = vec![(24, 23), (24, 24), (24, 25)];
        let built = run(&mut s);
        assert!(!built.is_empty());
        for g in &built {
            assert!(g.tier <= 3 && g.tier != 2, "{built:?}");
            assert!(!s.keep_clear.iter().any(|k| (k.0 - g.cell.0).abs() <= 1 && (k.1 - g.cell.1).abs() <= 1), "exit blocked: {g:?}");
        }
    }
}
