//! Tests of the map editor: each drives a `MapEditor` with clicks and keys on a temporary maps folder.

use super::*;

fn editor_with_map() -> Option<(MapEditor, Assets, PathBuf)> {
    let mut assets = opensow_formats::GameDir::locate().and_then(|g| Assets::load(g).ok())?;
    let mut m = MapEditor::new(&mut assets);
    m.create(0).ok()?;
    // one folder per test: tests run in parallel and each removes its own folder
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("opensow-editor-test-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // never the player's real maps folder: tests may save and delete
    m.folder = dir.clone();
    m.rescan();
    Some((m, assets, dir))
}

/// A playable demo map (`MapEditor::demo`) named `name`, saved in `dir` (other modules' tests); `None` without the install.
pub(crate) fn demo_map_file(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    let (mut m, _assets, own) = editor_with_map()?;
    let _ = std::fs::remove_dir_all(own);
    populate(&mut m);
    m.s.as_mut()?.name = TextField::new(name, custommap::MAX_NAME);
    m.save_in(dir).ok()
}

/// A generated map (`generated`) saved in `dir`; `None` without the install.
pub(crate) fn generated_map_file(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    let (mut m, _assets, own) = editor_with_map()?;
    let _ = std::fs::remove_dir_all(own);
    generated(&mut m, name).ok()?;
    // a player's headquarters on each start plateau
    assert!(m.arm_tool("hq") && m.place_at((6, 4), Side::Blue) && m.place_at((8, 57), Side::Green));
    m.save_in(dir).ok()
}

/// Both sides' headquarters, neutral buildings (a medium factory among them), turrets and units.
fn populate(m: &mut MapEditor) {
    assert!(m.demo());
    let l = m.s.as_ref().unwrap().ed.level();
    assert!(l.objects.iter().any(|o| o.kind == kinds::MEDIUM_FACTORY) && l.objects.iter().filter(|o| o.kind == kinds::HEADQUARTERS).count() == 2);
}

#[test]
fn controls_stay_inside_the_window_and_apart_at_every_size() {
    let Some((mut m, _a, _d)) = editor_with_map() else { return };
    populate(&mut m);
    let factory = m.s.as_ref().unwrap().ed.level().objects.iter().position(|o| o.kind == kinds::MEDIUM_FACTORY).unwrap();
    // nothing selected with each of the three cards up front, a headquarters, a factory
    for (sel, content) in [(None, 0), (None, 1), (None, 2), (Some(Sel::Object(0)), 0), (Some(Sel::Object(factory)), 0)] {
        m.s.as_mut().unwrap().ed.sel = sel;
        m.s.as_mut().unwrap().content = content;
        for (w, h) in [(640, 480), (800, 600), (1024, 768), (1280, 720), (1920, 1080), (2560, 1440)] {
            let controls = m.controls(w, h);
            assert!(controls.len() > 10);
            for (r, hit) in &controls {
                assert!(page::inside(*r, w, h) || r.y + r.h > h as i32, "{hit:?} at {w}x{h}: {r:?} leaves the window");
                if r.y + r.h > h as i32 {
                    continue; // below the fold: the sidebar scrolls
                }
                let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
                assert_eq!(m.hit(x, y, w, h), Some(*hit), "{hit:?} at {w}x{h} is covered");
            }
            for (i, (a, ha)) in controls.iter().enumerate() {
                for (b, hb) in &controls[i + 1..] {
                    let nested = matches!((ha, hb), (Hit::Palette(_), Hit::Palette(_)));
                    assert!(nested || !page::overlaps(*a, *b), "{ha:?} {a:?} overlaps {hb:?} {b:?} at {w}x{h}");
                }
            }
        }
    }
    // the start page too
    m.s = None;
    for (w, h) in [(640, 480), (1024, 768), (1920, 1080), (2560, 1080)] {
        for (r, hit) in m.controls(w, h) {
            assert!(page::inside(r, w, h), "{hit:?} at {w}x{h}: {r:?}");
        }
        assert!(m.controls(w, h).iter().any(|(_, h)| *h == Hit::Create));
    }
}

#[test]
fn the_mission_card_sets_the_countdown_and_the_win_rule() {
    use custommap::{Mission, Timer, TimerEnd, WinRule};
    let Some((mut m, _a, dir)) = editor_with_map() else { return };
    populate(&mut m);
    m.save_in(&dir).expect("saved");
    let timer = |m: &MapEditor| Timer::read(m.s.as_ref().unwrap().ed.level());
    // the card is reachable at every size, behind its chip where the sidebar is short
    for (w, h) in [(640, 480), (1024, 768), (1920, 1080), (2560, 1440)] {
        m.press(Hit::Content(0));
        assert!(m.controls(w, h).iter().any(|(_, h)| *h == Hit::Content(2)) || m.controls(w, h).iter().any(|(_, h)| *h == Hit::WinRule(1)), "{w}x{h}");
        m.press(Hit::Content(2));
        for hit in [Hit::Countdown(2), Hit::WinRule(1)] {
            assert!(m.controls(w, h).iter().any(|(_, h)| *h == hit), "{hit:?} at {w}x{h}");
        }
    }
    assert_eq!((timer(&m), m.s.as_ref().unwrap().mission), (None, Mission::default()));
    assert!(!m.controls(1920, 1080).iter().any(|(_, h)| matches!(h, Hit::Minutes(_))), "no minutes without a countdown");
    assert!(!m.s.as_ref().unwrap().dirty());
    m.press(Hit::Countdown(1));
    assert_eq!(timer(&m), Some(Timer { minutes: DEFAULT_MINUTES, end: TimerEnd::Victory }));
    assert!(m.s.as_ref().unwrap().dirty());
    m.press(Hit::Minutes(true));
    m.press(Hit::Countdown(2));
    assert_eq!(timer(&m), Some(Timer { minutes: 25, end: TimerEnd::Defeat }), "the minutes stay when the mode changes");
    // reinforcements: what arrives and where the carryalls land
    assert!(!m.controls(1920, 1080).iter().any(|(_, h)| *h == Hit::DropAdd));
    m.press(Hit::Countdown(3));
    assert_eq!(timer(&m).unwrap().end, TimerEnd::Reinforcements);
    for (w, h) in [(640, 480), (1024, 768), (1920, 1080), (2560, 1440)] {
        // every control exists; the short windows reach the lower ones with the wheel
        m.s.as_mut().unwrap().scroll = 0;
        let mut seen: Vec<Hit> = m.controls(w, h).into_iter().filter(|(r, _)| r.y + r.h <= h as i32).map(|(_, h)| h).collect();
        m.cursor = (w as i32 - 20, h as i32 / 2);
        m.wheel(-100.0, w, h);
        seen.extend(m.controls(w, h).into_iter().filter(|(r, _)| r.y + r.h <= h as i32).map(|(_, h)| h));
        for hit in [Hit::RCredits(true), Hit::RResearch(false), Hit::RAir(4, true), Hit::DropAdd, Hit::WinRule(1)] {
            assert!(seen.contains(&hit), "{hit:?} at {w}x{h}");
        }
    }
    m.s.as_mut().unwrap().scroll = 0;
    m.press(Hit::RCredits(true));
    m.press(Hit::RResearch(true));
    m.press(Hit::RResearch(false));
    m.press(Hit::RAir(0, true));
    m.press(Hit::RAir(0, true));
    m.press(Hit::DropAdd);
    assert_eq!(m.s.as_ref().unwrap().tool, Some(MapTool::Landing));
    // clicks on the map: a cell where no whole squad lands (water, cliffs, a building) or that overlaps another point is
    // refused; good ones are added until the list is full
    let (w, h) = (1920, 1080);
    m.draw(&mut vec![0; w * h], w, h);
    let click = |m: &mut MapEditor, c: (i32, i32)| {
        let v = m.s.as_ref().unwrap().view;
        let (x, y) = (((c.0 * 32 + 16) as f32 - v.x) * v.zoom, ((c.1 * 32 + 16) as f32 - v.y) * v.zoom);
        m.mouse_button(true, true, x as i32, y as i32, w, h);
        m.mouse_button(true, false, x as i32, y as i32, w, h);
    };
    let s = m.s.as_ref().unwrap();
    let (cw, chh) = ((s.scene.bg_w / 32) as i32, (s.scene.bg_h / 32) as i32);
    // cells whose centre is on the map part of the window, clear of the button in its top-left corner
    let v = s.view;
    let on_screen = |c: &(i32, i32)| {
        let (sx, sy) = (((c.0 * 32 + 16) as f32 - v.x) * v.zoom, ((c.1 * 32 + 16) as f32 - v.y) * v.zoom);
        sx > 10.0 && sy > 60.0 && (sx as usize) < map_width(w, h) - 10 && (sy as usize) < h - 10
    };
    let all: Vec<(i32, i32)> = (1..chh - 2).flat_map(|y| (2..cw - 3).map(move |x| (x, y))).filter(on_screen).collect();
    let wet = *all
        .iter()
        .find(|&&c| s.ed.can_land(c).is_err() && s.ed.pick((c.0 * 32 + 16) as f32, (c.1 * 32 + 16) as f32).is_none())
        .expect("the terrain has blocked ground");
    let hq = s.ed.level().objects.iter().find(|o| o.kind == kinds::HEADQUARTERS).map(|o| (o.cell_x + 1, o.cell_y + 1)).unwrap();
    for bad in [wet, hq] {
        click(&mut m, bad);
        assert!(m.s.as_ref().unwrap().reinforcement.drops.is_empty(), "{bad:?} refused");
        assert!(m.s.as_ref().unwrap().message.as_ref().is_some_and(|(_, c)| *c == WARN));
    }
    let mut good: Vec<(i32, i32)> = Vec::new();
    for &c in &all {
        let s = m.s.as_ref().unwrap();
        // off the other points and off any unit (a click on one selects it)
        if good.len() < custommap::MAX_DROPS
            && s.ed.can_land(c).is_ok()
            && s.ed.pick((c.0 * 32 + 16) as f32, (c.1 * 32 + 16) as f32).is_none()
            && !good.iter().any(|&g| custommap::landings_overlap(g, c))
        {
            good.push(c);
        }
    }
    assert_eq!(good.len(), custommap::MAX_DROPS);
    click(&mut m, good[0]);
    click(&mut m, (good[0].0 + 1, good[0].1 + 1));
    assert_eq!(m.s.as_ref().unwrap().reinforcement.drops.len(), 1, "an overlapping point is refused");
    for &c in &good[1..] {
        click(&mut m, c);
    }
    click(&mut m, (good[0].0 + 8, good[0].1));
    let f = m.s.as_ref().unwrap().reinforcement.clone();
    assert_eq!((f.credits, f.research, f.air[AIR_GROUP[0]], f.drops.len()), (500, 0, 2, custommap::MAX_DROPS));
    assert_eq!(f.drops.iter().map(|d| d.cell).collect::<Vec<_>>(), good);
    assert_eq!(f.drops.iter().map(|d| d.squad).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4, 5], "each new point the next squad");
    assert_eq!(m.s.as_ref().unwrap().tool, None, "full: the tool lets go");
    assert!(landing_issues(m.s.as_ref().unwrap()).is_empty());
    // the squad wraps around; a point is deleted by its own cross
    m.press(Hit::DropSquad(0, false));
    m.press(Hit::DropSquad(1, true));
    m.press(Hit::DropDelete(2));
    for (wd, hd) in [(640, 480), (1920, 1080)] {
        m.s.as_mut().unwrap().scroll = 0;
        m.cursor = (wd as i32 - 20, hd as i32 / 2);
        m.wheel(-100.0, wd, hd);
        assert!(m.controls(wd, hd).iter().any(|(_, h)| *h == Hit::DropDelete(4)), "{wd}x{hd}");
    }
    m.s.as_mut().unwrap().scroll = 0;
    let saved = custom_maps::load(&m.save_in(&dir).unwrap()).unwrap();
    let want: Vec<custommap::Landing> =
        [(0, 17), (1, 2), (3, 3), (4, 4), (5, 5)].iter().map(|&(i, q)| custommap::Landing { cell: good[i], squad: q }).collect();
    assert_eq!(saved.reinforcement.drops, want);
    // a building placed over a landing point afterwards: the checks say so
    {
        let s = m.s.as_mut().unwrap();
        let c = s.reinforcement.drops[0].cell;
        s.ed.sel = None;
        let i = editor::palette().iter().position(|&t| t == Thing::Object(105)).unwrap();
        s.ed.palette = i;
        s.ed.side = Side::Neutral;
        if s.ed.add(Editor::add_cell(Thing::Object(105), c)) {
            assert_eq!(landing_issues(s), vec![(0, LandingWhy::Ground)]);
            s.ed.undo();
        }
        assert!(landing_issues(s).is_empty());
    }
    m.open_file(&m.s.as_ref().unwrap().path.clone().unwrap()).unwrap();
    assert_eq!(m.s.as_ref().unwrap().reinforcement, saved.reinforcement);
    m.press(Hit::Countdown(2));
    for _ in 0..30 {
        m.press(Hit::Minutes(true));
    }
    assert_eq!(timer(&m).unwrap().minutes, MAX_MINUTES);
    for _ in 0..30 {
        m.press(Hit::Minutes(false));
    }
    assert_eq!(timer(&m).unwrap().minutes, MINUTE_STEP);
    // the countdown is part of the level: undo takes it back
    m.press(Hit::Undo);
    assert_eq!(timer(&m).unwrap().minutes, 2 * MINUTE_STEP);
    m.press(Hit::WinRule(1));
    let path = m.save_in(&dir).expect("saved");
    let back = custom_maps::load(&path).unwrap();
    assert_eq!(back.mission, Mission { win: WinRule::Everything });
    assert_eq!(back.timer(), Some(Timer { minutes: 10, end: TimerEnd::Defeat }));
    // reopened, the card shows the same; None takes the countdown out of the file again
    m.open_file(&path).unwrap();
    assert_eq!(m.s.as_ref().unwrap().mission, back.mission);
    m.press(Hit::Countdown(0));
    m.press(Hit::WinRule(0));
    let plain = custom_maps::load(&m.save_in(&dir).unwrap()).unwrap();
    assert_eq!((plain.timer(), plain.mission, plain.level.header[66]), (None, Mission::default(), 0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_escort_takes_pods_a_goal_and_waypoints_and_checks_the_way() {
    use custommap::{Problem, WinRule};
    let Some((mut m, _a, dir)) = editor_with_map() else { return };
    populate(&mut m);
    m.press(Hit::Content(2));
    m.press(Hit::WinRule(2));
    assert_eq!(m.s.as_ref().unwrap().mission.win, WinRule::Escort);
    let problems = |m: &MapEditor| m.s.as_ref().unwrap().map().problems();
    assert!(problems(&m).contains(&Problem::NoPods) && problems(&m).contains(&Problem::NoGoal));
    for (w, h) in [(640, 480), (1920, 1080)] {
        m.s.as_mut().unwrap().scroll = 0;
        let mut seen: Vec<Hit> = m.controls(w, h).into_iter().filter(|(r, _)| r.y + r.h <= h as i32).map(|(_, h)| h).collect();
        m.cursor = (w as i32 - 20, h as i32 / 2);
        m.wheel(-100.0, w, h);
        seen.extend(m.controls(w, h).into_iter().filter(|(r, _)| r.y + r.h <= h as i32).map(|(_, h)| h));
        for hit in [Hit::WinRule(2), Hit::GoalSet, Hit::WaypointAdd] {
            assert!(seen.contains(&hit), "{hit:?} at {w}x{h}");
        }
    }
    m.s.as_mut().unwrap().scroll = 0;
    let (w, h) = (1920, 1080);
    m.draw(&mut vec![0; w * h], w, h);
    let s = m.s.as_ref().unwrap();
    let (grid, size) = (s.ed.walkable(), s.ed.grid_size());
    let v = s.view;
    let on_screen = |c: (i32, i32)| {
        let (sx, sy) = (((c.0 * 32 + 16) as f32 - v.x) * v.zoom, ((c.1 * 32 + 16) as f32 - v.y) * v.zoom);
        sx > 10.0 && sy > 60.0 && (sx as usize) < map_width(w, h) - 10 && (sy as usize) < h - 10
    };
    let cells: Vec<(i32, i32)> = (2..size.1 - 2)
        .flat_map(|y| (2..size.0 - 2).map(move |x| (x, y)))
        .filter(|&c| on_screen(c) && s.ed.pick((c.0 * 32 + 16) as f32, (c.1 * 32 + 16) as f32).is_none())
        .collect();
    // a blue pod on open ground, a goal and a waypoint it can walk to
    let start = *cells.iter().find(|&&c| grid[(c.1 * size.0 + c.0) as usize]).unwrap();
    let reach: Vec<(i32, i32)> =
        cells.iter().copied().filter(|&c| c != start && (c.0 - start.0).abs() + (c.1 - start.1).abs() > 10 && walks(&grid, size, start, c)).collect();
    let blocked = *cells.iter().find(|&&c| !grid[(c.1 * size.0 + c.0) as usize]).unwrap();
    {
        let s = m.s.as_mut().unwrap();
        s.ed.side = Side::Blue;
        s.ed.palette = editor::palette().iter().position(|&t| t == Thing::Unit(custommap::POD)).unwrap();
        assert!(s.ed.add(Editor::add_cell(Thing::Unit(custommap::POD), start)));
        s.ed.sel = None;
    }
    assert_eq!(problems(&m), vec![Problem::NoGoal], "one pod is a convoy");
    let click = |m: &mut MapEditor, c: (i32, i32)| {
        let v = m.s.as_ref().unwrap().view;
        let (x, y) = (((c.0 * 32 + 16) as f32 - v.x) * v.zoom, ((c.1 * 32 + 16) as f32 - v.y) * v.zoom);
        m.mouse_button(true, true, x as i32, y as i32, w, h);
        m.mouse_button(true, false, x as i32, y as i32, w, h);
    };
    m.press(Hit::GoalSet);
    click(&mut m, blocked);
    assert_eq!(m.s.as_ref().unwrap().escort.goal, None, "blocked ground is refused");
    click(&mut m, reach[reach.len() - 1]);
    assert_eq!(m.s.as_ref().unwrap().escort.goal, Some(reach[reach.len() - 1]));
    assert_eq!(m.s.as_ref().unwrap().tool, None, "one goal: the tool lets go");
    m.press(Hit::WaypointAdd);
    click(&mut m, reach[0]);
    click(&mut m, reach[reach.len() / 2]);
    assert_eq!(m.s.as_ref().unwrap().escort.route, vec![reach[0], reach[reach.len() / 2]]);
    assert_eq!(m.s.as_ref().unwrap().tool, Some(MapTool::Waypoint), "waypoints keep coming");
    m.press(Hit::WaypointRemove);
    assert_eq!(problems(&m), vec![]);
    assert!(escort_issues(m.s.as_ref().unwrap()).is_empty());
    // a goal the pods cannot walk to (an island, a plateau) is reported, not refused
    let island = cells.iter().copied().find(|&c| grid[(c.1 * size.0 + c.0) as usize] && !walks(&grid, size, start, c));
    if let Some(c) = island {
        m.s.as_mut().unwrap().escort.goal = Some(c);
        assert_eq!(escort_issues(m.s.as_ref().unwrap()), vec![None]);
        m.s.as_mut().unwrap().escort.route.push(c);
        assert_eq!(escort_issues(m.s.as_ref().unwrap()), vec![Some(2)], "the leg to the island waypoint; from there to the goal is fine");
        m.s.as_mut().unwrap().escort.route.pop();
        m.s.as_mut().unwrap().escort.goal = Some(reach[reach.len() - 1]);
    }
    // blue's last headquarters may go on an escort map (not green's)
    let hq =
        |m: &MapEditor, side: i32| m.s.as_ref().unwrap().ed.level().objects.iter().position(|o| o.kind == kinds::HEADQUARTERS && o.raw[14] == side).unwrap();
    m.s.as_mut().unwrap().ed.sel = Some(Sel::Object(hq(&m, 1)));
    m.press(Hit::Delete);
    assert!(m.s.as_ref().unwrap().ed.level().objects.iter().any(|o| o.kind == kinds::HEADQUARTERS && o.raw[14] == 1), "green keeps its last one");
    m.s.as_mut().unwrap().ed.sel = Some(Sel::Object(hq(&m, 0)));
    m.press(Hit::Delete);
    assert!(!m.s.as_ref().unwrap().ed.level().objects.iter().any(|o| o.kind == kinds::HEADQUARTERS && o.raw[14] == 0));
    assert_eq!(problems(&m), vec![], "no blue headquarters is fine for an escort");
    m.press(Hit::Undo);
    // saved and read back
    m.s.as_mut().unwrap().name = TextField::new("Escort test", custommap::MAX_NAME);
    let back = custom_maps::load(&m.save_in(&dir).unwrap()).unwrap();
    assert_eq!((back.mission.win, back.escort.goal, back.escort.route.clone()), (WinRule::Escort, Some(reach[reach.len() - 1]), vec![reach[0]]));
    assert_eq!(custommap::pods(&back.level, 0), 1);
    // another rule lets the tools go
    m.press(Hit::GoalSet);
    m.press(Hit::WinRule(0));
    assert_eq!(m.s.as_ref().unwrap().tool, None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_map_is_made_saved_listed_and_reopened() {
    let Some((mut m, _a, dir)) = editor_with_map() else { return };
    populate(&mut m);
    let s = m.s.as_mut().unwrap();
    s.name = TextField::new("Žltá púšť", custommap::MAX_NAME);
    assert!(s.dirty());
    // sides: green gets more credits, blue no Advancer
    m.press(Hit::Credits(1, true));
    // blue's headquarters: its Advancer off and on again (the only switch)
    let hq = m.s.as_ref().unwrap().ed.level().objects.iter().position(|o| o.kind == kinds::HEADQUARTERS && o.raw[14] == 0).unwrap();
    m.s.as_mut().unwrap().ed.sel = Some(Sel::Object(hq));
    m.press(Hit::HqAdvancer(false));
    assert_eq!(custommap::advancers(m.s.as_ref().unwrap().ed.level(), &m.s.as_ref().unwrap().ed.ext, 0), 0);
    m.press(Hit::HqAdvancer(true));
    // the factory: lock all its lines, set the second's upgrade points
    let i = m.s.as_ref().unwrap().ed.level().objects.iter().position(|o| o.kind == kinds::MEDIUM_FACTORY).unwrap();
    m.s.as_mut().unwrap().ed.sel = Some(Sel::Object(i));
    m.press(Hit::LineLock(0));
    m.press(Hit::LineUnlock(1, true));
    m.press(Hit::Hp(true));
    m.press(Hit::Satellite(true));
    let path = m.save_in(&dir).expect("saved");
    assert_eq!(path.file_name().unwrap().to_string_lossy(), "zlta-pust.sowmap");
    assert!(!m.s.as_ref().unwrap().dirty());
    let back = custom_maps::load(&path).unwrap();
    assert_eq!(back.name, "Žltá púšť");
    assert_eq!(back.ext[i].unlock[1], 2500);
    assert_eq!(back.level.objects[i].raw[2], 0, "every line locked");
    assert_eq!(back.level.objects[i].raw[18], 55);
    assert_eq!(back.level.objects[i].raw[15], 1, "satellite protection is object word 15");
    assert_eq!(Sides::read(&back.level, 1).credits, 3500);
    assert!(back.problems().is_empty());
    assert_eq!(back.level.markers.iter().filter(|m| m.is_advancer()).count(), 2);
    // another map of the same name asks before replacing the file
    m.s = None;
    m.create(1).unwrap();
    m.s.as_mut().unwrap().name = TextField::new("zlta pust", custommap::MAX_NAME);
    assert!(m.save_in(&dir).is_err(), "first Save warns");
    assert!(m.save_in(&dir).is_ok(), "the second replaces");
    // deleting: the first press only asks, the second removes the file
    m.s = None;
    m.folder = dir.clone();
    m.rescan();
    assert!(path.exists());
    m.press(Hit::Row(0));
    m.press(Hit::DeleteMap);
    assert!(path.exists(), "asks first");
    m.press(Hit::Row(0));
    m.press(Hit::DeleteMap);
    assert!(path.exists(), "selecting again disarms");
    m.press(Hit::DeleteMap);
    assert!(!path.exists());
    assert!(m.maps.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn undo_and_leaving_with_unsaved_changes() {
    let Some((mut m, _a, _d)) = editor_with_map() else { return };
    populate(&mut m);
    let count = |m: &MapEditor| {
        let l = m.s.as_ref().unwrap().ed.level();
        l.objects.len() + l.markers.len()
    };
    let n = count(&m);
    m.press(Hit::Undo);
    assert_eq!(count(&m), n - 1);
    m.press(Hit::Redo);
    assert_eq!(count(&m), n);
    assert_eq!(m.press(Hit::Back), Event::None);
    assert!(m.editing(), "the first Back warns about unsaved changes");
    assert_eq!(m.press(Hit::Back), Event::None);
    assert!(!m.editing(), "the second leaves");
    assert_eq!(m.press(Hit::Back), Event::Leave);
}

#[test]
fn names_come_from_the_language_file() {
    let Some(mut game) = opensow_formats::GameDir::locate() else { return };
    game.set_language(opensow_formats::lang::Language::English);
    let Ok(mut assets) = Assets::load(game) else { return };
    let m = MapEditor::new(&mut assets);
    assert_eq!(m.thing_name(Thing::Unit(40)), "Advancer");
    assert_eq!(m.thing_name(Thing::Unit(1)), "Light AntiAir");
    assert_eq!(m.thing_name(Thing::Unit(46)), "Rogon Bomb");
    assert_eq!(m.thing_name(Thing::Unit(41)), "Kodiak");
    assert_eq!(m.thing_name(Thing::Object(25)), "Guard Cannon");
    assert_eq!(m.thing_name(Thing::Object(100)), "Headquarters");
    assert_eq!(m.thing_name(Thing::Object(104)), "Air Navigator");
    assert_eq!(m.thing_name(Thing::Object(108)), "Walker Factory");
    assert_eq!(m.thing_name(Thing::Object(107)), "WindTrap");
    assert_eq!(m.line_name(16), "Gold Influx 1");
    assert_eq!(m.line_name(19), "Research Influx 1");
    assert_eq!(m.line_name(24), "Energy Influx 3");
    assert_eq!(m.line_name(32), "Fighter Assault");
    assert_eq!(m.line_name(34), "Meteor Storm");
}

#[test]
fn the_wheel_never_zooms_out_past_the_whole_map() {
    let Some((mut m, _a, _d)) = editor_with_map() else { return };
    let (w, h) = (1920, 1080);
    let mut buf = vec![0u32; w * h];
    m.draw(&mut buf, w, h);
    let start = m.s.as_ref().unwrap().view.zoom;
    let settle = |m: &mut MapEditor| {
        for _ in 0..120 {
            m.update(1.0 / 60.0, 0.0);
        }
    };
    m.cursor = (100, 100);
    for _ in 0..10 {
        m.wheel(-1.0, w, h);
    }
    settle(&mut m);
    assert_eq!(m.s.as_ref().unwrap().view.zoom, start, "the start view already shows the whole map");
    m.wheel(1.0, w, h);
    // eased: part of the way after one frame, all of it after a while, 8 % a notch
    m.update(1.0 / 60.0, 0.0);
    let one = m.s.as_ref().unwrap().view.zoom;
    assert!(one > start && one < start * ZOOM_STEP, "{start} -> {one}");
    settle(&mut m);
    assert!((m.s.as_ref().unwrap().view.zoom - start * ZOOM_STEP).abs() < 1e-4);
    // the map point under the pointer stays put while zooming (once the map is larger than the view both ways: a smaller
    // map is centred)
    m.wheel(4.0, w, h);
    m.wheel(4.0, w, h);
    settle(&mut m);
    let (vx, vy, z) = {
        let v = m.s.as_ref().unwrap().view;
        (v.x, v.y, v.zoom)
    };
    let under = (vx + 100.0 / z, vy + 100.0 / z);
    m.wheel(2.0, w, h);
    settle(&mut m);
    let v = m.s.as_ref().unwrap().view;
    assert!(((v.x + 100.0 / v.zoom) - under.0).abs() < 0.5 && ((v.y + 100.0 / v.zoom) - under.1).abs() < 0.5);
    for _ in 0..30 {
        m.wheel(-1.0, w, h);
    }
    settle(&mut m);
    assert_eq!(m.s.as_ref().unwrap().view.zoom, start);
}

#[test]
fn held_camera_keys_scroll_smoothly_and_letters_stay_text_in_the_name_field() {
    use winit::keyboard::{Key, NamedKey, SmolStr};
    let Some((mut m, _a, _d)) = editor_with_map() else { return };
    let (w, h) = (640, 480);
    let mut buf = vec![0u32; w * h];
    m.draw(&mut buf, w, h);
    m.cursor = (100, 200);
    m.wheel(4.0, w, h);
    m.wheel(4.0, w, h);
    for _ in 0..120 {
        m.update(1.0 / 60.0, 0.0);
    }
    let at = |m: &MapEditor| (m.s.as_ref().unwrap().view.x, m.s.as_ref().unwrap().view.y);
    let zoom = m.s.as_ref().unwrap().view.zoom;
    let start = at(&m);
    let (key, ch) = (|k: NamedKey| Key::Named(k), |c: &str| Key::Character(SmolStr::new(c)));
    // D held for 10 frames of 1/60 s at 600 px/s: 100 screen px, in small steps
    m.key(&ch("d"), true, w, h);
    let mut xs = Vec::new();
    for _ in 0..10 {
        m.update(1.0 / 60.0, 600.0);
        xs.push(at(&m).0);
    }
    m.key(&ch("d"), false, w, h);
    m.update(1.0 / 60.0, 600.0);
    assert!((at(&m).0 - start.0 - 100.0 / zoom).abs() < 0.01, "{start:?} -> {:?}", at(&m));
    assert!(xs.windows(2).all(|p| (p[1] - p[0] - 10.0 / zoom).abs() < 0.01), "even steps every frame: {xs:?}");
    // arrows the same way; S and the Down arrow together are one direction
    m.key(&key(NamedKey::ArrowLeft), true, w, h);
    m.key(&ch("S"), true, w, h);
    m.update(1.0 / 60.0, 600.0);
    m.key(&key(NamedKey::ArrowLeft), false, w, h);
    m.key(&ch("S"), false, w, h);
    assert!(at(&m).0 < start.0 + 100.0 / zoom && at(&m).1 > start.1);
    // Ctrl/Cmd + a letter is a shortcut, not a camera key; a lone Command press does not stick
    m.key(&key(NamedKey::Super), true, w, h);
    m.set_ctrl(true);
    let before = at(&m);
    m.key(&ch("a"), true, w, h);
    m.update(0.1, 600.0);
    assert_eq!(at(&m), before);
    m.set_ctrl(false);
    m.key(&ch("a"), false, w, h);
    // losing the focus lets go of the keys
    m.key(&ch("w"), true, w, h);
    m.release_keys();
    let before = at(&m);
    m.update(0.1, 600.0);
    assert_eq!(at(&m), before);
    // in the name field the letters are text
    m.press(Hit::Name);
    m.key(&ch("d"), true, w, h);
    m.update(0.1, 600.0);
    assert_eq!(at(&m), before);
}

/// `n` playable maps in a fresh folder (names `Map 01`..).
pub(crate) fn many_maps(tag: &str, n: usize) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("opensow-many-maps-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for i in 0..n {
        let mut m = CustomMap::new(&format!("Map {:02}", i + 1), Terrain { warmonger: false, mission: 1 + (i as i32 % 22) });
        let hq = |x: i32, owner: i32| {
            let mut o = opensow_formats::maptweak::template(100, (x, 4), if owner == 0 { Side::Blue } else { Side::Green });
            o.raw[14] = owner;
            o
        };
        m.level.objects = vec![hq(4, 0), hq(30, 1)];
        m.ext = vec![ObjectExt::default(); 2];
        custom_maps::save_in(&dir, &m).unwrap();
    }
    dir
}

#[test]
fn the_start_pages_list_scrolls_through_many_maps() {
    let Some((mut m, _a, _d)) = editor_with_map() else { return };
    m.s = None;
    let dir = many_maps("editor", 30);
    m.folder = dir.clone();
    m.rescan();
    assert_eq!(m.maps.len(), 30);
    for (w, h) in [(640, 480), (1920, 1080)] {
        let rows = start_rows(&Geo::for_window(w, h));
        m.list_scroll = 0;
        let shown = |m: &MapEditor| m.controls(w, h).iter().filter_map(|(_, h)| if let Hit::Row(i) = h { Some(*i) } else { None }).collect::<Vec<_>>();
        assert_eq!(shown(&m), (0..rows).collect::<Vec<_>>(), "{w}x{h}: the first rows");
        // the wheel over the list scrolls, down to the last map and no further
        m.cursor = (10, h as i32 / 2);
        for _ in 0..100 {
            m.wheel(-1.0, w, h);
        }
        assert_eq!(m.list_scroll, 30 - rows, "{w}x{h}");
        assert_eq!(shown(&m), (30 - rows..30).collect::<Vec<_>>(), "{w}x{h}: the last rows");
        for (r, hit) in m.controls(w, h) {
            assert!(page::inside(r, w, h), "{hit:?} at {w}x{h}");
        }
        // the last map can be opened from there
        let last = m.centre_of(Hit::Row(29), w, h).expect("row 29 shown");
        m.mouse_button(true, true, last.0, last.1, w, h);
        m.mouse_button(true, false, last.0, last.1, w, h);
        assert_eq!(m.list_sel, Some(29));
        for _ in 0..100 {
            m.wheel(1.0, w, h);
        }
        assert_eq!(m.list_scroll, 0);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn draws_both_pages_at_the_extreme_sizes() {
    let Some((mut m, _a, _d)) = editor_with_map() else { return };
    populate(&mut m);
    for (w, h) in [(640, 480), (1920, 1080)] {
        let mut buf = vec![0u32; w * h];
        m.draw(&mut buf, w, h);
        assert!(buf.iter().filter(|&&p| p != 0).count() > w * h / 2);
    }
    m.s = None;
    m.load_thumbs(3);
    let mut buf = vec![0u32; 640 * 480];
    m.draw(&mut buf, 640, 480);
}

/// A generated map (docs/generated-maps.md): a small plan on M12's ground with pieces of M12, M14 and M01, opened in `m`.
pub(crate) fn generated(m: &mut MapEditor, name: &str) -> Result<(), String> {
    let text = format!(
        "name {name}\nbase M12\nseed 5\npiece M12 20,38 at 0,30\npiece M12 35,16 at 23,0\npiece M12 47,45 at 40,45\nstart 2,2 14x10\nstart 4,55 12x7\n"
    );
    m.generate(&opensow_viewer::layout::Plan::parse(&text)?).map(|_| ())
}

#[test]
fn a_generated_map_is_terrain_only_and_the_game_builds_the_editors_ground() {
    let Some((mut m, _a, dir)) = editor_with_map() else { return };
    generated(&mut m, "Generated").unwrap();
    let s = m.s.as_ref().unwrap();
    let layout = s.layout.clone().expect("a layout");
    assert_eq!(layout.features.len(), 3);
    let blocked = s.scene.map.cells.iter().filter(|c| c.blocked).count() as f32 / s.scene.map.cells.len() as f32;
    assert!(blocked > 0.1, "the pieces' cells are blocked: {blocked}");
    assert!(s.map().level.objects.is_empty(), "terrain only: the players place the buildings");
    let (bg, cells) = (s.scene.bg.clone(), s.scene.map.cells.clone());
    let path = m.save_in(&dir).expect("saved");
    m.open_file(&path).unwrap();
    assert!(m.s.as_ref().unwrap().scene.bg == bg, "reopened: the same ground");
    assert_eq!(m.s.as_ref().unwrap().layout, Some(layout));
    let played = custom_maps::load_scene(&m.game, &custom_maps::load(&path).unwrap().ground(), 0, None, None).unwrap();
    assert!(played.bg == bg && played.map.cells == cells, "the game's ground and cells are the editor's");
    let _ = std::fs::remove_dir_all(dir);
}
