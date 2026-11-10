//! opensow-launcher [--screenshot out.png [--size WxH] [--lang CZ] [--original DIR] [--warmonger DIR]]
//!
//! Window with two things to set: the language and the install folder of each game (docs/launcher.md). Choices are saved at once to
//! `launcher.cfg` (formats::launcher), which the game reads. "Play" starts `opensow-game` from the same folder as this program.

mod ui;

use opensow_formats::lang::Language;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use ui::{Action, App, Slot};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

enum UserEvent {
    Picked(Slot, Option<PathBuf>),
}

struct Launcher {
    app: App,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    proxy: EventLoopProxy<UserEvent>,
}

/// Ask the system for a folder (blocking; runs on its own thread).
fn pick_folder(title: &str) -> Option<PathBuf> {
    use std::process::Command;
    let out = if cfg!(target_os = "macos") {
        Command::new("osascript").args(["-e", &format!("POSIX path of (choose folder with prompt \"{title}\")")]).output()
    } else if cfg!(windows) {
        Command::new("powershell").args(["-NoProfile", "-Command", &format!("Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = '{title}'; if ($d.ShowDialog() -eq 'OK') {{ $d.SelectedPath }}")]).output()
    } else {
        Command::new("zenity").args(["--file-selection", "--directory", &format!("--title={title}")]).output()
    }
    .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !s.is_empty()).then(|| PathBuf::from(s))
}

fn play() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let game = exe.with_file_name(if cfg!(windows) { "opensow-game.exe" } else { "opensow-game" });
    if !game.exists() {
        return Err(format!("{} not found; build it with cargo build --release -p opensow-game", game.display()));
    }
    // both campaigns are one game: it opens on the one used last and switches on the world map
    std::process::Command::new(game).spawn().map(|_| ()).map_err(|e| e.to_string())
}

impl Launcher {
    fn save(&mut self) {
        if let Err(e) = opensow_formats::launcher::save(&self.app.settings) {
            self.app.message = format!("Could not save the settings: {e}");
        }
    }

    fn logical(&self, p: (f64, f64)) -> (f32, f32) {
        let size = self.window.as_ref().map(|w| w.inner_size()).unwrap_or_default();
        let (s, ox, oy) = App::fit(size.width as usize, size.height as usize);
        ((p.0 as f32 - ox) / s, (p.1 as f32 - oy) / s)
    }

    fn act(&mut self, a: Action) {
        match a {
            Action::Lang(l) => self.app.settings.language = l,
            Action::Clear(s) => self.app.set_path(s, None),
            Action::Detect => self.app.detect(),
            Action::Browse(s) => {
                let proxy = self.proxy.clone();
                let title = if s == Slot::Warmonger { "Choose the State of War Warmonger folder" } else { "Choose the State of War folder" };
                std::thread::spawn(move || {
                    let _ = proxy.send_event(UserEvent::Picked(s, pick_folder(title)));
                });
                return;
            }
            Action::Play => {
                self.save();
                match play() {
                    Ok(()) => event_exit(),
                    Err(e) => self.app.message = e,
                }
                return;
            }
        }
        self.save();
    }
}

fn event_exit() {
    std::process::exit(0);
}

impl ApplicationHandler<UserEvent> for Launcher {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("OpenSOW Launcher").with_inner_size(winit::dpi::LogicalSize::new(600.0, 520.0)).with_min_inner_size(winit::dpi::LogicalSize::new(300.0, 250.0));
        let icon = winit::window::Icon::from_rgba(ui::emblem_rgba(64), 64, 64).ok();
        let attrs = attrs.with_window_icon(icon);
        let window = Rc::new(el.create_window(attrs).expect("window"));
        let ctx = softbuffer::Context::new(window.clone()).expect("context");
        self.surface = Some(softbuffer::Surface::new(&ctx, window.clone()).expect("surface"));
        self.window = Some(window);
    }

    fn user_event(&mut self, _: &ActiveEventLoop, ev: UserEvent) {
        let UserEvent::Picked(slot, dir) = ev;
        if let Some(d) = dir {
            self.app.offer_dir(d, Some(slot));
            self.save();
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::CursorMoved { position, .. } => {
                self.app.cursor = self.logical((position.x, position.y));
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let (x, y) = self.app.cursor;
                if let Some(a) = self.app.click(x, y) {
                    self.act(a);
                }
            }
            WindowEvent::DroppedFile(p) => {
                if p.is_dir() {
                    let hint = self.app.slot_at(self.app.cursor.1);
                    self.app.offer_dir(p, hint);
                    self.save();
                } else {
                    self.app.message = "Drop the game's folder, not a file.".into();
                }
            }
            WindowEvent::RedrawRequested => {}
            _ => {}
        }
        if let (Some(w), Some(s)) = (&self.window, &mut self.surface) {
            let size = w.inner_size();
            let (Some(nw), Some(nh)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
            if s.resize(nw, nh).is_err() {
                return;
            }
            if let Ok(mut buf) = s.buffer_mut() {
                self.app.draw(&mut buf, size.width as usize, size.height as usize);
                let _ = buf.present();
            }
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
    let mut settings = opensow_formats::launcher::load();
    if let Some(path) = get("--icon-png") {
        // the window icon at SIZE (default 256), on a checker so the transparent corners show
        let n: usize = get("--size").and_then(|s| s.parse().ok()).unwrap_or(256);
        let px = ui::emblem_rgba(n);
        let buf: Vec<u32> = (0..n * n)
            .map(|i| {
                let bg = if (i % n / 16 + i / n / 16) % 2 == 0 { 0x808080 } else { 0xa0a0a0 };
                let a = px[i * 4 + 3] as u32;
                let ch = |s: u32, c: u8| ((bg >> s & 255) * (255 - a) + c as u32 * a) / 255;
                ch(16, px[i * 4]) << 16 | ch(8, px[i * 4 + 1]) << 8 | ch(0, px[i * 4 + 2])
            })
            .collect();
        write_png(&path, &buf, n, n);
        return;
    }
    if let Some(path) = get("--screenshot") {
        // headless: overrides from the command line, nothing is saved
        if let Some(l) = get("--lang") {
            settings.language = Language::from_code(&l);
        }
        settings.original = get("--original").map(PathBuf::from).or(settings.original);
        settings.warmonger = get("--warmonger").map(PathBuf::from).or(settings.warmonger);
        let (w, h) = get("--size").and_then(|s| s.split_once('x').map(|(a, b)| (a.parse().unwrap_or(600), b.parse().unwrap_or(520)))).unwrap_or((600, 500));
        let mut app = App::new(settings);
        app.message = get("--message").unwrap_or_default();
        if let Some(p) = get("--hover").and_then(|s| s.split_once(',').map(|(a, b)| (a.parse().unwrap_or(0.0), b.parse().unwrap_or(0.0)))) {
            app.cursor = p;
        }
        let mut buf = vec![0u32; w * h];
        app.draw(&mut buf, w, h);
        write_png(&path, &buf, w, h);
        return;
    }
    let el = EventLoop::<UserEvent>::with_user_event().build().expect("event loop");
    let mut l = Launcher { app: App::new(settings), window: None, surface: None, proxy: el.create_proxy() };
    el.run_app(&mut l).expect("run");
}
