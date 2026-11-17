//! Headless runs: `--screenshot FILE` builds the requested screen or mission from the command line, lets it run and
//! saves one frame as a PNG. This is how the game is checked without a window (see the commands in AGENTS.md).

use super::*;

/// The value that follows the switch `name` on the command line, e.g. `M01` for `--play M01`.
fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// Whether the switch `name` is on the command line.
fn has_arg(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// The whole `--screenshot` run: reads the other switches from `args`, sets `game` up accordingly, draws one frame and
/// writes it to `path`.
pub(super) fn screenshot(mut game: Game, path: &str, args: &[String]) {
    let get = |name: &str| arg_value(args, name);
    let has = |name: &str| has_arg(args, name);
    if let Some(n) = get("--boot-page").and_then(|n| n.parse().ok()) {
        game.boot = boot::Boot::open(&game.assets.game);
        if let Some(b) = game.boot.as_mut() {
            b.jump(n);
        }
    }
    if let Some(n) = get("--intro-frame").and_then(|n| n.parse().ok()) {
        match intro::Intro::open(&game.assets.game, &get("--movie").unwrap_or_else(|| "mI".into()), game.language) {
            Ok(mut i) => {
                i.jump(n);
                game.intro = Some(i);
            }
            Err(e) => eprintln!("cannot open the intro: {e}"),
        }
    }
    let (w, h) = get("--size").and_then(|s| s.split_once('x').map(|(a, b)| (a.parse().unwrap_or(1280), b.parse().unwrap_or(800)))).unwrap_or((1280, 800));
    let skirmish_map = get("--skirmish");
    if let Some(level) = get("--play").or_else(|| skirmish_map.clone()) {
        if let Some(map) = &skirmish_map {
            start_skirmish_from_args(&mut game, map, &get, &has);
        } else {
            game.start(&level.to_ascii_uppercase());
        }
        mission_switches(&mut game, args, w, h);
        if has("--paused") {
            if let Some(p) = game.play.as_mut() {
                p.paused = true;
            }
        }
        if let Some(m) = get("--results") {
            // --results won|lost shows the banner, won-stats|lost-stats the statistics --age ticks into the count (sample numbers
            // with --sample-stats)
            let stats =
                if has("--sample-stats") { sample_stats() } else { game.play.as_ref().map(|p| [p.world.stats[0], p.world.stats[1]]).unwrap_or_default() };
            let mut r = results::Results::new(&mut game.assets, !m.starts_with("lost"), false, stats, game.language);
            r.update(2.5);
            if m.ends_with("stats") {
                r.input(false, true);
                r.update(get("--age").and_then(|a| a.parse::<f64>().ok()).unwrap_or(60.0) / 60.0);
            }
            game.results = Some(r);
        }
        if has("--pause") {
            game.open_pause();
            if let Some(i) = get("--hover").and_then(|n| n.parse::<usize>().ok()) {
                let c = game.pause_menu().centre(ingame::Hit::Item(i), w, h);
                game.pause_menu().mouse_moved(c.0, c.1, w, h);
            }
        }
    } else {
        screen_switches(&mut game, args, w, h);
    }
    if game.lands.is_some()
        || game.options_menu.is_some()
        || game.saveui.is_some()
        || game.net_menu.is_some()
        || game.skirmish_menu.is_some()
        || game.map_editor.is_some()
        || game.results.is_some()
    {
        game.screen = None;
    } else {
        game.hover = get("--hover").and_then(|h| h.parse().ok());
    }
    let mut buf = vec![0u32; w * h];
    dev_switches(&mut game, args);
    if has("--gpu-check") {
        gpu_check(&mut game, w, h);
    }
    if has("--mouse-px") {
        // the HUD layout exists only after a draw: draw once, let the pointer hover, draw again
        game.draw(&mut buf, w, h);
        if let Some(p) = game.play.as_mut() {
            p.update(0.0, &game.settings, w, h);
        }
    }
    game.draw(&mut buf, w, h);
    write_png(path, &buf, w, h);
    println!("-> {path} ({w}x{h})");
}

/// The switches that act on the mission that was just started: camera, selection, orders, placing things, running
/// `--ticks` of the simulation and printing the `--report`.
fn mission_switches(game: &mut Game, args: &[String], w: usize, h: usize) {
    let get = |name: &str| arg_value(args, name);
    let has = |name: &str| has_arg(args, name);
    if let Some(p) = game.play.as_mut() {
        let pair = |s: String| s.split_once(',').map(|(a, b)| (a.parse::<f32>().unwrap_or(0.0), b.parse::<f32>().unwrap_or(0.0)));
        p.update(0.0, &game.settings, w, h);
        if let Some(z) = get("--zoom").and_then(|z| z.parse().ok()) {
            p.view.zoom = z;
            p.scene.warm_ground();
            p.centre_on_player(w, h);
        }
        if let Some((cx, cy)) = get("--center").and_then(pair) {
            p.view.x = (cx + 0.5) * 32.0 - w as f32 / p.view.zoom / 2.0;
            p.view.y = (cy + 0.5) * 32.0 - h as f32 / p.view.zoom / 2.0;
        }
        if has("--select-advancer") {
            for u in p.world.units.iter_mut().filter(|u| u.player == 0 && u.kind == kinds::ADVANCER) {
                u.selected = true;
            }
        }
        if let Some((px, py)) = get("--mouse-px").and_then(pair) {
            // headless: the pointer sits on this screen pixel (HUD hover)
            p.cursor = (px as i32, py as i32);
            p.edge_scroll = true;
        }
        if let Some((cx, cy)) = get("--mouse-cell").and_then(pair) {
            // headless: the pointer sprite hovers this cell (its type follows the selection and the object there)
            let (sx, sy) = p.view.to_screen(cx * 32.0 + 16.0, cy * 32.0 + 16.0);
            p.cursor = (sx, sy);
            p.edge_scroll = true;
        }
        if let Some(text) = get("--chat") {
            // headless: the chat line is open with this text typed (--chat-send presses Enter on it, so a cheat takes effect)
            p.chat_open();
            text.chars().for_each(|c| p.chat_type(c));
            if has("--chat-send") {
                p.chat_send();
            }
        }
        if let Some(c) = get("--arm-strike").and_then(|c| c.parse::<i32>().ok()) {
            // headless: the air-strike mode is armed for this force (the pointer and the stock pill show it)
            p.strike = opensow_formats::air::AirForce::from_code(c);
        }
        if let Some(spec) = get("--strike") {
            // headless: --strike CODE,CX,CY (30 bomber .. 34 asteroid) launches that air force for the player at the cell centre
            let f: Vec<i32> = spec.split(',').filter_map(|v| v.parse().ok()).collect();
            if let (Some(&c), Some(&cx), Some(&cy)) = (f.first(), f.get(1), f.get(2)) {
                if let Some(force) = opensow_formats::air::AirForce::from_code(c) {
                    if let Some(g) = opensow_formats::air::HEADER_ORDER.iter().position(|&x| x == force) {
                        p.world.air_stock[0][g] += 1;
                    }
                    p.world.launch_strike(0, force, cx * 32 + 16, cy * 32 + 16);
                    if has("--strike-god") {
                        // headless: the aircraft cannot be shot down on its way
                        if let Some(a) = p.world.aircraft.last_mut() {
                            a.hp = 1_000_000;
                        }
                    }
                }
            }
        }
        if has("--select-all") {
            p.world.select_all();
        }
        if let Some(k) = get("--info").and_then(|k| k.parse::<i32>().ok()) {
            // headless: show kind K in the info box after N HUD ticks
            let n = get("--info-ticks").and_then(|n| n.parse().ok()).unwrap_or(0);
            p.run_infobox(&mut game.assets, k, n);
        }
        if let Some((cx, cy)) = get("--select-building").and_then(pair) {
            // headless: select the building on this cell for the info box
            p.sel_building = p.world.structure_at(cx as i32 * 32 + 16, cy as i32 * 32 + 16);
            p.world.clear_selection();
        }
        if let Some((cx, cy)) = get("--rally").and_then(pair) {
            // headless: the selected own factory gets this cell as its rally point
            if let Some(b) = p.sel_building {
                p.world.set_rally(b, 0, cx as i32 * 32 + 16, cy as i32 * 32 + 16);
            }
        }
        if let Some(t) = get("--turret").and_then(|t| t.parse::<u32>().ok()) {
            // headless: builds a turret of this tier on the first good cell near the player's first building and looks at it
            p.world.credits[0] += 1000 * t as i32;
            let own = p.world.structures.iter().find(|s| s.owner == opensow_formats::level::Owner::Player).map(|s| s.cell);
            if let Some((ox, oy)) = own {
                'find: for r in 1..12 {
                    for dy in -r..=r {
                        for dx in -r..=r {
                            let c = (ox + dx, oy + dy);
                            if p.world.turret_cells(0, t, c).iter().all(|&ok| ok) && p.world.place_turret(0, t, c) {
                                eprintln!("turret placed at {c:?}");
                                p.look_at(c.0 as f32 * 32.0 + 16.0, c.1 as f32 * 32.0 + 16.0);
                                break 'find;
                            }
                        }
                    }
                }
            }
        }
        if let Some(c) = get("--credits").and_then(|c| c.parse::<i32>().ok()) {
            // headless: the player's purse (turret tabs dim below 200 * tier)
            p.world.turret_flags[0] = [true; 5];
            p.world.credits[0] = c;
        }
        if let Some(v) = get("--speed").and_then(|v| v.parse::<f32>().ok()) {
            // headless: the F3 / F4 message line for this game speed
            p.show_speed(v);
        }
        if has("--fps") {
            // headless: the "draw fps" counter
            p.show_fps = true;
        }
        if has("--keys") {
            // headless: F1's key help overlay
            p.show_keys = true;
        }
        if let Some(t) = get("--arm-turret").and_then(|t| t.parse::<u32>().ok()) {
            // headless: the turret of this tier is armed (the placement tiles follow the pointer cell)
            p.world.turret_flags[0] = [true; 5];
            p.world.credits[0] = 5000;
            p.turret_key(t);
        }
        if has("--arm-rally") {
            p.arm_rally();
        }
        if let Some((cx, cy)) = get("--act").and_then(pair) {
            // headless: every Advancer of the player works on the building at this cell
            if let Some(t) = p.world.structure_at(cx as i32 * 32 + 16, cy as i32 * 32 + 16) {
                let ids: Vec<u32> = p.world.player_units().filter(|u| u.kind == kinds::ADVANCER).map(|u| u.id).collect();
                for id in ids {
                    p.world.order_action(id, t);
                }
            }
        }
        if let Some((cx, cy)) = get("--destroy").and_then(pair) {
            // headless: the building covering this cell runs out of hit points (shows the destruction burst)
            p.world.debug_destroy_at(cx as i32, cy as i32);
        }
        for spec in args.iter().enumerate().filter(|(_, a)| *a == "--unit").filter_map(|(i, _)| args.get(i + 1)) {
            // headless: --unit KIND,PLAYER,PX,PY[,SELECTED[,HP]] puts a unit on a map pixel (repeatable)
            let f: Vec<i32> = spec.split(',').filter_map(|v| v.parse().ok()).collect();
            // a fifth number selects the new unit; with damage: a sixth sets its hit points
            if let [kind, player, x, y, rest @ ..] = &f[..] {
                let (kind, player, x, y) = (*kind, *player, *x, *y);
                let id = p.world.add_unit(kind, player, x, y);
                if let Some(u) = p.world.units.iter_mut().find(|u| u.id == id) {
                    u.selected = rest.first().is_some_and(|&v| v != 0);
                    if let Some(&hp) = rest.get(1) {
                        u.hp = hp;
                    }
                }
            }
        }
        for spec in args.iter().enumerate().filter(|(_, a)| *a == "--fx").filter_map(|(i, _)| args.get(i + 1)) {
            // headless (repeatable): --fx CX,CY,KIND,AGE puts an effect on a cell: KIND = blast0..blast6, turret, building, air, cloud, wreck, puff, decal (AGE = variant), satlaser, piece (AGE = top variant), missile, smoke
            let f: Vec<&str> = spec.split(',').collect();
            if let [cx, cy, "satlaser", _] = f[..] {
                // the satellite laser column over a cell (a bullet of type 10 at the cell's centre, 64 px up)
                let (x, y) = (cx.parse::<i32>().unwrap_or(0) * 32 + 16, cy.parse::<i32>().unwrap_or(0) * 32 + 16);
                if let Some(path) = opensow_formats::projectile::build_path(10, x, y - 79, x, y - 54, 0) {
                    p.world.push_bullet(sim::Bullet::new(10, 1000, 0, path));
                }
            } else if let [cx, cy, "missile", _] = f[..] {
                // an anti-air missile (bullet type 1) flying 250 px right and 60 px up from the cell's centre
                let (x, y) = (cx.parse::<i32>().unwrap_or(0) * 32 + 16, cy.parse::<i32>().unwrap_or(0) * 32 + 16);
                if let Some(path) = opensow_formats::projectile::build_path(1, x, y, x + 250, y - 60, 0) {
                    p.world.push_bullet(sim::Bullet::new(1, 0, 0, path));
                }
            } else if let [cx, cy, "piece", age] = f[..] {
                // a destroyed unit's flying piece (bullet type 15) from the cell's centre; AGE = the top variant 0..14
                let (x, y) = (cx.parse::<i32>().unwrap_or(0) * 32 + 16, cy.parse::<i32>().unwrap_or(0) * 32 + 16);
                if let Some(path) = opensow_formats::projectile::build_path(15, x, y, x + 50, y + 30, 0) {
                    let mut b = sim::Bullet::new(15, 40, 0, path);
                    b.variant = age.parse().unwrap_or(0);
                    p.world.push_bullet(b);
                }
            } else if let [cx, cy, kind, age] = f[..] {
                use opensow_formats::effects as e;
                let spec = match kind {
                    "turret" => e::TURRET_DESTROYED,
                    "building" => e::BUILDING_DESTROYED,
                    "air" => e::AIR_DESTROYED,
                    "cloud" => e::UNIT_CLOUD,
                    "wreck" => e::UNIT_WRECK,
                    "puff" => e::smoke_puff(0, 4),
                    "smoke" => e::trail(1).map_or(e::smoke_puff(0, 4), |t| t.spec), // the missile smoke (mode 14)
                    "decal" => {
                        let (x, y) = (cx.parse::<i32>().unwrap_or(0) * 32 + 16, cy.parse::<i32>().unwrap_or(0) * 32 + 16);
                        p.world.decals.push(sim::Decal { x, y, cell: (x / 32, y / 32), frame: age.parse::<u8>().unwrap_or(0) % 5 });
                        e::blast(0)
                    }
                    k => e::blast(k.trim_start_matches("blast").parse().unwrap_or(2)),
                };
                let (x, y) = (cx.parse::<i32>().unwrap_or(0) * 32 + 16, cy.parse::<i32>().unwrap_or(0) * 32 + 16);
                p.world.effects.push(sim::Effect { x, y, spec, age: age.parse().unwrap_or(0) });
            }
        }
        p.debug_no_mask = has("--no-mask");
        if let Some((cx, cy)) = get("--move").and_then(pair) {
            p.world.order_move_selected(cx as i32 * 32 + 16, cy as i32 * 32 + 16);
        }
        if let Some((cx, cy)) = get("--attack").and_then(pair) {
            // attack order of every selected unit on the building covering this cell
            if let Some(t) = p.world.enemy_at(cx as i32 * 32 + 16, cy as i32 * 32 + 16) {
                p.world.order_attack_selected(t);
            }
        }
        // --clouds D: the options' CLOUDS detail 0..=4 (the first-run default is 0 = none)
        if let Some(d) = get("--clouds").and_then(|d| d.parse().ok()) {
            game.settings.cloud_detail = d;
        }
        for _ in 0..get("--ticks").and_then(|t| t.parse().ok()).unwrap_or(0) {
            p.world.step();
            p.world.sky_step(game.settings.cloud_detail);
            p.take_tutorial_events();
        }
        game.settings.show_range = has("--range");
        if has("--report") {
            for u in &p.world.units {
                println!(
                    "unit {} kind {} player {} at ({:.0},{:.0}) cell {:?} hp {}/{} path {} target {:?}",
                    u.id,
                    u.kind,
                    u.player,
                    u.x,
                    u.y,
                    u.cell(),
                    u.hp,
                    u.max_hp,
                    u.path.len(),
                    u.target
                );
            }
            for s in p.world.structures.iter().filter(|s| {
                s.tally != [0; 3] || s.hp != s.max_hp || s.heal > 0 || s.owner == opensow_formats::level::Owner::Neutral || s.kind_id == kinds::RESEARCH_LAB
            }) {
                println!(
                    "structure kind {} cell {:?} owner {:?} hp {}/{} heal {} tally {:?} active {}",
                    s.kind_id, s.cell, s.owner, s.hp, s.max_hp, s.heal, s.tally, s.active
                );
            }
            if let Some(t) = &p.world.tutorial {
                println!("tutorial step {} arrow {} entered {}", t.step, t.arrow, t.entered);
            }
            for b in &p.world.bullets {
                println!(
                    "bullet type {} strength {} owner {} index {}/{} from {:?} to {:?}",
                    b.bullet_type,
                    b.strength,
                    b.owner,
                    b.index,
                    b.path.points.len(),
                    b.path.points.first(),
                    b.path.points.last()
                );
            }
            // the positional sounds the run raised (the first 64; nothing drains them headless)
            println!("sounds {:?}", p.world.sounds.iter().map(|s| s.0).collect::<Vec<_>>());
            for a in &p.world.aircraft {
                println!("aircraft {:?} at ({}, {}) alt {} hp {} frame {}", a.force, a.x, a.y, a.altitude, a.hp, a.frame);
            }
            println!("air stock {:?} launched {:?}", p.world.air_stock, [p.world.stats[0][6], p.world.stats[1][6]]);
            println!(
                "credits {:?} research {:?} enemy turrets {:?}",
                p.world.credits,
                p.world.research,
                p.world
                    .structures
                    .iter()
                    .filter(|s| s.alive() && kinds::TURRETS.contains(&s.kind_id) && format!("{:?}", s.owner) == "Enemy")
                    .map(|s| (s.kind_id, s.cell))
                    .collect::<Vec<_>>()
            );
            let owned = |o: &str| p.world.structures.iter().filter(|s| s.alive() && (100..=108).contains(&s.kind_id) && format!("{:?}", s.owner) == o).count();
            println!("buildings player {} enemy {} neutral {}", owned("Player"), owned("Enemy"), owned("Neutral"));
            println!("turret tiers player {:?} enemy {:?}", p.world.turret_flags[0], p.world.turret_flags[1]);
            println!("bullets {} tick {} outcome {:?}", p.world.bullets.len(), p.world.tick, p.world.outcome);
            println!("timer {:?} expired {} objective {:?}", p.world.timer, p.world.timer_expired, p.world.objective);
        }
    }
}

/// The `--screen NAME` switch and the switches of each screen: opens the screen and puts it into the asked state.
fn screen_switches(game: &mut Game, args: &[String], w: usize, h: usize) {
    let get = |name: &str| arg_value(args, name);
    let has = |name: &str| has_arg(args, name);
    game.screen = Some(match get("--screen").as_deref() {
        Some("levels") => Screen::Levels,
        Some("prototype") => Screen::Options,
        Some("options") => {
            game.open_options();
            // --page 0|1; --hover N: 0..13 arrow, 14.. label, 100+k row k of page 2, 200 page arrow, 300+k / 400+k left / right arrow of row k
            if let (Some(m), Some(n)) = (game.options_menu.as_mut(), get("--page").and_then(|n| n.parse::<usize>().ok())) {
                m.page = n.min(options::PAGES - 1);
            }
            if let (Some(m), Some(hit)) = (game.options_menu.as_mut(), get("--hover").and_then(|n| n.parse::<usize>().ok())) {
                let p = m.centre(match hit {
                    0..=13 => options::Hit::Arrow(hit),
                    14..=99 => options::Hit::Label(hit - 14),
                    100..=199 => options::Hit::Extra(hit - 100),
                    300..=399 => options::Hit::ExtraArrow(hit - 300, false),
                    400..=499 => options::Hit::ExtraArrow(hit - 400, true),
                    _ => options::Hit::Page(if m.page == 0 { 1 } else { 0 }),
                });
                let (ox, oy, sc) = (0.0, 0.0, 1.0f32);
                let _ = (ox, oy, sc);
                m.mouse_moved(p.0 * w as i32 / 640, p.1 * h as i32 / 480, w, h);
            }
            Screen::Main
        }
        Some("multiplayer") => {
            // --page 0|1|2 (lobby), --guest (joined side), --hover N (arrow 0..27), --chat
            game.open_multiplayer();
            if let Some(m) = game.net_menu.as_mut() {
                match get("--page").and_then(|n| n.parse::<usize>().ok()) {
                    Some(0) => *m = multiplayer::NetMenu::with_connections(&mut game.assets, Vec::new()),
                    Some(1) => m.set_sessions(if has("--sessions") { vec![multiplayer::SESSION_NAME.into(), "Second war".into()] } else { Vec::new() }),
                    _ => {
                        m.open_lobby(usize::from(has("--guest")));
                        if has("--chat") {
                            m.peer_joined();
                            m.typed('h');
                            m.typed('i');
                            m.enter();
                        }
                    }
                }
                if let Some(n) = get("--hover").and_then(|n| n.parse::<usize>().ok()) {
                    let p = m.centre(multiplayer::Hit::Arrow(n));
                    m.mouse_moved(p.0 * w as i32 / 640, p.1 * h as i32 / 480, w, h);
                }
            }
            Screen::Main
        }
        Some("skirmish") => {
            // --guest (human plays green), --map N (list index), --hover N (air card N 0..4 / turret card 5..9), --changed (one value off its default)
            game.open_skirmish();
            if let Some(m) = game.skirmish_menu.as_mut() {
                // --tab custom: the players' own maps (the maps folder; OPENSOW_MAPS_DIR for demos)
                if get("--tab").is_some_and(|t| t.eq_ignore_ascii_case("custom")) {
                    m.set_tab(skirmish_ui::Tab::Custom);
                }
                if let Some(n) = get("--map").and_then(|n| n.parse::<usize>().ok()) {
                    m.select_map(n);
                }
                if has("--guest") {
                    m.press(skirmish_ui::Hit::Swap);
                }
                if has("--changed") {
                    m.press(skirmish_ui::Hit::Step(0, 11));
                }
                m.sounds.clear();
                // --stars easy,normal,hard,brutal,...: demo stars on the first maps of the list
                for (i, v) in get("--stars").unwrap_or_default().split(',').enumerate() {
                    if let Some(d) = sim::Difficulty::ALL.into_iter().find(|d| d.name().eq_ignore_ascii_case(v)) {
                        m.demo_star(i, d);
                    }
                }
                if let Some(d) = get("--difficulty").and_then(|v| sim::Difficulty::ALL.into_iter().find(|d| d.name().eq_ignore_ascii_case(&v))) {
                    m.difficulty = d;
                }
                if let Some(n) = get("--hover").and_then(|n| n.parse::<usize>().ok()) {
                    let hit = if n < 5 { skirmish_ui::Hit::Step(0, n * 2 + 1) } else { skirmish_ui::Hit::Turret(0, (n - 5).min(4)) };
                    if let Some(p) = m.centre(hit, w, h) {
                        m.mouse_moved(p.0, p.1, w, h);
                        m.update(37);
                    }
                }
            }
            Screen::Main
        }
        Some("mapeditor") => {
            // the start page; --new N creates a map on terrain N (--warmonger-terrain for Warmonger's), --open FILE opens one,
            // --row N chooses a row of the list; on a map: --demo places a sample, --name TEXT [--focus], --select CX,CY,
            // --tool NAME [--at CX,CY] arms a palette item (and points at a cell), --side blue|green, --content sides,
            // --save writes it to the maps folder
            game.open_map_editor();
            if let Some(m) = game.map_editor.as_mut() {
                m.load_thumbs(usize::MAX);
                // --plan FILE: generates the plan's map (docs/generated-maps.md, docs/map-plans/); --save-map writes it
                if let Some(file) = get("--plan") {
                    let t = std::time::Instant::now();
                    match std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}")).and_then(|text| opensow_viewer::layout::Plan::parse(&text)) {
                        Ok(plan) => match m.generate(&plan) {
                            Ok(g) if has("--spaces") => eprintln!("open spaces (cells to a wall, at): {:?}", g.open_spaces),
                            Ok(g) => eprintln!(
                                "{}: {} features, {} trail dabs, {:.0} % blocked, front {} at {:?}, path {} (straight {}), widest open {} at {:?}, {} building spots, {} leaks, rooms sealed {:?}, seed {}, {:.1} s",
                                plan.name,
                                g.layout.features.len(),
                                g.layout.trails.len(),
                                g.blocked * 100.0,
                                g.front,
                                g.cut,
                                g.path.0,
                                g.path.1,
                                g.widest.0,
                                g.widest.1,
                                g.building_spots,
                                g.leaks,
                                g.sealed,
                                g.seed,
                                t.elapsed().as_secs_f32()
                            ),
                            Err(e) => eprintln!("cannot generate: {e}"),
                        },
                        Err(e) => eprintln!("bad plan: {e}"),
                    }
                }
                if let Some(n) = get("--new").and_then(|n| n.parse::<usize>().ok()) {
                    if let Err(e) = m.create(n) {
                        eprintln!("cannot create a map: {e}");
                    }
                }
                if let Some(p) = get("--open") {
                    if let Err(e) = m.open_file(std::path::Path::new(&p)) {
                        eprintln!("cannot open {p}: {e}");
                    }
                }
                if let Some(n) = get("--row").and_then(|n| n.parse::<usize>().ok()) {
                    m.choose_row(n);
                }
                if let Some(n) = get("--terrain").and_then(|n| n.parse::<usize>().ok()) {
                    m.terrain = n;
                }
                if has("--demo") && !m.demo() {
                    eprintln!("no room for the demo objects");
                }
                if let Some(name) = get("--name") {
                    m.set_name(&name, has("--focus"));
                }
                if has("--save-map") {
                    if let Err(e) = m.save() {
                        eprintln!("cannot save: {e}");
                    }
                }
                let cell = |v: Option<String>| {
                    v.and_then(|v| v.split_once(',').and_then(|(a, b)| Some((a.trim().parse::<i32>().ok()?, b.trim().parse::<i32>().ok()?))))
                };
                if let Some(c) = cell(get("--select")) {
                    m.select_cell(c);
                }
                if let Some(t) = get("--tool") {
                    if !m.arm_tool(&t) {
                        eprintln!("no palette item {t}");
                    }
                    if let Some(c) = cell(get("--at")) {
                        m.point_at_cell(c, w, h);
                    }
                }
                if has("--areas") {
                    m.show_build_areas(true);
                }
                match get("--content").as_deref() {
                    Some("sides") => drop(m.press(mapeditor_ui::Hit::Content(1))),
                    Some("mission") => drop(m.press(mapeditor_ui::Hit::Content(2))),
                    _ => {}
                }
                // --timer MIN[,victory|defeat|reinforcements] --win all: the map's mission
                if get("--timer").is_some() || get("--win").is_some() {
                    use opensow_formats::custommap::{Mission, Timer, TimerEnd, WinRule};
                    let timer = get("--timer").and_then(|t| {
                        let (min, end) = t.split_once(',').unwrap_or((t.as_str(), "victory"));
                        let end = match end.trim() {
                            "defeat" => TimerEnd::Defeat,
                            "reinforcements" => TimerEnd::Reinforcements,
                            _ => TimerEnd::Victory,
                        };
                        Some(Timer { minutes: min.trim().parse().ok()?, end })
                    });
                    let win = match get("--win").as_deref() {
                        Some("all") => WinRule::Everything,
                        Some("escort") => WinRule::Escort,
                        _ => WinRule::Headquarters,
                    };
                    m.set_mission(timer, Mission { win });
                }
                // --reinforce CREDITS,RESEARCH[,bombers,asteroids,carryalls,triplers,fighters] --drops CX,CY[,SQUAD][;..]
                // (unchecked: a demo may set landing points the editor would refuse)
                if get("--reinforce").is_some() || get("--drops").is_some() {
                    let n: Vec<i32> = get("--reinforce").unwrap_or_default().split(',').filter_map(|v| v.trim().parse().ok()).collect();
                    let v = |k: usize| n.get(k).copied().unwrap_or(0);
                    let drops = get("--drops")
                        .unwrap_or_default()
                        .split(';')
                        .filter_map(|d| {
                            let n: Vec<i32> = d.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                            (n.len() >= 2)
                                .then(|| opensow_formats::custommap::Landing { cell: (n[0], n[1]), squad: n.get(2).copied().unwrap_or(0).clamp(0, 17) as u8 })
                        })
                        .collect();
                    m.set_reinforcement(opensow_formats::custommap::Reinforcement {
                        credits: v(0),
                        research: v(1),
                        air: std::array::from_fn(|g| v(2 + g).clamp(0, 20) as u8),
                        drops,
                    });
                }
                // --delete: removes the --select'ed object (after the mission flags: an escort map may lose blue's last HQ)
                if has("--delete") {
                    m.press(mapeditor_ui::Hit::Delete);
                }
                // --goal CX,CY --route "CX,CY;CX,CY" --pods "CX,CY;CX,CY": an escort (with --win escort)
                if get("--goal").is_some() || get("--route").is_some() || get("--pods").is_some() {
                    let cells = |k: &str| get(k).unwrap_or_default().split(';').filter_map(|c| cell(Some(c.to_string()))).collect::<Vec<_>>();
                    let escort = opensow_formats::custommap::Escort { goal: cell(get("--goal")), route: cells("--route") };
                    if !m.set_escort(escort, &cells("--pods")) {
                        eprintln!("a pod does not fit");
                    }
                }
                if has("--save") {
                    match m.save() {
                        Ok(p) => println!("saved {}", p.display()),
                        Err(e) => eprintln!("cannot save: {e}"),
                    }
                }
                m.sounds.clear();
            }
            Screen::Main
        }
        Some("pause") => {
            game.open_pause();
            Screen::Pause
        }
        Some(page @ ("load" | "save")) => {
            // --level L sets the level a save stores, --row N selects a row, --type TEXT types into it (save), --confirm presses LOAD/SAVE, --hover N hovers a row
            game.open_saveui(if page == "load" { saveui::Mode::Load } else { saveui::Mode::Save });
            if let Some(m) = game.saveui.as_mut() {
                if let Some(l) = get("--level") {
                    m.set_level(&l);
                }
                if let Some(r) = get("--row").and_then(|n| n.parse::<usize>().ok()) {
                    m.select(r);
                }
                if let Some(t) = get("--type") {
                    m.type_text(&t);
                }
                // --age N: N ticks of the page's update first (the sparks run along the table lines)
                for _ in 0..get("--age").and_then(|a| a.parse::<u32>().ok()).unwrap_or(0) {
                    m.update(1);
                }
                if has("--confirm") {
                    println!("{:?}", m.confirm());
                }
                if let Some(r) = get("--hover").and_then(|n| n.parse::<usize>().ok()) {
                    let c = m.centre(saveui::Hit::Row(r));
                    let p = saveui::SaveUi::to_screen(c, w, h);
                    m.mouse_moved(p.0, p.1, w, h);
                }
            }
            Screen::Main
        }
        Some("results") => {
            // the statistics screen with sample numbers, --age ticks into the count
            let mut r = results::Results::new(&mut game.assets, true, false, sample_stats(), game.language);
            r.update(2.5);
            r.input(false, true);
            r.update(get("--age").and_then(|a| a.parse::<f64>().ok()).unwrap_or(600.0) / 60.0);
            game.results = Some(r);
            Screen::Main
        }
        Some("message") => Screen::Message("Mission complete!\nThe enemy is defeated.".into()),
        Some("lands") => {
            // --land N: that land selected --age seconds ago (0 = the plain map); --hover N hovers a land on the map
            if has("--unlock-all") {
                game.progress.states[1..=22].fill(1);
                game.progress.states[22] = 2;
            }
            game.open_lands(false);
            if let Some(l) = game.lands.as_mut() {
                l.show(get("--land").and_then(|n| n.parse().ok()).unwrap_or(0), get("--age").and_then(|n| n.parse().ok()).unwrap_or(10.0));
                l.set_hover(get("--hover").and_then(|n| n.parse().ok()));
                if has("--missing-warmonger") {
                    l.show_missing();
                }
                l.update(get("--clock").and_then(|n| n.parse().ok()).unwrap_or(0.0));
            }
            Screen::Main
        }
        _ => Screen::Main,
    });
}

/// The `--dev` switch: the developer panel's cheats and tools.
fn dev_switches(game: &mut Game, args: &[String]) {
    let get = |name: &str| arg_value(args, name);
    if let Some(spec) = get("--dev") {
        // headless dev tools: --dev show,unlock,god,rich,freeze,x2|x5|x10,enemy,arm=KIND (a hidden panel only shows its badge)
        for t in spec.split(',') {
            match t {
                "show" => game.dev.visible = true,
                "unlock" => game.dev.unlock_all = true,
                "god" => game.dev.invulnerable = true,
                "rich" => game.dev.rich = true,
                "freeze" => game.dev.freeze_enemy = true,
                "enemy" => game.dev.side = 1,
                "air" => {
                    let side = game.dev.side;
                    if let Some(p) = game.play.as_mut() {
                        p.world.air_stock[side] = [5; 5];
                    }
                }
                t if t.starts_with('x') => game.dev.speed = t[1..].parse().unwrap_or(1),
                t if t.starts_with("place=") => {
                    // place=KIND:CX:CY puts a turret / building / unit of the chosen side on a cell (like a click with the tool armed)
                    let side = game.dev.side;
                    let v: Vec<i32> = t[6..].split(':').filter_map(|n| n.parse().ok()).collect();
                    if let (Some(p), [kind, cx, cy]) = (game.play.as_mut(), v.as_slice()) {
                        if devtools::is_structure(*kind) {
                            p.world.dev_place_structure(*kind, side, (*cx, *cy));
                        } else {
                            p.world.add_unit_near(*kind, side as i32, cx * 32 + 16, cy * 32 + 16);
                        }
                    }
                }
                t if t.starts_with("arm=") => {
                    let side = game.dev.side;
                    if let Some(p) = game.play.as_mut() {
                        p.dev_spawn = t[4..].parse().ok().map(|k| (k, side));
                    }
                }
                _ => {}
            }
        }
    }
}

/// `--skirmish MAP [--side green] [--difficulty easy|normal|hard|brutal] [--credits BLUE,GREEN] [--research BLUE,GREEN]
/// [--spectate [--blue-difficulty D]]`: the lobby's choices from the command line (docs/skirmish.md), headless or windowed.
pub(super) fn start_skirmish_from_args(game: &mut Game, map: &str, get: &dyn Fn(&str) -> Option<String>, has: &dyn Fn(&str) -> bool) {
    let pair = |k: &str| get(k).and_then(|s| s.split_once(',').and_then(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?))));
    let level = |k: &str| sim::Difficulty::ALL.into_iter().find(|d| get(k).is_some_and(|v| v.eq_ignore_ascii_case(d.name())));
    let difficulty = level("--difficulty").unwrap_or_default();
    let spectate = has("--spectate").then(|| level("--blue-difficulty").unwrap_or(difficulty));
    game.headless_skirmish(
        map,
        usize::from(get("--side").is_some_and(|s| s.eq_ignore_ascii_case("green"))),
        pair("--credits"),
        pair("--research"),
        difficulty,
        spectate,
    );
}

/// `--gpu-check`: the frame drawn on the CPU, recorded + replayed on the CPU, and replayed on the GPU; prints how many pixels differ.
pub(super) fn gpu_check(game: &mut Game, w: usize, h: usize) {
    use opensow_viewer::record;
    let mut cpu = vec![0u32; w * h];
    let t0 = Instant::now();
    game.draw(&mut cpu, w, h);
    let t_cpu = t0.elapsed();
    let mut dummy = vec![0u32; w * h];
    let t0 = Instant::now();
    record::begin(w, h);
    game.draw(&mut dummy, w, h);
    let rec = record::end().expect("recording");
    let t_rec = t0.elapsed();
    let diff = |a: &[u32], b: &[u32]| {
        let bad: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        let first: Vec<String> = bad.iter().take(5).map(|&i| format!("({},{}) cpu {:06x} got {:06x}", i % w, i / w, a[i], b[i])).collect();
        format!("{} differ{}", bad.len(), if first.is_empty() { String::new() } else { format!(": {}", first.join(", ")) })
    };
    println!(
        "gpu-check {w}x{h}: {} commands, {} lens, {} map words, tainted {}; cpu draw {:.1} ms, record {:.1} ms",
        rec.cmds.len(),
        rec.cmds.iter().filter(|c| c.op == record::op::LENS).count(),
        rec.maps.len(),
        rec.tainted,
        t_cpu.as_secs_f64() * 1e3,
        t_rec.as_secs_f64() * 1e3
    );
    // The first draws above also fill caches (enhanced sprites, the atlas). What a running game pays per frame is the
    // steady cost: the fastest of a few more draws.
    let fastest = |f: &mut dyn FnMut()| {
        (0..8)
            .map(|_| {
                let t = Instant::now();
                f();
                t.elapsed().as_secs_f64() * 1e3
            })
            .fold(f64::MAX, f64::min)
    };
    let steady_cpu = fastest(&mut || game.draw(&mut dummy, w, h));
    let steady_rec = fastest(&mut || {
        record::begin(w, h);
        game.draw(&mut dummy, w, h);
        let _ = record::end();
    });
    println!("  steady state: cpu draw {steady_cpu:.1} ms, record {steady_rec:.1} ms");
    let mut replayed = vec![0u32; w * h];
    record::replay(&rec, &mut replayed);
    println!("  cpu replay: {}", diff(&cpu, &replayed));
    match gpu::headless() {
        Some((device, queue, name)) => {
            let mut r = replay::Replay::new(&device);
            let out = r.readback(&device, &queue, &rec);
            let t0 = Instant::now();
            let _ = r.readback(&device, &queue, &rec);
            println!(
                "  gpu replay ({name}, {:.1} ms incl. upload + readback): {}; against the cpu replay: {}",
                t0.elapsed().as_secs_f64() * 1e3,
                diff(&cpu, &out),
                diff(&replayed, &out)
            );
        }
        None => println!("  no GPU adapter"),
    }
    gpu_check_synthetic();
}

/// Every recorded operation (all blend modes, the lens, masks, the `ui` and `gdi` helpers) on made-up sprites at several zooms,
/// drawn on the CPU and replayed on the GPU.
pub(super) fn gpu_check_synthetic() {
    use opensow_formats::effects::Blit;
    use opensow_formats::sprite::Frame;
    use opensow_viewer::record;
    use opensow_viewer::render::{self, View};
    let Some((device, queue, _)) = gpu::headless() else { return };
    let mut r = replay::Replay::new(&device);
    let frame = |w: u16, h: u16, seed: u32| {
        let n = (w as usize) * (h as usize);
        let px: Vec<u16> = (0..n as u32).map(|i| (i.wrapping_mul(2_654_435_761).wrapping_add(seed.wrapping_mul(40_503)) >> 7) as u16).collect();
        Frame {
            width: w,
            height: h,
            hot_x: (w / 3) as i16,
            hot_y: (h / 2) as i16,
            extra: [0; 4],
            pixels: px,
            mask: (0..n).map(|i| !(i * 7 + seed as usize).is_multiple_of(5)).collect(),
            enhanced: Default::default(),
        }
    };
    let (a, b, m) = (frame(23, 17, 1), frame(60, 44, 2), frame(30, 22, 3));
    let gdi = gdi::Gdi::load();
    let (w, h) = (331usize, 207usize);
    for zoom in [1.0f32, 1.37, 2.0, 2.5, 3.0] {
        let view = View { x: -3.3, y: 4.1, zoom };
        let draw = |buf: &mut [u32]| {
            let mut mask = vec![false; w * h];
            render::rect(buf, w, h, -5, 3, 140, 170, (200, 10, 99), 77);
            render::blit(buf, w, h, &view, &a, 10, 12, false);
            render::blit(buf, w, h, &view, &b, 30, 20, true);
            render::stamp_frame(&mut mask, w, h, &view, &m, 25, 25);
            render::blit_shadow_masked(buf, w, h, &view, &a, 28, 26, &mask);
            opensow_viewer::fx::blob_masked(buf, w, h, &view, 30.0, 28.0, 20.0, 12.0, 140.0, Some(&mask));
            for (k, mode) in [Blit::Plain, Blit::Additive, Blit::AddHalf, Blit::Average, Blit::Smoke, Blit::Darken, Blit::Shade, Blit::Lens, Blit::Lens]
                .into_iter()
                .enumerate()
            {
                let masked = (k % 2 == 1).then_some(&mask[..]);
                render::blit_blend_masked(buf, w, h, &view, &b, 5 + 9 * k as i32, 8 + 5 * k as i32, mode, masked);
            }
            render::blit_masked(buf, w, h, &view, &b, &m, 40, 30, (3, 2, 290, 150));
            render::blit_clip(buf, w, h, &view, &a, 60, 40, false, (50, 30, 180, 155));
            ui::sprite_lit(buf, w, h, &a, 200, 10, zoom, 90);
            ui::sprite_glow(buf, w, h, &b, 210, 40, zoom);
            ui::sprite_add(buf, w, h, &b, 150, 100, zoom, true);
            ui::sprite_add(buf, w, h, &a, 250, 120, zoom, false);
            ui::sprite_dim(buf, w, h, &b, 170, 150, zoom, 120);
            ui::dim_sub(buf, w, h, ui::Rect { x: 100, y: 90, w: 80, h: 60 }, 40);
            ui::dim(buf, w, h, ui::Rect { x: 120, y: 140, w: 280, h: 30 }, 90);
            if let Some(g) = &gdi {
                g.draw_rgb(buf, w, h, "Lens & glow 123", 12.0, 170.0, zoom.min(2.0), (255, 220, 90));
            }
        };
        // both start from black (the GPU clears its picture)
        let mut cpu_black = vec![0u32; w * h];
        draw(&mut cpu_black);
        record::begin(w, h);
        let mut dummy = vec![0u32; w * h];
        draw(&mut dummy);
        let rec = record::end().unwrap();
        let out = r.readback(&device, &queue, &rec);
        let bad = (0..w * h).filter(|&i| out[i] != cpu_black[i]).count();
        for i in (0..w * h).filter(|&i| out[i] != cpu_black[i]).take(4) {
            println!("    ({},{}) cpu {:06x} gpu {:06x}", i % w, i / w, cpu_black[i], out[i]);
        }
        println!("  synthetic zoom {zoom}: {} commands ({} lens), {bad} differ", rec.cmds.len(), rec.cmds.iter().filter(|c| c.op == record::op::LENS).count());
    }
}
