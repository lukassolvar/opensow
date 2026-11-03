//! `opensow-game`: a playable prototype with the original menus and a skirmish on the campaign levels.
//!
//! ```text
//! opensow-game [--no-intro] [--mute] [--lang SK|CZ|PL|HU|RU|CHINNESSE]   start the game window (plays the intro movie first; any key or click skips it)
//! opensow-game --screenshot out.png [--size WxH] [--screen main|levels|options|pause|message]
//!              [--mute] [--intro-frame N [--movie mI|mO|M01..M22]] [--hover N] [--play M01 [--ticks N] [--select-all] [--move CX,CY] [--zoom Z] [--center CX,CY] [--range]]
//! ```

mod assets;
mod intro;
mod lands;
mod menu;
mod music;
mod play;
mod sfx;
mod sim;
mod ui;

use assets::Assets;
use menu::{Action, Screen, Settings, SPEEDS};
use opensow_formats::GameDir;
use play::Play;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

/// What happens when the movie on screen ends or is skipped.
#[derive(Debug, Clone, PartialEq)]
enum Then {
    Menu,
    /// The briefing is over: start the mission.
    Level(String),
    /// The outro is over: show the closing message.
    Screen(Screen),
}

/// The last campaign mission; winning it plays the outro (`mO.sgv`).
const FINAL_LEVEL: &str = "M22";

struct Game {
    assets: Assets,
    settings: Settings,
    levels: Vec<String>,
    /// Menu shown on top of (or instead of) the game.
    screen: Option<Screen>,
    back: Vec<Screen>,
    play: Option<Box<Play>>,
    hover: Option<usize>,
    pressed: Option<usize>,
    shift: bool,
    scale: f32,
    cursor: (i32, i32),
    quit: bool,
    /// The intro movie while it plays; it replaces everything else on screen.
    intro: Option<intro::Intro>,
    then: Then,
    language: usize,
    /// Play the briefing movies and the outro (off for headless runs and `--no-intro`).
    movies: bool,
    /// Do not open the audio device (`--mute`, tests).
    mute: bool,
    /// The campaign map while it is on screen; it replaces everything else.
    lands: Option<lands::Lands>,
    /// Which lands are open (`player.cfg` in the original; kept in memory here, never written into the install).
    progress: opensow_formats::config::PassedLevels,
    /// The running mission was chosen on the campaign map: the map comes back afterwards.
    campaign: bool,
    music: music::Music,
    sfx: sfx::Sfx,
}

impl Game {
    fn new(assets: Assets) -> Game {
        let levels = opensow_viewer::scene::Scene::list(&assets.game);
        Game { assets, settings: Settings::default(), levels, screen: Some(Screen::Main), back: Vec::new(), play: None, hover: None, pressed: None, shift: false, scale: 1.0, cursor: (0, 0), quit: false, intro: None, then: Then::Menu, language: 0, movies: false, mute: false, lands: None, progress: opensow_formats::config::PassedLevels::initial(), campaign: false, music: music::Music::new(), sfx: sfx::Sfx::new() }
    }

    fn buttons(&mut self, w: usize, h: usize) -> Vec<menu::Button> {
        match &self.screen {
            Some(s) => menu::layout(s, &mut self.assets, &self.levels, &self.settings, w, h),
            None => Vec::new(),
        }
    }

    /// Play a sound effect by the exe's id (silent unless the real game turned sound on).
    fn play_sfx(&mut self, id: u8) {
        self.sfx.play(&self.assets.game, id, 1.0);
    }

    /// Switch the background music (only in the real game: movies on and not muted).
    fn play_music(&mut self, name: &str) {
        if self.movies && !self.mute {
            self.music.play(&self.assets.game, name);
        }
    }

    /// Show a movie, then do `then`; without a movie (missing file, movies off) `then` happens at once.
    fn play_movie(&mut self, name: &str, then: Then) {
        self.then = then;
        if self.movies {
            match intro::Intro::open(&self.assets.game, name, self.language) {
                Ok(mut m) => {
                    self.music.stop();
                    if !self.mute && !m.start_sound() {
                        eprintln!("{name}: no audio device, playing silently");
                    }
                    self.intro = Some(m);
                    return;
                }
                Err(e) => eprintln!("no movie {name}: {e}"),
            }
        }
        self.end_movie();
    }

    fn end_movie(&mut self) {
        self.intro = None;
        match std::mem::replace(&mut self.then, Then::Menu) {
            Then::Menu => self.play_music("Menu"),
            Then::Level(l) => self.start(&l),
            Then::Screen(s) => self.screen = Some(s),
        }
    }

    /// Start a mission the way the exe does: its briefing movie first.
    fn begin(&mut self, level: &str) {
        self.play_movie(level, Then::Level(level.to_owned()));
    }

    fn start(&mut self, level: &str) {
        match Play::new(&mut self.assets, level, &self.settings) {
            Ok(p) => {
                self.play_music(&format!("Level{:02}", 1 + music::pick(4)));
                self.play = Some(Box::new(p));
                self.screen = None;
                self.back.clear();
            }
            Err(e) => {
                eprintln!("cannot start {level}: {e}");
                self.screen = Some(Screen::Message(format!("Cannot start mission {level}:\n{e}")));
            }
        }
    }

    fn act(&mut self, a: Action) {
        match a {
            Action::NewGame => self.open_lands(true),
            Action::Unavailable(msg) => self.push(Screen::Message(msg.to_owned())),
            Action::Options => self.push(Screen::Options),
            Action::Exit => self.quit = true,
            Action::Level(l) => {
                self.campaign = false;
                self.begin(&l)
            }
            Action::Back => {
                if matches!(self.screen, Some(Screen::Message(_))) && self.play.is_some() {
                    if self.campaign {
                        // after a campaign mission the map comes back
                        self.play = None;
                        self.screen = None;
                        self.open_lands(false);
                    } else {
                        self.act(Action::ToMain);
                    }
                } else {
                    self.screen = Some(self.back.pop().unwrap_or(Screen::Main));
                }
            }
            Action::CycleSpeed => {
                let i = SPEEDS.iter().position(|&s| s == self.settings.ticks_per_second).map_or(0, |i| (i + 1) % SPEEDS.len());
                self.settings.ticks_per_second = SPEEDS[i];
            }
            Action::ToggleHp => self.settings.hp_bars ^= true,
            Action::ToggleRange => self.settings.show_range ^= true,
            Action::ToggleSquad => self.settings.starter_squad ^= true,
            Action::Resume => {
                self.screen = None;
                self.back.clear();
            }
            Action::Restart => {
                if let Some(l) = self.play.as_ref().map(|p| p.level.clone()) {
                    self.start(&l);
                }
            }
            Action::ToMain => {
                self.play = None;
                self.campaign = false;
                self.play_music("Menu");
                self.back.clear();
                self.screen = Some(Screen::Main);
            }
        }
    }

    /// Open the campaign map; `video` plays the BRIEFING animation first (New game).
    fn open_lands(&mut self, video: bool) {
        self.screen = None;
        self.back.clear();
        self.play_music(if music::pick(2) == 0 { "Brief" } else { "Brief2" });
        self.lands = Some(lands::Lands::new(&mut self.assets, self.progress.clone(), video && self.movies, self.mute));
    }

    /// Play the sounds the campaign map asked for.
    fn lands_sounds(&mut self) {
        let ids: Vec<u8> = self.lands.as_mut().map(|l| std::mem::take(&mut l.sounds)).unwrap_or_default();
        for id in ids {
            self.play_sfx(id);
        }
    }

    /// What the map decided: BACK to the main menu, or ACCEPT a land's mission.
    fn lands_event(&mut self, e: lands::Event) {
        match e {
            lands::Event::None => {}
            lands::Event::Back => {
                self.lands = None;
                self.screen = Some(Screen::Main);
                self.play_music("Menu");
            }
            lands::Event::Accept(land) => {
                self.lands = None;
                self.campaign = true;
                self.begin(&format!("M{land:02}"));
            }
        }
    }

    fn push(&mut self, next: Screen) {
        if let Some(cur) = self.screen.replace(next) {
            self.back.push(cur);
        }
    }

    fn draw(&mut self, buf: &mut [u32], w: usize, h: usize) {
        if let Some(i) = &self.intro {
            i.draw(buf, w, h);
            return;
        }
        if let Some(l) = self.lands.as_mut() {
            l.draw(buf, w, h);
            return;
        }
        if let Some(play) = self.play.as_mut() {
            play.draw(&mut self.assets, &self.settings, buf, w, h, self.scale);
        }
        let buttons = self.buttons(w, h);
        if let Some(screen) = self.screen.clone() {
            menu::draw(&screen, &buttons, self.hover, &mut self.assets, buf, w, h, self.play.is_some());
        }
    }

    /// Per-frame update; opens the outcome message when the mission ends.
    fn update(&mut self, dt: f32, w: usize, h: usize) {
        if let Some(i) = self.intro.as_mut() {
            i.update(dt as f64);
            if i.finished() {
                self.end_movie();
            }
            return;
        }
        if let Some(l) = self.lands.as_mut() {
            l.update(dt as f64);
            self.lands_sounds();
            return;
        }
        if self.screen.is_none() {
            if let Some(p) = self.play.as_mut() {
                p.update(dt, &self.settings, w, h);
                let (zoom, vx, vy) = (p.view.zoom, p.view.x, p.view.y);
                let heard: Vec<(u8, bool)> = p.world.sounds.drain(..).filter(|&(_, x, y, _)| {
                    // only what is on screen (the exe pans by position; this just leaves out the far-away battles)
                    let (sx, sy) = ((x as f32 - vx) * zoom, (y as f32 - vy) * zoom);
                    sx > -64.0 && sy > -64.0 && sx < w as f32 + 64.0 && sy < h as f32 + 64.0
                }).map(|(id, _, _, j)| (id, j)).collect();
                for (id, jitter) in heard {
                    self.sfx.play(&self.assets.game, id, if jitter { sfx::jitter() } else { 1.0 });
                }
                let (outcome, level) = (p.outcome(), p.level.clone());
                match outcome {
                    Some(sim::Outcome::Victory) => {
                        let done = Screen::Message("Mission complete!\nThe enemy is defeated.".into());
                        if self.campaign {
                            // CPassedLevels::Pass: this land is done and the next one opens
                            let n = level.trim_start_matches(['M', 'm']).parse().unwrap_or(0);
                            self.progress.pass(n);
                        }
                        if level.eq_ignore_ascii_case(FINAL_LEVEL) {
                            self.play_movie("mO", Then::Screen(done));
                        } else {
                            self.screen = Some(done);
                        }
                    }
                    Some(sim::Outcome::Defeat) => self.screen = Some(Screen::Message("Mission failed.\nYour forces were destroyed.".into())),
                    None => {}
                }
            }
        }
    }

    fn mouse_moved(&mut self, x: i32, y: i32, w: usize, h: usize) {
        self.cursor = (x, y);
        if let Some(l) = self.lands.as_mut() {
            l.mouse_moved(x, y, w, h);
            self.lands_sounds();
            return;
        }
        if let Some(p) = self.play.as_mut() {
            p.cursor = (x, y);
            p.edge_scroll = true;
        }
        let buttons = self.buttons(w, h);
        let hover = menu::hit(&buttons, x, y);
        if hover.is_some() && hover != self.hover && self.screen.is_some() {
            self.play_sfx(30); // mainmenu-move
        }
        self.hover = hover;
    }

    fn mouse_button(&mut self, button: MouseButton, down: bool, x: i32, y: i32, w: usize, h: usize) {
        if let Some(l) = self.lands.as_mut() {
            if down && button == MouseButton::Left {
                let e = l.click(x, y, w, h);
                self.progress = l.progress.clone();
                self.lands_sounds();
                self.lands_event(e);
            }
            return;
        }
        if self.intro.is_some() {
            if down {
                self.end_movie();
            }
            return;
        }
        if self.screen.is_some() {
            let buttons = self.buttons(w, h);
            let hit = menu::hit(&buttons, x, y);
            if button == MouseButton::Left {
                if down {
                    self.pressed = hit;
                } else {
                    if hit.is_some() && hit == self.pressed {
                        let a = buttons[hit.unwrap()].action.clone();
                        self.play_sfx(match a {
                            Action::CycleSpeed => 33,
                            Action::ToggleHp | Action::ToggleRange | Action::ToggleSquad => 34,
                            _ => 29, // mainmenu-ok
                        });
                        self.act(a);
                        self.hover = None;
                    }
                    self.pressed = None;
                }
            }
            return;
        }
        let Some(p) = self.play.as_mut() else { return };
        match (button, down) {
            (MouseButton::Left, true) => p.left_down(x, y),
            (MouseButton::Left, false) => p.left_up(x, y, self.shift),
            (MouseButton::Right, true) => p.right_click(x, y),
            _ => {}
        }
        let ids: Vec<u8> = std::mem::take(&mut p.ui_sounds);
        for id in ids {
            self.play_sfx(id);
        }
    }

    fn key(&mut self, key: &Key, down: bool, w: usize, h: usize) {
        if let Some(l) = self.lands.as_mut() {
            if down {
                let e = if matches!(key, Key::Named(NamedKey::Escape)) { l.back() } else if l.playing_video() { l.back() } else { lands::Event::None };
                self.lands_event(e);
            }
            return;
        }
        // developer shortcut: every mission, whether or not the campaign has opened it
        if down && matches!(self.screen, Some(Screen::Main)) && matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("l")) {
            self.push(Screen::Levels);
            return;
        }
        if self.intro.is_some() {
            if down {
                self.end_movie();
            }
            return;
        }
        let name = match key {
            Key::Named(NamedKey::ArrowLeft) => Some("left"),
            Key::Named(NamedKey::ArrowRight) => Some("right"),
            Key::Named(NamedKey::ArrowUp) => Some("up"),
            Key::Named(NamedKey::ArrowDown) => Some("down"),
            Key::Character(c) => match c.to_ascii_lowercase().as_str() {
                "a" => Some("left"),
                "d" => Some("right"),
                "w" => Some("up"),
                "s" => Some("down"),
                _ => None,
            },
            _ => None,
        };
        if let (Some(n), Some(p)) = (name, self.play.as_mut()) {
            p.key(n, down);
        }
        if let Key::Named(NamedKey::Shift) = key {
            self.shift = down;
        }
        if !down {
            return;
        }
        match key {
            Key::Named(NamedKey::Escape) => match self.screen {
                None if self.play.is_some() => self.screen = Some(Screen::Pause),
                Some(Screen::Pause) => self.act(Action::Resume),
                Some(Screen::Main) => {}
                Some(_) => self.act(Action::Back),
                None => {}
            },
            Key::Named(NamedKey::Space) if self.screen.is_none() => {
                if let Some(p) = self.play.as_mut() {
                    p.paused ^= true;
                }
            }
            Key::Character(c) if self.screen.is_none() && c.eq_ignore_ascii_case("q") => {
                if let Some(p) = self.play.as_mut() {
                    p.world.select_all();
                }
            }
            Key::Character(c) if self.screen.is_none() && matches!(c.as_str(), "1" | "2" | "3" | "4" | "5") => {
                if let Some(p) = self.play.as_mut() {
                    let tier: u32 = c.parse().unwrap_or(1);
                    p.placing = (p.world.credits[0] >= 200 * tier as i32).then_some(tier);
                }
            }
            Key::Character(c) if self.screen.is_none() && matches!(c.to_ascii_lowercase().as_str(), "b" | "m" | "c" | "y" | "f") => {
                use opensow_formats::air::AirForce as F;
                if let Some(p) = self.play.as_mut() {
                    let f = match c.to_ascii_lowercase().as_str() {
                        "b" => F::Bomber,
                        "m" => F::Asteroid,
                        "c" => F::Carryall,
                        "y" => F::Tripler,
                        _ => F::Fighter,
                    };
                    p.strike = Some(f);
                }
            }
            Key::Character(c) if self.screen.is_none() && c.eq_ignore_ascii_case("t") => {
                if let Some(p) = self.play.as_mut() {
                    p.world.tech_up_all(0);
                }
            }
            Key::Character(c) if self.screen.is_none() && c.eq_ignore_ascii_case("r") => {
                if let Some(p) = self.play.as_mut() {
                    p.world.repair_mode ^= true;
                }
            }
            _ => {}
        }
        let _ = (w, h);
    }
}

struct App {
    game: Game,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    last: Instant,
}

impl App {
    fn size(&self) -> (usize, usize) {
        self.window.as_ref().map_or((1280, 800), |w| (w.inner_size().width.max(1) as usize, w.inner_size().height.max(1) as usize))
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("OpenSOW").with_inner_size(LogicalSize::new(1280.0, 800.0));
        let window = Arc::new(el.create_window(attrs).expect("window"));
        let ctx = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        self.surface = Some(softbuffer::Surface::new(&ctx, window.clone()).expect("surface"));
        self.game.scale = window.scale_factor() as f32;
        self.window = Some(window);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.game.quit {
            el.exit();
            return;
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(16)));
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let (w, h) = self.size();
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now.duration_since(self.last).as_secs_f32().min(0.1);
                self.last = now;
                self.game.update(dt, w, h);
                let Some(surface) = self.surface.as_mut() else { return };
                if surface.resize(NonZeroU32::new(w as u32).unwrap(), NonZeroU32::new(h as u32).unwrap()).is_err() {
                    return;
                }
                if let Ok(mut buf) = surface.buffer_mut() {
                    self.game.draw(&mut buf, w, h);
                    let _ = buf.present();
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.game.mouse_moved(position.x as i32, position.y as i32, w, h),
            WindowEvent::MouseInput { state, button, .. } => {
                let (x, y) = self.game.cursor;
                self.game.mouse_button(button, state == ElementState::Pressed, x, y, w, h);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                if self.game.screen.is_none() {
                    if let Some(p) = self.game.play.as_mut() {
                        p.wheel(dy, w, h);
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => self.game.key(&event.logical_key, event.state == ElementState::Pressed, w, h),
            _ => {}
        }
    }
}

fn write_png(path: &str, buf: &[u32], w: usize, h: usize) {
    let rgb: Vec<u8> = buf.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path).expect("create png")), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut w| w.write_image_data(&rgb)).expect("write png");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let has = |name: &str| args.iter().any(|a| a == name);
    let game_dir = GameDir::locate().unwrap_or_else(|| {
        eprintln!("error: game not found; set OPENSOW_GAME_DIR");
        std::process::exit(1);
    });
    let assets = Assets::load(game_dir).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });
    let mut game = Game::new(assets);
    game.language = get("--lang").and_then(|l| intro::LANGUAGES.iter().position(|n| n.eq_ignore_ascii_case(&l))).unwrap_or(0);

    if let Some(path) = get("--screenshot") {
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
        if let Some(level) = get("--play") {
            game.start(&level.to_ascii_uppercase());
            if let Some(p) = game.play.as_mut() {
                let pair = |s: String| s.split_once(',').map(|(a, b)| (a.parse::<f32>().unwrap_or(0.0), b.parse::<f32>().unwrap_or(0.0)));
                p.update(0.0, &game.settings, w, h);
                if let Some(z) = get("--zoom").and_then(|z| z.parse().ok()) {
                    p.view.zoom = z;
                    p.centre_on_player(w, h);
                }
                if let Some((cx, cy)) = get("--center").and_then(pair) {
                    p.view.x = (cx + 0.5) * 32.0 - w as f32 / p.view.zoom / 2.0;
                    p.view.y = (cy + 0.5) * 32.0 - h as f32 / p.view.zoom / 2.0;
                }
                if has("--select-all") {
                    p.world.select_all();
                }
                if let Some((cx, cy)) = get("--act").and_then(pair) {
                    // headless: every Advancer of the player works on the building at this cell
                    if let Some(t) = p.world.structure_at(cx as i32 * 32 + 16, cy as i32 * 32 + 16) {
                        let ids: Vec<u32> = p.world.player_units().filter(|u| u.kind == 40).map(|u| u.id).collect();
                        for id in ids {
                            p.world.order_action(id, t);
                        }
                    }
                }
                if let Some(spec) = get("--fx") {
                    // headless: --fx CX,CY,KIND,AGE puts an effect on a cell: KIND = blast0..blast6, turret, building, air, cloud, wreck, puff, decal (AGE = variant)
                    let f: Vec<&str> = spec.split(',').collect();
                    if let [cx, cy, kind, age] = f[..] {
                        use opensow_formats::effects as e;
                        let spec = match kind {
                            "turret" => e::TURRET_DESTROYED,
                            "building" => e::BUILDING_DESTROYED,
                            "air" => e::AIR_DESTROYED,
                            "cloud" => e::UNIT_CLOUD,
                            "wreck" => e::UNIT_WRECK,
                            "puff" => e::smoke_puff(0, 4),
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
                if let Some((cx, cy)) = get("--move").and_then(pair) {
                    p.world.order_move_selected(cx as i32 * 32 + 16, cy as i32 * 32 + 16);
                }
                for _ in 0..get("--ticks").and_then(|t| t.parse().ok()).unwrap_or(0) {
                    p.world.step();
                }
                game.settings.show_range = has("--range");
                if has("--report") {
                    for u in &p.world.units {
                        println!("unit {} kind {} player {} at ({:.0},{:.0}) cell {:?} hp {}/{} path {} target {:?}", u.id, u.kind, u.player, u.x, u.y, u.cell(), u.hp, u.max_hp, u.path.len(), u.target);
                    }
                    for s in p.world.structures.iter().filter(|s| s.tally != [0; 3] || s.hp != s.max_hp || s.heal > 0 || s.owner == opensow_formats::level::Owner::Neutral || s.kind_id == 106) {
                        println!("structure kind {} cell {:?} owner {:?} hp {}/{} heal {} tally {:?} active {}", s.kind_id, s.cell, s.owner, s.hp, s.max_hp, s.heal, s.tally, s.active);
                    }
                    println!("bullets {} tick {} outcome {:?}", p.world.bullets.len(), p.world.tick, p.world.outcome);
                }
            }
            if has("--pause") {
                game.screen = Some(Screen::Pause);
            }
        } else {
            game.screen = Some(match get("--screen").as_deref() {
                Some("levels") => Screen::Levels,
                Some("options") => Screen::Options,
                Some("pause") => Screen::Pause,
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
                        l.update(get("--clock").and_then(|n| n.parse().ok()).unwrap_or(0.0));
                    }
                    Screen::Main
                }
                _ => Screen::Main,
            });
        }
        if game.lands.is_some() {
            game.screen = None;
        } else {
            game.hover = get("--hover").and_then(|h| h.parse().ok());
        }
        let mut buf = vec![0u32; w * h];
        game.draw(&mut buf, w, h);
        write_png(&path, &buf, w, h);
        println!("-> {path} ({w}x{h})");
        return;
    }

    game.mute = has("--mute");
    game.sfx.enabled = !game.mute;
    if !has("--no-intro") {
        game.movies = true;
        game.play_movie("mI", Then::Menu);
    }
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App { game, window: None, surface: None, last: Instant::now() };
    event_loop.run_app(&mut app).expect("run");
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    fn game() -> Option<Game> {
        Assets::load(GameDir::locate()?).ok().map(Game::new)
    }

    #[test]
    fn menu_navigation() {
        let Some(mut g) = game() else { return };
        let (w, h) = (1280, 800);
        let b = g.buttons(w, h);
        assert_eq!(b.len(), 6);
        let first = b[0].rect;
        g.mouse_moved(first.x + 5, first.y + 5, w, h);
        assert_eq!(g.hover, Some(0));
        g.mouse_button(MouseButton::Left, true, first.x + 5, first.y + 5, w, h);
        g.mouse_button(MouseButton::Left, false, first.x + 5, first.y + 5, w, h);
        // New game opens the campaign map; Escape (or BACK) returns to the main menu
        assert!(g.lands.is_some() && g.screen.is_none());
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert!(g.lands.is_none());
        assert_eq!(g.screen, Some(Screen::Main));
        // unavailable entries show a message and go back
        let load = g.buttons(w, h)[1].rect;
        g.mouse_button(MouseButton::Left, true, load.x + 5, load.y + 5, w, h);
        g.mouse_button(MouseButton::Left, false, load.x + 5, load.y + 5, w, h);
        assert!(matches!(g.screen, Some(Screen::Message(_))));
        g.act(Action::Back);
        assert_eq!(g.screen, Some(Screen::Main));
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        assert!(buf.iter().any(|&p| p != 0));
    }

    #[test]
    fn playing_a_mission() {
        let Some(mut g) = game() else { return };
        let (w, h) = (1024, 768);
        g.start("M01");
        assert!(g.play.is_some() && g.screen.is_none());
        g.update(0.0, w, h);
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        // box-select everything on screen, then order a move
        g.mouse_button(MouseButton::Left, true, 2, 30, w, h);
        g.mouse_button(MouseButton::Left, false, w as i32 - 2, h as i32 - 30, w, h);
        let selected = g.play.as_ref().unwrap().world.units.iter().filter(|u| u.selected).count();
        assert!(selected >= 4, "box selection picked {selected}");
        g.mouse_moved(w as i32 / 2, h as i32 / 2, w, h);
        g.mouse_button(MouseButton::Right, true, w as i32 / 2, h as i32 / 2 - 100, w, h);
        assert!(g.play.as_ref().unwrap().world.units.iter().any(|u| u.selected && !u.path.is_empty()));
        for _ in 0..30 {
            g.update(0.05, w, h);
        }
        g.draw(&mut buf, w, h);
        // pause menu
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert_eq!(g.screen, Some(Screen::Pause));
        g.draw(&mut buf, w, h);
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert!(g.screen.is_none());
        g.key(&Key::Character(SmolStr::new("q")), true, w, h);
        g.act(Action::ToMain);
        assert_eq!(g.screen, Some(Screen::Main));
        assert!(g.play.is_none());
    }

    #[test]
    fn briefing_movie_before_the_mission_and_outro_after_the_last_one() {
        let Some(mut g) = game() else { return };
        let (w, h) = (640, 480);
        g.movies = true;
        g.mute = true;
        // choosing a mission shows its briefing first; a click skips it and the mission starts
        g.act(Action::Level("M01".into()));
        assert_eq!(g.intro.as_ref().map(|i| i.name.as_str()), Some("M01"));
        assert!(g.play.is_none());
        let mut buf = vec![0u32; w * h];
        g.update(0.1, w, h);
        g.draw(&mut buf, w, h);
        assert!(buf.iter().any(|&p| p != 0));
        g.mouse_button(MouseButton::Left, true, 5, 5, w, h);
        assert!(g.intro.is_none() && g.play.is_some() && g.screen.is_none());
        // restarting does not replay the briefing
        g.act(Action::Restart);
        assert!(g.intro.is_none() && g.play.is_some());
        // winning the final mission plays the outro, then shows the message
        g.start(FINAL_LEVEL);
        g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
        g.update(0.0, w, h);
        assert_eq!(g.intro.as_ref().map(|i| i.name.as_str()), Some("mO"));
        g.key(&Key::Named(NamedKey::Space), true, w, h);
        assert!(g.intro.is_none() && matches!(g.screen, Some(Screen::Message(_))));
        // winning any other mission goes straight to the message
        g.start("M02");
        g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
        g.update(0.0, w, h);
        assert!(g.intro.is_none() && matches!(g.screen, Some(Screen::Message(_))));
    }

    #[test]
    fn campaign_map_to_mission_and_back_with_progress() {
        let Some(mut g) = game() else { return };
        let (w, h) = (640, 480);
        g.act(Action::NewGame);
        assert!(g.lands.is_some());
        // pick land 1 (Canada) on its pixels, accept at the bottom left
        let (px, py) = (170, 190);
        g.mouse_moved(px, py, w, h);
        assert_eq!(g.lands.as_ref().unwrap().hovered(), Some(1));
        g.mouse_button(MouseButton::Left, true, px, py, w, h);
        assert_eq!(g.lands.as_ref().unwrap().selected(), Some(1));
        g.mouse_moved(40, 450, w, h);
        g.mouse_button(MouseButton::Left, true, 40, 450, w, h);
        assert!(g.lands.is_none() && g.play.is_some(), "ACCEPT starts mission 1");
        assert_eq!(g.play.as_ref().unwrap().level, "M01");
        // winning passes the land, opens the next one in the exe's order and brings the map back
        g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
        g.update(0.0, w, h);
        assert_eq!((g.progress.states[1], g.progress.states[6]), (1, 2));
        g.act(Action::Back);
        assert!(g.lands.is_some() && g.play.is_none());
        assert_eq!(g.lands.as_ref().unwrap().progress.states[6], 2);
    }

    #[test]
    fn menus_and_the_map_raise_the_exes_interface_sounds() {
        let Some(mut g) = game() else { return };
        let (w, h) = (640, 480);
        // hovering a main-menu button plays mainmenu-move (30), clicking mainmenu-ok (29)
        let first = g.buttons(w, h)[0].rect;
        g.mouse_moved(first.x + 5, first.y + 5, w, h);
        assert_eq!(g.sfx.played, vec![30]);
        g.mouse_button(MouseButton::Left, true, first.x + 5, first.y + 5, w, h);
        g.mouse_button(MouseButton::Left, false, first.x + 5, first.y + 5, w, h);
        assert_eq!(g.sfx.played, vec![30, 29]);
        // the map: a land under the pointer says its location, selecting it flashes, frames and speaks
        g.sfx.played.clear();
        g.mouse_moved(170, 190, w, h);
        g.mouse_button(MouseButton::Left, true, 170, 190, w, h);
        assert_eq!(g.sfx.played, vec![41, 46, 41, 44]);
        // a shot from a turret or unit raises its muzzle sound
        g.sfx.played.clear();
        g.lands = None;
        g.start("M01");
        g.update(0.0, w, h);
        g.sfx.played.clear();
        let p = g.play.as_mut().unwrap();
        let (vx, vy) = (p.view.x as i32 + 50, p.view.y as i32 + 50);
        p.world.sounds.push((4, vx, vy, true));
        p.world.sounds.push((5, vx + 100_000, vy, true)); // far off screen: not heard
        g.update(0.0, w, h);
        assert!(!g.sfx.played.contains(&5));
        assert!(g.sfx.played.contains(&4), "{:?}", g.sfx.played);
    }
}
