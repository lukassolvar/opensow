//! Tests of the simulation (`World`): whole-world scenarios built with small synthetic maps.

use super::*;

fn open_world(w: i32, h: i32) -> World {
    World::new((w, h), vec![false; (w * h) as usize])
}

/// The statistics screen's counters (docs/results.md): dead units are counted per side.
#[test]
fn statistics_count_lost_units() {
    let mut w = open_world(20, 20);
    w.add_unit(7, 0, 5 * CELL, 5 * CELL);
    w.add_unit(7, 1, 9 * CELL, 5 * CELL);
    w.add_unit(7, 1, 9 * CELL, 9 * CELL);
    w.units[1].hp = 0;
    w.units[2].hp = 0;
    w.step();
    assert_eq!(w.stats[1][3], 2, "two enemy units lost");
    assert_eq!(w.stats[0][3], 0);
}

/// Dev tools (`devtools.rs`): the cheats only touch what they promise.
#[test]
fn cheats_keep_units_alive_fill_the_purse_and_freeze_the_enemy() {
    let mut w = open_world(20, 20);
    w.add_unit(6, 0, 5 * CELL, 5 * CELL);
    w.add_unit(6, 1, 12 * CELL, 5 * CELL);
    w.units[0].hp = 0;
    w.units[1].hp = 0;
    w.step();
    assert_eq!(w.units.len(), 0, "without cheats both die");
    let mut w = open_world(20, 20);
    w.cheats = Cheats { invulnerable: true, rich: true, freeze_enemy: true };
    w.add_unit(6, 0, 5 * CELL, 5 * CELL);
    let enemy = w.add_unit(6, 1, 12 * CELL, 5 * CELL);
    // a second enemy unit keeps the game running: the win/lose check runs every tick (`0x421230`)
    w.add_unit(6, 1, 18 * CELL, 18 * CELL);
    w.units[0].hp = 0;
    w.units[1].hp = 0;
    w.ai = true;
    w.step();
    assert_eq!(w.units.iter().find(|u| u.player == 0).map(|u| u.hp), Some(UnitKind::max_hp(6).unwrap()), "own unit restored");
    assert!(w.unit(enemy).is_none_or(|u| u.hp <= 0), "the enemy is not protected");
    assert_eq!((w.credits[0], w.research[0]), (CREDITS_CAP, RESEARCH_CAP));
    assert_eq!(w.ai_timer, 0, "the computer player did not run");
    w.cheats.freeze_enemy = false;
    w.step();
    assert_ne!(w.ai_timer, 0, "it runs again once thawed");
}

/// Unit destruction (docs/effects.md): a dying tank throws a piece of strength 40 with its top index as variant, a walker
/// none, and every kind but the Advancer leaves a burning wreck that throws 20 smoke columns (0x45b0a0).
#[test]
fn dying_units_throw_pieces_and_leave_burning_wrecks() {
    let mut w = open_world(20, 20);
    w.add_unit(6, 0, 5 * CELL, 5 * CELL); // kind 6: tier 3 artillery -> top index 2 * 5 + 1
    w.units[0].hp = 0;
    w.step();
    // the piece is made at the unit's turn and flies in the same tick's bullet pass (`0x42dd93`), so it has left its start
    let piece = w.bullets.iter().find(|b| b.bullet_type == 15).expect("a piece");
    assert_eq!((piece.strength, piece.variant), (40, 11));
    assert!(piece.index < piece.path.points.len());
    assert_eq!(w.wrecks.len(), 1);
    for _ in 0..400 {
        w.step();
    }
    assert!(w.wrecks.is_empty(), "removed after the last column");
    let mut w = open_world(20, 20);
    w.add_unit(43, 0, 5 * CELL, 5 * CELL);
    w.units[0].hp = 0;
    w.step();
    assert!(w.bullets.iter().all(|b| b.bullet_type != 15), "walkers leave no piece");
    assert_eq!(w.wrecks.len(), 1);
    let mut w = open_world(20, 20);
    w.add_unit(kinds::ADVANCER, 0, 5 * CELL, 5 * CELL);
    w.units[0].hp = 0;
    w.step();
    assert!(w.wrecks.is_empty(), "the Advancer leaves no wreck");
}

/// Shell smoke (`effects::trail`): an anti-air missile (type 1) leaves a darkening puff of `missileSmoke` (mode 14) after 8 path
/// steps and every 4 steps after, none on the landing step; a meteor (type 24) drops one at once.
#[test]
fn flying_shells_leave_smoke_trails() {
    let mut w = open_world(20, 20);
    let path = build_path(1, 100, 300, 400, 300, 0).unwrap();
    let steps = path.points.len() as i32;
    w.bullets.push(Bullet::new(1, 0, 0, path));
    let mut puffs = 0;
    for _ in 0..200 {
        w.step();
        puffs += w.effects.iter().filter(|e| e.spec.array == 0x5e6788 && e.age == 1).count();
        assert!(w.effects.iter().filter(|e| e.spec.array == 0x5e6788).all(|e| e.spec.mode == 14));
    }
    // first after 8 steps, then every 4, none on the last (the landing) step: 1 + (steps - 1 - 8) / 4
    assert_eq!(puffs as i32, 1 + (steps - 1 - 8) / 4, "steps {steps}");
    let mut w = open_world(20, 20);
    let path = build_path(24, 100, 300, 160, 300, 0).unwrap();
    w.bullets.push(Bullet::new(24, 0, 0, path));
    w.step();
    assert!(w.effects.iter().any(|e| e.spec.array == 0x5e9fb8), "the first meteor smoke comes with the first step");
}

/// 0x472a90: the AI's target classes come from a reachability wave, not from the straight distance.
#[test]
fn the_ai_picks_reachable_targets_only() {
    let mut w = open_world(20, 20);
    for y in 0..20 {
        w.blocked[(y * 20 + 4) as usize] = true; // a wall splits the map
    }
    let near_but_walled = w.add_unit(7, 0, 6 * CELL + 16, 10 * CELL + 16);
    let far_but_reachable = w.add_unit(7, 0, 2 * CELL + 16, 2 * CELL + 16);
    let (u, _, _) = w.nearest_enemy(1, (2, 10));
    assert_eq!(u, Some(Target::Unit(far_but_reachable)));
    assert_ne!(u, Some(Target::Unit(near_but_walled)));
    // with a gap in the wall the nearer one is met first
    w.blocked[(10 * 20 + 4) as usize] = false;
    w.occ = None; // the grids are built once per tick
    let (u, _, _) = w.nearest_enemy(1, (2, 10));
    assert_eq!(u, Some(Target::Unit(near_but_walled)));
}

/// Own buildings are skipped, everything else (enemy and neutral) is a target; turrets are their own class.
#[test]
fn the_scan_sorts_buildings_into_turrets_and_others_and_skips_own() {
    let mut w = open_world(20, 20);
    let mut own = factory(Owner::Enemy, 100, 101);
    own.cell = (5, 5);
    w.add_structure(own);
    let mut neutral = factory(Owner::Neutral, 100, 101);
    neutral.cell = (10, 5);
    w.add_structure(neutral);
    let (u, t, b) = w.nearest_enemy(1, (3, 5));
    assert_eq!((u, t), (None, None));
    assert_eq!(b, Some(Target::Structure(1)), "own building skipped, neutral one found");
    let (_, _, b) = w.nearest_enemy(0, (3, 5));
    assert_eq!(b, Some(Target::Structure(0)), "for the player the enemy building comes first");
}

/// `0x42ebd0` countdown rewards and `0x41f140` start carryalls: squads are set after the launch, mission 12 pays six asteroids.
#[test]
fn mission_scripts_launch_carryalls_with_their_own_squads() {
    let mut w = open_world(20, 20);
    w.mission_no = Some(12);
    w.reinforcements();
    assert_eq!(w.air_stock[0], [3, 6, 0, 0, 0], "3 bombers and 6 asteroids (HEADER_ORDER)");
    assert_eq!(w.aircraft.iter().map(|a| a.squad).collect::<Vec<_>>(), vec![16, 16, 16]);
    let mut w = open_world(20, 20);
    w.mission_no = Some(7);
    w.reinforcements();
    assert_eq!(w.aircraft.iter().map(|a| a.squad).collect::<Vec<_>>(), vec![11, 11]);
    let mut w = open_world(20, 20);
    w.mission_no = Some(17);
    w.reinforcements();
    assert_eq!(w.aircraft.iter().map(|a| a.squad).collect::<Vec<_>>(), vec![10, 10]);
    // the start carryalls: mission 1 two (squad 12), mission 4 one (squad 17), nothing elsewhere
    for (m, squads) in [(1, vec![12, 12]), (4, vec![17]), (2, vec![])] {
        let mut w = open_world(20, 20);
        w.mission_no = Some(m);
        w.start_airlift();
        assert_eq!(w.aircraft.iter().map(|a| a.squad).collect::<Vec<_>>(), squads, "mission {m}");
    }
}

/// 0x4303e0 / 0x430770: the tech-level hint of missions 1 and 6 and the fighter hint of mission 1.
#[test]
fn the_hint_boxes_of_missions_1_and_6_follow_the_exe() {
    let mk = |mission: u32| {
        let mut w = open_world(20, 20);
        w.mission_no = Some(mission);
        w.add_unit(7, 1, 18 * CELL, 18 * CELL);
        let mut f = factory(Owner::Player, 5000, 102);
        f.slots[0].max_level = 2;
        f.slots[1].max_level = 2;
        w.add_structure(f);
        w
    };
    let mut w = mk(1);
    w.update_hint();
    assert_eq!(w.hint, None, "needs 500 research points");
    w.research[0] = 500;
    w.update_hint();
    assert_eq!(w.hint, Some(Hint { text: 280, half: 0x94, text_dx: 0x89 }));
    w.structures[0].slots[0].level = 2;
    w.update_hint();
    assert_eq!(w.hint, Some(Hint { text: 280, half: 0xbc, text_dx: 0xb1 }), "one slot at its limit: wider box");
    w.structures[0].slots[1].level = 2;
    w.update_hint();
    assert_eq!(w.hint, None, "both at the limit, no fighters yet");
    w.air_stock[0][4] = 8;
    w.update_hint();
    assert_eq!(w.hint, Some(Hint { text: 281, half: 0xc1, text_dx: 0xb6 }));
    assert_eq!(w.hint_phase, 1);
    w.air_stock[0][4] = 7; // a fighter was used
    w.update_hint();
    assert_eq!((w.hint, w.hint_phase), (None, 2));
    // mission 6: only while no slot of any medium factory is at its limit
    let mut w = mk(6);
    w.research[0] = 600;
    w.update_hint();
    assert_eq!(w.hint, Some(Hint { text: 280, half: 0x94, text_dx: 0x89 }));
    w.structures[0].slots[1].level = 2;
    w.update_hint();
    assert_eq!(w.hint, None);
    let mut w = mk(2);
    w.research[0] = 600;
    w.update_hint();
    assert_eq!(w.hint, None, "other missions have no hint");
}

/// Runs the tutorial script frame by frame, clicking the arrow button whenever it waits.
fn tut_run(w: &mut World, frames: usize) {
    for _ in 0..frames {
        if w.tutorial.as_ref().is_some_and(|t| t.arrow) {
            w.tutorial_arrow_clicked();
        }
        w.step();
    }
}

fn tut_step(w: &World) -> u32 {
    w.tutorial.as_ref().unwrap().step
}

/// 0x4325b0, cases 0..7 (docs/tutorial.md): arrow clicks, the scroll tasks, the Advancer, the training factory.
#[test]
fn the_tutorial_starts_with_the_scroll_lessons_and_the_advancer() {
    let mut w = open_world(64, 64);
    w.start_tutorial();
    w.add_unit(7, 1, 60 * CELL, 60 * CELL);
    // without a click on the arrow button the script halts at step 2
    for _ in 0..10 {
        w.step();
    }
    assert_eq!(tut_step(&w), 2, "0 -> 1 -> 2 at once (step 1 has no arrow), then it waits for the arrow");
    assert!(w.tutorial.as_ref().unwrap().arrow);
    for _ in 0..10 {
        w.step();
    }
    assert_eq!(tut_step(&w), 2);
    w.tutorial_arrow_clicked();
    w.step();
    assert!(!w.tutorial.as_ref().unwrap().arrow);
    assert_eq!(tut_step(&w), 2, "the task: scroll with the right mouse button");
    w.tutorial.as_mut().unwrap().rmb_scroll = true;
    w.step();
    assert_eq!(tut_step(&w), 3);
    tut_run(&mut w, 1); // arrow
    w.step();
    for k in 0..3 {
        w.tutorial.as_mut().unwrap().keys[k] = true;
    }
    w.step();
    assert_eq!(tut_step(&w), 3, "all four scroll keys are needed");
    w.tutorial.as_mut().unwrap().keys[3] = true;
    w.step();
    assert_eq!(tut_step(&w), 4);
    tut_run(&mut w, 3);
    assert_eq!(tut_step(&w), 5, "Lesson A: info, then the Advancer");
    tut_run(&mut w, 2);
    assert!(w.units.iter().any(|u| u.player == 0 && u.kind == kinds::ADVANCER && u.cell() == (9, 46)), "the Advancer appears at (9, 46)");
    assert_eq!(w.camera_request, Some((0x120, 0x5c0)));
    // selected + go-to order -> step 6, then 7 builds the training light factory (neutral) and a fresh Advancer
    let adv = w.units.iter().find(|u| u.kind == kinds::ADVANCER).unwrap().id;
    w.units.iter_mut().find(|u| u.id == adv).unwrap().selected = true;
    w.order_move_selected(12 * CELL, 46 * CELL);
    tut_run(&mut w, 1);
    assert_eq!(tut_step(&w), 6);
    tut_run(&mut w, 4);
    assert_eq!(tut_step(&w), 7);
    w.step();
    assert_eq!(w.structures.len(), 1);
    assert_eq!((w.structures[0].kind_id, w.structures[0].owner, w.structures[0].cell), (101, Owner::Neutral, (8, 8)));
    assert_eq!(w.units.iter().filter(|u| u.player == 0).count(), 1, "the map was wiped; the new Advancer stands at (5, 9)");
    // capturing the factory moves on to step 8
    w.structures[0].owner = Owner::Player;
    tut_run(&mut w, 3);
    assert_eq!(tut_step(&w), 8);
}

/// The lessons B..E build their scenes and wait for their conditions (cases 10..27).
#[test]
fn the_tutorial_builds_the_scenes_and_ends_with_the_enemy_headquarters() {
    let mut w = open_world(64, 64);
    w.start_tutorial();
    w.ai = false;
    let set = |w: &mut World, step: u32| {
        let t = w.tutorial.as_mut().unwrap();
        t.step = step;
        t.entered = true;
        t.arrow = false;
    };
    // step 10: a damaged own light factory at (46, 39) to repair
    set(&mut w, 10);
    tut_run(&mut w, 1);
    assert_eq!(tut_step(&w), 10);
    let f = w.structures.iter().position(|s| s.kind_id == kinds::LIGHT_FACTORY).unwrap();
    assert!(w.structures[f].hp < w.structures[f].max_hp / 2);
    w.structures[f].hp = w.structures[f].max_hp;
    tut_run(&mut w, 1);
    assert_eq!(tut_step(&w), 11);
    // step 12: gold mine, two enemy cannons, 120 credits; 200 credits -> 13, killing the cannons -> 14
    set(&mut w, 12);
    tut_run(&mut w, 1);
    assert_eq!((w.credits[0], w.units.iter().filter(|u| u.player == 1).count()), (120, 2));
    w.credits[0] = 200;
    tut_run(&mut w, 1);
    assert_eq!(tut_step(&w), 13);
    w.units.retain(|u| u.player == 0);
    tut_run(&mut w, 1);
    assert_eq!(tut_step(&w), 14);
    // step 15: air base and four enemy cannons; a fighter in stock -> 16; the icon -> 17; all enemies dead -> 18
    set(&mut w, 15);
    tut_run(&mut w, 1);
    assert_eq!((w.units.iter().filter(|u| u.player == 1).count(), w.credits), (4, [0, 0]));
    let fighter = HEADER_ORDER.iter().position(|&h| h == AirForce::Fighter).unwrap();
    w.air_stock[0][fighter] = 1;
    tut_run(&mut w, 1);
    assert_eq!(tut_step(&w), 16);
    w.tutorial.as_mut().unwrap().arrow = false;
    w.tutorial.as_mut().unwrap().air_icon = true;
    tut_run(&mut w, 1);
    assert_eq!(tut_step(&w), 17);
    assert!(!w.tutorial.as_ref().unwrap().arrow, "step 17 has no arrow");
    w.units.retain(|u| u.player == 0);
    tut_run(&mut w, 1);
    assert_eq!((tut_step(&w), w.air_stock[0]), (18, [0; 5]), "the air force icons are cleared");
    // lesson D: step 19 builds a neutral research lab and a medium factory with one tech-level to gain, then 20 at once
    set(&mut w, 19);
    tut_run(&mut w, 1);
    assert_eq!(w.research[0], 400);
    assert_eq!(tut_step(&w), 19);
    tut_run(&mut w, 1);
    assert_eq!(tut_step(&w), 20);
    w.tutorial.as_mut().unwrap().arrow = false;
    let lab = w.structures.iter().position(|s| s.kind_id == kinds::RESEARCH_LAB).unwrap();
    w.structures[lab].owner = Owner::Player;
    tut_run(&mut w, 1);
    assert_eq!(tut_step(&w), 21 + w.tut_buildings().iter().position(|&b| b == lab).unwrap() as u32);
    set(&mut w, 21);
    w.research[0] = 500;
    tut_run(&mut w, 1);
    assert_eq!(tut_step(&w), 22);
    w.tutorial.as_mut().unwrap().arrow = false;
    let mf = w.structures.iter().position(|s| s.kind_id == kinds::MEDIUM_FACTORY).unwrap();
    w.structures[mf].owner = Owner::Player;
    w.structures[mf].slots[0].level = 1;
    tut_run(&mut w, 1);
    assert!(tut_step(&w) >= 23);
    // lesson E: step 24 builds the bases; the enemy HQ gone -> 26, then 27 ends the mission with a victory
    set(&mut w, 24);
    tut_run(&mut w, 1);
    let ehq = w.structures.iter().position(|s| s.kind_id == kinds::HEADQUARTERS && s.owner == Owner::Enemy).unwrap();
    assert!(w.structures[ehq].hp < w.structures[ehq].max_hp);
    assert!(w.structures.iter().any(|s| s.kind_id == kinds::HEADQUARTERS && s.owner == Owner::Player) && w.units.iter().any(|u| u.kind == kinds::ADVANCER));
    assert_eq!(w.outcome, None, "no HQ rules inside the tutorial");
    w.structures[ehq].hp = 0;
    tut_run(&mut w, 1);
    assert_eq!(tut_step(&w), 26);
    tut_run(&mut w, 3);
    assert_eq!(w.outcome, Some(Outcome::Victory));
}

/// Step 8 jumps to `9 + index` of the last non-Advancer unit (case 0x4330e4): with the Advancer at index 0 the first produced
/// unit (index 1) skips the upgrade lesson (step 9), as in the exe.
#[test]
fn tutorial_step_8_follows_the_exes_index_arithmetic() {
    let mut w = open_world(64, 64);
    w.start_tutorial();
    w.ai = false;
    w.add_unit(7, 1, 60 * CELL, 60 * CELL);
    {
        let t = w.tutorial.as_mut().unwrap();
        t.step = 8;
        t.arrow = false;
    }
    w.add_unit(kinds::ADVANCER, 0, 5 * CELL, 9 * CELL);
    w.add_unit(7, 0, 6 * CELL, 9 * CELL);
    tut_run(&mut w, 1);
    assert_eq!(tut_step(&w), 10);
}

/// The global route is kept as it is (the exe only drops its first cell per planned step, `0x473190`) and a unit finishes
/// the segment in flight when it gets a new order, so a crowd crossing open ground changes direction only a few times.
#[test]
fn a_crowd_in_open_ground_does_not_wobble() {
    let mut w = open_world(24, 24);
    let ids: Vec<u32> = (0..6).map(|i| w.add_unit(7, 0, (4 + i % 3) * CELL + 16, (4 + i / 3) * CELL + 16)).collect();
    for &id in &ids {
        w.order_move(id, (20, 20), None);
    }
    let mut last = vec![u32::MAX; ids.len()];
    let mut changes = vec![0; ids.len()];
    for t in 0..2500 {
        w.step();
        for (k, &id) in ids.iter().enumerate() {
            let c = w.unit(id).unwrap().chassis.as_ref().unwrap().target;
            if c != last[k] {
                last[k] = c;
                changes[k] += 1;
            }
        }
        // a new order every 7 ticks (the chase does this) must not make the units turn back mid-segment
        if t % 7 == 0 {
            for &id in &ids {
                w.order_move(id, (20, 20), None);
            }
        }
    }
    assert!(changes.iter().all(|&c| c <= 6), "direction changes per unit: {changes:?}");
    assert!(
        ids.iter().all(|&id| w.unit(id).unwrap().cell().0 >= 15),
        "they got there: {:?}",
        ids.iter()
            .map(|&id| {
                let u = w.unit(id).unwrap();
                (u.cell(), u.step, u.path.len())
            })
            .collect::<Vec<_>>()
    );
}

/// A walking unit turns faster than a standing one: the walking step runs the turn counter once per pixel step and the
/// tail once more (`0x46f65f`, `0x46f8b9`).
#[test]
fn a_walking_unit_turns_twice_per_tick_when_it_steps() {
    let mut w = open_world(14, 9);
    let id = w.add_unit(7, 0, 2 * CELL + 16, 4 * CELL + 16);
    w.order_move(id, (10, 4), None); // due east, the unit starts facing north
    let mut ticks_to_turn = 0;
    while w.unit(id).unwrap().chassis.as_ref().unwrap().facing != 8 {
        w.step();
        ticks_to_turn += 1;
        assert!(ticks_to_turn < 100);
    }
    // 8 turn steps at one per 5 counter ticks: 40 counter ticks; the walking step adds one on every tick that owes a pixel step
    assert!(ticks_to_turn < 40, "turned in {ticks_to_turn} ticks");
}

#[test]
fn units_walk_around_walls() {
    let mut w = open_world(12, 12);
    for y in 0..9 {
        w.blocked[(y * 12 + 6) as usize] = true;
    }
    let id = w.add_unit(7, 0, 2 * CELL + 16, 2 * CELL + 16);
    w.order_move(id, (10, 2), None);
    let path = w.unit(id).unwrap().path.clone();
    assert!(path.len() >= 9, "must detour below the wall, got {path:?}");
    assert!(path.iter().all(|&(x, y)| !w.is_blocked((x as i32) / CELL, (y as i32) / CELL)));
    for _ in 0..3000 {
        w.step();
    }
    let u = w.unit(id).unwrap();
    assert_eq!(u.cell(), (10, 2));
}

#[test]
fn a_destroyed_unit_makes_four_clouds_a_wreck_and_flying_pieces() {
    use opensow_formats::effects::{UNIT_CLOUD, UNIT_WRECK};
    let mut w = open_world(14, 9);
    let id = w.add_unit(7, 0, 5 * CELL + 16, 4 * CELL + 16);
    w.units.iter_mut().find(|u| u.id == id).unwrap().hp = 0;
    w.step();
    let clouds = |w: &World| w.effects.iter().filter(|e| e.spec == UNIT_CLOUD).count();
    assert_eq!(clouds(&w), 4);
    assert_eq!(w.effects.iter().filter(|e| e.spec == UNIT_WRECK).count(), 1);
    assert!(w.bullets.iter().any(|b| b.bullet_type == 15), "a cannon unit throws type 15 pieces");
    // the Advancer: clouds only
    let mut w = open_world(14, 9);
    let id = w.add_unit(kinds::ADVANCER, 0, 5 * CELL + 16, 4 * CELL + 16);
    w.units.iter_mut().find(|u| u.id == id).unwrap().hp = 0;
    w.step();
    assert_eq!(clouds(&w), 4);
    assert_eq!(w.effects.iter().filter(|e| e.spec == UNIT_WRECK).count(), 0);
    assert!(w.bullets.is_empty());
}

#[test]
fn a_bomb_dropped_by_an_eastbound_bomber_uses_the_right_sprite_variant() {
    let mut w = open_world(14, 9);
    w.air_stock[0][0] = 1;
    assert!(w.launch_strike(0, AirForce::Bomber, 8 * CELL, 4 * CELL));
    let right = w.aircraft[0].right;
    let mut seen = None;
    for _ in 0..2000 {
        w.step();
        if let Some(b) = w.bullets.iter().find(|b| b.bullet_type == 18) {
            seen = Some(b.variant);
            break;
        }
    }
    assert_eq!(seen, Some(right as u32));
}

/// `0x40fd30`: every one of the 34 ring points of a Tripler pod makes a small blast sprite and (free ground) two smoke puffs.
#[test]
fn a_tripler_pod_lands_with_a_ring_of_blasts() {
    let mut w = open_world(14, 9);
    let path = build_path(17, 5 * CELL, 2 * CELL, 5 * CELL, 4 * CELL, 0).unwrap();
    w.bullets.push(Bullet::new(17, 0, 0, path));
    let small = blast(1).array;
    for _ in 0..200 {
        w.step();
        if w.bullets.is_empty() {
            break;
        }
    }
    assert!(w.bullets.is_empty(), "the pod landed");
    let blasts = w.effects.iter().filter(|e| e.spec.array == small).count();
    assert_eq!(blasts, 1 + TRIPLER_RING_USED, "the impact's own blast plus one per ring point");
    assert!(w.effects.iter().filter(|e| e.spec.mode == 8).count() >= 2 * TRIPLER_RING_USED, "two puffs on every free ring point");
}

#[test]
fn legs_move_while_a_unit_has_a_route_and_stop_at_rest() {
    let mut w = open_world(14, 9);
    let id = w.add_unit(7, 0, 2 * CELL + 16, 4 * CELL + 16);
    w.order_move(id, (10, 4), None);
    let mut phases = std::collections::HashSet::new();
    for _ in 0..200 {
        w.step();
        phases.insert(w.unit(id).unwrap().chassis.as_ref().unwrap().phase);
    }
    assert!(phases.len() > 1, "the walk phase advances on the way");
    for _ in 0..3000 {
        w.step();
    }
    assert!(w.unit(id).unwrap().path.is_empty());
    let rest = w.unit(id).unwrap().chassis.as_ref().unwrap().phase;
    for _ in 0..300 {
        w.step();
        assert_eq!(w.unit(id).unwrap().chassis.as_ref().unwrap().phase, rest, "no animation at rest");
    }
}

#[test]
fn a_corpse_blocks_its_cell_for_430_steps_and_ends_in_blasts() {
    use opensow_formats::effects::{blast, CORPSE_LIFE};
    let mut w = open_world(14, 9);
    let id = w.add_unit(7, 0, 5 * CELL + 16, 4 * CELL + 16); // kind 7: tier 0 (spread 30), two size-2 blasts at the end
    w.units.iter_mut().find(|u| u.id == id).unwrap().hp = 0;
    w.step();
    assert_eq!(w.corpses.len(), 1);
    assert_eq!(w.corpses[0].left, CORPSE_LIFE - 1);
    assert_eq!(w.occupancy().unwrap().ground.cells[(5 * 128 + 4) as usize], 0xFFFFFD, "the corpse blocks its cell");
    let blasts = |w: &World| w.effects.iter().filter(|e| e.spec == blast(2)).count();
    let before = blasts(&w);
    for _ in 0..(CORPSE_LIFE - 31) {
        w.step();
    }
    assert_eq!(blasts(&w), before, "nothing before the timer is below 30");
    w.step();
    w.step();
    assert_eq!(blasts(&w), before + 2);
    for _ in 0..40 {
        w.step();
    }
    assert!(w.corpses.is_empty(), "the slot is freed after the timer runs out");
    assert_eq!(w.occupancy().unwrap().ground.cells[(5 * 128 + 4) as usize], 0);
}

#[test]
fn a_dead_advancers_corpse_does_not_block_the_air_cell() {
    let mut w = open_world(14, 9);
    let id = w.add_unit(kinds::ADVANCER, 1, 5 * CELL + 16, 4 * CELL + 16);
    w.units.iter_mut().find(|u| u.id == id).unwrap().hp = 0;
    w.add_unit(7, 0, 12 * CELL, 8 * CELL);
    w.step();
    assert_eq!(w.corpses.len(), 1);
    let (x, y) = (w.corpses[0].x.div_euclid(CELL), w.corpses[0].y.div_euclid(CELL));
    assert_eq!(w.occupancy().unwrap().air.cells[(x * 128 + y) as usize], 0, "the spot stays free for the next Advancer");
}

#[test]
fn units_do_not_walk_through_each_other() {
    // a one-cell-wide corridor with a unit standing in it: the walker behind it cannot pass, but in the open it goes around
    let mut w = open_world(14, 9);
    let blocker = w.add_unit(7, 0, 7 * CELL + 16, 4 * CELL + 16);
    let walker = w.add_unit(7, 0, 2 * CELL + 16, 4 * CELL + 16);
    w.order_move(walker, (12, 4), None);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..3000 {
        w.step();
        seen.insert(w.unit(walker).unwrap().cell());
        assert_ne!(w.unit(walker).unwrap().cell(), w.unit(blocker).unwrap().cell(), "walked into the other unit");
    }
    assert_eq!(w.unit(walker).unwrap().cell(), (12, 4), "goes around in the open");
    assert!(seen.iter().any(|c| c.1 != 4), "left the straight line to avoid the unit");
    // in a corridor it stops (the exe resets the goal to the unit's own position)
    let mut w = open_world(14, 9);
    for x in 0..14 {
        for y in [3, 5] {
            w.blocked[(y * 14 + x) as usize] = true;
        }
    }
    w.add_unit(7, 0, 7 * CELL + 16, 4 * CELL + 16);
    let walker = w.add_unit(7, 0, 2 * CELL + 16, 4 * CELL + 16);
    w.order_move(walker, (12, 4), None);
    for _ in 0..3000 {
        w.step();
    }
    assert!(w.unit(walker).unwrap().cell().0 < 7, "cannot pass the unit in the corridor");
}

#[test]
fn a_walled_off_destination_moves_to_the_nearest_reachable_cell() {
    // 0x4774c0: two units plug a corridor (the local search finds no way around them); the destination behind them moves to
    // the nearest cell the walker can reach, the one in front of the plug
    let mut w = open_world(16, 9);
    for x in 0..16 {
        for y in [3, 5] {
            w.blocked[(y * 16 + x) as usize] = true;
        }
    }
    w.add_unit(7, 0, 8 * CELL + 16, 4 * CELL + 16);
    w.add_unit(7, 0, 9 * CELL + 16, 4 * CELL + 16);
    let walker = w.add_unit(7, 0, 2 * CELL + 16, 4 * CELL + 16);
    w.order_move(walker, (13, 4), None);
    for _ in 0..4000 {
        w.step();
    }
    assert_eq!(w.unit(walker).unwrap().cell(), (7, 4), "walks up to the plug instead of waiting at the start");
}

#[test]
fn units_sent_to_one_cell_do_not_stack() {
    // the exe stamps the cell a unit is heading to (0x473190), so a second unit sees it taken and stops short
    let mut w = open_world(16, 9);
    let ids: Vec<u32> = (0..4).map(|k| w.add_unit(7, 0, (2 + k) * CELL + 16, (2 + 2 * k) * CELL + 16)).collect();
    for &id in &ids {
        w.order_move(id, (12, 4), None);
    }
    for _ in 0..3000 {
        w.step();
    }
    for (a, &ia) in ids.iter().enumerate() {
        for &ib in &ids[a + 1..] {
            let (ua, ub) = (w.unit(ia).unwrap(), w.unit(ib).unwrap());
            assert!((ua.x - ub.x).abs() + (ua.y - ub.y).abs() > 16.0, "units {ia} and {ib} stand on the same spot ({}, {})", ua.x, ua.y);
        }
    }
}

#[test]
fn a_new_route_still_avoids_units_that_stopped_ahead() {
    // re-ordering a walking unit (the chase does it every 7 ticks) must plan the next segment around units again
    let mut w = open_world(16, 5);
    for x in 0..16 {
        for y in [0, 1, 3, 4] {
            w.blocked[(y * 16 + x) as usize] = true;
        }
    }
    let walker = w.add_unit(7, 0, 2 * CELL + 16, 2 * CELL + 16);
    let blocker = w.add_unit(7, 0, 8 * CELL + 16, 2 * CELL + 16);
    for t in 0..2000 {
        if t % 7 == 0 {
            w.order_move(walker, (13, 2), None);
        }
        w.step();
        let (a, b) = (w.unit(walker).unwrap(), w.unit(blocker).unwrap());
        assert!((a.x - b.x).abs() + (a.y - b.y).abs() > 16.0, "walked into the unit at tick {t}");
    }
}

#[test]
fn a_pod_plans_around_units_like_every_other_ground_unit() {
    // the exe's movement code has no branch on kind 46 (docs/walking.md): in a one-cell corridor a pod queues behind a
    // unit that stands in its way instead of walking through it
    let mut w = open_world(16, 5);
    for x in 0..16 {
        for y in [0, 1, 3, 4] {
            w.blocked[(y * 16 + x) as usize] = true;
        }
    }
    let pod = w.add_unit(kinds::POD, 0, 2 * CELL + 16, 2 * CELL + 16);
    let blocker = w.add_unit(7, 0, 8 * CELL + 16, 2 * CELL + 16);
    for t in 0..2000 {
        if t % 7 == 0 {
            w.order_move(pod, (13, 2), None);
        }
        w.step();
        let (a, b) = (w.unit(pod).unwrap(), w.unit(blocker).unwrap());
        assert!((a.x - b.x).abs() + (a.y - b.y).abs() > 16.0, "the pod walked into the unit at tick {t}");
    }
}

#[test]
fn a_walking_tank_keeps_its_top_on_its_heading_and_an_idle_one_looks_around() {
    let mut w = open_world(16, 16);
    let id = w.add_unit(7, 0, 2 * CELL + 16, 2 * CELL + 16);
    assert_eq!(w.unit(id).unwrap().aim.as_ref().unwrap().facing, 15, "the top initialiser's facing (0x47abb5)");
    for _ in 0..200 {
        w.step(); // it first turns to north (the initialisers' target 0)
    }
    w.order_move(id, (12, 2), None); // due east = facing 8
    for _ in 0..300 {
        w.step();
    }
    let u = w.unit(id).unwrap();
    assert_eq!(u.chassis.as_ref().unwrap().facing, 8);
    assert_eq!(u.aim.as_ref().unwrap().facing, 8, "the top follows the chassis while walking");
    // standing still it looks around by itself within 240 + 900 ticks (once the walk is over)
    let mut seen = std::collections::HashSet::new();
    for _ in 0..20000 {
        w.step();
        seen.insert(w.unit(id).unwrap().aim.as_ref().unwrap().facing);
    }
    assert!(seen.len() > 3, "idle top turns to random facings: {seen:?}");
    assert_eq!(w.unit(id).unwrap().chassis.as_ref().unwrap().facing, 8, "the hull stays");
}

#[test]
fn a_new_unit_starts_like_the_exes_initialisers_and_turns_to_north() {
    let mut w = open_world(16, 16);
    let id = w.add_unit(7, 0, 2 * CELL + 16, 2 * CELL + 16);
    let u = w.unit(id).unwrap();
    let c = u.chassis.as_ref().unwrap();
    assert_eq!((c.facing, c.target), (8, 0), "chassis [+0x30] = 8, [+0x34] = 0");
    assert_eq!(u.aim.as_ref().unwrap().facing, 15, "top [+0x6c] = 15");
    assert!((240..1140).contains(&u.look), "[unit+0x8c] = 240 + random(900), got {}", u.look);
    for _ in 0..200 {
        w.step();
    }
    let u = w.unit(id).unwrap();
    assert_eq!(u.chassis.as_ref().unwrap().facing, 0, "the hull turns to north by itself");
    assert_eq!(u.aim.as_ref().unwrap().facing, 0, "and the top follows it");
}

#[test]
fn units_are_drawn_with_the_initialisers_offsets() {
    // (chassis [+0x2c], top [+0x60]) from the exe's initialisers (`unit_defaults.rs`)
    assert_eq!(super::draw_offsets(1), (-10, -10));
    assert_eq!(super::draw_offsets(2), (-4, 0));
    assert_eq!(super::draw_offsets(12), (-4, 3));
    assert_eq!(super::draw_offsets(41), (-16, -16));
    assert_eq!(super::draw_offsets(kinds::ADVANCER), (0, 0));
    assert_eq!(super::draw_offsets(999), (0, 0));
}

#[test]
fn the_advancer_flies_over_the_ground_and_always_spins() {
    let mut w = open_world(12, 12);
    for y in 0..12 {
        w.blocked[(y * 12 + 6) as usize] = true;
    }
    let id = w.add_unit(kinds::ADVANCER, 0, 2 * CELL + 16, 2 * CELL + 16);
    let f0 = w.unit(id).unwrap().chassis.as_ref().unwrap().facing;
    for _ in 0..30 {
        w.step();
    }
    assert_ne!(w.unit(id).unwrap().chassis.as_ref().unwrap().facing, f0, "spins while idle");
    w.order_move(id, (10, 2), None);
    // the air layer's wave (0x474b30) knows only the border terrain: the ground wall is no obstacle
    assert!(w.unit(id).unwrap().path.len() >= 8, "a route of cells, not one leg");
    for _ in 0..1000 {
        w.step();
    }
    assert_eq!(w.unit(id).unwrap().cell(), (10, 2));
}

#[test]
fn the_advancer_goes_around_no_fly_areas_of_the_air_layer() {
    let mut w = open_world(12, 12);
    // a no-fly wall in the border layer with a gap at the bottom; the ground map is open
    for y in 0..9 {
        w.air_blocked[(y * 12 + 6) as usize] = true;
    }
    let id = w.add_unit(kinds::ADVANCER, 0, 2 * CELL + 16, 2 * CELL + 16);
    w.order_move(id, (10, 2), None);
    let mut deepest = 0;
    for _ in 0..2000 {
        w.step();
        deepest = deepest.max(w.unit(id).unwrap().cell().1);
    }
    assert_eq!(w.unit(id).unwrap().cell(), (10, 2));
    assert!(deepest >= 9, "flew through the gap, not across the wall (deepest cell row {deepest})");
    // a beam approach (a type-2 order) is a straight line whatever the layers hold
    w.order_fly_straight(id, (2, 2));
    assert_eq!(w.unit(id).unwrap().path.len(), 1);
}

#[test]
fn a_factory_produces_a_unit_every_22_seconds() {
    let mut w = open_world(12, 12);
    w.tutorial_step = Some(1); // tutorial speed: 22 s per ground unit
    w.add_unit(10, 0, 16, 16); // keeps the player alive (no defeat)
    let slots = vec![SlotState { kind: 7, progress: 0, target: 2000, level: 0, max_level: 0, waiting: 0, unlock: 0 }];
    w.add_structure(Structure {
        object: 0,
        kind: ObjectKind::LightFactory,
        owner: Owner::Player,
        cell: (5, 5),
        hp: 100,
        max_hp: 100,
        aim: None,
        weapon: 0,
        target: None,
        slots,
        active: 1,
        kind_id: 101,
        tally: [0; 3],
        heal: 0,
        upgrade: 0,
        protected: false,
        attacked: false,
        sat_cd: 0,
        advancer: None,
        rally: None,
        dying: None,
    });
    for _ in 0..1300 {
        w.step();
    }
    assert_eq!(w.units.len(), 1, "90 steps/s: 2000 steps need 1334 ticks");
    for _ in 0..100 {
        w.step();
    }
    assert_eq!(w.units.len(), 2);
    assert_eq!((w.units[1].kind, w.units[1].player), (7, 0));
}

#[test]
fn factory_units_leave_by_the_bottom_right_exit_else_the_top_left_one_else_wait() {
    // exits are tested in the static grid: only terrain and buildings block them, units never do
    let build = |blockers: &[(i32, i32)], unit_on_exit: bool| {
        let mut w = open_world(14, 14);
        w.tutorial_step = Some(1);
        w.add_unit(10, 0, 16, 16);
        for &(x, y) in blockers {
            let i = w.cell_index(x, y).unwrap();
            w.blocked[i] = true;
        }
        if unit_on_exit {
            w.add_unit(10, 0, 8 * CELL + 16, 8 * CELL + 16);
        }
        let slots = vec![SlotState { kind: 7, progress: 0, target: 2000, level: 0, max_level: 0, waiting: 0, unlock: 0 }];
        w.add_structure(Structure {
            object: 0,
            kind: ObjectKind::LightFactory,
            owner: Owner::Player,
            cell: (5, 5),
            hp: 100,
            max_hp: 100,
            aim: None,
            weapon: 0,
            target: None,
            slots,
            active: 1,
            kind_id: 101,
            tally: [0; 3],
            heal: 0,
            upgrade: 0,
            protected: false,
            attacked: false,
            sat_cd: 0,
            advancer: None,
            rally: None,
            dying: None,
        });
        w
    };
    // the order the exe gives: a straight walk to the exit cell (type 2), then a route two cells further (type 1)
    let order = |w: &World| {
        w.units.iter().find(|u| u.kind == 7).map(|u| (u.path.first().map(|&(x, y)| ((x as i32).div_euclid(CELL), (y as i32).div_euclid(CELL))), u.straight))
    };
    let mut w = build(&[], true); // a unit standing on the exit does not matter
    for _ in 0..1340 {
        w.step();
    }
    assert_eq!(order(&w), Some((Some((8, 8)), Some((8, 10)))));
    let mut w = build(&[(8, 8)], false);
    for _ in 0..1340 {
        w.step();
    }
    assert_eq!(order(&w), Some((Some((4, 4)), Some((4, 2)))));
    let mut w = build(&[(8, 8), (4, 4)], false);
    for _ in 0..1340 + 240 {
        w.step();
    }
    assert!(w.units.iter().all(|u| u.kind != 7));
    assert!(w.log.iter().any(|&(_, id)| id == msg::NO_SPACE));
}

#[test]
fn factory_units_walk_on_to_the_rally_point_and_a_new_order_replaces_it() {
    let build = |owner: Owner, rally: Option<(i32, i32)>| {
        let mut w = open_world(16, 16);
        w.tutorial_step = Some(1);
        w.add_unit(10, 0, 16, 16);
        let slots = vec![SlotState { kind: 7, progress: 0, target: 2000, level: 0, max_level: 0, waiting: 0, unlock: 0 }];
        w.add_structure(Structure {
            object: 0,
            kind: ObjectKind::LightFactory,
            owner,
            cell: (5, 5),
            hp: 100,
            max_hp: 100,
            aim: None,
            weapon: 0,
            target: None,
            slots,
            active: 1,
            kind_id: 101,
            tally: [0; 3],
            heal: 0,
            upgrade: 0,
            protected: false,
            attacked: false,
            sat_cd: 0,
            advancer: None,
            rally: None,
            dying: None,
        });
        if let Some((x, y)) = rally {
            assert!(w.set_rally(0, if owner == Owner::Player { 0 } else { 1 }, x, y));
        }
        w
    };
    let cannon = |w: &World| w.units.iter().find(|u| u.kind == 7).map(|u| u.cell());
    // the point snaps to the cell centre, and the unit ends on it after the exit orders
    let mut w = build(Owner::Player, Some((13 * CELL + 3, 4 * CELL + 30)));
    assert_eq!(w.structures[0].rally, Some((13 * CELL + 16, 4 * CELL + 16)));
    for _ in 0..1340 + 1500 {
        w.step();
    }
    let u = w.units.iter().find(|u| u.kind == 7).unwrap();
    assert_eq!(u.px(), (13 * CELL + 16, 4 * CELL + 16), "reached the rally point");
    // without a point it stops two cells beyond the exit
    let mut w = build(Owner::Player, None);
    for _ in 0..1340 + 1500 {
        w.step();
    }
    assert_eq!(cannon(&w), Some((8, 10)));
    // an order given on the way replaces the queued rally order
    let mut w = build(Owner::Player, Some((13 * CELL + 16, 4 * CELL + 16)));
    for _ in 0..1340 + 60 {
        w.step();
    }
    let id = w.units.iter().find(|u| u.kind == 7).unwrap().id;
    w.units.iter_mut().find(|u| u.id == id).unwrap().selected = true;
    w.order_move_selected(2 * CELL + 16, 12 * CELL + 16);
    for _ in 0..2000 {
        w.step();
    }
    assert_eq!(cannon(&w), Some((2, 12)));
    // only the player's factories use it, and a capture clears it
    let mut w = build(Owner::Enemy, None);
    w.structures[0].rally = Some((13 * CELL + 16, 4 * CELL + 16));
    for _ in 0..1340 + 1500 {
        w.step();
    }
    assert!(w.units.iter().filter(|u| u.player == 1).all(|u| u.cell() != (13, 4)));
    // points outside the map are skipped
    let mut w = build(Owner::Player, None);
    w.structures[0].rally = Some((16 * CELL + 1, 4 * CELL));
    for _ in 0..1340 + 1500 {
        w.step();
    }
    assert_eq!(cannon(&w), Some((8, 10)));
    // other building kinds cannot be given one
    let mut w = build(Owner::Player, None);
    w.structures[0].kind_id = 104;
    assert!(!w.set_rally(0, 0, 64, 64));
}

#[test]
fn a_flamer_bullet_leaves_a_trail_of_puffs() {
    // 0x4118e2: types 8 and 9 drop a blast_verysmall tail puff every 6th path step, none on the landing step
    let mut w = open_world(20, 20);
    let path = opensow_formats::projectile::build_path(9, 100, 100, 160, 100, 0).unwrap(); // 60 steps
    w.bullets.push(Bullet::new(9, 0, 0, path));
    for _ in 0..200 {
        w.step();
        if w.bullets.is_empty() {
            break;
        }
    }
    let at: Vec<i32> = w.effects.iter().filter(|e| e.spec.array == 0x5e6230).map(|e| e.x).collect();
    assert_eq!(at, vec![106, 112, 118, 124, 130, 136, 142, 148, 154], "puffs every 6 px, none at the end x=160");
}

#[test]
fn a_type_27_bullet_leaves_blue_sparks_every_fifth_step() {
    // 0x411bc2: bullet type 27 (1S, walker 5W1) drops a bodky_b spark animation (frames 4..6, period 1, x3) every 5 path steps
    let mut w = open_world(20, 20);
    let path = opensow_formats::projectile::build_path(27, 100, 100, 160, 100, 0).unwrap();
    w.bullets.push(Bullet::new(27, 0, 0, path));
    let mut seen = Vec::new();
    for _ in 0..200 {
        w.step();
        seen.extend(w.effects.iter().filter(|e| e.spec.array == 0x5e6bd0 && e.age == 1).map(|e| e.x));
        if w.bullets.is_empty() {
            break;
        }
    }
    assert!(seen.len() >= 8, "sparks along the path: {seen:?}");
    assert!(seen.windows(2).all(|p| p[1] - p[0] == 5), "every 5 px: {seen:?}");
}

#[test]
fn the_advancer_gun_beam_starts_at_the_sprite_and_plays_tt_fire() {
    // the spawner starts at position + hover offset + the top's -16 (not the half-height aim point) and the constructor plays id 56
    let mut w = open_world(20, 20);
    w.add_unit(7, 1, 19 * CELL, 19 * CELL);
    let a = w.add_unit(kinds::ADVANCER, 0, 5 * CELL + 16, 5 * CELL + 16);
    let v = w.add_unit(1, 1, 9 * CELL + 16, 5 * CELL + 16);
    let mut start = None;
    for _ in 0..200 {
        w.step();
        if w.sounds.iter().any(|s| s.0 == GUN_SOUND) {
            start = w.sounds.iter().find(|s| s.0 == GUN_SOUND).map(|s| (s.1, s.2));
            break;
        }
    }
    let (fx, fy) = w.unit(a).unwrap().flyer_points().1;
    let (sx, sy) = start.expect("tt-fire plays");
    assert!((sx - fx).abs() <= 3 && (sy - (fy - 16)).abs() <= 3, "start {sx},{sy} vs sprite {fx},{fy}");
    assert!(w.unit(v).is_some());
}

#[test]
fn the_advancer_gun_draws_a_blue_beam_for_the_player_and_a_green_one_for_the_enemy() {
    // 0x411b0c: bullet type 31 drops a bodky_b (player 0) / bodky_g (other sides) spark animation every 2 path steps
    for (shooter, victim, array) in [(0, 1, 0x5e6bd0u32), (1, 0, 0x5e6c70)] {
        let mut w = open_world(20, 20);
        w.add_unit(7, 1 - shooter, 19 * CELL, 19 * CELL); // keeps the mission from ending
        w.add_unit(kinds::ADVANCER, shooter, 5 * CELL + 16, 5 * CELL + 16);
        let v = w.add_unit(1, victim, 9 * CELL + 16, 5 * CELL + 16);
        let hp = w.unit(v).unwrap().hp;
        let mut sparks = 0;
        for _ in 0..200 {
            w.step();
            sparks += w.effects.iter().filter(|e| e.age == 1 && e.spec.array == array).count();
            if w.unit(v).is_none_or(|u| u.hp < hp) {
                break;
            }
        }
        assert!(sparks > 20, "shooter {shooter}: {sparks} sparks along the 4-cell beam");
        assert!(w.unit(v).is_none_or(|u| u.hp < hp), "and the shot lands");
    }
}

/// Walker 1 fires two shots per volley: shot k starts at `.po1` table k's point and ends at the target plus the `.po2` point
/// minus that point (`0x47d0a5..0x47d0cb`), so the two bolts are parallel and a barrel apart.
#[test]
fn a_two_barrel_volley_flies_two_parallel_bolts() {
    let table = |f: &dyn Fn(usize) -> (i16, i16)| -> Vec<u8> {
        (0..32)
            .flat_map(|i| {
                let (x, y) = f(i);
                [x.to_le_bytes(), y.to_le_bytes()].concat()
            })
            .collect()
    };
    // raw points: barrel 0 at (-6, 0), barrel 1 at (6, 0) for every facing (muzzle = -raw)
    let mut po1 = table(&|_| (-6, 0));
    po1.extend(table(&|_| (6, 0)));
    let mut w = open_world(30, 30);
    w.turret_muzzles.insert(41, opensow_formats::fire::FireTables::parse(&po1).unwrap());
    w.turret_muzzles.insert(REFERENCE_KEY + 41, opensow_formats::fire::FireTables::parse(&table(&|_| (0, 0))).unwrap());
    w.add_unit(41, 0, 5 * CELL + 16, 10 * CELL + 16);
    w.add_unit(7, 1, 11 * CELL + 16, 10 * CELL + 16);
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    for _ in 0..200 {
        w.step();
        let volley: Vec<&Bullet> = w.bullets.iter().filter(|b| b.bullet_type == 27).collect();
        if volley.len() >= 2 {
            for b in volley.iter().take(2) {
                starts.push(b.path.points[0]);
                ends.push(*b.path.points.last().unwrap());
            }
            break;
        }
    }
    assert!(starts.len() >= 2, "two bolts in the first volley, got {}", starts.len());
    let (a, b) = (starts[0], starts[1]);
    assert_eq!((a.0 - b.0).abs(), 12, "the barrels are 12 px apart: {a:?} {b:?}");
    assert_eq!(ends[0].0 - ends[1].0, a.0 - b.0, "the bolts end as far apart as they start (parallel)");
}

/// The volley spawner plays the weapon's muzzle sound for ground units and walkers too (`sfx::muzzle`): walker 1 = effect 16.
#[test]
fn firing_units_raise_their_muzzle_sound() {
    for (kind, want) in [(41, 16u8), (43, 17), (45, 18), (7, 4), (4, 1)] {
        let mut w = open_world(30, 30);
        w.add_unit(kind, 0, 5 * CELL + 16, 10 * CELL + 16);
        w.add_unit(7, 1, 5 * CELL + 16 + if kind == 45 { CELL / 2 } else { 4 * CELL }, 10 * CELL + 16);
        let mut heard = false;
        for _ in 0..400 {
            w.step();
            heard |= w.sounds.iter().any(|s| s.0 == want);
            w.sounds.clear();
        }
        assert!(heard, "kind {kind} should play effect {want}");
    }
}

/// Special tiers 2 and 3 have no muzzle sound; their bullets (types 28 / 29) sound in the constructor instead (effects 8 / 9,
/// `effects::constructor_sound`), once per bullet at its start point. Tier 1 keeps its muzzle sound 7 and nothing else.
#[test]
fn special_tiers_two_and_three_sound_from_the_bullet_constructor() {
    for (kind, want, not) in [(14, 8u8, 9u8), (15, 9, 8), (13, 7, 8)] {
        let mut w = open_world(30, 30);
        w.add_unit(kind, 0, 5 * CELL + 16, 10 * CELL + 16);
        w.add_unit(7, 1, 9 * CELL + 16, 10 * CELL + 16);
        let (mut heard, mut wrong, mut bullets) = (0, 0, 0);
        for _ in 0..400 {
            let before = w.bullets.len();
            w.step();
            bullets += w.bullets.len().saturating_sub(before);
            heard += w.sounds.iter().filter(|s| s.0 == want).count();
            wrong += w.sounds.iter().filter(|s| s.0 == not || (kind == 13 && s.0 == 9)).count();
            w.sounds.clear();
        }
        assert!(heard > 0, "kind {kind} should play effect {want}");
        assert_eq!(wrong, 0, "kind {kind} plays only its own sound");
        if kind != 13 {
            assert!(heard >= bullets.min(1), "kind {kind}: {heard} sounds for {bullets} new bullets");
        }
    }
}

/// An attack order beats the regular target priority (`0x47b140`): a unit in range of both an enemy unit and the building it
/// was ordered to attack shoots the building.
#[test]
fn units_shoot_what_they_were_ordered_to_attack_when_it_is_in_range() {
    let mut w = open_world(30, 30);
    let a = w.add_unit(7, 0, 5 * CELL + 16, 10 * CELL + 16);
    let enemy = w.add_unit(7, 1, 8 * CELL + 16, 10 * CELL + 16);
    w.add_structure(Structure {
        object: 0,
        kind: ObjectKind::LightFactory,
        owner: Owner::Enemy,
        cell: (9, 12),
        hp: 4000,
        max_hp: 4000,
        aim: None,
        weapon: 0,
        target: None,
        slots: Vec::new(),
        active: 0,
        kind_id: 101,
        tally: [0; 3],
        heal: 0,
        upgrade: 0,
        protected: false,
        attacked: false,
        sat_cd: 0,
        advancer: None,
        rally: None,
        dying: None,
    });
    w.cheats.freeze_enemy = true;
    w.units.iter_mut().find(|u| u.id == a).unwrap().selected = true;
    // no order: the nearest unit is the target
    w.step();
    assert_eq!(w.unit(a).unwrap().target, Some(Target::Unit(enemy)));
    // ordered onto the building (in range, 4 cells away): it is shot at although the unit is closer
    w.order_attack_selected(Target::Structure(0));
    for _ in 0..40 {
        w.step();
        assert_eq!(w.unit(a).map(|u| u.target), Some(Some(Target::Structure(0))), "tick {}", w.tick);
    }
}

#[test]
fn a_range_one_walker_next_to_a_building_wall_shoots_it() {
    // walker 45 has range 0-1: the exe's scan (0x47b140) finds the building on any footprint cell of the static grid, so a
    // walker standing next to the wall sees it although the building's middle cell is farther away
    let mut w = open_world(30, 30);
    w.add_structure(Structure {
        object: 0,
        kind: ObjectKind::LightFactory,
        owner: Owner::Enemy,
        cell: (10, 10),
        hp: 4000,
        max_hp: 4000,
        aim: None,
        weapon: 0,
        target: None,
        slots: Vec::new(),
        active: 0,
        kind_id: 101,
        tally: [0; 3],
        heal: 0,
        upgrade: 0,
        protected: false,
        attacked: false,
        sat_cd: 0,
        advancer: None,
        rally: None,
        dying: None,
    });
    let wall = w.structures[0].blocked_cells()[0];
    let me = (wall.0 - 1, wall.1);
    assert!(!w.structures[0].blocked_cells().contains(&me));
    let mid = w.structures[0].center_cell();
    assert!((me.0 - mid.0).abs().max((me.1 - mid.1).abs()) > 1, "the middle cell is out of range");
    let index = w.index();
    assert_eq!(w.find_target(&index, 0, me, 0, 1, None), Some(Target::Structure(0)));
}

#[test]
fn a_unit_in_range_of_both_prefers_its_order_target_over_the_priority_rule() {
    let mut w = open_world(30, 30);
    w.add_unit(7, 1, 8 * CELL + 16, 10 * CELL + 16);
    let far = w.add_unit(7, 1, 11 * CELL + 16, 10 * CELL + 16);
    let me = (5, 10);
    let index = w.index();
    assert_eq!(w.find_target(&index, 0, me, 0, 8, Some(Target::Unit(far))), Some(Target::Unit(far)));
    assert_ne!(w.find_target(&index, 0, me, 0, 8, None), Some(Target::Unit(far)));
    // out of range (a 6-cell weapon): the nearest unit again
    assert_ne!(w.find_target(&index, 0, me, 0, 4, Some(Target::Unit(far))), Some(Target::Unit(far)));
}

#[test]
fn a_cannon_kills_a_unit_in_range() {
    let mut w = open_world(20, 20);
    w.add_unit(7, 0, 5 * CELL + 16, 5 * CELL + 16); // cannon tier 1, range 0..6 cells
    w.add_unit(10, 1, 9 * CELL + 16, 5 * CELL + 16); // enemy flamer, 4 cells away (outside its own range 4? 4 is inside)
    for _ in 0..4000 {
        w.step();
    }
    // the cannon (20 dmg per hit, 180 hp) beats the flamer's 3-damage hits at range; at least one side died
    assert!(w.units.iter().filter(|u| u.player == 1).count() == 0 || w.units.iter().filter(|u| u.player == 0).count() == 0);
}

#[test]
fn units_attack_neutral_and_enemy_buildings() {
    for owner in [Owner::Neutral, Owner::Enemy] {
        let mut w = open_world(20, 20);
        w.add_unit(9, 0, 5 * CELL + 16, 5 * CELL + 16);
        w.add_structure(Structure {
            object: 0,
            kind: ObjectKind::LightFactory,
            owner,
            cell: (8, 5),
            hp: 400,
            max_hp: 400,
            aim: None,
            weapon: 0,
            target: None,
            slots: Vec::new(),
            active: 0,
            kind_id: 101,
            tally: [0; 3],
            heal: 0,
            upgrade: 0,
            protected: false,
            attacked: false,
            sat_cd: 0,
            advancer: None,
            rally: None,
            dying: None,
        });
        for _ in 0..600 {
            w.step();
        }
        assert!(w.structures[0].hp < 400 || w.structures[0].owner == Owner::Player, "{owner:?} building untouched");
    }
}

#[test]
fn an_advancer_flying_to_a_building_still_shoots_an_enemy_advancer() {
    let mut w = open_world(30, 30);
    let me = w.add_unit(kinds::ADVANCER, 0, 2 * CELL + 16, 2 * CELL + 16);
    let foe = w.add_unit(kinds::ADVANCER, 1, 6 * CELL + 16, 2 * CELL + 16);
    w.add_structure(Structure {
        object: 0,
        kind: ObjectKind::LightFactory,
        owner: Owner::Enemy,
        cell: (25, 25),
        hp: 400,
        max_hp: 400,
        aim: None,
        weapon: 0,
        target: None,
        slots: Vec::new(),
        active: 0,
        kind_id: 101,
        tally: [0; 3],
        heal: 0,
        upgrade: 0,
        protected: false,
        attacked: false,
        sat_cd: 0,
        advancer: None,
        rally: None,
        dying: None,
    });
    w.units.iter_mut().find(|u| u.id == me).unwrap().selected = true;
    assert!(w.order_action(me, 0));
    let hp = w.unit(foe).unwrap().hp;
    for _ in 0..400 {
        w.step();
    }
    assert!(w.unit(foe).is_none_or(|u| u.hp < hp), "the flying Advancer must fire at the enemy Advancer in range");
}

#[test]
fn an_advancer_may_be_ordered_against_flyers_and_anti_air_only() {
    let mut w = open_world(30, 30);
    let me = w.add_unit(kinds::ADVANCER, 0, 2 * CELL + 16, 2 * CELL + 16);
    let foe_adv = w.add_unit(kinds::ADVANCER, 1, 12 * CELL + 16, 2 * CELL + 16);
    let foe_aa = w.add_unit(1, 1, 12 * CELL + 16, 8 * CELL + 16);
    let foe_gnd = w.add_unit(7, 1, 12 * CELL + 16, 14 * CELL + 16);
    assert!(w.advancer_can_attack(Target::Unit(foe_adv)));
    assert!(w.advancer_can_attack(Target::Unit(foe_aa)));
    assert!(!w.advancer_can_attack(Target::Unit(foe_gnd)));
    w.units.iter_mut().find(|u| u.id == me).unwrap().selected = true;
    w.order_attack_selected(Target::Unit(foe_adv));
    assert!(w.unit(me).unwrap().chase.is_some());
}

#[test]
fn an_attack_order_chases_a_neutral_building() {
    let mut w = open_world(30, 30);
    let u = w.add_unit(7, 0, 2 * CELL + 16, 2 * CELL + 16);
    w.add_structure(Structure {
        object: 0,
        kind: ObjectKind::LightFactory,
        owner: Owner::Neutral,
        cell: (25, 25),
        hp: 400,
        max_hp: 400,
        aim: None,
        weapon: 0,
        target: None,
        slots: Vec::new(),
        active: 0,
        kind_id: 101,
        tally: [0; 3],
        heal: 0,
        upgrade: 0,
        protected: false,
        attacked: false,
        sat_cd: 0,
        advancer: None,
        rally: None,
        dying: None,
    });
    w.units.iter_mut().find(|x| x.id == u).unwrap().selected = true;
    w.order_attack_selected(Target::Structure(0));
    for _ in 0..3 {
        w.step();
    }
    assert!(w.unit(u).unwrap().chase.is_some(), "the chase must survive");
    for _ in 0..6000 {
        w.step();
    }
    assert!(w.structures[0].hp < 400 || w.structures[0].owner != Owner::Neutral);
}

/// Regression: shooting a neutral or enemy building down captures it for the shooter instead of destroying it.
#[test]
fn shooting_a_building_down_captures_it() {
    for owner in [Owner::Neutral, Owner::Enemy] {
        let mut w = open_world(20, 20);
        w.add_unit(7, 1, 18 * CELL, 18 * CELL); // keeps the mission from ending
        w.add_unit(7, 0, 5 * CELL + 16, 5 * CELL + 16);
        let mut b = factory(owner, 300, 101);
        b.cell = (8, 5);
        w.add_structure(b);
        for _ in 0..3000 {
            w.step();
        }
        let s = &w.structures[0];
        assert!(s.alive(), "{owner:?} building was destroyed instead of captured");
        assert_eq!(s.owner, Owner::Player, "{owner:?} building not captured");
    }
}

/// The building goes to whoever dealt most damage; every source (units, turrets, air units, nukes) goes through `explode`
/// with its owner, the Advancer through its beam tally.
#[test]
fn the_biggest_damage_dealer_takes_a_contested_building() {
    for (blue, green, winner) in [(300, 100, Owner::Player), (100, 300, Owner::Enemy), (250, 120, Owner::Player)] {
        let mut w = open_world(20, 20);
        w.add_unit(7, 0, CELL, CELL);
        w.add_unit(7, 1, 18 * CELL, 18 * CELL);
        let mut b = factory(Owner::Neutral, blue + green, 101);
        b.cell = (8, 8);
        w.add_structure(b);
        let (x, y) = (8 * CELL + 16, 8 * CELL + 16);
        w.explode(x, y, 2, blue, 0);
        w.explode(x, y, 2, green, 1);
        settle(&mut w);
        let s = &w.structures[0];
        assert_eq!(s.owner, winner, "blue {blue} green {green}");
    }
}

/// The map's no-marks layer (`+0x2c0cc`) keeps scorch decals and single puffs away (`0x410459`, `0x4106ce`); a cell outside
/// it takes them. Type 15 hits make the three lockstep puffs wherever the ground is free.
#[test]
fn no_marks_cells_get_no_decals_or_single_puffs() {
    let mut w = open_world(20, 20);
    w.no_marks = vec![false; 400];
    w.no_marks[5 * 20 + 5] = true;
    let puffs_before = w.effects.len();
    w.impact_ground_effects(5 * 32 + 8, 5 * 32 + 8, 3);
    assert!(w.decals.is_empty(), "no decal on a no-marks cell");
    assert_eq!(w.effects.len(), puffs_before, "type 3 never makes puffs");
    w.impact_ground_effects(6 * 32 + 8, 6 * 32 + 8, 3);
    assert_eq!(w.decals.len(), 1);
    // type 18: two puffs on an open cell, none on a no-marks one
    let n = w.effects.len();
    w.impact_ground_effects(5 * 32 + 8, 5 * 32 + 8, 18);
    assert_eq!(w.effects.len(), n);
    w.impact_ground_effects(7 * 32 + 8, 7 * 32 + 8, 18);
    assert_eq!(w.effects.len(), n + 2);
    // type 15 on the no-marks cell: no single puff, but the roll-1 hits still make their three
    let mut counts = std::collections::BTreeSet::new();
    for _ in 0..40 {
        let n = w.effects.len();
        w.impact_ground_effects(5 * 32 + 8, 5 * 32 + 8, 15);
        counts.insert(w.effects.len() - n);
    }
    assert_eq!(counts, [0, 3].into_iter().collect(), "only the roll-1 hits puff on a no-marks cell");
}

/// `0x458930`: a building's quake lasts 0x57 / 2 = 43 ticks of steps (2..=9 px each axis, magnitude shrinking with the
/// distance to the screen centre), then the steps are undone one per tick; a turret's lasts 15; QUAKE off does nothing.
#[test]
fn a_far_away_destruction_does_not_shake_the_screen() {
    let mut w = open_world(8, 8);
    w.quake.start(QUAKE_BUILDING, 4000, 4000);
    for _ in 0..60 {
        assert_eq!(w.quake_step(true, (0, 0), (640, 480)), None);
    }
    assert!(w.quake.stack.is_empty());
}

#[test]
fn quake_shakes_then_undoes_every_step() {
    for (duration, steps) in [(QUAKE_BUILDING, 43usize), (QUAKE_TURRET, 15)] {
        let mut w = open_world(20, 20);
        w.quake.start(duration, 320, 240);
        let mut sum = (0, 0);
        let mut ticks = 0;
        while let Some((dx, dy)) = w.quake_step(true, (0, 0), (640, 480)) {
            sum = (sum.0 + dx, sum.1 + dy);
            ticks += 1;
            if ticks <= steps {
                assert!(dx.abs() >= 2 && dx.abs() <= 2 + 3 && dy.abs() >= 2 && dy.abs() <= 2 + 3, "{dx} {dy}");
                // distance 0: 9 / 2 = 4 -> r in 0..4
            }
        }
        assert_eq!(ticks, 2 * steps);
        assert_eq!(sum, (0, 0), "the camera comes back");
    }
    let mut w = open_world(20, 20);
    w.quake.start(QUAKE_BUILDING, 0, 0);
    assert_eq!(w.quake_step(false, (0, 0), (640, 480)), None);
    assert_eq!(w.quake_step(true, (0, 0), (640, 480)), None, "switching it off forgets the shake");
}

/// The exe has friendly fire on buildings (0x42f2c0 has no owner check): a shell of the captor that lands on the freshly
/// rebuilt 1-hp building destroys it again, and with no foreign tally it goes to player 1 (0x43ac2f).
#[test]
fn friendly_shots_hit_buildings_as_in_the_exe() {
    let mut w = open_world(20, 20);
    w.add_unit(7, 1, 18 * CELL, 18 * CELL);
    let mut b = factory(Owner::Neutral, 100, 101);
    b.cell = (8, 8);
    w.add_structure(b);
    let (x, y) = (8 * CELL + 16, 8 * CELL + 16);
    w.explode(x, y, 2, 100, 0);
    settle(&mut w);
    assert_eq!(w.structures[0].owner, Owner::Player, "captured");
    w.explode(x, y, 2, 100, 0); // a second shell of the same volley lands on the 1-hp rebuild
    settle(&mut w);
    assert_eq!(w.structures[0].owner, Owner::Enemy, "friendly fire destroyed it; nobody else's damage -> player 1");
}

#[test]
fn a_building_nobody_damaged_goes_to_the_green_side() {
    let mut w = open_world(20, 20);
    w.add_unit(7, 0, CELL, CELL);
    w.add_unit(7, 1, 18 * CELL, 18 * CELL);
    for owner in [Owner::Neutral, Owner::Player, Owner::Enemy] {
        let mut b = factory(owner, 0, 101);
        b.cell = (8, 8);
        b.tally = [0, 0, 0];
        w.structures.clear();
        w.add_structure(b);
        w.structure_destroyed(0);
        settle(&mut w);
        assert_eq!(w.structures[0].owner, Owner::Enemy, "{owner:?}");
        assert!(w.structures[0].hp > 0);
    }
}

#[test]
fn a_destroyed_building_throws_a_blast_and_six_debris_bullets_on_random_curves() {
    let mut w = open_world(20, 20);
    let table: Vec<u8> = (0..opensow_formats::rng::TABLE_LEN as u32 * 2).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    w.set_game_rng(GameRng::from_file(&table).unwrap());
    w.add_unit(7, 0, CELL, CELL);
    let mut b = factory(Owner::Player, 0, 101);
    b.cell = (8, 8);
    w.add_structure(b);
    w.structure_destroyed(0);
    let types: Vec<u8> = w.bullets.iter().map(|b| b.bullet_type).collect();
    assert_eq!(types, [14, 14, 14, 13, 13, 13]);
    let (bw, bh) = w.structures[0].size();
    let start = w.bullets[0].path.points[0];
    // the burst point lies inside the footprint (10 px margin), the curves start next to it
    assert!(w.bullets.iter().all(|b| b.strength == 80 && b.speed >= 30));
    assert!(start.0 > 8 * CELL - 160 && start.0 < (8 + bw) * CELL + 160 && start.1 > 8 * CELL - 160 && start.1 < (8 + bh) * CELL + 160);
    assert!(w.effects.iter().any(|e| e.spec == BUILDING_DESTROYED), "the e_bubg blast");
    assert!(w.sounds.iter().any(|s| s.0 == 25));
}

/// A change of hands drops the upgrades: one active slot and the bought tech-levels.
#[test]
fn a_captured_building_loses_its_upgrades() {
    let mut w = open_world(20, 20);
    let mut b = factory(Owner::Enemy, 0, 101);
    b.cell = (8, 8);
    b.tally = [500, 0, 0];
    b.active = 2;
    b.slots[0].kind = 8;
    b.slots[0].level = 1;
    w.add_structure(b);
    w.structures[0].hp = 0;
    w.structure_destroyed(0);
    settle(&mut w);
    let s = &w.structures[0];
    assert_eq!(s.owner, Owner::Player);
    assert_eq!((s.active, s.slots[0].level, s.slots[0].kind), (1, 0, 7));
    // a neutral building keeps its level (the exe copies it back only for old owner 2), but loses bought tech-levels too
    let mut w = open_world(20, 20);
    let mut b = factory(Owner::Neutral, 0, 101);
    b.cell = (8, 8);
    b.tally = [500, 0, 0];
    b.active = 2;
    w.add_structure(b);
    w.structures[0].hp = 0;
    w.structure_destroyed(0);
    settle(&mut w);
    assert_eq!((w.structures[0].owner, w.structures[0].active), (Owner::Player, 2));
}

/// Satellite protection does not survive a change of hands, in either direction: the exe builds a new object whose
/// constructor (`0x40a330`) zeroes `[b+0x114]` (`0x40a3ef`).
#[test]
fn a_captured_building_loses_its_satellite_protection() {
    for (owner, tally, winner) in [(Owner::Enemy, [500, 0, 0], Owner::Player), (Owner::Player, [0, 500, 0], Owner::Enemy)] {
        let mut w = open_world(20, 20);
        let mut b = factory(owner, 0, 101);
        b.cell = (8, 8);
        b.tally = tally;
        b.protected = true;
        b.attacked = true;
        b.sat_cd = 100;
        w.add_structure(b);
        w.structures[0].hp = 0;
        w.structure_destroyed(0);
        settle(&mut w);
        let s = &w.structures[0];
        assert_eq!(s.owner, winner);
        assert_eq!((s.protected, s.attacked, s.sat_cd), (false, false, 0));
    }
}

#[test]
fn a_captured_building_and_a_destroyed_turret_both_burst_and_the_burst_spares_the_rebuild() {
    let mut w = open_world(20, 20);
    let table: Vec<u8> = (0..opensow_formats::rng::TABLE_LEN as u32 * 2).map(|i| (i.wrapping_mul(2246822519) >> 11) as u8).collect();
    w.set_game_rng(GameRng::from_file(&table).unwrap());
    w.add_unit(7, 0, CELL, CELL);
    let mut b = factory(Owner::Enemy, 0, 101);
    b.cell = (8, 8);
    b.tally = [500, 0, 0]; // the player did the damage: capture
    w.add_structure(b);
    w.structures[0].hp = 0;
    w.structure_destroyed(0);
    assert_eq!(w.structures[0].owner, Owner::Enemy, "the dead building stays where it was until its animation ends");
    assert_eq!(w.bullets.len(), 6);
    settle(&mut w);
    assert_eq!(w.structures[0].owner, Owner::Player, "captured");
    assert_eq!(w.structures[0].hp, 1, "the bursts' own blasts ran while the building was dead, so the rebuild keeps its hit point");
    assert_eq!(w.bullets.len(), 24, "four bursts of six");
    // a turret: removed, but it still throws the burst with the turret blast
    let mut w = open_world(20, 20);
    w.set_game_rng(GameRng::from_file(&table).unwrap());
    w.add_unit(7, 0, CELL, CELL);
    let mut t = factory(Owner::Enemy, 0, 25);
    t.cell = (8, 8);
    t.kind = ObjectKind::Turret(1);
    w.add_structure(t);
    w.structures[0].hp = 0;
    w.structure_destroyed(0);
    assert_eq!(w.bullets.len(), 6);
    assert!(w.effects.iter().any(|e| e.spec == TURRET_DESTROYED));
    assert!(w.structures[0].present() && !w.structures[0].alive());
    settle(&mut w);
    assert_eq!(w.bullets.len(), 6, "a turret bursts once");
    assert!(!w.structures[0].present());
}

#[test]
fn a_unit_leaving_a_cell_frees_it_for_units_planning_later_in_the_same_tick() {
    let mut w = open_world(12, 12);
    let id = w.add_unit(7, 0, 3 * CELL + 31, 3 * CELL + 16);
    let i = w.units.iter().position(|u| u.id == id).unwrap();
    let own = 0x3000000 + id;
    let at = |w: &mut World, c: (i32, i32)| w.occupancy().unwrap().stat[(c.0 * 128 + c.1) as usize];
    assert_eq!(at(&mut w, (3, 3)), own, "the static grid holds the unit's cell");
    w.units[i].x += 1.0; // one pixel step across the cell border
    w.occupancy_moved(i, (3, 3));
    assert_eq!(at(&mut w, (3, 3)), 0, "the cell it left is free at once");
    assert_eq!(at(&mut w, (4, 3)), own, "and the new one is its own");
}

/// 0x476989: the static grid `0x5eac38` holds every ground unit's current cell, and a look-ahead cell holding another object
/// gives no step, so a unit standing there holds the walker back instead of being walked around.
#[test]
fn a_unit_standing_on_the_look_ahead_cell_holds_the_walker_back() {
    let setup = |ahead: usize| {
        let mut w = open_world(20, 12);
        let id = w.add_unit(7, 0, 2 * CELL + 16, 5 * CELL + 16);
        w.order_move(id, (14, 5), None);
        let i = w.units.iter().position(|u| u.id == id).unwrap();
        let (x, y) = w.units[i].path[ahead - 1];
        w.add_unit(7, 0, x as i32, y as i32);
        w.occ = None;
        (w, i)
    };
    let (mut w, i) = setup(6);
    assert!(w.avoid(i), "the look-ahead cell (6 ahead) is taken: no step");
    assert_eq!(w.units[i].step, None);
    let (mut w, i) = setup(3);
    assert!(!w.avoid(i), "a unit nearer than the look-ahead cell is walked around");
    assert!(w.units[i].step.is_some());
}

/// Runs the destruction animations to their end; returns the passes it took.
fn settle(w: &mut World) -> usize {
    let mut passes = 0;
    while w.structures.iter().any(|s| s.dying.is_some()) {
        for i in 0..w.structures.len() {
            w.destruction_pass(i);
        }
        passes += 1;
        assert!(passes < 2000);
    }
    passes
}

#[test]
fn the_destruction_animation_takes_four_bursts_for_a_building_and_one_for_a_turret() {
    let table: Vec<u8> = (0..opensow_formats::rng::TABLE_LEN as u32 * 2).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    for (kind, bursts) in [(101, 4), (100, 4), (25, 1), (29, 1)] {
        let mut w = open_world(20, 20);
        w.set_game_rng(GameRng::from_file(&table).unwrap());
        w.add_unit(7, 0, CELL, CELL);
        let mut b = factory(Owner::Enemy, 0, kind);
        b.cell = (8, 8);
        if kind < 30 {
            b.kind = ObjectKind::Turret((kind - 24) as u8);
        }
        w.add_structure(b);
        w.structure_destroyed(0);
        let passes = settle(&mut w);
        assert_eq!(w.bullets.len(), 6 * bursts, "kind {kind}");
        // each gap is 90..=104 passes: the end comes after `bursts` gaps
        assert!((90 * bursts..=104 * bursts).contains(&passes), "kind {kind}: {passes} passes");
    }
}

/// An enemy factory at (8, 8) with `n` player units next to it.
fn turret_world(n: usize, credits: i32, flags: [bool; 5]) -> World {
    let mut w = open_world(24, 24);
    w.add_unit(7, 0, CELL, CELL);
    let mut b = factory(Owner::Enemy, 100, 101);
    b.cell = (8, 8);
    w.add_structure(b);
    for i in 0..n {
        w.add_unit(7, 0, (6 + i as i32) * CELL + 16, 12 * CELL + 16);
    }
    w.credits[1] = credits;
    w.turret_flags[1] = flags;
    w
}

fn turrets(w: &World) -> Vec<(i32, (i32, i32))> {
    w.structures.iter().filter(|s| matches!(s.kind, ObjectKind::Turret(_))).map(|s| (s.kind_id, s.cell)).collect()
}

#[test]
fn a_hard_skirmish_computer_builds_turrets_with_the_campaign_routine() {
    // Hard aims its air strikes with `ai_air`; the 600-step timer must still run the exe's turret routine
    let mut w = turret_world(3, 50_000, [true; 5]);
    w.skirmish = true;
    w.difficulty = Difficulty::Hard;
    w.ai_strategy = 3;
    w.ai_timer = 100_000;
    w.ai_strike = 600;
    for t in 1..600 {
        w.tick = t;
        w.step_ai(1);
    }
    assert!(turrets(&w).is_empty());
    w.tick = 600;
    w.step_ai(1);
    assert!((1..=2).contains(&turrets(&w).len()), "{:?}", turrets(&w));
}

#[test]
fn a_skirmish_computer_advancer_takes_the_campaign_pick_and_never_its_own_hq() {
    let mut w = open_world(40, 20);
    w.structures.clear();
    let mut hq = factory(Owner::Enemy, 4000, 100);
    hq.kind = ObjectKind::Headquarters;
    hq.cell = (2, 8);
    hq.slots.clear();
    w.add_structure(hq);
    let mut wind = factory(Owner::Neutral, 6768, 107);
    wind.kind = ObjectKind::Wind;
    wind.cell = (20, 8);
    wind.slots.clear();
    let wind_k = w.structures.len();
    w.add_structure(wind);
    let adv = w.add_unit(kinds::ADVANCER, 1, 3 * CELL, 7 * CELL);
    w.skirmish = true;
    w.step_ai(1);
    assert_eq!(w.unit(adv).and_then(|u| u.action).map(|(_, k)| k), Some(wind_k), "the damaged own HQ (0x42fbbd: -10000) is passed over");
}

#[test]
fn skirmish_anti_air_units_go_after_the_enemy_advancer_anywhere() {
    let mut w = open_world(80, 20);
    let aa = w.add_unit(1, 1, 2 * CELL, 2 * CELL);
    let adv = w.add_unit(kinds::ADVANCER, 0, 70 * CELL, 15 * CELL);
    w.skirmish = true;
    w.step_ai(1);
    assert_eq!(w.unit(aa).and_then(|u| u.chase), Some(Target::Unit(adv)));
}

#[test]
fn the_skirmish_computer_airlifts_a_squad_to_a_building_under_attack() {
    let mut w = open_world(48, 24);
    w.structures.clear();
    let mut hq = factory(Owner::Enemy, 13536, 100);
    hq.kind = ObjectKind::Headquarters;
    hq.cell = (2, 10);
    hq.protected = true;
    hq.slots.clear();
    w.add_structure(hq);
    let mut mine = factory(Owner::Enemy, 13536, 105);
    mine.kind = ObjectKind::GoldMine;
    mine.cell = (20, 10);
    mine.slots.clear();
    w.add_structure(mine);
    let attackers: Vec<u32> = (0..3).map(|i| w.add_unit(7, 0, 15 * CELL + 16, (10 + i) * CELL + 16)).collect();
    let g = HEADER_ORDER.iter().position(|&h| h == AirForce::Carryall).unwrap();
    w.air_stock[1][g] = 2;
    w.carry_counter = 9; // the next squad is 0: four kind 7 and four kind 10 (the computer does not know that)
    w.ai = true;
    w.skirmish = true;
    let before = w.units.len();
    let mut chasing = 0;
    for _ in 0..1500 {
        w.step();
        chasing = chasing.max(
            w.units
                .iter()
                .filter(|u| u.player == 1 && u.kind != kinds::ADVANCER && u.hp > 0 && matches!(u.chase, Some(Target::Unit(id)) if attackers.contains(&id)))
                .count(),
        );
        if chasing > 0 {
            break;
        }
    }
    assert!(w.air_stock[1][g] < 2, "carryalls flown (a second one may follow to a spot beside the first)");
    assert!(w.units.len() > before, "the squad landed");
    assert!(chasing >= 4, "the squad goes for the attackers: {chasing}");
    let dropped: Vec<(i32, i32)> = w.units.iter().filter(|u| u.player == 1 && u.kind != kinds::ADVANCER).map(|u| u.cell()).collect();
    assert!(dropped.iter().all(|c| c.0 >= 13 && c.0 <= 23), "landed between the attackers and the mine: {dropped:?}");
}

#[test]
fn start_units_stand_around_each_hq_on_free_cells() {
    let mut w = open_world(40, 40);
    w.structures.clear();
    for (owner, cell) in [(Owner::Player, (6, 6)), (Owner::Enemy, (30, 30))] {
        let mut hq = factory(owner, 13536, 100);
        hq.kind = ObjectKind::Headquarters;
        hq.cell = cell;
        hq.slots.clear();
        w.add_structure(hq);
    }
    let kinds = crate::skirmish::start_units("01-goldmine valley");
    for side in 0..2 {
        w.add_start_units(side, kinds, |c| c.1 == 20); // a row of trees
    }
    for (p, home) in [(0, (8, 8)), (1, (32, 32))] {
        let mine: Vec<&Unit> = w.units.iter().filter(|u| u.player == p).collect();
        let mut got: Vec<i32> = mine.iter().map(|u| u.kind).collect();
        got.sort();
        let mut want = kinds.to_vec();
        want.sort();
        assert_eq!(got, want, "player {p}: the same twelve");
        for u in &mine {
            let c = u.cell();
            assert!((c.0 - home.0).abs().max((c.1 - home.1).abs()) <= 16, "around the HQ: {c:?}");
            assert!(!w.structures.iter().any(|s| s.covers(c)), "not inside a building: {c:?}");
        }
    }
    let cells: std::collections::HashSet<(i32, i32)> = w.units.iter().map(|u| u.cell()).collect();
    assert_eq!(cells.len(), w.units.len(), "one unit per cell");
    assert!(cells.iter().all(|c| c.1 != 20), "none under the trees");
    // two 3x2 blocks of six, each column one pair of one kind, apart from each other and 6+ cells from the HQ's middle
    for (p, home) in [(0, (8, 8)), (1, (32, 32))] {
        let mine: Vec<&Unit> = w.units.iter().filter(|u| u.player == p).collect();
        for (b, block) in mine.chunks(6).enumerate() {
            let (x0, y0) = block.iter().map(|u| u.cell()).min().unwrap();
            for (i, u) in block.iter().enumerate() {
                assert_eq!(u.cell(), (x0 + i as i32 / 2, y0 + i as i32 % 2), "player {p} block {b} unit {i}");
                assert_eq!(u.kind, kinds[6 * b + i]);
            }
            for c in 0..3 {
                assert_eq!(block[2 * c].kind, block[2 * c + 1].kind, "column {c} is a pair");
            }
            let (mx2, my2) = (x0 * 2 + 2 - home.0 * 2, y0 * 2 + 1 - home.1 * 2);
            assert!((mx2.abs().max(my2.abs()) + 1) / 2 >= 6, "player {p} block {b} at {:?} is not by the HQ", (x0, y0));
        }
        let (a, b) = (mine[0].cell(), mine[6].cell());
        assert!((a.0 - b.0).abs() >= 4 || (a.1 - b.1).abs() >= 3, "the blocks stand apart: {a:?} {b:?}");
    }
}

#[test]
fn the_campaign_computer_never_calls_a_carryall() {
    let mut w = turret_world(4, 0, [false; 5]);
    let g = HEADER_ORDER.iter().position(|&h| h == AirForce::Carryall).unwrap();
    w.air_stock[1][g] = 3;
    w.ai = true;
    for _ in 0..1200 {
        w.step();
    }
    assert_eq!(w.air_stock[1][g], 3);
}

#[test]
fn the_computer_builds_only_the_turret_tiers_its_level_allows() {
    // four player units around the factory: threat 4; 2000 credits want a tier 5 turret
    let mut w = turret_world(4, 2000, [false; 5]);
    w.ai_build_turret(1);
    assert!(turrets(&w).is_empty(), "no flag, no turret (levels 1, 3, 4, 7, 8)");
    let mut w = turret_world(4, 2000, [true, false, false, false, false]);
    w.ai_build_turret(1);
    assert_eq!(turrets(&w).iter().map(|t| t.0).collect::<Vec<_>>(), [25], "5 -> 4 -> 3 -> 1 down to the one allowed tier");
    assert_eq!(w.credits[1], 1800);
    let mut w = turret_world(4, 2000, [false, false, false, false, true]);
    w.ai_build_turret(1);
    assert_eq!(turrets(&w).iter().map(|t| t.0).collect::<Vec<_>>(), [29]);
    assert_eq!(w.credits[1], 1000);
    // tier 3 falls to tier 1, skipping tier 2; tier 2 alone never steps down
    let mut w = turret_world(4, 1200, [true, true, false, false, false]); // 1001..1500: threat 4 wants tier 3
    w.ai_build_turret(1);
    assert_eq!(turrets(&w).iter().map(|t| t.0).collect::<Vec<_>>(), [25]);
    let mut w = turret_world(0, 1200, [true, true, true, true, true]); // threat 0: nothing is wanted
    w.ai_build_turret(1);
    assert!(turrets(&w).is_empty());
}

#[test]
fn thresholds_follow_the_credit_bands() {
    // (credits, units around the factory) -> the tier the exe asks for with every flag set
    for (credits, units, tier) in [
        (500, 2, None),
        (500, 3, Some(1)),
        (500, 5, Some(1)),
        (700, 5, Some(3)),
        (900, 6, Some(4)),
        (900, 7, Some(4)),
        (1000, 7, Some(5)),
        (1200, 1, None),
        (1200, 2, Some(1)),
        (1200, 4, Some(3)),
        (1200, 5, Some(4)),
        (1200, 6, Some(5)),
        (3000, 2, Some(3)),
        (3000, 4, Some(5)),
    ] {
        let mut w = turret_world(units, credits, [true; 5]);
        w.ai_build_turret(1);
        let got = turrets(&w).first().map(|t| t.0 - 24);
        assert_eq!(got, tier, "{credits} credits, threat {units}");
    }
}

#[test]
fn the_turret_stands_on_one_of_the_constructors_eight_spots() {
    // a light factory (3x3, first blocked row 1) at (8, 8)
    let spots = [(10, 7), (12, 9), (12, 10), (10, 12), (8, 12), (6, 10), (6, 9), (8, 7)];
    let mut w = turret_world(3, 500, [true; 5]);
    w.ai_build_turret(1);
    let t = turrets(&w);
    assert_eq!(t.len(), 1);
    assert!(spots.contains(&t[0].1), "{:?}", t);
}

#[test]
fn a_dead_units_slot_counts_for_the_threat_and_walkers_count_double() {
    let mut w = turret_world(0, 500, [true; 5]);
    w.corpses.push(Corpse { x: 9 * CELL, y: 12 * CELL, left: 100, variant: 0, air: false, blasted: false, player: 0, kind: 43, slot: u32::MAX });
    w.corpses.push(Corpse { x: 9 * CELL, y: 12 * CELL, left: 100, variant: 0, air: false, blasted: false, player: 0, kind: 7, slot: u32::MAX });
    w.ai_build_turret(1); // 2 + 1 = 3
    assert_eq!(turrets(&w).iter().map(|t| t.0).collect::<Vec<_>>(), [25]);
}

#[test]
fn an_advancer_over_the_building_makes_the_computer_build_anti_air() {
    let mut w = turret_world(0, 500, [true; 5]);
    let a = w.structures[1.min(w.structures.len() - 1)].action_cell();
    let a = if w.structures.len() > 1 { a } else { w.structures[0].action_cell() };
    w.add_unit(kinds::ADVANCER, 0, a.0 * CELL + 16, a.1 * CELL + 16);
    w.ai_build_turret(1); // threat 1 (the Advancer), free 6: free wins, 500 credits -> tier 2 (kind 26)
    assert_eq!(turrets(&w).iter().map(|t| t.0).collect::<Vec<_>>(), [26]);
}

#[test]
fn handle_slots_are_reused_and_decide_ties_between_buildings() {
    // two enemy factories: a freed slot is refilled first, and a tie between buildings goes to the lower slot
    let mut w = open_world(40, 24);
    w.add_unit(7, 0, CELL, CELL);
    for x in [4, 12] {
        let mut b = factory(Owner::Enemy, 100, 101);
        b.cell = (x, 8);
        w.add_structure(b);
    }
    assert_eq!(w.slot_tables[1], [Some(0), Some(1)]);
    w.slot_free(1, 0);
    let mut b = factory(Owner::Enemy, 100, 101);
    b.cell = (24, 8);
    w.add_structure(b);
    assert_eq!(w.slot_tables[1], [Some(2), Some(1)], "the hole at slot 0 is refilled");
    // the same number of player units next to the factory in slot 0 (cell 24) and the one in slot 1 (cell 12)
    for cx in [25, 13] {
        for i in 0..4 {
            w.add_unit(7, 0, (cx + i % 2) * CELL + 16, 12 * CELL + 16);
        }
    }
    w.credits[1] = 500;
    w.turret_flags[1] = [true; 5];
    w.ai_build_turret(1);
    let t = w.structures.iter().find(|s| matches!(s.kind, ObjectKind::Turret(_))).expect("a turret");
    assert!(t.cell.0 > 18, "next to the factory in slot 0 (cell 24), not the one in slot 1 (cell 12): {:?}", t.cell);
}

#[test]
fn units_of_both_sides_contest_a_neutral_building() {
    for (blue, green, winner) in [(1, 3, Owner::Enemy), (3, 1, Owner::Player)] {
        let mut w = open_world(24, 12);
        for k in 0..blue {
            w.add_unit(7, 0, 8 * CELL + 16, (3 + 2 * k) * CELL + 16);
        }
        for k in 0..green {
            w.add_unit(7, 1, 17 * CELL + 16, (3 + 2 * k) * CELL + 16);
        }
        let mut b = factory(Owner::Neutral, 2500, 101);
        b.cell = (12, 5);
        b.max_hp = 2500;
        w.add_structure(b);
        for _ in 0..6000 {
            w.step();
            if w.structures[0].owner != Owner::Neutral {
                break;
            }
        }
        assert_eq!(w.structures[0].owner, winner, "blue {blue} vs green {green}, hp {} tally {:?}", w.structures[0].hp, w.structures[0].tally);
    }
}

#[test]
fn anti_air_does_not_shoot_ground() {
    let mut w = open_world(20, 20);
    let a = w.add_unit(1, 0, 5 * CELL + 16, 5 * CELL + 16);
    let e = w.add_unit(7, 1, 7 * CELL + 16, 5 * CELL + 16);
    w.units.iter_mut().find(|u| u.id == e).unwrap().hp = 100000;
    for _ in 0..600 {
        w.step();
    }
    assert_eq!(w.unit(e).unwrap().hp, 100000, "anti-air weapons ignore ground targets");
    assert!(w.unit(a).is_some());
}

/// The ground search (0x47b140) reads grid 0x5eac38, the flying Advancer lives in the air grid 0x60ac38 that only
/// anti-air shooters search (0x47ad30).
#[test]
fn only_anti_air_units_shoot_the_advancer() {
    for (kind, hit) in [(7, false), (1, true)] {
        let mut w = open_world(20, 20);
        let _g = w.add_unit(kind, 0, 5 * CELL + 16, 5 * CELL + 16);
        let adv = w.add_unit(kinds::ADVANCER, 1, 7 * CELL + 16, 5 * CELL + 16);
        let hp = w.unit(adv).unwrap().hp;
        for _ in 0..600 {
            w.step();
            if w.unit(adv).is_none_or(|u| u.hp < hp) {
                break;
            }
        }
        let hurt = w.unit(adv).is_none_or(|u| u.hp < hp);
        assert_eq!(hurt, hit, "kind {kind}");
    }
}

/// `0x47b3c0`: anti-air range is `range * 50` px against aircraft, so a tier-1 anti-air unit (0-9) engages a bomber 400 px away
/// (the port used to stop at 9.5 cells of 32 px).
#[test]
fn anti_air_engages_aircraft_out_to_fifty_pixels_per_range_cell() {
    let mut w = open_world(40, 12);
    let aa = w.add_unit(1, 0, 20 * CELL + 16, 5 * CELL + 16);
    w.air_stock[1][0] = 1;
    assert!(w.launch_strike(1, AirForce::Bomber, 30 * CELL, 5 * CELL + 16));
    let (ux, _) = w.unit(aa).unwrap().px();
    w.aircraft[0].x = ux - 420; // flying right, towards the unit
    let mut first = None;
    for t in 0..60 {
        w.step();
        if first.is_none() && w.air_targets.contains_key(&(false, aa as usize)) {
            first = Some((t, ux - w.aircraft[0].x));
        }
    }
    let (_, d) = first.expect("the bomber was picked as a target");
    assert!(d > 320, "picked {d} px away, beyond the old 304 px");
}

/// `0x42f30e`: the area damage skips flyers, so a bomb does not hurt the hovering Advancer standing under it.
#[test]
fn bombs_do_not_hurt_the_hovering_advancer() {
    let mut w = open_world(14, 9);
    let adv = w.add_unit(kinds::ADVANCER, 0, 5 * CELL + 16, 4 * CELL + 16);
    let tank = w.add_unit(7, 0, 6 * CELL, 4 * CELL + 16);
    let (hp_a, hp_t) = (w.unit(adv).unwrap().hp, w.unit(tank).unwrap().hp);
    w.explode(5 * CELL + 16, 4 * CELL + 16, 18, 110, 1);
    assert_eq!(w.unit(adv).unwrap().hp, hp_a);
    assert_eq!(w.unit(tank).unwrap().hp, hp_t - 110, "ground units within 32 px take the bomb's 110");
}

#[test]
fn the_anti_air_turret_shoots_an_enemy_advancer() {
    let mut w = open_world(20, 20);
    let mut t = factory(Owner::Player, 1, 0);
    t.kind = ObjectKind::Turret(2);
    t.cell = (5, 5);
    t.weapon = 110;
    t.aim = TopAim::new(110, 0);
    t.max_hp = 1000;
    t.hp = 1000;
    w.structures.clear();
    w.add_structure(t);
    let adv = w.add_unit(kinds::ADVANCER, 1, 7 * CELL + 16, 5 * CELL + 16);
    let hp = w.unit(adv).unwrap().hp;
    for _ in 0..600 {
        w.step();
        if w.unit(adv).is_none_or(|u| u.hp < hp) {
            return;
        }
    }
    panic!("the turret never hit the Advancer");
}

#[test]
fn the_anti_air_turret_launches_a_missile_that_hits_its_target_by_handle_when_it_lands() {
    let mut w = open_world(20, 20);
    let mut t = factory(Owner::Player, 1, 0);
    t.kind = ObjectKind::Turret(2);
    t.kind_id = 26;
    t.cell = (5, 5);
    t.weapon = 110;
    t.aim = TopAim::new(110, 0);
    t.max_hp = 1000;
    t.hp = 1000;
    w.structures.clear();
    w.add_structure(t);
    let adv = w.add_unit(kinds::ADVANCER, 1, 7 * CELL + 16, 5 * CELL + 16);
    let hp = w.unit(adv).unwrap().hp;
    for _ in 0..600 {
        w.step();
        if let Some(b) = w.bullets.iter().find(|b| b.bullet_type == 1) {
            // the strength is taken off the handle's unit when the bullet lands (0x4108bc), not at the launch
            assert_eq!((b.strength, b.target), (15, Some(HitTarget::Unit(adv))));
            assert_eq!(w.unit(adv).unwrap().hp, hp);
            // the launch sounds: the volley spawner's muzzle sound for kind 110 (`0x47cf40`), `sfx/turets/02.wav`
            assert!(w.sounds.iter().any(|s| s.0 == 12 && s.3), "the launch plays the turret's muzzle sound");
            assert!(!w.sounds.iter().any(|s| s.0 == 20), "the impact sound only comes when the missile lands");
            for _ in 0..200 {
                w.step();
            }
            assert!(w.unit(adv).is_none_or(|u| u.hp < hp), "the missile hit");
            return;
        }
    }
    panic!("no missile was launched");
}

#[test]
fn the_anti_air_turret_stops_shooting_when_the_bomber_flies_out_of_range() {
    let mut w = open_world(60, 20);
    let mut t = factory(Owner::Player, 1, 0);
    t.kind = ObjectKind::Turret(2);
    t.kind_id = 26;
    t.cell = (5, 10);
    t.weapon = 110;
    t.aim = TopAim::new(110, 0);
    t.max_hp = 1000;
    t.hp = 1000;
    w.structures.clear();
    w.add_structure(t);
    w.add_unit(7, 1, 55 * CELL, 18 * CELL); // keeps the mission from ending
    w.air_stock[1][0] = 1;
    assert!(w.launch_strike(1, AirForce::Bomber, 50 * CELL, 10 * CELL));
    let max = w.structures[0].aim.as_ref().unwrap().params.max_range as i32 * 50;
    w.aircraft[0].x = 5 * CELL - 300;
    w.aircraft[0].hp = 1_000_000;
    let (mut shots_out_of_range, mut shots) = (0, 0);
    let mut seen = 0;
    for _ in 0..1500 {
        w.step();
        let Some(a) = w.aircraft.first() else { break };
        let d = (a.x - (5 * CELL + 16)).abs();
        let n = w.bullets.iter().filter(|b| b.bullet_type == 1).count();
        if n > seen {
            shots += 1;
            if d > max {
                shots_out_of_range += 1;
            }
        }
        seen = n;
    }
    assert!(shots > 0, "the turret shot at the bomber at all");
    assert_eq!(shots_out_of_range, 0, "no new missile once the bomber is well past the range (max {max})");
}

#[test]
fn a_shot_at_an_advancer_meets_its_hovering_sprite() {
    let mut w = open_world(20, 20);
    let id = w.add_unit(kinds::ADVANCER, 1, 8 * CELL + 16, 8 * CELL + 16);
    let u = w.unit(id).unwrap();
    let (aim, shot) = u.flyer_points();
    let h = u.sprite_y().round() as i32 - u.px().1;
    assert!(h < -40, "the Advancer hovers well above the ground point");
    assert_eq!(shot, (u.px().0, u.px().1 + h));
    assert_eq!(aim, (u.px().0, u.px().1 + h / 2));
}

#[test]
fn a_turret_shell_starts_at_the_muzzle_point_of_the_table() {
    // a synthetic `.po1` table: every facing's stored point is (0, 30), so the muzzle is 30 px above the top's anchor
    let table: Vec<u8> = (0..32).flat_map(|_| [0i16.to_le_bytes(), 30i16.to_le_bytes()].concat()).collect();
    let mut w = open_world(20, 20);
    w.turret_muzzles.insert(109, opensow_formats::fire::FireTables::parse(&table).unwrap());
    let mut t = factory(Owner::Player, 1, 0);
    t.kind = ObjectKind::Turret(1);
    t.kind_id = 25;
    t.cell = (5, 5);
    t.weapon = 109;
    t.aim = TopAim::new(109, 0);
    t.max_hp = 1000;
    t.hp = 1000;
    w.structures.clear();
    w.add_structure(t);
    w.add_unit(kinds::POD, 1, 7 * CELL + 16, 8 * CELL + 16);
    let want = (5 * CELL + 16, 5 * CELL + 16 - 11 - 30);
    for _ in 0..900 {
        w.step();
        if let Some(b) = w.bullets.first() {
            assert_eq!(
                b.path.points.first().map(|p| (p.0 - want.0).abs() <= 2 && (p.1 - want.1).abs() <= 2),
                Some(true),
                "first point {:?}, muzzle {want:?}",
                b.path.points.first()
            );
            return;
        }
    }
    panic!("the turret never fired");
}

/// The Advancer's gun top (0x47a8af; flyer search 0x47aed0 then 0x47ad30) fires at enemy ground anti-air units and
/// enemy Advancers, but not at other ground units or the anti-air turret's kind.
#[test]
fn the_advancer_shoots_anti_air_units_and_advancers_only() {
    for (kind, victim_is_adv, hit) in [(1, false, true), (2, false, true), (7, false, false), (110, false, false), (40, true, true)] {
        let mut w = open_world(20, 20);
        let _a = w.add_unit(kinds::ADVANCER, 0, 5 * CELL + 16, 5 * CELL + 16);
        let v = w.add_unit(kind, 1, 8 * CELL + 16, 5 * CELL + 16);
        assert_eq!(victim_is_adv, kind == kinds::ADVANCER);
        let hp = w.unit(v).unwrap().hp;
        for _ in 0..900 {
            w.step();
            if w.unit(v).is_none_or(|u| u.hp < hp) {
                break;
            }
        }
        assert_eq!(w.unit(v).is_none_or(|u| u.hp < hp), hit, "victim kind {kind}");
    }
}

#[test]
fn advancers_and_nuclear_vehicles_are_selected_alone() {
    let mut w = open_world(20, 20);
    let a = w.add_unit(kinds::ADVANCER, 0, 3 * CELL + 16, 2 * CELL + 16);
    let n = w.add_unit(kinds::POD, 0, 4 * CELL + 16, 2 * CELL + 16);
    w.add_unit(7, 0, 5 * CELL + 16, 2 * CELL + 16);
    w.add_unit(8, 0, 6 * CELL + 16, 2 * CELL + 16);
    // box and select-all take only regular units
    assert_eq!(w.select_rect(0, 0, 10 * CELL, 4 * CELL, false), 2);
    assert!(!w.unit(a).unwrap().selected && !w.unit(n).unwrap().selected);
    w.select_all();
    assert_eq!(w.units.iter().filter(|u| u.selected).count(), 2);
    // clicking one selects it alone, even additively; a regular unit added afterwards replaces it
    assert_eq!(w.select_at(3 * CELL + 16, w.unit(a).unwrap().sprite_y() as i32, 10.0, true), 1);
    assert!(w.unit(a).unwrap().selected);
    assert_eq!(w.select_at(5 * CELL + 16, 2 * CELL + 16, 10.0, true), 1);
    assert!(!w.unit(a).unwrap().selected);
    assert_eq!(w.select_at(4 * CELL + 16, w.unit(n).unwrap().sprite_y() as i32, 10.0, true), 1);
    assert!(w.unit(n).unwrap().selected);
}

#[test]
fn a_box_also_selects_enemy_units_but_orders_ignore_them() {
    let mut w = open_world(20, 20);
    let own = w.add_unit(7, 0, 3 * CELL + 16, 2 * CELL + 16);
    let foe = w.add_unit(7, 1, 5 * CELL + 16, 2 * CELL + 16);
    let adv = w.add_unit(kinds::ADVANCER, 1, 6 * CELL + 16, 2 * CELL + 16);
    assert_eq!(w.select_rect(0, 0, 10 * CELL, 4 * CELL, false), 2);
    assert!(w.unit(own).unwrap().selected && w.unit(foe).unwrap().selected && !w.unit(adv).unwrap().selected);
    w.order_move_selected(8 * CELL, 8 * CELL);
    assert!(w.unit(foe).unwrap().path.is_empty(), "an enemy in the selection takes no order");
    // a click on an enemy never selects it (`0x417a0f`: owner test)
    assert_eq!(w.select_at(5 * CELL + 16, 2 * CELL + 16, 10.0, false), 0);
}

#[test]
fn selection_and_group_orders_spread_units() {
    let mut w = open_world(20, 20);
    for k in 0..4 {
        w.add_unit(7, 0, (2 + k) * CELL + 16, 2 * CELL + 16);
    }
    assert_eq!(w.select_rect(0, 0, 10 * CELL, 4 * CELL, false), 4);
    w.order_move_selected(10 * CELL + 16, 10 * CELL + 16);
    let mut ends: Vec<_> = w.units.iter().map(|u| u.path.last().copied().unwrap()).collect();
    ends.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ends.dedup();
    assert_eq!(ends.len(), 4, "each unit gets its own cell");
}

#[test]
fn destroying_the_enemy_hq_wins() {
    let mut w = open_world(20, 20);
    w.add_unit(9, 0, 5 * CELL + 16, 5 * CELL + 16);
    w.add_structure(Structure {
        object: 0,
        kind: ObjectKind::Headquarters,
        owner: Owner::Enemy,
        cell: (8, 5),
        hp: 50,
        max_hp: 50,
        aim: None,
        weapon: 0,
        target: None,
        slots: Vec::new(),
        active: 0,
        kind_id: 100,
        tally: [0; 3],
        heal: 0,
        upgrade: 0,
        protected: true,
        attacked: false,
        sat_cd: 0,
        advancer: None,
        rally: None,
        dying: None,
    });
    for _ in 0..4000 {
        w.step();
    }
    assert_eq!(w.outcome, Some(Outcome::Victory));
}

/// A custom map's "destroy everything" rule (`Objective::destroy_all`): the fallen headquarters is not enough.
#[test]
fn a_destroy_everything_objective_needs_the_last_enemy_unit_gone() {
    let mut w = open_world(20, 20);
    w.objective.destroy_all = true;
    w.add_unit(9, 0, 5 * CELL + 16, 5 * CELL + 16);
    let last = w.add_unit(1, 1, 18 * CELL + 16, 18 * CELL + 16); // anti-air, far away
    w.add_structure(Structure {
        object: 0,
        kind: ObjectKind::Headquarters,
        owner: Owner::Enemy,
        cell: (8, 5),
        hp: 50,
        max_hp: 50,
        aim: None,
        weapon: 0,
        target: None,
        slots: Vec::new(),
        active: 0,
        kind_id: 100,
        tally: [0; 3],
        heal: 0,
        upgrade: 0,
        protected: true,
        attacked: false,
        sat_cd: 0,
        advancer: None,
        rally: None,
        dying: None,
    });
    for _ in 0..4000 {
        w.step();
    }
    assert!(!w.structures.iter().any(|s| s.kind_id == kinds::HEADQUARTERS && s.owner == Owner::Enemy && s.present()), "the headquarters fell");
    assert_eq!(w.outcome, None, "an enemy unit is left");
    w.units.iter_mut().find(|u| u.id == last).unwrap().hp = 0;
    for _ in 0..200 {
        w.step();
    }
    assert!(w.units.iter().any(|u| u.kind == kinds::ADVANCER && u.player == 1), "the homeless Advancer does not count");
    assert_eq!(w.outcome, Some(Outcome::Victory));
}

/// A custom map's reinforcement countdown (mode 2): nothing ends, the human gets what the map holds.
#[test]
fn a_custom_reinforcement_countdown_pays_out_once_and_lands_squads() {
    let mut w = open_world(40, 40);
    w.add_unit(9, 0, 5 * CELL + 16, 5 * CELL + 16);
    w.add_unit(1, 1, 38 * CELL + 16, 38 * CELL + 16);
    // squad 17: two walkers; squad 5: eight anti-air units
    w.reinforcement = Some(Reinforcement { credits: 5000, research: 1500, air: [2, 0, 1, 0, 4], drops: vec![((20, 10), 17), ((20, 20), 5)] });
    let counter = w.carry_counter;
    w.timer = Some((0, 2, 2));
    let (credits, research, stock) = (w.credits[0], w.research[0], w.air_stock[0]);
    for _ in 0..110 {
        w.step();
    }
    assert!(!w.timer_expired && w.aircraft.is_empty() && w.credits[0] == credits, "not before 0:00");
    for _ in 0..200 {
        w.step();
    }
    assert!(w.timer_expired);
    assert_eq!(w.outcome, None, "the game goes on");
    assert_eq!((w.credits[0] - credits, w.research[0] - research), (5000, 1500));
    assert_eq!([0, 1, 2, 3, 4].map(|g| w.air_stock[0][g] - stock[g]), [2, 0, 1, 0, 4], "the carryalls that land cost no stock");
    let carryalls: Vec<_> =
        w.aircraft.iter().filter(|a| a.force == AirForce::Carryall && a.side == 0).map(|a| (a.target.0 / CELL, a.target.1 / CELL)).collect();
    assert_eq!(carryalls, vec![(20, 10), (20, 20)]);
    assert_eq!(w.carry_counter, counter, "chosen squads leave the cycle alone");
    assert!(w.log.iter().any(|&(_, id)| id == msg::REINFORCEMENTS));
    // they land their squads, and nothing is paid twice
    let before = w.units.iter().filter(|u| u.player == 0).count();
    for _ in 0..3000 {
        w.step();
    }
    let landed = |kind: i32| w.units.iter().filter(|u| u.player == 0 && u.kind == kind).count();
    assert_eq!((landed(41), landed(1)), (2, 8), "the chosen squads, all of them on open ground");
    assert_eq!(w.units.iter().filter(|u| u.player == 0).count(), before + 10);
    assert_eq!(w.credits[0] - credits, 5000);
}

/// A custom escort: the pods walk the route by script at the convoy pace, take no orders, and the first to reach the goal
/// wins; losing them all loses, the enemy's headquarters does not decide it.
#[test]
fn a_custom_escort_walks_its_pods_through_the_route_to_the_goal() {
    let mut w = open_world(40, 40);
    let pod = w.add_unit(kinds::POD, 0, 3 * CELL + 16, 3 * CELL + 16);
    w.add_unit(1, 1, 38 * CELL + 16, 38 * CELL + 16);
    w.set_convoy(vec![(3, 20), (20, 20)]);
    assert_eq!(w.walk_params_of(46).unwrap().speed, 7, "the convoy pace");
    // no player orders
    w.units.iter_mut().find(|u| u.id == pod).unwrap().selected = true;
    w.order_move_selected(30 * CELL, 3 * CELL);
    let mut south = false;
    let mut ticks = 0;
    while w.outcome.is_none() && ticks < 60 * 60 * 6 {
        w.step();
        ticks += 1;
        let c = w.unit(pod).unwrap().cell();
        assert!(c.0 <= 4 || south, "it goes south first (the waypoint), not east: {c:?} at {ticks}");
        south |= c.1 >= 19;
    }
    assert_eq!(w.outcome, Some(Outcome::Victory));
    let c = w.unit(pod).unwrap().cell();
    assert!((c.0 - 20).abs() <= 1 && (c.1 - 20).abs() <= 1, "{c:?}");
    // about 33 cells of 32 px at 7 px/s: some 150 s
    assert!((60 * 130..60 * 170).contains(&ticks), "{ticks} ticks");
    // the convoy shot down: lost
    let mut w = open_world(40, 40);
    let pod = w.add_unit(kinds::POD, 0, 3 * CELL + 16, 3 * CELL + 16);
    w.add_unit(1, 1, 38 * CELL + 16, 38 * CELL + 16);
    w.set_convoy(vec![(20, 20)]);
    w.step();
    w.units.iter_mut().find(|u| u.id == pod).unwrap().hp = 0;
    for _ in 0..100 {
        w.step();
    }
    assert_eq!(w.outcome, Some(Outcome::Defeat));
}

/// The countdown's end modes outside the campaign (header word 67): 0 holding out wins, 1 running out of time loses.
#[test]
fn a_countdown_ends_a_skirmish_by_its_mode() {
    for (mode, want) in [(0, Outcome::Victory), (1, Outcome::Defeat)] {
        let mut w = open_world(20, 20);
        w.add_unit(9, 0, 5 * CELL + 16, 5 * CELL + 16);
        w.add_unit(1, 1, 18 * CELL + 16, 18 * CELL + 16);
        w.timer = Some((0, 3, mode));
        for _ in 0..170 {
            w.step();
        }
        assert_eq!(w.outcome, None, "mode {mode}: the clock still runs");
        for _ in 0..200 {
            w.step();
        }
        assert_eq!(w.outcome, Some(want), "mode {mode}");
    }
}

fn factory(owner: Owner, hp: i32, kind_id: i32) -> Structure {
    let slots = vec![
        SlotState { kind: 7, progress: 0, target: 2000, level: 0, max_level: 0, waiting: 0, unlock: 0 },
        SlotState { kind: 13, progress: 0, target: 2000, level: 0, max_level: 0, waiting: 0, unlock: 0 },
    ];
    Structure {
        object: 0,
        kind: ObjectKind::LightFactory,
        owner,
        cell: (6, 5),
        hp,
        max_hp: 13536,
        aim: None,
        weapon: 0,
        target: None,
        slots,
        active: 1,
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

#[test]
fn the_advancer_captures_an_enemy_building_and_it_heals_in() {
    let mut w = open_world(16, 12);
    w.add_structure(factory(Owner::Enemy, 400, 101));
    w.add_unit(7, 1, 15 * CELL, 11 * CELL); // keeps the mission from ending
    let id = w.add_unit(kinds::ADVANCER, 0, 3 * CELL, 5 * CELL + 16);
    assert!(w.order_action(id, 0));
    for _ in 0..500 {
        w.step();
    }
    // the dead building stays where it was while its animation runs (about 3 s), then changes hands
    assert!(w.structures[0].dying.is_some() || w.structures[0].owner == Owner::Player);
    for _ in 0..400 {
        w.step();
    }
    let s = &w.structures[0];
    assert_eq!(s.owner, Owner::Player, "captured");
    assert!(s.hp > 1000 && s.heal < s.max_hp, "hp {} heal {}", s.hp, s.heal);
    w.step();
    for _ in 0..300 {
        w.step();
    }
    assert_eq!(w.structures[0].hp, 13536, "fully healed");
}

#[test]
fn a_second_advancer_stops_next_to_the_one_working_on_the_building() {
    // the order is a normal go-to (`0x46b8b0` -> `0x4706a0` type 1): the cell above the building is taken, so the late
    // Advancer ends beside the first one and never shoots (the original behaves the same)
    let mut w = open_world(16, 12);
    w.add_structure(factory(Owner::Enemy, 13000, 101));
    w.add_unit(7, 1, 15 * CELL, 11 * CELL);
    let a = w.add_unit(kinds::ADVANCER, 0, 3 * CELL, 5 * CELL + 16);
    let b = w.add_unit(kinds::ADVANCER, 0, 3 * CELL, 8 * CELL);
    assert!(w.order_action(a, 0));
    for _ in 0..600 {
        w.step();
    }
    let cell = w.structures[0].action_cell();
    assert_eq!(w.unit(a).unwrap().cell(), cell);
    assert!(w.order_action(b, 0));
    for _ in 0..900 {
        w.step();
    }
    let (ua, ub) = (w.unit(a).unwrap(), w.unit(b).unwrap());
    assert_eq!(ua.cell(), cell, "the first Advancer keeps the spot");
    assert_ne!(ub.cell(), cell, "the second one cannot enter it");
    assert!(ub.action.is_none(), "and its order lapses, it does not shoot");
}

#[test]
fn the_advancer_upgrades_and_repairs_own_buildings() {
    let mut w = open_world(16, 12);
    w.add_structure(factory(Owner::Player, 13000, 101));
    let id = w.add_unit(kinds::ADVANCER, 0, 3 * CELL, 5 * CELL + 16);
    assert!(w.order_action(id, 0));
    for _ in 0..1200 {
        w.step();
    }
    assert_eq!(w.structures[0].active, 2, "second slot unlocked after 2000 points at 40 per shot");
    w.repair_mode = true;
    assert!(w.order_action(id, 0));
    for _ in 0..400 {
        w.step();
    }
    assert_eq!(w.structures[0].hp, 13536);
}

#[test]
fn destroyed_turrets_vanish_instead_of_changing_hands() {
    let mut w = open_world(16, 12);
    let mut t = factory(Owner::Enemy, 100, 26);
    t.slots.clear();
    t.active = 0;
    w.add_structure(t);
    w.add_unit(7, 1, 15 * CELL, 11 * CELL);
    let id = w.add_unit(kinds::ADVANCER, 0, 3 * CELL, 5 * CELL + 16);
    assert!(w.order_action(id, 0));
    for _ in 0..500 {
        w.step();
    }
    assert!(!w.structures[0].alive());
    assert_eq!(w.structures[0].owner, Owner::Enemy);
}

#[test]
fn satellite_protection_kills_a_foreign_advancer() {
    let mut w = open_world(16, 12);
    let mut f = factory(Owner::Enemy, 13000, 102);
    f.protected = true;
    w.add_structure(f);
    w.add_unit(7, 1, 15 * CELL, 11 * CELL);
    let id = w.add_unit(kinds::ADVANCER, 0, 3 * CELL, 5 * CELL + 16);
    assert!(w.order_action(id, 0));
    let mut laser = 0;
    for _ in 0..600 {
        w.step();
        laser = laser.max(w.bullets.iter().filter(|b| b.bullet_type == 10).count());
    }
    assert!(w.unit(id).is_none(), "the Advancer is destroyed");
    assert_eq!(laser, 1, "one satellite laser was fired");
    assert!(w.bullets.iter().all(|b| b.bullet_type != 10), "and it has landed without doing anything else");
    assert_eq!(w.structures[0].owner, Owner::Enemy);
}

/// Skirmish difficulty (docs/skirmish.md): only the computer's side is scaled, Normal is the exe's behaviour, hundredths carry over.
#[test]
fn difficulty_scales_the_computers_income_only_and_levels_are_ordered() {
    for (d, expect) in [(Difficulty::Easy, 12), (Difficulty::Normal, 12), (Difficulty::Hard, 12), (Difficulty::Brutal, 12)] {
        let mut w = open_world(16, 12);
        w.difficulty = d;
        for owner in [Owner::Player, Owner::Enemy] {
            let mut mine = factory(owner, 13536, 105);
            mine.kind = ObjectKind::GoldMine;
            mine.cell = (2 + 5 * (owner == Owner::Enemy) as i32, 2);
            mine.active = 2; // 3 credits per second
            w.add_structure(mine);
        }
        w.add_unit(7, 0, 8 * CELL, 8 * CELL);
        w.add_unit(7, 1, 15 * CELL, 11 * CELL);
        w.ai = true;
        w.cheats.freeze_enemy = true; // the computer does not spend what it earns
        for _ in 0..4 * 60 + 5 {
            w.step();
        }
        assert_eq!(w.credits[0], 12, "{d:?}: the human's mine is never scaled (3 x 4 s)");
        // 3 credits x pct per second for four payouts; the hundredths are kept, not lost
        assert_eq!(w.credits[1], expect, "{d:?}");
    }
    assert_eq!(Difficulty::Brutal.next(), Difficulty::Easy);
    assert_eq!(Difficulty::Normal.handicap(), Handicap { income_pct: 100, free_windtrap_levels: 0, think_every: 1, strike_period: 600 });
    assert!(Difficulty::ALL.windows(2).all(|p| {
        let (a, b) = (p[0].handicap(), p[1].handicap());
        a.income_pct <= b.income_pct && a.free_windtrap_levels <= b.free_windtrap_levels && a.think_every >= b.think_every && a.strike_period >= b.strike_period
    }));
}

/// Hard and Brutal give the computer free windtrap levels (12 steps/s each), never the human.
#[test]
fn hard_and_brutal_give_the_computers_factories_free_windtrap_levels() {
    for (d, expect) in [(Difficulty::Normal, 30), (Difficulty::Hard, 42), (Difficulty::Brutal, 54)] {
        let mut w = open_world(16, 12);
        w.difficulty = d;
        w.ai = true;
        let mut ours = factory(Owner::Player, 13536, 101);
        ours.cell = (2, 2);
        let mut theirs = factory(Owner::Enemy, 13536, 101);
        theirs.cell = (9, 2);
        w.add_structure(ours);
        w.add_structure(theirs);
        w.add_unit(7, 0, 8 * CELL, 8 * CELL);
        w.add_unit(7, 1, 15 * CELL, 11 * CELL);
        for _ in 0..60 {
            w.step();
        }
        let progress = |i: usize| w.structures[i].slots[0].progress;
        assert_eq!(progress(0), 30, "{d:?}: the human's factory runs at the base rate");
        assert_eq!(progress(1), expect, "{d:?}: base 30 + 12 per free level");
    }
}

/// `--spectate`: the computer plays side 0 too, with its own difficulty and its own timers.
#[test]
fn a_spectated_game_runs_the_computer_for_both_sides_with_separate_timers() {
    let mut w = open_world(30, 30);
    w.ai = true;
    w.difficulty = Difficulty::Hard;
    w.difficulty0 = Difficulty::Easy;
    assert_eq!((w.difficulty_of(0), w.difficulty_of(1)), (Difficulty::Hard, Difficulty::Hard), "no spectate: side 0 is the human");
    w.spectate = true;
    assert_eq!((w.difficulty_of(0), w.difficulty_of(1)), (Difficulty::Easy, Difficulty::Hard));
    let id = w.add_unit(7, 0, 4 * CELL, 4 * CELL);
    w.add_unit(6, 1, 25 * CELL, 25 * CELL);
    w.ai_timer = 1000;
    w.spectate_ai = (3, 1000, 600, 0);
    w.tick = 120; // Easy thinks every 120 ticks
    w.step_ai_spectate();
    assert!(w.unit(id).is_some_and(|u| !u.path.is_empty()), "blue's idle unit gets orders");
    assert_eq!(w.ai_timer, 1000, "green's timer is untouched by blue's run");
}

#[test]
fn the_campaign_computer_builds_turrets_only_when_the_strike_timer_runs_out() {
    let mut w = turret_world(3, 50_000, [true; 5]);
    w.ai_strategy = 3;
    w.ai_timer = 100_000;
    w.ai_strike = 600;
    for t in 1..600 {
        w.tick = t;
        w.step_ai(1);
    }
    assert!(turrets(&w).is_empty(), "0x42f942 skips the turret routine while the 600-step timer runs");
    w.tick = 600;
    w.step_ai(1);
    let n = turrets(&w).len();
    assert!((1..=2).contains(&n), "the timer ran out: the routine runs twice at most, built {n}");
}

/// `0x42f882`: the campaign computer orders with `0x469fe0(target, 0)`, which plans one route (`0x46eb40`) and clears the
/// order target again (`0x46a141..0x46a14d`). The unit walks to the neutral lab without keeping it as its target and, once
/// the route ends, picks again with the strategy of that moment (here the headquarters).
#[test]
fn a_campaign_computer_order_is_one_route_and_the_unit_picks_again_when_it_ends() {
    let mut w = open_world(48, 20);
    w.structures.clear();
    let mut hq = factory(Owner::Player, 13536, 100);
    hq.kind = ObjectKind::Headquarters;
    hq.cell = (40, 8);
    hq.slots.clear();
    w.add_structure(hq);
    let mut lab = factory(Owner::Neutral, 6768, kinds::RESEARCH_LAB);
    lab.kind = ObjectKind::ResearchLab;
    lab.cell = (4, 8);
    lab.slots.clear();
    w.add_structure(lab);
    let id = w.add_unit(7, 1, 14 * CELL + 16, 9 * CELL + 16);
    w.ai_strategy = 4; // building first: the lab is the nearest
    w.ai_timer = 100_000;
    w.ai_strike = 100_000;
    w.tick = 1;
    w.step_ai(1);
    let u = w.unit(id).unwrap();
    assert!(u.chase.is_none(), "the order target is cleared again");
    let end = *u.path.last().expect("a route to the lab");
    assert!(end.0 < (14 * CELL) as f32, "the route leads to the lab: {end:?}");
    w.ai_strategy = 1;
    for _ in 0..3000 {
        w.step();
        if w.unit(id).is_some_and(|u| u.path.is_empty()) {
            break;
        }
    }
    let u = w.unit(id).unwrap();
    assert!(u.path.is_empty() && u.cell().0 < 12, "arrived at the lab: {:?}", u.cell());
    w.step_ai(1);
    let end = *w.unit(id).unwrap().path.last().expect("a new route");
    assert!(end.0 > (30 * CELL) as f32, "the new pick is the headquarters: {end:?}");
}

/// Missions 3 and 8 (escort) skip the strategy (`0x42f81d..0x42f828` -> `0x42f86e`): an idle unit goes for the nearest enemy
/// unit, the pods included, and only without one for the headquarters, even when the strategy says headquarters.
#[test]
fn in_the_escort_missions_the_computer_goes_for_the_pods_not_the_headquarters() {
    let run = |mission: Option<u32>, custom: bool| {
        let mut w = open_world(48, 20);
        w.structures.clear();
        let mut hq = factory(Owner::Player, 13536, 100);
        hq.kind = ObjectKind::Headquarters;
        hq.cell = (40, 8);
        hq.slots.clear();
        w.add_structure(hq);
        w.add_unit(kinds::POD, 0, 4 * CELL + 16, 9 * CELL + 16);
        let id = w.add_unit(7, 1, 14 * CELL + 16, 9 * CELL + 16);
        w.mission_no = mission;
        if custom {
            // a custom map's escort plays on the skirmish path
            w.skirmish = true;
            w.set_convoy(vec![(4, 18)]);
        }
        w.ai_strategy = 1; // the headquarters
        w.ai_timer = 100_000;
        w.ai_strike = 100_000;
        w.tick = 1;
        w.step_ai(1);
        let u = w.unit(id).unwrap();
        assert!(u.chase.is_none(), "the order is one route, as in the exe");
        u.path.last().map(|p| p.0).expect("a route")
    };
    assert!(run(Some(3), false) < (14 * CELL) as f32, "mission 3: to the pod");
    assert!(run(Some(8), false) < (14 * CELL) as f32, "mission 8: to the pod");
    assert!(run(Some(5), false) > (30 * CELL) as f32, "another mission: strategy 1, the headquarters");
    assert!(run(None, true) < (14 * CELL) as f32, "a custom escort: to the pod as in missions 3 and 8");
}

#[test]
fn an_easy_computer_re_orders_idle_units_only_every_two_seconds() {
    let count = |d: Difficulty| {
        let mut w = open_world(30, 30);
        w.difficulty = d;
        w.add_unit(6, 0, 4 * CELL, 4 * CELL);
        let id = w.add_unit(7, 1, 25 * CELL, 25 * CELL);
        w.ai = true;
        w.ai_strategy = 3;
        w.ai_timer = 1000;
        w.tick = 1;
        w.step_ai(1);
        w.unit(id).is_some_and(|u| !u.path.is_empty())
    };
    assert!(count(Difficulty::Normal), "the exe orders an idle unit at once");
    assert!(!count(Difficulty::Easy), "an easy computer waits for its next think tick");
}

#[test]
fn mines_and_labs_earn_every_second_and_tech_levels_cost_research() {
    let mut w = open_world(16, 12);
    let mut mine = factory(Owner::Player, 13536, 105);
    mine.kind = ObjectKind::GoldMine;
    mine.cell = (2, 2);
    mine.active = 2;
    w.add_structure(mine);
    let mut f = factory(Owner::Player, 13536, 102);
    f.slots[0].max_level = 2;
    w.add_structure(f);
    w.add_unit(7, 0, 8 * CELL, 8 * CELL);
    w.add_unit(7, 1, 15 * CELL, 11 * CELL);
    for _ in 0..130 {
        w.step();
    }
    assert_eq!(w.credits[0], 6, "3 credits per second at two levels, paid on ticks 60 and 120");
    w.research[0] = 1200;
    assert!(w.tech_up(1, 0, 0));
    assert_eq!((w.structures[1].slots[0].kind, w.research[0]), (8, 700));
    assert!(w.tech_up(1, 0, 0));
    assert!(!w.tech_up(1, 0, 0), "level limit reached");
    assert_eq!(w.structures[1].slots[0].kind, 9);
    assert!(!w.place_turret(0, 3, (12, 7)), "turret 3 costs 600, only 6 credits");
    w.credits[0] = 700;
    assert!(!w.place_turret(0, 3, (14, 9)), "too far from every own building");
    w.no_build = vec![false; (w.cells.0 * w.cells.1) as usize];
    w.no_build[(7 * w.cells.0 + 12) as usize] = true;
    assert!(!w.place_turret(0, 3, (12, 7)), "the map's no-build layer refuses the cell");
    w.no_build.clear();
    assert!(w.place_turret(0, 3, (12, 7)));
    assert_eq!(w.credits[0], 100);
    for _ in 0..30 {
        w.step();
    }
    let t = w.structures.last().unwrap();
    assert_eq!((t.hp, t.max_hp), (1540, 1540), "healed in at 150 per tick");
}

#[test]
fn an_air_base_fills_the_stock_and_a_bomber_strike_hits_a_turret() {
    let mut w = open_world(40, 12);
    w.tutorial_step = Some(1); // fast production: 44 s per air strike
    let mut base = factory(Owner::Player, 13536, 104);
    base.kind = ObjectKind::AirBase;
    base.slots = vec![SlotState { kind: 30, progress: 0, target: 4000, level: 0, max_level: 0, waiting: 0, unlock: 0 }];
    w.add_structure(base);
    w.add_unit(7, 0, 8 * CELL, 8 * CELL);
    w.add_unit(7, 1, 39 * CELL, 11 * CELL);
    let mut t = factory(Owner::Enemy, 900, 25);
    t.kind = ObjectKind::Turret(1);
    t.slots.clear();
    t.active = 0;
    t.cell = (20, 5);
    w.add_structure(t);
    for _ in 0..2700 {
        w.step();
    }
    assert_eq!(w.air_stock[0][0], 1, "one bomber after 44 s");
    assert!(w.launch_strike(0, AirForce::Bomber, 20 * CELL + 16, 5 * CELL + 16));
    assert!(w.player_sounds.contains(&68), "the local launch pings airforce-dest, as a priority sound");
    assert!(w.effects.iter().any(|e| e.spec.array == 0x5e5fb0), "and shows the cross_as marker at the target");
    assert_eq!(w.air_stock[0][0], 0);
    for _ in 0..1500 {
        w.step();
    }
    assert!(w.structures[1].hp <= 900 - 3 * 110, "the carpet bombing hit the turret (hp {})", w.structures[1].hp);
    assert!(w.aircraft.is_empty(), "the bomber left the map");
}

/// `0x41146d`: a bomb's speed grows by the steps it took on every tick, so a 200 px fall lasts about 2 s, not the 6.7 s a constant
/// 30 steps per second would take.
#[test]
fn a_bomb_falls_faster_and_faster() {
    let mut w = open_world(20, 12);
    let path = build_path(18, 5 * CELL, 0, 5 * CELL + 45, 200, 0).unwrap();
    w.bullets.push(Bullet::new(18, 110, 0, path));
    let mut ticks = 0;
    while !w.bullets.is_empty() && ticks < 1000 {
        w.step();
        ticks += 1;
    }
    assert!((100..160).contains(&ticks), "landed after {ticks} ticks");
}

/// `0x467f03` / `0x454da9`: a launch of the local player disables the air icons for 180 ticks; the enemy's launches do not.
#[test]
fn a_launch_locks_the_air_icons_for_three_seconds() {
    let mut w = open_world(20, 12);
    w.air_stock[0][0] = 2;
    w.air_stock[1][0] = 1;
    assert!(w.launch_strike(1, AirForce::Bomber, 8 * CELL, 4 * CELL));
    assert_eq!(w.strike_lock, 0, "the computer's strike does not lock the player's icons");
    assert!(w.launch_strike(0, AirForce::Bomber, 8 * CELL, 4 * CELL));
    assert_eq!(w.strike_lock, STRIKE_LOCK_TICKS);
    for _ in 0..STRIKE_LOCK_TICKS - 1 {
        w.step();
    }
    assert_eq!(w.strike_lock, 1);
    w.step();
    assert_eq!(w.strike_lock, 0);
}

fn turret_with_top(w: &mut World, cell: (i32, i32), hp: i32) -> usize {
    let mut t = factory(Owner::Enemy, hp, 25);
    t.kind = ObjectKind::Turret(1);
    t.slots.clear();
    t.active = 0;
    t.cell = cell;
    t.aim = TopAim::new(109, 0);
    w.add_structure(t);
    w.structures.len() - 1
}

/// `0x42f346..0x42f38d`: the gun top (a unit of kind 109..113) within 32 px redirects the blast's full strength to its turret,
/// on top of the cell test. A bomb on the turret's cell does 110 (cell) + 110 (top).
#[test]
fn a_blast_next_to_a_turrets_gun_top_damages_the_turret_twice() {
    let mut w = open_world(20, 12);
    let t = turret_with_top(&mut w, (10, 5), 900);
    w.explode(10 * CELL + 16, 5 * CELL + 16, 18, 110, 0);
    assert_eq!(w.structures[t].hp, 900 - 220);
    // 40 px away, in the neighbouring cell: nothing hits the cell and the top is out of reach
    w.explode(11 * CELL + 24, 5 * CELL + 16, 18, 110, 0);
    assert_eq!(w.structures[t].hp, 900 - 220);
    // 30 px away in the neighbouring cell: only the top
    w.explode(11 * CELL + 14, 5 * CELL + 16, 18, 110, 0);
    assert_eq!(w.structures[t].hp, 900 - 330);
    // a turret still building has no top: the cell test alone
    let u = turret_with_top(&mut w, (4, 5), 900);
    w.structures[u].heal = 100;
    w.explode(4 * CELL + 16, 5 * CELL + 16, 18, 110, 0);
    assert_eq!(w.structures[u].hp, 900 - 110);
}

#[test]
fn a_fighter_strike_damages_a_turret() {
    let mut w = open_world(40, 12);
    let t = turret_with_top(&mut w, (20, 5), 5000);
    w.air_stock[0][4] = 1;
    assert!(w.launch_strike(0, AirForce::Fighter, 20 * CELL + 16, 5 * CELL + 16));
    for _ in 0..600 {
        w.step();
    }
    // the missiles land 22 px left and right of the aim point: outside the 1x1 cell, so it is the gun top that takes them
    assert!(w.structures[t].hp <= 5000 - 4 * 150, "the fighter's missiles hurt the turret (hp {})", w.structures[t].hp);
}

/// The exe never stops a walking chassis because its weapon has a target: the unit loop updates the top (`0x479c60`) and the
/// chassis (`0x46f140`) every tick independently, and no weapon code clears the chassis orders (only `0x4706a0` and the chase
/// replan `0x46eb40` write them). A unit chasing a far target keeps walking while it shoots at whatever is in range.
#[test]
fn units_keep_walking_while_they_shoot() {
    let mut w = open_world(40, 14);
    w.ai = false;
    let a = w.add_unit(7, 0, 3 * CELL + 16, 5 * CELL + 16);
    w.add_unit(7, 1, 8 * CELL + 16, 5 * CELL + 16); // stands next to the route, in range
    let far = w.add_unit(7, 1, 36 * CELL + 16, 9 * CELL + 16);
    w.units.iter_mut().find(|u| u.id == a).unwrap().chase = Some(Target::Unit(far));
    let (mut shooting, mut moving_and_shooting) = (0, 0);
    let mut last = w.unit(a).unwrap().x;
    for _ in 0..600 {
        w.step();
        let Some(u) = w.unit(a) else { break };
        let moved = (u.x - last).abs() > 0.01;
        last = u.x;
        if u.target.is_some() {
            shooting += 1;
            if moved {
                moving_and_shooting += 1;
            }
        }
    }
    assert!(shooting > 20, "the unit shot ({shooting} ticks with a target)");
    // a walker only moves on some ticks (7..30 px/s of 60 ticks), so compare with the walking speed instead of the ticks
    assert!(moving_and_shooting > 40, "it kept walking while shooting ({moving_and_shooting} of {shooting} ticks)");
    assert!(w.unit(a).is_none_or(|u| u.x > 8.0 * CELL as f32), "it walked past the enemy it was shooting at");
}

#[test]
fn convoy_pods_take_no_player_orders_and_walk_slowly() {
    let mut w = open_world(30, 12);
    w.set_mission(Some(3));
    let pod = w.add_unit(kinds::POD, 0, 8 * CELL + 16, 5 * CELL + 16);
    let tank = w.add_unit(7, 0, 9 * CELL + 16, 5 * CELL + 16);
    w.units.iter_mut().for_each(|u| u.selected = true);
    w.order_move_selected(20 * CELL, 5 * CELL);
    assert!(w.unit(pod).unwrap().path.is_empty(), "no go-to order for the pod");
    assert!(!w.unit(tank).unwrap().path.is_empty(), "other units still obey");
    assert_eq!(w.walk_params_of(46).unwrap().speed, 7);
    w.set_mission(Some(1));
    assert_eq!(w.walk_params_of(46).unwrap().speed, 30);
}

#[test]
fn a_rogon_bomb_blows_up_on_its_target_and_damages_around_it() {
    let mut w = open_world(30, 12);
    let victim = w.add_unit(7, 1, 12 * CELL + 16, 5 * CELL + 16);
    let near = w.add_unit(7, 1, 13 * CELL + 16, 5 * CELL + 16);
    let far = w.add_unit(7, 1, 25 * CELL + 16, 5 * CELL + 16);
    w.add_unit(7, 0, 2 * CELL, 2 * CELL);
    let pod = w.add_unit(kinds::POD, 0, 8 * CELL + 16, 5 * CELL + 16);
    w.units.iter_mut().find(|u| u.id == pod).unwrap().chase = Some(Target::Unit(victim));
    for _ in 0..900 {
        w.step();
    }
    assert!(w.unit(pod).is_none(), "the bomb is gone");
    assert!(w.unit(victim).is_none(), "the target died");
    assert!(w.unit(near).is_none(), "a unit next to the blast died too");
    assert!(w.unit(far).is_some(), "far away units survive");
    assert!(w.nukes.is_empty());
}

#[test]
fn dev_tools_place_turrets_and_buildings_anywhere_but_not_on_each_other() {
    let mut w = open_world(30, 12);
    assert!(w.dev_place_structure(29, 1, (5, 5)), "a turret far from any own building");
    let t = w.structures.last().unwrap();
    assert!(t.alive() && t.hp == t.max_hp && t.aim.is_some() && t.owner == Owner::Enemy);
    assert!(!w.dev_place_structure(103, 0, (3, 4)), "overlaps the turret (footprint 5 x 4 from (3, 4))");
    assert!(w.dev_place_structure(103, 0, (10, 4)));
    assert!(w.structures.last().unwrap().covers((14, 7)));
    assert!(!w.dev_place_structure(100, 0, (28, 10)), "leaves the map");
    assert!(!w.dev_place_structure(40, 0, (1, 1)), "not a structure kind");
}

#[test]
fn volley_scatter_stays_inside_the_weapons_spread_and_is_not_constant() {
    let mut w = open_world(30, 12);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..200 {
        let (jx, jy) = w.volley_scatter(113);
        assert!(jx.abs() < 20 && jy.abs() < 20);
        seen.insert((jx, jy));
        assert_eq!(w.volley_scatter(15), (0, 0), "spread 1 never moves the end point");
    }
    assert!(seen.len() > 20, "the turret's six rounds land on different points");
}

#[test]
fn a_rogon_bomb_never_touches_a_walker_and_a_dying_pod_leaves_no_wreck() {
    let mut w = open_world(30, 12);
    let walker = w.add_unit(41, 1, 12 * CELL + 16, 5 * CELL + 16);
    w.add_unit(7, 0, 2 * CELL, 2 * CELL);
    let pod = w.add_unit(kinds::POD, 0, 8 * CELL + 16, 5 * CELL + 16);
    w.units.iter_mut().find(|u| u.id == pod).unwrap().chase = Some(Target::Unit(walker));
    for _ in 0..600 {
        w.step();
    }
    assert!(w.unit(pod).is_some(), "the walker's handle is in the other grid: no touch");
    assert!(w.unit(walker).is_some());
    // a pod shot down explodes too, without the generic death clouds. The walker goes first: the blasts would kill it, and green's
    // unit turn comes after blue's bullet pass in the same tick, so its own death clouds would land in this count
    w.units.retain(|u| u.id != walker);
    let before = w.effects.len();
    w.units.iter_mut().find(|u| u.id == pod).unwrap().hp = 0;
    w.step();
    assert_eq!(w.nukes.len(), 1);
    assert!(w.effects.len() <= before + 21, "only blast sprites, no death clouds");
    // each blast is an e_unit2 cloud jittered by up to 16 px, not a fixed blast sprite on the ring cell (0x4113b0..0x4113f1)
    let fresh: Vec<_> = w.effects.iter().skip(before).collect();
    assert!(fresh.iter().all(|e| e.spec == UNIT_CLOUD));
    assert!(fresh.iter().any(|e| (e.x - 16).rem_euclid(CELL) != 0 || (e.y - 16).rem_euclid(CELL) != 0), "off the cell grid");
}
/// The unit's line is the exe's pixel line (`0x46ee80`) kept as a list of points; the incremental Bresenham the sim used before
/// (error term `err += minor`, minor step when `2*err >= major`) walks exactly the same pixels.
#[test]
fn the_stored_pixel_line_is_the_incremental_bresenham() {
    let mut seed = 12345u32;
    let mut rnd = |m: i32| {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        ((seed >> 8) % m as u32) as i32
    };
    for _ in 0..2000 {
        let (x0, y0, x1, y1) = (rnd(300), rnd(300), rnd(300), rnd(300));
        let pts = opensow_formats::walk::pixel_line(x0, y0, x1, y1);
        let (dx0, dy0) = (x1 - x0, y1 - y0);
        let (major, minor) = (dx0.abs().max(dy0.abs()), dx0.abs().min(dy0.abs()));
        let (mut x, mut y, mut err) = (x0, y0, 0);
        let mut walked = vec![(x, y)];
        while (x, y) != (x1, y1) {
            let (dx, dy) = (x1 - x, y1 - y);
            err += minor;
            let minor_step = 2 * err >= major && minor > 0;
            if minor_step {
                err -= major;
            }
            let (sx, sy) = (dx.signum(), dy.signum());
            if dx.abs() >= dy.abs() {
                x += sx;
                if minor_step {
                    y += sy;
                }
            } else {
                y += sy;
                if minor_step {
                    x += sx;
                }
            }
            walked.push((x, y));
        }
        assert_eq!(walked, pts, "({x0},{y0}) -> ({x1},{y1})");
    }
}

#[test]
fn a_custom_map_sets_unlock_points_headquarters_without_advancers_and_neutral_start_health() {
    use opensow_formats::custommap::{CustomMap, ObjectExt, Terrain};
    use opensow_formats::level::Object;
    use opensow_formats::map::{Cell, Map};
    let (mw, mh) = (40u32, 40u32);
    let map = Map {
        a: 0,
        b: 0,
        width: mw,
        height: mh,
        cells: (0..mw * mh).map(|i| Cell { x: (i % mw) as u8, y: (i / mw) as u8, blocked: false, border: false, no_build: false, no_marks: false }).collect(),
    };
    let obj = |kind: i32, x: i32, y: i32, owner: i32, pct: i32| {
        let mut raw = [0i32; 19];
        (raw[0], raw[1], raw[14], raw[16], raw[17], raw[18]) = (123, kind, owner, x, y, pct);
        Object { kind, cell_x: x, cell_y: y, raw }
    };
    let mut m = CustomMap::new("t", Terrain { warmonger: false, mission: 1 });
    let mut factory = obj(102, 20, 20, 0, 100);
    opensow_formats::maptweak::Production { active: 1, lines: vec![(7, 0), (13, 0)] }.write(&mut factory);
    m.level.objects = vec![obj(100, 2, 2, 0, 100), obj(100, 10, 2, 0, 100), obj(100, 30, 30, 1, 100), factory, obj(105, 2, 30, 2, 80)];
    m.ext = vec![ObjectExt::default(); 5];
    m.ext[0].no_advancer = true;
    m.ext[3].unlock = [0, 600, 0, 0, 0];
    let back = CustomMap::parse(&m.to_bytes()).unwrap();
    let mut w = World::from_level(&map, &back.level);
    w.apply_custom(&back.ext);
    w.neutral_start_hp_from_level(&back.level);
    assert_eq!(w.units.iter().filter(|u| u.kind == kinds::ADVANCER && u.player == 0).count(), 1, "one blue Advancer: the second HQ's");
    assert!(w.structures[0].advancer.is_none() && w.structures[1].advancer.is_some());
    let mine = &w.structures[4];
    assert_eq!(mine.hp, mine.max_hp * 80 / 100, "the neutral mine starts at its 80 %");
    let f = &w.structures[3];
    assert_eq!((f.slots[1].unlock_points(), f.slots[0].unlock_points()), (600, f.slots[0].target));
    // the HQ without an Advancer never builds one
    for _ in 0..5 {
        w.step_hq();
    }
    assert!(w.structures[0].advancer.is_none());
    assert_eq!(w.units.iter().filter(|u| u.kind == kinds::ADVANCER).count(), 2);
}

/// Key X (`0x418dc0`, `formats::scatter`): each unit walks 6 half cells past its own spot, away from the middle of the box; a
/// blocked cell makes it try 4, then 2.
#[test]
fn scatter_sends_the_selection_away_from_its_middle() {
    let mut w = open_world(30, 30);
    let ids: Vec<u32> = [(10, 10), (12, 10), (11, 12)].iter().map(|&(x, y)| w.add_unit(7, 0, x * CELL + 16, y * CELL + 16)).collect();
    assert!(ids.iter().all(|&id| w.unit(id).unwrap().is_idle()));
    let dests = |w: &mut World| {
        for u in w.units.iter_mut() {
            u.selected = true;
        }
        assert!(w.scatter_selected());
        ids.iter().map(|&id| w.unit(id).unwrap().dest).collect::<Vec<_>>()
    };
    // box middle = half cell (23, 23): up-left (15, 15) -> cell (7, 7), up-right (31, 15) -> (15, 7), straight down (23, 31) -> (11, 15)
    let c = |x: i32, y: i32| Some((x * CELL + 16, y * CELL + 16));
    assert_eq!(dests(&mut w), vec![c(7, 7), c(15, 7), c(11, 15)]);
    assert!(ids.iter().all(|&id| !w.unit(id).unwrap().is_idle()));
    // a blocked cell: the next try is 4 half cells, (17, 17) -> cell (8, 8)
    let mut w2 = open_world(30, 30);
    for &(x, y) in &[(10, 10), (12, 10), (11, 12)] {
        w2.add_unit(7, 0, x * CELL + 16, y * CELL + 16);
    }
    let i = (7 * 30 + 7) as usize;
    w2.blocked[i] = true;
    let ids2: Vec<u32> = w2.units.iter().map(|u| u.id).collect();
    for u in w2.units.iter_mut() {
        u.selected = true;
    }
    assert!(w2.scatter_selected());
    assert_eq!(w2.unit(ids2[0]).unwrap().dest, c(8, 8));
}

/// A range-1 walker (kind 45) sent at a building or a turret from any side stands still beside it and keeps it as its target:
/// the scan reaches the footprint cells (the exe's 7-cell neighbour list for range 1, `0x47b1ad`), and the chase does not keep
/// ordering it into the building (its legs used to walk on the spot; turret 1 was out of reach on a diagonal).
#[test]
fn a_range_one_walker_stands_still_and_shoots_buildings_and_turrets_from_every_side() {
    for kid in [25, 26, 28, 100, 101, 102, 104, 105, 108] {
        for (dx, dy) in [(-8, 0), (8, 0), (0, -8), (0, 8), (-7, -7), (7, -7), (-7, 7), (7, 7)] {
            let mut w = open_world(40, 40);
            let a = w.add_unit(45, 0, (15 + dx) * CELL + 16, (15 + dy) * CELL + 16);
            w.add_structure(Structure {
                object: 0,
                kind: ObjectKind::LightFactory,
                owner: Owner::Enemy,
                cell: (14, 14),
                hp: 400000,
                max_hp: 400000,
                aim: None,
                weapon: 0,
                target: None,
                slots: Vec::new(),
                active: 0,
                kind_id: kid,
                tally: [0; 3],
                heal: 0,
                upgrade: 0,
                protected: false,
                attacked: false,
                sat_cd: 0,
                advancer: None,
                rally: None,
                dying: None,
            });
            w.cheats.freeze_enemy = true;
            w.units.iter_mut().find(|u| u.id == a).unwrap().selected = true;
            w.order_attack_selected(Target::Structure(0));
            for _ in 0..1800 {
                w.step();
            }
            let phase = w.unit(a).unwrap().chassis.as_ref().unwrap().phase;
            for t in 0..600 {
                w.step();
                let u = w.unit(a).unwrap();
                assert_eq!(u.target, Some(Target::Structure(0)), "kind {kid} from ({dx},{dy}), tick {t}: the walker shoots it");
                assert!(!u.moved && u.chassis.as_ref().unwrap().phase == phase, "kind {kid} from ({dx},{dy}), tick {t}: legs still");
            }
        }
    }
}
