//! Projectiles, ported from `State of War.exe` (shot spawner `0x47cf70`, bullet constructor `0x411dc0`, movement
//! `0x411220`, impact `0x4107f0`, area damage `0x42f2c0`, speed table builder `0x436df0`, distance `0x437720`).
//!
//! - A weapon top stores a **bullet type** (`[obj+0x34]`) and a **strength** (`[obj+0x38]`); the spawner passes both
//!   to the bullet constructor, which sets the type's parameters ([`bullet_type`]) and keeps the strength in `[bullet+0x2660]`.
//! - A bullet follows a precomputed path of points. Its **speed** `[bullet+0x2664]` is the number of path steps per 60 ticks
//!   (60 = one step per tick); the steps of each tick come from [`steps_at`], a table built by distributing `v` steps evenly
//!   over a 60-tick cycle.
//! - On impact the bullet subtracts its strength from the unit it was aimed at (turrets: `[obj+0x2c]`, units: current HP
//!   `[unit+0x34]`) and calls the **area damage** routine: every *ground* unit (`[unit+0x4b] == 0`) closer than 32 px
//!   ([`distance`]) loses the full strength, friend or foe, and the building in the impact cell loses it too unless it is
//!   shielded (`[bld+0x94] == 100`).

/// Projectile type and strength per unit kind's weapon top (`[obj+0x34]`, `[obj+0x38]` of the top constructor case).
pub fn weapon_shot(kind: i32) -> Option<(u8, u32)> {
    Some(match kind {
        1 => (1, 13),
        2 => (1, 15),
        3 => (1, 17),
        4 => (3, 32),
        5 => (4, 69),
        6 => (4, 85),
        7 => (6, 20),
        8 => (6, 24),
        9 => (6, 26),
        10 => (9, 3),
        11 => (9, 6),
        12 => (9, 5),
        13 => (27, 70),
        14 => (28, 133),
        15 => (29, 75),
        // the Advancer's gun top (0x47a8af): bullet type 31, strength 64
        40 => (31, 64),
        41 => (27, 18),
        42 => (1, 21),
        43 => (34, 22),
        44 => (7, 7),
        45 => (30, 920),
        // turret tops 109-113 (turrets 1-5); the fifth fires 6 projectiles per shot (`[top+0x30]` = 6)
        109 => (6, 39),
        110 => (1, 15),
        111 => (11, 40),
        112 => (7, 16),
        113 => (2, 6),
        _ => return None,
    })
}

/// Parameters a bullet type sets in the constructor (`0x411dc0`, jump table `0x412490`, 35 types).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulletType {
    /// Path steps per 60 ticks (`[+0x2664]`); `None` where the type sets no speed.
    pub speed: Option<u32>,
    /// `[+0x264c]` (default 100): a step counter; for type 1 it paces the smoke puffs of the trail. Other uses not traced.
    pub interval: Option<u32>,
    /// `[+0x2670, +0x2674, +0x2678, +0x267c]` where all four are set (sprite animation parameters, meaning not traced).
    pub anim: Option<[i32; 4]>,
    /// Types 13, 14 and 32 compute their speed from the random generator (`speed`/`interval` here are only the static stores),
    /// types 28, 29 and 31 also run effect code or branch, and types 2 and 13 draw a random start value; none of this is
    /// modelled, and those seven types are not cross-checked against the exe (`bullet_types_match_the_exes_constructor`).
    /// Types 19-21 (spark loops, `sparks.rs`) and 23 (branch on its variant) have their loops and branch decoded separately and
    /// are checked by `scripts/verify_against_exe.py`.
    pub partial: bool,
}

const fn bt(speed: Option<u32>, interval: Option<u32>, anim: Option<[i32; 4]>, partial: bool) -> BulletType {
    BulletType { speed, interval, anim, partial }
}

/// The exe's bullet type table: entry `type - 1` describes bullet type `type` (1..=35).
pub const BULLET_TYPES: [BulletType; 35] = [
    bt(Some(360), Some(8), None, false),                // 1  anti-air gun bullet
    bt(Some(180), Some(8), None, false),                // 2
    bt(Some(180), Some(8), None, false),                // 3  (1A artillery uses type 3)
    bt(Some(120), Some(8), None, false),                // 4  (2A, 3A)
    bt(Some(1800), None, None, false),                  // 5
    bt(Some(1800), None, None, false),                  // 6  (cannons 1C 2C 3C)
    bt(Some(1800), None, None, false),                  // 7  (walker 5W4)
    bt(Some(180), Some(6), None, false),                // 8
    bt(Some(180), Some(6), None, false),                // 9  (flamers)
    bt(Some(60), None, Some([0, 4, 1, 4]), false),      // 10
    bt(Some(210), None, Some([0, 4, 1, 5]), false),     // 11
    bt(Some(180), None, Some([0, 3, 1, 10]), false),    // 12
    bt(None, None, Some([0, 16, 1, 2]), false),         // 13
    bt(None, Some(3), None, false),                     // 14
    bt(Some(30), None, Some([0, 16, 1, 3]), false),     // 15
    bt(Some(30), None, Some([0, 16, 1, 3]), false),     // 16
    bt(Some(360), None, None, false),                   // 17
    bt(Some(30), None, None, false),                    // 18
    bt(Some(60), None, None, false),                    // 19
    bt(Some(60), Some(2), None, false),                 // 20
    bt(Some(60), None, None, false),                    // 21
    bt(Some(20), None, Some([10, 178, 1, 1]), false),   // 22
    bt(Some(60), None, None, false),                    // 23
    bt(Some(240), Some(0), Some([0, 16, 1, 2]), false), // 24
    bt(Some(240), Some(0), Some([0, 16, 1, 2]), false), // 25
    bt(Some(150), None, Some([0, 8, 1, 2]), false),     // 26
    bt(Some(600), Some(5), None, false),                // 27 (1S, 5W1)
    bt(Some(600), Some(5), Some([0, 19, 1, 2]), false), // 28 (2S)
    bt(Some(300), Some(5), None, false),                // 29 (3S)
    bt(Some(300), Some(5), None, false),                // 30 (5W5)
    bt(Some(600), Some(2), None, false),                // 31
    bt(None, Some(4), None, false),                     // 32
    bt(Some(360), None, Some([0, 1, 1, 4]), false),     // 33
    bt(Some(210), None, None, false),                   // 34 (5W3)
    bt(Some(100), None, None, false),                   // 35
];

/// Parameters of bullet type `t` (1..=35).
pub fn bullet_type(t: u8) -> Option<BulletType> {
    BULLET_TYPES.get((t as usize).checked_sub(1)?).copied()
}

/// Maximum number of points of a path (`0x640` ints = 800 x,y pairs).
pub const MAX_POINTS: usize = 800;

/// A bullet's precomputed flight path (`0x40ef60`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Path {
    /// Screen positions of the bullet, one per path step. The start point itself is not included; the last point is the end
    /// point. For arcing types the lift is already subtracted from `y`.
    pub points: Vec<(i32, i32)>,
    /// Height of the bullet above its ground track at each point (0 for straight types); the exe keeps it at `[bullet+0xc4+4i]`
    /// (probably for the shadow).
    pub lift: Vec<i32>,
    /// Index of the first step taken (`[bullet+0x2644] / 2`): the `delay` argument, or 0 when it is not shorter than the path.
    pub first_step: usize,
    /// Type 1 only (`[bullet+0x2654]`, stored by `0x40efab`): the missile's flight facing, `facing(start, end, 32)` (`0x46b180`) of
    /// the path builder's own arguments, i.e. of the start point (not the first path point) and the end. The draw uses it directly as
    /// the frame of the `missile` sprite (32 of its 40 frames are reached).
    pub facing: Option<u8>,
}

/// Builds the path of a bullet of `bullet_type` from `(x1, y1)` to `(x2, y2)` with an initial skip of `delay` steps
/// (`[bullet+0x26ac]`; a random 0..4 for type 2, 0 otherwise).
///
/// The ground track is an integer Bresenham line (long axis steps by one pixel each time). Types 2 and 3 lift the
/// bullet by `i - i*i/N` pixels at step `i` of `N`, type 4 by `(i - i*i/N) * 20 / 13`, types 15 and 16 by `4i - 4i*i/N`
/// (divisions truncate towards zero as in the exe); a path of 800 points (the cap) is not lifted. Types 13 and 14 use random curves from a table built at start-up, which
/// is not available, so they return `None`.
pub fn build_path(bullet_type: u8, x1: i32, y1: i32, x2: i32, y2: i32, delay: u32) -> Option<Path> {
    if bullet_type == 13 || bullet_type == 14 {
        return None;
    }
    let (dx, dy) = (x2 - x1, y2 - y1);
    let (sx, sy) = (if dx < 0 { -1 } else { 1 }, if dy < 0 { -1 } else { 1 });
    let (adx, ady) = (dx.abs(), dy.abs());
    let mut points: Vec<(i32, i32)> = Vec::new();
    let (mut x, mut y) = (x1, y1);
    if adx > ady {
        let mut e = 2 * ady - adx;
        while x != x2 && points.len() < MAX_POINTS {
            x += sx;
            if e >= 0 {
                y += sy;
            }
            points.push((x, y));
            e += 2 * if e >= 0 { ady - adx } else { ady };
        }
    } else {
        let mut e = 2 * adx - ady;
        while y != y2 && points.len() < MAX_POINTS {
            y += sy;
            if e >= 0 {
                x += sx;
            }
            points.push((x, y));
            e += 2 * if e >= 0 { adx - ady } else { adx };
        }
    }
    let n = points.len() as i32;
    let mut lift = vec![0; points.len()];
    let h = |i: i32| -> i32 {
        match bullet_type {
            2 | 3 => i - i * i / n,
            4 => (i - i * i / n) * 20 / 13,
            15 | 16 => 4 * i - 4 * i * i / n,
            _ => 0,
        }
    };
    // A path that reaches the 800-point cap jumps over the arc step in the exe, so it stays straight.
    if n > 0 && points.len() < MAX_POINTS && matches!(bullet_type, 2 | 3 | 4 | 15 | 16) {
        for (i, p) in points.iter_mut().enumerate() {
            let l = h(i as i32);
            lift[i] = l;
            p.1 -= l;
        }
    }
    let skip = delay as usize * 2;
    let first_step = if points.len() * 2 <= skip { 0 } else { skip } / 2;
    let facing = (bullet_type == 1).then(|| crate::facing::facing(x1, y1, x2, y2, 32) as u8);
    Some(Path { points, lift, first_step, facing })
}

/// Ticks a bullet needs for `steps` path steps at `speed` (steps per 60 ticks), counting from tick 0 of the speed cycle.
pub fn flight_ticks(speed: u32, steps: usize) -> u32 {
    let (mut done, mut t) = (0usize, 0u32);
    if speed == 0 {
        return 0;
    }
    while done < steps {
        done += steps_at(speed, t) as usize;
        t += 1;
    }
    t
}

/// Number of cycle positions of the speed table.
pub const CYCLE: usize = 60;

/// Row `d` (1..=60) of the speed table (`0x436df0`): `d` evenly spaced marks over 60 ticks, found by stepping a
/// 32-bit float by `60 / d` and marking `trunc(x) - 1` while `x <= 60`.
fn base_row(d: u32) -> [u8; CYCLE] {
    let mut row = [0u8; CYCLE];
    if d == 0 {
        return row;
    }
    let step = 60.0f32 / d as f32;
    let mut x = step;
    while x <= 60.0 {
        row[(x as i32 - 1).clamp(0, CYCLE as i32 - 1) as usize] = 1;
        x += step;
    }
    row
}

/// Path steps a bullet of the given `speed` advances on tick `tick % 60`. Speeds above 60 add one step every tick per
/// full 60 (`row(v) = row(v - 60) + 1`); the exe's table holds speeds up to 2398.
pub fn steps_at(speed: u32, tick: u32) -> u32 {
    let (whole, rem) = (speed.saturating_sub(1) / 60, if speed == 0 { 0 } else { (speed - 1) % 60 + 1 });
    whole + base_row(rem)[(tick % CYCLE as u32) as usize] as u32
}

/// Integer distance used by the area damage (`0x437720`): `trunc(sqrt(dx^2 + dy^2))`, 200 when either axis differs by more.
pub fn distance(x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    let (dx, dy) = ((x1 - x2).abs(), (y1 - y2).abs());
    if dx > 200 || dy > 200 {
        return 200;
    }
    ((dx * dx + dy * dy) as f64).sqrt() as i32
}

/// Radius of the area damage in pixels (a unit is hit when [`distance`] is less than this).
pub const SPLASH_RADIUS: i32 = 32;

/// What the area damage routine needs to know about a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitRef {
    /// X in map pixels.
    pub x: i32,
    /// Y in map pixels.
    pub y: i32,
    /// Whether the unit is in the air (the Advancer).
    pub flying: bool,
    /// The unit's owner.
    pub owner: u32,
    /// The unit's kind.
    pub kind: i32,
}

/// How a bullet type damages the building in its impact cell (`0x42f2c0` tail `0x42f3cc..0x42f462`): `type - 17` indexes a byte table at
/// `0x42f474` (19 entries, types 17..=35) into three cases; every other type goes to the third.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingHit {
    /// `0x42f44f`: types 17, 18, 26 (Tripler pod, bomber bomb, fighter bomb): full damage unless the building is a Headquarters.
    FullUnlessHq,
    /// `0x42f430`: types 24, 25, 35 (the two meteors, the Rogon explosion): **half** damage (`strength / 2`), none on a Headquarters.
    HalfUnlessHq,
    /// `0x42f458`: every other type: full damage, whatever the building.
    Full,
}

/// The case of `bullet_type` in the building-damage table.
pub fn building_hit(bullet_type: u8) -> BuildingHit {
    match bullet_type {
        17 | 18 | 26 => BuildingHit::FullUnlessHq,
        24 | 25 | 35 => BuildingHit::HalfUnlessHq,
        _ => BuildingHit::Full,
    }
}

/// Damage (and damage tally) a shell of `strength` does to a building of kind `building_kind` in its impact cell. The "shield" the exe
/// tests is `[bld+0x94] == 100`, the building kind of the Headquarters. Half damage is `(strength - sign) >> 1` in the exe, i.e. rounded
/// toward zero.
pub fn building_damage(bullet_type: u8, strength: i32, building_kind: i32) -> i32 {
    match building_hit(bullet_type) {
        BuildingHit::Full => strength,
        BuildingHit::FullUnlessHq => {
            if building_kind == 100 {
                0
            } else {
                strength
            }
        }
        BuildingHit::HalfUnlessHq => {
            if building_kind == 100 {
                0
            } else {
                (strength - (strength >> 31)) >> 1
            }
        }
    }
}

/// Indices of the units that lose HP to an explosion at `(x, y)` from a bullet of `bullet_type` fired by `owner`.
/// Friendly units are hit too; flying units never are. Kind 45 (the strongest walker) is spared by its owner's type-30
/// bullets. Kinds 109..=113 (turrets under construction) are not hit directly: the exe damages the building they
/// belong to instead, which is not modelled here.
pub fn splash_victims(units: &[UnitRef], x: i32, y: i32, bullet_type: u8, owner: u32) -> Vec<usize> {
    units
        .iter()
        .enumerate()
        .filter(|(_, u)| {
            !u.flying
                && !(bullet_type == 30 && u.kind == 45 && u.owner == owner)
                && !crate::kinds::TURRET_TOPS.contains(&u.kind)
                && distance(x, y, u.x, u.y) < SPLASH_RADIUS
        })
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_rows_sum_to_the_speed() {
        for v in [1u32, 7, 30, 59, 60, 61, 100, 120, 180, 210, 300, 360, 600, 1800] {
            let total: u32 = (0..60).map(|t| steps_at(v, t)).sum();
            assert_eq!(total, v, "speed {v}");
        }
        assert_eq!(steps_at(0, 5), 0);
    }

    #[test]
    fn speed_row_shapes() {
        assert!((0..60).all(|t| steps_at(60, t) == 1)); // one step every tick
        assert!((0..60).all(|t| steps_at(360, t) == 6));
        // 30 steps per cycle: one step every other tick (marks at floor(2k) - 1 -> positions 1, 3, 5, ...)
        let row: Vec<u32> = (0..6).map(|t| steps_at(30, t)).collect();
        assert_eq!(row, [0, 1, 0, 1, 0, 1]);
        // 120: two per tick; 90: 1 plus an alternating extra
        assert!((0..60).all(|t| steps_at(120, t) == 2));
        assert_eq!((0..60).map(|t| steps_at(90, t)).collect::<Vec<_>>().iter().filter(|&&s| s == 2).count(), 30);
    }

    #[test]
    fn distances() {
        assert_eq!(distance(0, 0, 3, 4), 5);
        assert_eq!(distance(10, 10, 10, 10), 0);
        assert_eq!(distance(0, 0, 31, 0), 31);
        assert_eq!(distance(0, 0, 201, 0), 200);
        assert_eq!(distance(0, 0, 100, 201), 200);
        assert_eq!(distance(0, 0, 23, 23), 32); // sqrt(1058) = 32.5 -> 32
    }

    #[test]
    fn straight_paths_are_bresenham_lines() {
        let p = build_path(6, 0, 0, 10, 4, 0).unwrap();
        assert_eq!(p.points.len(), 10);
        assert_eq!(p.points[0], (1, 0)); // the start point is not part of the path
        assert_eq!(*p.points.last().unwrap(), (10, 4));
        assert!(p.lift.iter().all(|&l| l == 0));
        // each step moves at most one pixel on both axes (8-connected)
        let mut prev = (0, 0);
        for &q in &p.points {
            assert!((q.0 - prev.0).abs() <= 1 && (q.1 - prev.1).abs() <= 1);
            prev = q;
        }
        // y-major lines and negative directions
        let p = build_path(6, 5, 5, 3, -5, 0).unwrap();
        assert_eq!((p.points.len(), *p.points.last().unwrap()), (10, (3, -5)));
        let p = build_path(6, 0, 0, -8, 0, 0).unwrap();
        assert_eq!(p.points, (1..=8).map(|i| (-i, 0)).collect::<Vec<_>>());
        // equal points: no path; a single pixel step
        assert!(build_path(6, 3, 3, 3, 3, 0).unwrap().points.is_empty());
        assert_eq!(build_path(6, 3, 3, 4, 3, 0).unwrap().points, vec![(4, 3)]);
    }

    #[test]
    fn arcs_lift_the_bullet_by_a_parabola() {
        let n = 100;
        let flat = build_path(6, 0, 200, n, 200, 0).unwrap();
        let t3 = build_path(3, 0, 200, n, 200, 0).unwrap();
        let t4 = build_path(4, 0, 200, n, 200, 0).unwrap();
        let t15 = build_path(15, 0, 200, n, 200, 0).unwrap();
        assert_eq!(flat.points.len(), n as usize);
        let peak = |p: &Path| *p.lift.iter().max().unwrap();
        assert_eq!(peak(&t3), 50 - 2500 / 100); // i = 50: 25
        assert_eq!(peak(&t4), (50 - 25) * 20 / 13); // 38
        assert_eq!(peak(&t15), 4 * 50 - 4 * 2500 / 100); // 100
                                                         // the arc keeps the x track and subtracts the lift from y; the last points come back to the ground
        for (i, (q, f)) in t4.points.iter().zip(&flat.points).enumerate() {
            assert_eq!((q.0, q.1 + t4.lift[i]), (f.0, f.1));
        }
        assert!(t3.lift[0] == 0 && t3.lift[99] <= 1 && t15.lift[99] <= 4);
    }

    #[test]
    fn delay_skips_steps_unless_it_is_longer_than_the_path() {
        assert_eq!(build_path(6, 0, 0, 10, 0, 3).unwrap().first_step, 3);
        assert_eq!(build_path(6, 0, 0, 10, 0, 10).unwrap().first_step, 0);
        assert_eq!(build_path(6, 0, 0, 10, 0, 99).unwrap().first_step, 0);
    }

    #[test]
    fn random_curve_types_have_no_path_and_long_paths_are_capped() {
        assert!(build_path(13, 0, 0, 10, 0, 0).is_none() && build_path(14, 0, 0, 10, 0, 0).is_none());
        assert_eq!(build_path(6, 0, 0, 5000, 0, 0).unwrap().points.len(), MAX_POINTS);
        // capped paths skip the arc step
        let capped = build_path(4, 0, 0, 5000, 0, 0).unwrap();
        assert!(capped.lift.iter().all(|&l| l == 0) && capped.points.iter().all(|p| p.1 == 0));
        assert!(build_path(4, 0, 0, 799, 0, 0).unwrap().lift.iter().any(|&l| l > 0));
    }

    #[test]
    fn flight_time_follows_the_speed() {
        assert_eq!(flight_ticks(60, 100), 100); // one step per tick
        assert_eq!(flight_ticks(360, 60), 10); // six steps per tick
        assert_eq!(flight_ticks(120, 100), 50);
        assert_eq!(flight_ticks(0, 10), 0);
    }

    #[test]
    fn tables_cover_every_weapon() {
        for kind in (1..=15).chain(41..=45) {
            let (t, strength) = weapon_shot(kind).unwrap_or_else(|| panic!("kind {kind}"));
            let b = bullet_type(t).unwrap();
            assert!(strength > 0 && b.speed.is_some(), "kind {kind}: type {t} has no speed");
        }
        assert_eq!(weapon_shot(7), Some((6, 20)));
        assert_eq!(bullet_type(6).unwrap().speed, Some(1800));
        assert_eq!(bullet_type(36), None);
        assert_eq!(bullet_type(0), None);
    }

    #[test]
    fn splash_hits_ground_units_in_range_including_friends() {
        let u = |x, y, flying, owner, kind| UnitRef { x, y, flying, owner, kind };
        let units = [
            u(100, 100, false, 0, 7),   // direct hit, friendly
            u(120, 110, false, 1, 7),   // 22 px away
            u(135, 100, false, 1, 7),   // 35 px away
            u(100, 100, true, 1, 41),   // flying: never hit
            u(100, 101, false, 1, 111), // turret under construction: handled via its building
            u(100, 100, false, 0, 45),  // spared by its owner's type-30 bullets
        ];
        assert_eq!(splash_victims(&units, 100, 100, 6, 0), vec![0, 1, 5]);
        assert_eq!(splash_victims(&units, 100, 100, 30, 0), vec![0, 1]);
        assert_eq!(splash_victims(&units, 100, 100, 30, 1), vec![0, 1, 5]);
    }
}

/// Offsets of the sub-blasts of a type-17 event (a Tripler pod, `0x40f590` -> table built at `0x412520` into `0x513328`):
/// the exe loops over the first 34 pairs `(dx, dy)` (an ellipse of about 64 x 41 px), adds a random -5..4 px to both
/// coordinates and runs the area damage `0x42f2c0(x, y, 100, owner, 17)` for each. This is the Tripler Strafe's damage.
pub const TRIPLER_RING: [(i32, i32); 36] = [
    (64, 1),
    (64, 9),
    (59, 18),
    (52, 25),
    (42, 32),
    (32, 36),
    (27, 38),
    (17, 40),
    (8, 41),
    (1, 40),
    (64, -9),
    (59, -18),
    (52, -25),
    (42, -32),
    (32, -36),
    (27, -38),
    (17, -40),
    (8, -41),
    (-64, -1),
    (-64, -9),
    (-59, -18),
    (-52, -25),
    (-42, -32),
    (-32, -36),
    (-27, -38),
    (-17, -40),
    (-8, -41),
    (-1, -40),
    (-64, 9),
    (-59, 18),
    (-52, 25),
    (-42, 32),
    (-32, 36),
    (-27, 38),
    (-17, 40),
    (-8, 41),
];
/// How many points of the ring above the Tripler uses.
pub const TRIPLER_RING_USED: usize = 34;
/// Strength of each blast of the Tripler's ring.
pub const TRIPLER_BLAST_STRENGTH: i32 = 100;

#[cfg(test)]
mod building_damage_tests {
    use super::*;

    #[test]
    fn meteors_and_the_rogon_blast_halve_and_the_hq_is_immune_to_air_strikes() {
        assert_eq!(building_damage(24, 101, 102), 50);
        assert_eq!(building_damage(35, 100, 102), 50);
        assert_eq!(building_damage(25, 100, 100), 0);
        assert_eq!(building_damage(18, 100, 100), 0);
        assert_eq!(building_damage(18, 100, 102), 100);
        assert_eq!(building_damage(17, 100, 100), 0);
        assert_eq!(building_damage(26, 7, 100), 0);
        // ordinary shells and the Advancer-adjacent types ignore the HQ rule
        assert_eq!(building_damage(4, 85, 100), 85);
        assert_eq!(building_damage(30, 920, 100), 920);
        assert_eq!(building_damage(15, 40, 100), 40);
    }
}
