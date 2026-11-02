//! `opensow-game`: a playable prototype with the original menus and a skirmish on the campaign levels.
//!
//! ```text
//! opensow-game                                   start the game window
//! opensow-game --screenshot out.png [--size WxH] [--screen main|levels|options|pause|message]
//!              [--hover N] [--play M01 [--ticks N] [--select-all] [--move CX,CY] [--zoom Z] [--center CX,CY] [--range]]
//! ```

mod assets;
mod menu;
mod play;
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
}

impl Game {
    fn new(assets: Assets) -> Game {
        let levels = opensow_viewer::scene::Scene::list(&assets.game);
        Game { assets, settings: Settings::default(), levels, screen: Some(Screen::Main), back: Vec::new(), play: None, hover: None, pressed: None, shift: false, scale: 1.0, cursor: (0, 0), quit: false }
    }

    fn buttons(&mut self, w: usize, h: usize) -> Vec<menu::Button> {
        match &self.screen {
            Some(s) => menu::layout(s, &mut self.assets, &self.levels, &self.settings, w, h),
            None => Vec::new(),
        }
    }

    fn start(&mut self, level: &str) {
        match Play::new(&mut self.assets, level, &self.settings) {
            Ok(p) => {
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
            Action::NewGame => self.push(Screen::Levels),
            Action::Unavailable(msg) => self.push(Screen::Message(msg.to_owned())),
            Action::Options => self.push(Screen::Options),
            Action::Exit => self.quit = true,
            Action::Level(l) => self.start(&l),
            Action::Back => {
                if matches!(self.screen, Some(Screen::Message(_))) && self.play.is_some() {
                    self.act(Action::ToMain);
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
                self.back.clear();
                self.screen = Some(Screen::Main);
            }
        }
    }

    fn push(&mut self, next: Screen) {
        if let Some(cur) = self.screen.replace(next) {
            self.back.push(cur);
        }
    }

    fn draw(&mut self, buf: &mut [u32], w: usize, h: usize) {
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
        if self.screen.is_none() {
            if let Some(p) = self.play.as_mut() {
                p.update(dt, &self.settings, w, h);
                match p.outcome() {
                    Some(sim::Outcome::Victory) => self.screen = Some(Screen::Message("Mission complete!\nThe enemy is defeated.".into())),
                    Some(sim::Outcome::Defeat) => self.screen = Some(Screen::Message("Mission failed.\nYour forces were destroyed.".into())),
                    None => {}
                }
            }
        }
    }

    fn mouse_moved(&mut self, x: i32, y: i32, w: usize, h: usize) {
        self.cursor = (x, y);
        if let Some(p) = self.play.as_mut() {
            p.cursor = (x, y);
            p.edge_scroll = true;
        }
        let buttons = self.buttons(w, h);
        self.hover = menu::hit(&buttons, x, y);
    }

    fn mouse_button(&mut self, button: MouseButton, down: bool, x: i32, y: i32, w: usize, h: usize) {
        if self.screen.is_some() {
            let buttons = self.buttons(w, h);
            let hit = menu::hit(&buttons, x, y);
            if button == MouseButton::Left {
                if down {
                    self.pressed = hit;
                } else {
                    if hit.is_some() && hit == self.pressed {
                        let a = buttons[hit.unwrap()].action.clone();
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
    }

    fn key(&mut self, key: &Key, down: bool, w: usize, h: usize) {
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

    if let Some(path) = get("--screenshot") {
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
                _ => Screen::Main,
            });
        }
        game.hover = get("--hover").and_then(|h| h.parse().ok());
        let mut buf = vec![0u32; w * h];
        game.draw(&mut buf, w, h);
        write_png(&path, &buf, w, h);
        println!("-> {path} ({w}x{h})");
        return;
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
        assert_eq!(g.screen, Some(Screen::Levels));
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
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
}
