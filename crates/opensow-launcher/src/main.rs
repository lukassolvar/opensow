//! opensow-launcher [--screenshot out.png [--size WxH] [--page home|original|warmonger|settings] [--tab launcher|game|extras]
//!                   [--lang CZ] [--original DIR] [--warmonger DIR] [--hover X,Y] [--message TEXT] [--update STATE] [--time SECS]]
//!
//! Window with a sidebar (home, State of War, Warmonger, settings): locate each game, start it, choose the language and edit the
//! game's settings (docs/launcher.md). Folders and language are saved at once to `launcher.cfg` (formats::launcher), which the game
//! reads; the game settings go to the per-user data folder, where "Play" starts `opensow-game` (from this program's folder).

// Windows: release builds (without the `dev` feature) open no console window behind the program (docs/release.md).
#![cfg_attr(all(windows, not(feature = "dev")), windows_subsystem = "windows")]

mod desktop;
mod settings;
mod text;
mod ui;
mod update;

use opensow_formats::lang::Language;
use settings::GameSettings;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};
use ui::{Action, App, Page, Slot, Tab, UpdateState};
use update::{Fail, Release};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

enum UserEvent {
    Picked(Slot, Option<PathBuf>),
    /// The answer of the release check (failures show only on the Settings page).
    Checked(Result<Release, Fail>),
    /// Bytes of the update downloaded so far.
    Progress(u64),
    /// The update is in place (or why not).
    Installed(Result<Release, Fail>),
}

struct Launcher {
    app: App,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    proxy: EventLoopProxy<UserEvent>,
    /// When the window opened; the update badge's pulse runs on it.
    start: Instant,
    /// When the next animation frame is due.
    next_frame: Instant,
}

/// Ask the system for a folder (blocking; runs on its own thread).
fn pick_folder(title: &str) -> Option<PathBuf> {
    use std::process::Command;
    let out = if cfg!(target_os = "macos") {
        Command::new("osascript").args(["-e", &format!("POSIX path of (choose folder with prompt \"{title}\")")]).output()
    } else if cfg!(windows) {
        windows_folder_dialog(title).output()
    } else {
        Command::new("zenity").args(["--file-selection", "--directory", &format!("--title={title}")]).output()
    }
    .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim_start_matches('\u{feff}').trim().to_owned();
    (out.status.success() && !s.is_empty()).then(|| PathBuf::from(s))
}

/// PowerShell's folder dialog on Windows, built so that it works for every user:
/// - no console window: PowerShell is a console program, so without `CREATE_NO_WINDOW` Windows opens an empty terminal for it;
/// - the dialog's owner is a hidden top-most form, otherwise the dialog may open behind the launcher and only the terminal shows;
/// - the path is written as UTF-8: the console's default code page (852, 437, ...) mangles names like `C:\Users\Matúš`;
/// - the prompt comes in through an environment variable, so no quote in a translation can break the script.
fn windows_folder_dialog(title: &str) -> std::process::Command {
    const SCRIPT: &str = "$ErrorActionPreference = 'Stop'; \
        [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false; \
        Add-Type -AssemblyName System.Windows.Forms; \
        [System.Windows.Forms.Application]::EnableVisualStyles(); \
        $owner = New-Object System.Windows.Forms.Form -Property @{ TopMost = $true; ShowInTaskbar = $false }; \
        $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
        $d.Description = $env:OPENSOW_PICK_TITLE; \
        $d.ShowNewFolderButton = $false; \
        if ($d.ShowDialog($owner) -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::Out.Write($d.SelectedPath) }; \
        $owner.Dispose()";
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-STA", "-Command", SCRIPT]).env("OPENSOW_PICK_TITLE", title);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Starts the game in the data folder (where it finds the settings this launcher edits), on the campaign used last.
fn play(data_dir: &std::path::Path) -> Result<(), text::Message> {
    let exe = std::env::current_exe().map_err(|e| text::Message::Raw(e.to_string()))?;
    let game = exe.with_file_name(if cfg!(windows) { "opensow-game.exe" } else { "opensow-game" });
    if !game.exists() {
        return Err(text::Message::GameMissing(game.display().to_string()));
    }
    std::fs::create_dir_all(data_dir).map_err(|e| text::Message::Raw(e.to_string()))?;
    let mut cmd = std::process::Command::new(game);
    cmd.current_dir(data_dir);
    cmd.spawn().map(|_| ()).map_err(|e| text::Message::Raw(e.to_string()))
}

impl Launcher {
    fn save(&mut self) {
        if let Err(e) = opensow_formats::launcher::save(&self.app.settings) {
            self.app.message = text::Message::SaveFailed(e.to_string());
        }
    }

    fn save_game(&mut self) {
        if let Err(e) = self.app.game.save_to(&self.app.data_dir) {
            self.app.message = text::Message::SaveFailed(e.to_string());
        }
    }

    fn act(&mut self, a: Action) {
        match a {
            Action::Nav(p) => {
                if self.app.page != p {
                    self.app.begin_fade(ui::FadeKind::Page);
                }
                self.app.page = p;
                self.app.refresh();
                return;
            }
            Action::Tab(t) => {
                if self.app.tab != t {
                    self.app.begin_fade(ui::FadeKind::Page);
                }
                self.app.tab = t;
                return;
            }
            Action::Lang(l) => {
                self.app.begin_fade(ui::FadeKind::Swap);
                self.app.settings.language = l;
                // the page follows at once; the window's title too
                if let Some(w) = &self.window {
                    w.set_title(self.app.tx().window_title());
                }
            }
            Action::Detect => self.app.detect(),
            Action::Browse(s) => {
                let proxy = self.proxy.clone();
                let title = self.app.tx().choose_folder(s == Slot::Warmonger);
                std::thread::spawn(move || {
                    let _ = proxy.send_event(UserEvent::Picked(s, pick_folder(title)));
                });
                return;
            }
            Action::Play => {
                // the folders are looked at again: files may be gone since the page was drawn
                self.app.refresh();
                if !self.app.playable(Slot::Original) {
                    self.app.message = text::Message::Missing(Slot::Original.name().into());
                    self.app.page = Page::Game(Slot::Original);
                    return;
                }
                self.save();
                self.save_game();
                match play(&self.app.data_dir) {
                    Ok(()) => std::process::exit(0),
                    Err(e) => self.app.message = e,
                }
                return;
            }
            Action::Toggle(f) => {
                self.app.game.toggle(f);
                self.save_game();
                return;
            }
            Action::Step(f, d) => {
                self.app.game.step(f, d);
                self.save_game();
                return;
            }
            Action::CheckUpdate => self.check_update(),
            Action::InstallUpdate => {
                if let UpdateState::Available(r) = self.app.update.clone() {
                    self.install_update(r);
                }
                return;
            }
            Action::ReleaseNotes => {
                match &self.app.update {
                    UpdateState::Available(r) | UpdateState::Downloading(r, _) | UpdateState::Installed(r) => update::open_url(&r.url),
                    _ => update::open_url(&update::releases_url()),
                }
                return;
            }
            Action::Repo => {
                update::open_url(&update::repo_url());
                return;
            }
            Action::Updates => {
                self.app.begin_fade(ui::FadeKind::Page);
                self.app.page = Page::Settings;
                self.app.tab = Tab::Launcher;
                return;
            }
            Action::Restart => {
                // the new launcher replaced this file; start it and leave
                if let Ok(exe) = std::env::current_exe() {
                    if std::process::Command::new(exe).spawn().is_ok() {
                        std::process::exit(0);
                    }
                }
                return;
            }
            Action::ToggleAutoCheck => self.app.settings.check_updates = !self.app.settings.check_updates,
            Action::Reset => {
                self.app.game = GameSettings::default();
                self.app.message = text::Message::SettingsReset;
                self.save_game();
                return;
            }
        }
        self.save();
    }
}

impl Launcher {
    /// Asks GitHub for the latest release on a thread of its own.
    fn check_update(&mut self) {
        if matches!(self.app.update, UpdateState::Checking | UpdateState::Downloading(..)) {
            return;
        }
        self.app.update = UpdateState::Checking;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            // a newer release brings its notes in the other languages too
            let r = update::fetch_latest().map(|mut r| {
                if r.is_newer() {
                    update::fetch_translations(&mut r);
                }
                r
            });
            let _ = proxy.send_event(UserEvent::Checked(r));
        });
    }

    /// Downloads the release's archive and its `.sha256` into the data folder's `update` folder, then installs it next to this
    /// program; the progress is the file's size, read while `curl` writes it.
    fn install_update(&mut self, r: Release) {
        let (Some(archive), Some(checksum)) = (r.archive.clone(), r.checksum.clone()) else { return };
        let Some(target) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) else { return };
        let work = self.app.data_dir.join("update");
        self.app.update = UpdateState::Downloading(r.clone(), 0);
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let sha = update::fetch_text(&checksum.browser_download_url)?;
                let file = work.join(&archive.name);
                let mut child = update::start_download(&archive.browser_download_url, &file).map_err(|e| Fail::Network(e.to_string()))?;
                let status = loop {
                    if let Some(s) = child.try_wait().map_err(|e| Fail::Network(e.to_string()))? {
                        break s;
                    }
                    let _ = proxy.send_event(UserEvent::Progress(std::fs::metadata(&file).map_or(0, |m| m.len())));
                    std::thread::sleep(std::time::Duration::from_millis(150));
                };
                if !status.success() {
                    return Err(Fail::Network(format!("download failed ({status})")));
                }
                let _ = proxy.send_event(UserEvent::Progress(archive.size));
                update::install(&file, &sha, &work, &target)?;
                let _ = std::fs::remove_file(&file);
                Ok(r)
            })();
            let _ = proxy.send_event(UserEvent::Installed(result));
        });
    }
}

impl ApplicationHandler<UserEvent> for Launcher {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.app.tx().window_title())
            .with_inner_size(winit::dpi::LogicalSize::new(1000.0, 620.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(640.0, 420.0));
        let icon = winit::window::Icon::from_rgba(ui::ICON_RGBA.to_vec(), 64, 64).ok();
        let attrs = with_app_id(attrs.with_window_icon(icon), desktop::APP_ID);
        let window = Rc::new(el.create_window(attrs).expect("window"));
        let ctx = softbuffer::Context::new(window.clone()).expect("context");
        self.surface = Some(softbuffer::Surface::new(&ctx, window.clone()).expect("surface"));
        self.window = Some(window);
        self.app.time = self.start.elapsed().as_secs_f32();
        self.app.intro();
    }

    fn user_event(&mut self, _: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Picked(slot, dir) => {
                if let Some(d) = dir {
                    self.app.offer_dir(d, Some(slot));
                    self.save();
                }
            }
            UserEvent::Checked(r) => {
                // the home page's card changes to the new release's notes: fade
                if matches!(r, Ok(ref r) if r.is_newer()) {
                    self.app.time = self.start.elapsed().as_secs_f32();
                    self.app.begin_fade(ui::FadeKind::Swap);
                }
                self.app.update = match r {
                    Ok(r) if r.is_newer() => UpdateState::Available(r),
                    Ok(_) => UpdateState::UpToDate,
                    Err(f) => UpdateState::Failed(f),
                }
            }
            UserEvent::Progress(n) => {
                if let UpdateState::Downloading(_, got) = &mut self.app.update {
                    *got = n;
                }
            }
            UserEvent::Installed(r) => {
                self.app.update = match r {
                    Ok(r) => UpdateState::Installed(r),
                    Err(f) => UpdateState::Failed(f),
                }
            }
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// While something moves, frames at `App::frame_interval`; otherwise the launcher sleeps until an event comes.
    fn new_events(&mut self, _: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. } | StartCause::Init) {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // frames keep their rhythm: the next one is due one interval after the last was due, not after it finished drawing
        el.set_control_flow(match self.app.frame_interval() {
            Some(s) => {
                let now = Instant::now();
                let step = Duration::from_secs_f32(s);
                if self.next_frame <= now {
                    self.next_frame = (self.next_frame + step).max(now + Duration::from_millis(2));
                }
                ControlFlow::WaitUntil(self.next_frame)
            }
            None => {
                self.next_frame = Instant::now();
                ControlFlow::Wait
            }
        });
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, ev: WindowEvent) {
        // the moment this event happens, for the animations it starts
        self.app.time = self.start.elapsed().as_secs_f32();
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::CursorMoved { position, .. } => {
                self.app.cursor = self.app.to_logical(position.x as f32, position.y as f32);
            }
            WindowEvent::CursorLeft { .. } => self.app.cursor = (-1.0, -1.0),
            // back in the window: the game folders may have changed meanwhile
            WindowEvent::Focused(true) => self.app.refresh(),
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let (x, y) = self.app.cursor;
                if let Some(a) = self.app.click(x, y) {
                    self.act(a);
                }
            }
            WindowEvent::DroppedFile(p) => {
                if p.is_dir() {
                    let hint = self.app.drop_slot();
                    self.app.offer_dir(p, hint);
                    self.save();
                } else {
                    self.app.message = text::Message::DropFolder;
                }
            }
            _ => {}
        }
        if let (Some(w), Some(s)) = (&self.window, &mut self.surface) {
            let size = w.inner_size();
            let (Some(nw), Some(nh)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
            if s.resize(nw, nh).is_err() {
                return;
            }
            if let Ok(mut buf) = s.buffer_mut() {
                self.app.resize(size.width as usize, size.height as usize);
                self.app.draw(&mut buf, size.width as usize, size.height as usize);
                let _ = buf.present();
            }
        }
    }
}

/// Writes the frame buffer (`0x00RRGGBB` pixels) as a PNG for `--screenshot`. The same helper lives in
/// `opensow_viewer::screenshot`; the launcher keeps its own copy so that it does not depend on the whole renderer.
fn write_png(path: &str, buf: &[u32], w: usize, h: usize) {
    let rgb: Vec<u8> = buf.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path).expect("create png")), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut w| w.write_image_data(&rgb)).expect("write png");
}

/// `--update checking|uptodate|available|downloading|installed|noreleases|failed`: a made-up update state for headless screenshots.
fn demo_update(state: &str) -> UpdateState {
    let asset = |n: &str| update::Asset { name: n.into(), browser_download_url: String::new(), size: 40_000_000 };
    let r = Release {
        tag: "v0.4.0".into(),
        version: (0, 4, 0),
        name: "Skirmish map editor".into(),
        url: String::new(),
        archive: Some(asset("opensow.zip")),
        checksum: Some(asset("opensow.zip.sha256")),
        notes: [
            "## New\n- **Network play over LAN.**\n- **Replays of skirmish games.**\n## Fixed\n- **Carryalls pick up the right unit.**\n- **Music volume is kept.**\n- **Faster map loading.**",
            "## Novinky\n- **Hra po lokálnej sieti.**\n- **Záznamy potýčok.**\n## Opravy\n- **Transportéry berú správnu jednotku.**\n- **Hlasitosť hudby sa pamätá.**\n- **Rýchlejšie načítanie máp.**",
            "## Novinky\n- **Hra po místní síti.**\n- **Záznamy potyček.**\n## Opravy\n- **Transportéry berou správnou jednotku.**\n- **Hlasitost hudby se pamatuje.**\n- **Rychlejší načítání map.**",
            "",
            "",
        ]
        .map(update::parse_notes),
        translations: vec![],
    };
    match state {
        "checking" => UpdateState::Checking,
        "uptodate" => UpdateState::UpToDate,
        "available" => UpdateState::Available(r),
        "downloading" => UpdateState::Downloading(r, 17_000_000),
        "installed" => UpdateState::Installed(r),
        "noreleases" => UpdateState::Failed(Fail::NoReleases),
        "failed" => UpdateState::Failed(Fail::Network("Could not resolve host: api.github.com".into())),
        _ => UpdateState::Idle,
    }
}

/// Developer features (cargo feature `dev`, on by default; release archives are built without it, docs/release.md): the
/// headless `--screenshot` and `--icon-png`. `--check-update` stays (it helps with update reports).
const DEV: bool = cfg!(feature = "dev");

/// The Wayland app id and X11 class, so the desktop matches the window to `opensow.desktop` and its icon (desktop.rs).
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "dragonfly"))]
fn with_app_id(attrs: winit::window::WindowAttributes, id: &str) -> winit::window::WindowAttributes {
    let attrs = winit::platform::wayland::WindowAttributesExtWayland::with_name(attrs, id, id);
    winit::platform::x11::WindowAttributesExtX11::with_name(attrs, id, id)
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "dragonfly")))]
fn with_app_id(attrs: winit::window::WindowAttributes, _id: &str) -> winit::window::WindowAttributes {
    attrs
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let mut settings = opensow_formats::launcher::load();
    let data_dir = opensow_formats::launcher::data_dir();
    let game = GameSettings::load_from(&data_dir);
    if args.iter().any(|a| a == "--check-update") {
        // what the launcher would find: the latest release of OPENSOW_UPDATE_REPO (default `update::REPO`) for this system
        println!("repo {} platform {} this version {}", update::repo(), update::platform(), update::VERSION);
        match update::fetch_latest() {
            Ok(r) => println!(
                "latest {} ({}) newer {} archive {:?} checksum {:?} notes {} translated {:?}",
                r.tag,
                r.name,
                r.is_newer(),
                r.archive.map(|a| a.name),
                r.checksum.map(|a| a.name),
                r.notes[0].len(),
                r.translations.iter().map(|(_, a)| a.name.clone()).collect::<Vec<_>>()
            ),
            Err(f) => println!("{f:?}"),
        }
        return;
    }
    if let Some(path) = get("--icon-png").filter(|_| DEV) {
        // the window icon at SIZE (default 256), on a checker so the transparent corners show
        let n: usize = get("--size").and_then(|s| s.parse().ok()).unwrap_or(256);
        let px = ui::icon_rgba(n);
        let buf: Vec<u32> = (0..n * n)
            .map(|i| {
                let bg = if (i % n / 16 + i / n / 16).is_multiple_of(2) { 0x808080 } else { 0xa0a0a0 };
                let a = px[i * 4 + 3] as u32;
                let ch = |s: u32, c: u8| ((bg >> s & 255) * (255 - a) + c as u32 * a) / 255;
                ch(16, px[i * 4]) << 16 | ch(8, px[i * 4 + 1]) << 8 | ch(0, px[i * 4 + 2])
            })
            .collect();
        write_png(&path, &buf, n, n);
        return;
    }
    if let Some(path) = get("--screenshot").filter(|_| DEV) {
        // headless: overrides from the command line, nothing is saved
        if let Some(l) = get("--lang") {
            settings.language = Language::from_code(&l);
        }
        // an empty value = not located
        let dir = |name: &str, old: Option<PathBuf>| get(name).map_or(old, |d| (!d.is_empty()).then(|| PathBuf::from(d)));
        settings.original = dir("--original", settings.original);
        settings.warmonger = dir("--warmonger", settings.warmonger);
        let (w, h) = get("--size").and_then(|s| s.split_once('x').map(|(a, b)| (a.parse().unwrap_or(1000), b.parse().unwrap_or(620)))).unwrap_or((1000, 620));
        let mut app = App::new(settings, game, data_dir);
        if let Some(u) = get("--update") {
            app.update = demo_update(&u);
        }
        // the moment of the update badge's pulse (seconds)
        app.time = get("--time").and_then(|t| t.parse().ok()).unwrap_or(0.0);
        app.page = match get("--page").as_deref() {
            Some("original") => Page::Game(Slot::Original),
            Some("warmonger") => Page::Game(Slot::Warmonger),
            Some("settings") => Page::Settings,
            _ => Page::Home,
        };
        app.tab = match get("--tab").as_deref() {
            Some("launcher") => Tab::Launcher,
            Some("extras") => Tab::Extras,
            _ => Tab::Game,
        };
        if let Some(m) = get("--message") {
            app.message = text::Message::Raw(m);
        }
        app.resize(w, h);
        if let Some(p) = get("--hover").and_then(|s| s.split_once(',').map(|(a, b)| (a.parse().unwrap_or(0.0), b.parse().unwrap_or(0.0)))) {
            app.cursor = p;
        }
        let mut buf = vec![0u32; w * h];
        app.draw(&mut buf, w, h);
        write_png(&path, &buf, w, h);
        return;
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        update::remove_leftovers(&dir);
    }
    // release files only: a development build would point the desktop entry at the target folder
    if !DEV {
        desktop::install();
    }
    let el = EventLoop::<UserEvent>::with_user_event().build().expect("event loop");
    let mut l = Launcher {
        app: App::new(settings, game, data_dir),
        window: None,
        surface: None,
        proxy: el.create_proxy(),
        start: Instant::now(),
        next_frame: Instant::now(),
    };
    if l.app.settings.check_updates {
        l.check_update();
    }
    el.run_app(&mut l).expect("run");
}
