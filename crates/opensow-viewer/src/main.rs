//! `opensow-viewer [LEVEL] [--screenshot out.png] [--size WxH] [--zoom Z] [--center CX,CY] [--facing 0..31] [--aim 0..31] [--walk TICKS] [--fire TICK] [--shot x1,y1,x2,y2,type]... [--range CX,CY,MIN,MAX]... [--overlay blocked,grid,objects,markers,sprites,muzzles]`
//!
//! Keys: arrows/WASD or left-drag pan, wheel or +/- zoom, B blocked cells, G grid,
//! O objects (squares for things without art), M markers, X sprites, F muzzle points, [ ] turn all units, K play the walk cycle, V loop the fire/recoil sequences, N/P next/previous level, Home fit map, Esc quit.

use opensow_formats::GameDir;
use opensow_viewer::render::{object_at, render, Overlays, View};
use opensow_viewer::scene::Scene;
use std::num::NonZeroU32;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowId};

struct Args {
    level: String,
    screenshot: Option<String>,
    size: (usize, usize),
    zoom: Option<f32>,
    center: Option<(f32, f32)>,
    facing: usize,
    aim: Option<usize>,
    walk: Option<u32>,
    fire: Option<u32>,
    shots: Vec<(i32, i32, i32, i32, u8)>,
    ranges: Vec<(i32, i32, usize, usize)>,
    overlays: Overlays,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { level: "M01".into(), screenshot: None, size: (1280, 800), zoom: None, center: None, facing: 0, aim: None, walk: None, fire: None, shots: Vec::new(), ranges: Vec::new(), overlays: Overlays::default() };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |what: &str| it.next().ok_or(format!("{arg} needs {what}"));
        match arg.as_str() {
            "--screenshot" => a.screenshot = Some(value("a path")?),
            "--zoom" => a.zoom = Some(value("a number")?.parse().map_err(|_| "bad --zoom")?),
            "--aim" => a.aim = Some(value("0..31")?.parse::<usize>().map_err(|_| "bad --aim")? % 32),
            "--walk" => a.walk = Some(value("a tick count")?.parse().map_err(|_| "bad --walk")?),
            "--shot" => {
                let v = value("x1,y1,x2,y2,type (map pixels)")?;
                let n: Vec<i32> = v.split(',').map(|t| t.trim().parse().map_err(|_| "bad --shot")).collect::<Result<_, _>>()?;
                if n.len() != 5 {
                    return Err("--shot expects x1,y1,x2,y2,type".into());
                }
                a.shots.push((n[0], n[1], n[2], n[3], n[4] as u8));
            }
            "--range" => {
                let v = value("CX,CY,MIN,MAX (cells, rings)")?;
                let n: Vec<i32> = v.split(',').map(|t| t.trim().parse().map_err(|_| "bad --range")).collect::<Result<_, _>>()?;
                if n.len() != 4 || n[2] < 0 || n[3] < n[2] || n[3] > 63 {
                    return Err("--range expects CX,CY,MIN,MAX with 0 <= MIN <= MAX <= 63".into());
                }
                a.ranges.push((n[0], n[1], n[2] as usize, n[3] as usize));
            }
            "--fire" => a.fire = Some(value("a tick (1 = first tick of the fire sequence)")?.parse().map_err(|_| "bad --fire")?),
            "--facing" => a.facing = value("0..31")?.parse::<usize>().map_err(|_| "bad --facing")? % 32,
            "--center" => {
                let v = value("CX,CY (cells)")?;
                let (x, y) = v.split_once(',').ok_or("--center expects CX,CY")?;
                a.center = Some((x.parse().map_err(|_| "bad center x")?, y.parse().map_err(|_| "bad center y")?));
            }
            "--size" => {
                let v = value("WxH")?;
                let (w, h) = v.split_once('x').ok_or("--size expects WxH")?;
                a.size = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--overlay" => {
                let v = value("a list")?;
                a.overlays = Overlays { blocked: false, grid: false, objects: false, markers: false, sprites: false, muzzles: false, walk: None, fire: None, foreground: false };
                for o in v.split(',').filter(|s| !s.is_empty()) {
                    match o {
                        "blocked" => a.overlays.blocked = true,
                        "grid" => a.overlays.grid = true,
                        "objects" => a.overlays.objects = true,
                        "markers" => a.overlays.markers = true,
                        "sprites" => {
                            a.overlays.sprites = true;
                            a.overlays.foreground = true;
                        }
                        "muzzles" => a.overlays.muzzles = true,
                        _ => return Err(format!("unknown overlay {o}")),
                    }
                }
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s => a.level = s.to_ascii_uppercase(),
        }
    }
    Ok(a)
}

fn fit(scene: &Scene, w: usize, h: usize) -> View {
    let mut v = View { x: 0.0, y: 0.0, zoom: (w as f32 / scene.bg_w as f32).min(h as f32 / scene.bg_h as f32) };
    v.clamp(scene, w, h);
    v
}

fn main() {
    let args = parse_args().unwrap_or_else(|e| {
        eprintln!("error: {e}\n\n{}", include_str!("main.rs").lines().take(4).collect::<Vec<_>>().join("\n"));
        std::process::exit(2);
    });
    let game = GameDir::locate().unwrap_or_else(|| {
        eprintln!("error: game not found; set OPENSOW_GAME_DIR");
        std::process::exit(1);
    });
    let mut scene = Scene::load(&game, &args.level, args.facing, args.aim).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    scene.shots = args
        .shots
        .iter()
        .filter_map(|&(x1, y1, x2, y2, t)| opensow_formats::projectile::build_path(t, x1, y1, x2, y2, 0))
        .collect();

    scene.ranges = args.ranges.clone();

    if let Some(path) = &args.screenshot {
        let (w, h) = args.size;
        let mut view = fit(&scene, w, h);
        if let Some(z) = args.zoom {
            view.zoom = z;
            view.clamp(&scene, w, h);
        }
        if let Some((cx, cy)) = args.center {
            view.x = (cx + 0.5) * 32.0 - w as f32 / view.zoom / 2.0;
            view.y = (cy + 0.5) * 32.0 - h as f32 / view.zoom / 2.0;
            view.clamp(&scene, w, h);
        }
        let mut buf = vec![0u32; w * h];
        let mut overlays = args.overlays;
        overlays.walk = args.walk;
        overlays.fire = args.fire.map(|t| t.saturating_sub(1));
        render(&scene, &view, overlays, &mut buf, w, h);
        write_png(path, &buf, w, h);
        println!("{} ({}x{} cells) -> {path}", scene.name, scene.map.width, scene.map.height);
        return;
    }

    let levels = Scene::list(&game);
    let idx = levels.iter().position(|l| *l == scene.name).unwrap_or(0);
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App { game, levels, idx, scene, view: View { x: 0.0, y: 0.0, zoom: 1.0 }, ov: args.overlays, window: None, surface: None, drag: None, cursor: PhysicalPosition::new(0.0, 0.0), needs_fit: true, autoplay: false, fire_play: false, ticks: 0, last_tick: std::time::Instant::now() };
    event_loop.run_app(&mut app).expect("run");
}

fn write_png(path: &str, buf: &[u32], w: usize, h: usize) {
    let rgb: Vec<u8> = buf.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path).expect("create png")), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut w| w.write_image_data(&rgb)).expect("write png");
}

struct App {
    game: GameDir,
    levels: Vec<String>,
    idx: usize,
    scene: Scene,
    view: View,
    ov: Overlays,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    drag: Option<PhysicalPosition<f64>>,
    cursor: PhysicalPosition<f64>,
    needs_fit: bool,
    autoplay: bool,
    fire_play: bool,
    ticks: u32,
    last_tick: std::time::Instant,
}

impl App {
    fn size(&self) -> (usize, usize) {
        self.window.as_ref().map(|w| w.inner_size()).map_or((1, 1), |s| (s.width.max(1) as usize, s.height.max(1) as usize))
    }

    fn redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn switch_level(&mut self, step: isize) {
        if self.levels.is_empty() {
            return;
        }
        let n = self.levels.len() as isize;
        let mut i = self.idx as isize;
        for _ in 0..n {
            i = (i + step).rem_euclid(n);
            match Scene::load(&self.game, &self.levels[i as usize], self.scene.facing, self.scene.aim) {
                Ok(s) => {
                    self.idx = i as usize;
                    self.scene = s;
                    self.needs_fit = true;
                    if let Some(w) = &self.window {
                        w.set_title(&format!("OpenSOW - {}", self.scene.name));
                    }
                    return;
                }
                Err(e) => eprintln!("skipping {}: {e}", self.levels[i as usize]),
            }
        }
    }

    fn update_title(&self) {
        let Some(w) = &self.window else { return };
        let (mx, my) = (self.view.x + self.cursor.x as f32 / self.view.zoom, self.view.y + self.cursor.y as f32 / self.view.zoom);
        let mut t = format!("OpenSOW - {}  cell ({},{})", self.scene.name, (mx / 32.0).floor() as i32, (my / 32.0).floor() as i32);
        if let Some(l) = &self.scene.level {
            let stock: Vec<String> = l.air_stock().iter().filter(|s| s.counts.iter().any(|&c| c != 0)).map(|s| format!("{} {:?}", s.force.name(), s.counts)).collect();
            if !stock.is_empty() {
                t += &format!("  |  air: {}", stock.join(", "));
            }
        }
        if let Some(o) = object_at(&self.scene, mx, my) {
            t += &format!("  |  {} {:?} hp {} ({},{})", o.object_kind().name(), o.owner(), o.hp(), o.cell_x, o.cell_y);
        }
        w.set_title(&t);
    }

    fn pan(&mut self, dx: f32, dy: f32) {
        let (w, h) = self.size();
        self.view.x += dx / self.view.zoom;
        self.view.y += dy / self.view.zoom;
        self.view.clamp(&self.scene, w, h);
        self.redraw();
    }

    fn zoom_at(&mut self, factor: f32, sx: f32, sy: f32) {
        let (w, h) = self.size();
        let (mx, my) = (self.view.x + sx / self.view.zoom, self.view.y + sy / self.view.zoom);
        self.view.zoom *= factor;
        self.view.zoom = self.view.zoom.clamp(0.1, 4.0);
        self.view.x = mx - sx / self.view.zoom;
        self.view.y = my - sy / self.view.zoom;
        self.view.clamp(&self.scene, w, h);
        self.redraw();
    }
}

/// Game ticks per second used for the walk preview. The real tick length was not traced; this is an assumption.
const PREVIEW_TICKS_PER_SECOND: u32 = 20;

impl ApplicationHandler for App {
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if !self.autoplay && !self.fire_play {
            el.set_control_flow(winit::event_loop::ControlFlow::Wait);
            return;
        }
        let tick = std::time::Duration::from_millis(1000 / PREVIEW_TICKS_PER_SECOND as u64);
        let now = std::time::Instant::now();
        if now.duration_since(self.last_tick) >= tick {
            self.last_tick = now;
            self.ticks = self.ticks.wrapping_add(1);
            if self.autoplay {
                self.ov.walk = Some(self.ticks);
            }
            if self.fire_play {
                self.ov.fire = Some(self.ticks);
            }
            self.redraw();
        }
        el.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(self.last_tick + tick));
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title(format!("OpenSOW - {}", self.scene.name)).with_inner_size(LogicalSize::new(1280.0, 800.0));
        let window = Arc::new(el.create_window(attrs).expect("window"));
        let ctx = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        self.surface = Some(softbuffer::Surface::new(&ctx, window.clone()).expect("surface"));
        self.window = Some(window);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(_) => {
                self.needs_fit |= self.view.zoom == 1.0 && self.view.x == 0.0;
                self.redraw();
            }
            WindowEvent::RedrawRequested => {
                let (w, h) = self.size();
                if self.needs_fit {
                    self.view = fit(&self.scene, w, h);
                    self.needs_fit = false;
                }
                self.view.clamp(&self.scene, w, h);
                let Some(surface) = self.surface.as_mut() else { return };
                if surface.resize(NonZeroU32::new(w as u32).unwrap(), NonZeroU32::new(h as u32).unwrap()).is_err() {
                    return;
                }
                if let Ok(mut buf) = surface.buffer_mut() {
                    render(&self.scene, &self.view, self.ov, &mut buf, w, h);
                    let _ = buf.present();
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                self.drag = (state == ElementState::Pressed).then_some(self.cursor);
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(last) = self.drag {
                    self.pan((last.x - position.x) as f32, (last.y - position.y) as f32);
                    self.drag = Some(position);
                }
                self.cursor = position;
                self.update_title();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.zoom_at(1.15f32.powf(dy), self.cursor.x as f32, self.cursor.y as f32);
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let (w, h) = self.size();
                let step = 96.0;
                match event.key_without_modifiers() {
                    Key::Named(NamedKey::Escape) => el.exit(),
                    Key::Named(NamedKey::ArrowLeft) => self.pan(-step, 0.0),
                    Key::Named(NamedKey::ArrowRight) => self.pan(step, 0.0),
                    Key::Named(NamedKey::ArrowUp) => self.pan(0.0, -step),
                    Key::Named(NamedKey::ArrowDown) => self.pan(0.0, step),
                    Key::Named(NamedKey::Home) => {
                        self.needs_fit = true;
                        self.redraw();
                    }
                    Key::Character(c) => match c.to_ascii_lowercase().as_str() {
                        "q" => el.exit(),
                        "a" => self.pan(-step, 0.0),
                        "d" => self.pan(step, 0.0),
                        "w" => self.pan(0.0, -step),
                        "s" => self.pan(0.0, step),
                        "+" | "=" => self.zoom_at(1.25, w as f32 / 2.0, h as f32 / 2.0),
                        "-" => self.zoom_at(0.8, w as f32 / 2.0, h as f32 / 2.0),
                        "b" => {
                            self.ov.blocked ^= true;
                            self.redraw();
                        }
                        "g" => {
                            self.ov.grid ^= true;
                            self.redraw();
                        }
                        "o" => {
                            self.ov.objects ^= true;
                            self.redraw();
                        }
                        "m" => {
                            self.ov.markers ^= true;
                            self.redraw();
                        }
                        "[" | "]" => {
                            let f = self.scene.facing;
                            let next = if c.as_str() == "]" { (f + 1) % 32 } else { (f + 31) % 32 };
                            self.scene.set_facing(&self.game, next);
                            if let Some(w) = &self.window {
                                w.set_title(&format!("OpenSOW - {}  facing {next}", self.scene.name));
                            }
                            self.redraw();
                        }
                        "v" => {
                            self.fire_play ^= true;
                            self.ticks = 0;
                            self.ov.fire = self.fire_play.then_some(0);
                            self.last_tick = std::time::Instant::now();
                            self.redraw();
                        }
                        "k" => {
                            self.autoplay ^= true;
                            self.ticks = 0;
                            self.ov.walk = self.autoplay.then_some(0);
                            self.last_tick = std::time::Instant::now();
                            self.redraw();
                        }
                        "f" => {
                            self.ov.muzzles ^= true;
                            self.redraw();
                        }
                        "x" => {
                            self.ov.sprites ^= true;
                            self.redraw();
                        }
                        "n" => {
                            self.switch_level(1);
                            self.redraw();
                        }
                        "p" => {
                            self.switch_level(-1);
                            self.redraw();
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
