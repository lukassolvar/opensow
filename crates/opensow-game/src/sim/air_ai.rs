//! The computer's bombers, fighters and triplers for skirmishes on Hard and Brutal (not in the exe: its routine launches a
//! random force every 600 ticks at a density peak in the upper map and leaves the rest of the stock unused, docs/ai.md
//! item 6). docs/skirmish.md "Air strikes".
//!
//! A player spends the air forces where they do the most good and does not sit on them:
//! - the **bomber** (carpet of 110-strength bombs, about 9 cells wide) and the **tripler** (204 blasts of 100 in a small
//!   ellipse) go where many enemy units and buildings stand close together; they are shot down by anti-air, so every anti-air
//!   gun covering the aim point halves the value and three rule it out;
//! - the **fighter** is immune to anti-air (8000 hp), so it goes for the anti-air turrets and units first and then for
//!   the biggest cluster, whatever guards it;
//! - own units and buildings in the blast count against a spot;
//! - a force is launched as soon as a spot is worth it; a big stock lowers the bar, so nothing piles up.
//!
//! Pure: the caller describes the map; no random numbers are drawn.

/// The forces the computer launches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Force {
    Bomber,
    Fighter,
    Tripler,
}

/// An enemy unit or building.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obj {
    pub cell: (i32, i32),
    /// What destroying it is worth (a tank 1, a factory 4).
    pub w: f32,
    /// An anti-air turret or unit: the fighter's favourite, and a danger to the others.
    pub aa: bool,
}

/// Anti-air that covers `range` cells around `cell`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flak {
    pub cell: (i32, i32),
    pub range: f32,
}

/// Blast radius in cells.
pub fn radius(f: Force) -> f32 {
    match f {
        Force::Bomber => 4.5,
        Force::Tripler => 2.5,
        Force::Fighter => 3.0,
    }
}

fn dist(a: (i32, i32), b: (i32, i32)) -> f32 {
    (((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)) as f32).sqrt()
}

/// What `force` is worth at `at`.
pub fn value(force: Force, at: (i32, i32), foes: &[Obj], own: &[(i32, i32)], flak: &[Flak]) -> f32 {
    let r = radius(force);
    let hit: f32 = foes.iter().filter(|o| dist(o.cell, at) <= r).map(|o| o.w + if force == Force::Fighter && o.aa { 2.0 } else { 0.0 }).sum();
    let friends = own.iter().filter(|&&c| dist(c, at) <= r + 1.0).count() as f32;
    let guns = flak.iter().filter(|f| dist(f.cell, at) <= f.range).count();
    if force != Force::Fighter && guns >= 3 {
        return 0.0;
    }
    let v = hit - 2.0 * friends;
    if force == Force::Fighter { v } else { v * 0.5f32.powi(guns as i32) }
}

/// The launch to make now: the best (force, aim cell) over the stock `[bombers, fighters, triplers]`, if it clears the bar
/// (5; 3 with three or more of that force in stock, 2 with six).
pub fn decide(stock: [u32; 3], foes: &[Obj], own: &[(i32, i32)], flak: &[Flak]) -> Option<(Force, (i32, i32))> {
    let mut best: Option<(f32, Force, (i32, i32))> = None;
    for (n, force) in [Force::Bomber, Force::Fighter, Force::Tripler].into_iter().enumerate() {
        if stock[n] == 0 {
            continue;
        }
        let bar = match stock[n] {
            0..=2 => 5.0,
            3..=5 => 3.0,
            _ => 2.0,
        };
        for o in foes {
            let v = value(force, o.cell, foes, own, flak);
            if v >= bar && best.map_or(true, |(b, _, _)| v > b + 1e-4) {
                best = Some((v, force, o.cell));
            }
        }
    }
    best.map(|(_, f, c)| (f, c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tank(x: i32, y: i32) -> Obj {
        Obj { cell: (x, y), w: 1.0, aa: false }
    }

    #[test]
    fn it_bombs_a_cluster_and_ignores_a_lone_unit() {
        let cluster: Vec<Obj> = (0..8).map(|i| tank(40 + i % 3, 20 + i / 3)).collect();
        let d = decide([1, 0, 0], &cluster, &[], &[]).expect("a cluster is worth a bomber");
        assert_eq!(d.0, Force::Bomber);
        assert!(dist(d.1, (41, 21)) < 3.0);
        assert_eq!(decide([1, 0, 0], &[tank(40, 20)], &[], &[]), None, "one tank is not worth it");
    }

    #[test]
    fn anti_air_rules_out_the_vulnerable_forces_but_not_the_fighter() {
        let cluster: Vec<Obj> = (0..8).map(|i| tank(40 + i % 3, 20 + i / 3)).collect();
        let flak: Vec<Flak> = (0..3).map(|i| Flak { cell: (38 + i, 24), range: 9.0 }).collect();
        assert_eq!(decide([2, 0, 2], &cluster, &[], &flak), None);
        assert_eq!(decide([2, 1, 2], &cluster, &[], &flak).map(|d| d.0), Some(Force::Fighter));
    }

    #[test]
    fn the_fighter_clears_anti_air_first_and_own_units_count_against_a_spot() {
        let mut foes: Vec<Obj> = (0..6).map(|i| tank(10 + i % 3, 10 + i / 3)).collect();
        foes.push(Obj { cell: (50, 50), w: 4.0, aa: true });
        foes.push(Obj { cell: (51, 50), w: 4.0, aa: true });
        let d = decide([0, 1, 0], &foes, &[], &[]).unwrap();
        assert!(dist(d.1, (50, 50)) < 2.0, "{d:?}");
        // own tanks in the cluster make it a bad spot
        let own: Vec<(i32, i32)> = (0..6).map(|i| (10 + i % 3, 10 + i / 3)).collect();
        assert_eq!(decide([1, 0, 0], &foes[..6], &own, &[]), None);
    }

    #[test]
    fn a_big_stock_lowers_the_bar() {
        let pair = [tank(40, 20), tank(41, 20), tank(40, 21)];
        assert_eq!(decide([1, 0, 0], &pair, &[], &[]), None);
        assert!(decide([3, 0, 0], &pair, &[], &[]).is_some());
    }
}
