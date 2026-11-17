//! `opensow-game`: a playable prototype with the original menus and a skirmish on the campaign levels.
//!
//! ```text
//! opensow-game [--no-intro] [--mute] [--windowed] [--lang SK|CZ|CS|CT]   start the game fullscreen (F11 toggles, `--windowed` opens a window; shows the boot pictures and plays the intro movie first; any key or click skips it)
//!   `--no-enhance` (or OPENSOW_ENHANCE=0) turns off the run-time texture enhancement of the zoomed-in world (docs/enhance.md); `--no-fx` (OPENSOW_FX=0) the soft/contact shadows and explosion glow (docs/fx.md); `--no-sound-enhance` (OPENSOW_SOUND_ENHANCE=0) plays the sounds exactly as before: no sinc resampling, fades or limiter (docs/audio.md)
//! opensow-game --screenshot out.png [--size WxH] [--screen main|levels|options|pause|message|results]
//!              [--mute] [--boot-page N] [--intro-frame N [--movie mI|mO|M01..M22]] [--hover N] [--play M01 [--ticks N] [--select-all] [--select-building CX,CY] [--move CX,CY] [--zoom Z] [--center CX,CY] [--range] [--fx CX,CY,KIND,AGE]... [--no-mask]]
//! ```
//!
//! This file holds the `Game` (everything the program knows: the open screens, the mission being played, settings) and
//! the `App` (the window around it), and `main`, which reads the command line. Their methods live in:
//!
//! | module | what it does |
//! |---|---|
//! | `game_flow` | start-up, progress, sounds and movies, menu actions, per-frame `update` and `draw` |
//! | `game_screens` | what each screen's events do: campaign map, pause, skirmish, map editor, options, save/load, results |
//! | `game_input` | mouse, keyboard and text input |
//! | `window` | winit events, scaling and presenting the frame, the system cursor |
//! | `headless` | `--screenshot` runs and `--gpu-check` |
//! | `tests` | tests that drive a `Game` like the window does |

// Windows: release builds (without the `dev` feature) open no console window behind the program (docs/release.md).
#![cfg_attr(all(windows, not(feature = "dev")), windows_subsystem = "windows")]

mod assets;
mod audio;
mod boot;
mod custom_maps;
mod devtools;
mod edition;
mod game_flow;
mod game_input;
mod game_screens;
mod gdi;
mod gpu;
mod headless;
mod hifi;
mod hud;
mod infobox;
mod ingame;
mod intro;
mod keybinds;
mod lands;
mod mapeditor_ui;
mod menu;
mod multiplayer;
mod music;
mod options;
mod page;
mod play;
mod replay;
mod results;
mod saveui;
mod sfx;
mod sim;
mod skirmish;
mod skirmish_ui;
mod stars;
mod textinput;
mod tr;
mod tutorial_panel;
mod ui;
mod window;

use assets::Assets;
use menu::{Action, Screen, Settings, SPEEDS};
use opensow_formats::kinds;
use opensow_formats::GameDir;
use opensow_viewer::screenshot::write_png;
use play::Play;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Cursor, CursorGrabMode, CustomCursor, Fullscreen, Window, WindowId};

/// What happens when the movie on screen ends or is skipped.
#[derive(Debug, Clone, PartialEq)]
enum Then {
    Menu,
    /// The briefing is over: start the mission.
    Level(String),
    /// The outro is over: show the closing message.
    Screen(Screen),
}

/// Developer features (cargo feature `dev`, on by default; release archives are built without it, docs/release.md): the F12
/// dev tools panel, headless `--screenshot` runs, `--skirmish` straight starts, `OPENSOW_START`, `OPENSOW_PERF` and
/// `OPENSOW_AUDIO_DUMP`. Without it those switches are ignored and the game starts normally.
const DEV: bool = cfg!(feature = "dev");

/// Made-up counters for the statistics screen's headless screenshots.
fn sample_stats() -> [[u32; 8]; 2] {
    [[3, 5, 12, 7, 4, 1, 2, 0], [1, 2, 20, 15, 0, 3, 1, 1]]
}

/// The last campaign mission; winning it plays the outro (`mO.sgv`).
const FINAL_LEVEL: &str = "M22";
/// The tutorial is level 0 (`0x41f140(0)`).
const TUTORIAL_LEVEL: &str = "M00";
/// Automatic save into the quick save slot every 5 minutes of game time (not in the original; `Game::autosave`).
const AUTOSAVE_TICKS: u64 = 5 * 60 * sim::TICKS_PER_SECOND as u64;

struct Game {
    assets: Assets,
    /// The key event being handled comes from the numeric keypad (winit `KeyLocation::Numpad`).
    numpad: bool,
    /// The digit of the physical key being handled (top row 1-9, or numpad 0-9), whatever the layout: the original reads scan codes.
    digit: Option<u8>,
    /// The physical key of the event being handled (set by the window; tests and headless runs leave it `None` and the
    /// logical key is mapped, `keybinds::code_of`).
    code: Option<winit::keyboard::KeyCode>,
    /// Game speed factor `[0x49ceec]` (F3 / F4, 0.5..3.0 in steps of 0.5; it stays from mission to mission).
    game_speed: f32,
    settings: Settings,
    levels: Vec<String>,
    /// Menu shown on top of (or instead of) the game.
    screen: Option<Screen>,
    back: Vec<Screen>,
    play: Option<Box<Play>>,
    /// The carryall squad counter `[0x89cf50]`, kept over missions as in the exe (docs/air.md).
    carry_counter: usize,
    hover: Option<usize>,
    pressed: Option<usize>,
    shift: bool,
    /// F5 pressed in a mission: the next drawn frame is grabbed and saved to slot 0 (`0x416811`, docs/saveload.md).
    quick_save: bool,
    /// Automatic saves into the quick save slot (at the start of a mission and every `AUTOSAVE_TICKS`; not in the original). Only
    /// the windowed game turns it on, so headless runs and tests never write the player's saves.
    autosave: bool,
    /// The pending quick save is an automatic one: no "game saved" speech.
    quick_save_auto: bool,
    scale: f32,
    cursor: (i32, i32),
    quit: bool,
    /// The intro movie while it plays; it replaces everything else on screen.
    intro: Option<intro::Intro>,
    /// The boot screens (Cypronia logo, title picture) while they show; they come before the intro movie.
    boot: Option<boot::Boot>,
    then: Then,
    language: usize,
    /// Play the briefing movies and the outro (off for headless runs and `--no-intro`).
    movies: bool,
    /// Do not open the audio device (`--mute`, tests).
    mute: bool,
    /// The campaign map while it is on screen; it replaces everything else.
    lands: Option<lands::Lands>,
    /// The options menu (original art) while it is open; it replaces everything else on screen.
    options_menu: Option<options::OptionsMenu>,
    /// The load / save page (original art) while it is open; it replaces everything else on screen.
    saveui: Option<saveui::SaveUi>,
    /// The multiplayer screens (docs/multiplayer-menus.md).
    net_menu: Option<multiplayer::NetMenu>,
    /// The skirmish setup screen (`skirmish_ui.rs`).
    skirmish_menu: Option<skirmish_ui::SkirmishMenu>,
    /// The map editor (`mapeditor_ui.rs`, docs/map-editor-ingame.md).
    map_editor: Option<mapeditor_ui::MapEditor>,
    /// Control or Command held (`WindowEvent::ModifiersChanged`).
    ctrl: bool,
    /// The running skirmish's choices (map, setup, the human's side), for RESTART (docs/skirmish.md).
    skirmish: Option<SkirmishRun>,
    /// The stars earned in skirmishes (`stars.rs`).
    stars: stars::Stars,
    /// The Esc menu while the pause screen is up (created when it opens, dropped when it closes).
    ingame: Option<ingame::InGameMenu>,
    /// The banner and statistics screens after a mission while they are up (states 9 / 0xa and 0x16 of the exe, docs/results.md).
    results: Option<results::Results>,
    /// The options chosen in it (`State of War_1.cfg` and the audio object in the original; kept in memory here).
    options: options::GameOptions,
    /// Which lands are open (`player.cfg` in the original; kept in memory here, never written into the install).
    progress: opensow_formats::config::PassedLevels,
    /// The running mission was chosen on the campaign map: the map comes back afterwards.
    campaign: bool,
    music: music::Music,
    sfx: sfx::Sfx,
    /// Developer tools overlay (`devtools.rs`); not part of the original.
    dev: devtools::DevTools,
}

/// The running skirmish's choices, for RESTART and for the star; a save of a skirmish stores them (JSON, `saveui::Saved::skirmish`)
/// so that loading builds the same map again before the snapshot is applied.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct SkirmishRun {
    map: String,
    setup: multiplayer::Setup,
    human: usize,
    difficulty: sim::Difficulty,
    /// Dev tool (`--spectate`): the computer plays blue too, at this difficulty (docs/skirmish.md).
    spectate: Option<sim::Difficulty>,
    /// The lobby still had the map's own credits, research, air forces and turrets: a win earns a star.
    default_setup: bool,
    /// A custom map's file (`map` is then its stars key `custom/<stem>`), docs/map-editor-ingame.md.
    custom: Option<std::path::PathBuf>,
}

struct App {
    game: Game,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    /// The GPU presenter (scales and presents with vsync); `None` = the softbuffer path.
    gpu: Option<gpu::Gpu>,
    last: Instant,
    /// The mission pointer as the system's cursor (moved by the compositor at the mouse's rate, not once per frame): the
    /// pictures made so far and the one that is set.
    cursors: std::collections::HashMap<(usize, u32), CustomCursor>,
    cursor_key: Option<(usize, u32)>,
    /// `OPENSOW_PERF=1`: per second frames and the milliseconds spent in update / cursor / draw / present.
    perf: Option<Perf>,
    /// The game renders at the Options resolution into this buffer; it is scaled into the window (aspect kept, black bars).
    frame: Vec<u32>,
    /// The resolution the window was last sized for (a change in the options resizes the window once).
    window_res: (u32, u32),
    /// Mission frames are drawn by the GPU (`OPENSOW_CPU_DRAW=1` draws them on the CPU and only presents on the GPU).
    replay: bool,
    /// Open the window fullscreen (borderless on the current screen); `--windowed` turns it off, F11 toggles.
    fullscreen: bool,
    /// The window has the keyboard focus (the cursor is only held inside it while focused).
    focused: bool,
    /// The cursor is held inside the fullscreen window: `Some(true)` by the system (`CursorGrabMode::Confined`),
    /// `Some(false)` by moving it back when it leaves (macOS, which has no confinement), `None` = free.
    confined: Option<bool>,
    /// The last pointer position inside the window (window pixels), where a pointer that left is put back.
    last_pointer: (f64, f64),
}

#[derive(Default)]
struct Perf {
    since: Option<Instant>,
    frames: u32,
    ms: [f64; 4],
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let has = |name: &str| args.iter().any(|a| a == name);
    let mut game_dir = GameDir::locate().unwrap_or_else(|| {
        eprintln!("error: game not found; set OPENSOW_GAME_DIR");
        std::process::exit(1);
    });
    // Warmonger: a sibling folder is found by `locate`; otherwise `--warmonger DIR` or OPENSOW_WARMONGER_DIR (a launcher will set it)
    if let Some(dir) = get("--warmonger") {
        if !game_dir.set_warmonger(std::path::Path::new(&dir)) {
            eprintln!("error: {dir} is not a Warmonger install");
            std::process::exit(1);
        }
    }
    let headless = DEV && has("--screenshot");
    game_dir.edition = match get("--edition").as_deref() {
        Some(e) if e.eq_ignore_ascii_case("warmonger") => opensow_formats::Edition::Warmonger,
        Some(_) => opensow_formats::Edition::Original,
        None if !headless => edition::saved(),
        None => opensow_formats::Edition::Original,
    };
    if game_dir.edition == opensow_formats::Edition::Warmonger && game_dir.warmonger.is_none() {
        eprintln!("error: Warmonger was asked for but its install was not found; use --warmonger DIR or set OPENSOW_WARMONGER_DIR");
        game_dir.edition = opensow_formats::Edition::Original;
    }
    // --lang takes a code (EN SK CZ CS CT; PL HU RU fall back to English) or the movie folder name; else the launcher's / install's language
    let chosen = get("--lang").and_then(|l| {
        opensow_formats::lang::Language::from_code(&l)
            .or_else(|| (l.eq_ignore_ascii_case("CHINNESSE")).then_some(opensow_formats::lang::Language::ChineseSimplified))
    });
    if let Some(l) = chosen {
        game_dir.set_language(l);
    }
    let assets = Assets::load(game_dir).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });
    let mut game = Game::new(assets);
    game.apply_enhance();
    let lang = game.assets.game.language();
    game.assets.game.set_language(lang);
    // movie folders (both Chinese scripts share one)
    game.language = intro::LANGUAGES.iter().position(|n| Some(*n) == lang.diff_dir().or(Some(""))).unwrap_or(0);

    if let Some(path) = get("--screenshot").filter(|_| DEV) {
        headless::screenshot(game, &path, &args);
        return;
    }

    game.autosave = true;
    game.mute = has("--mute");
    game.sfx.enabled = !game.mute;
    // a windowed `--skirmish MAP` goes straight into the game (no intro, no menus)
    let straight = get("--skirmish").filter(|_| DEV);
    if let Some(map) = &straight {
        headless::start_skirmish_from_args(&mut game, map, &get, &has);
    }
    if !has("--no-intro") && straight.is_none() {
        game.movies = true;
        match boot::Boot::open(&game.assets.game) {
            Some(b) => {
                game.boot = Some(b);
                game.boot_music();
            }
            None => game.play_movie("mI", Then::Menu),
        }
    }
    if let Some(m) = std::env::var("OPENSOW_START").ok().filter(|_| DEV) {
        game.start(&m);
    }
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App {
        game,
        window: None,
        surface: None,
        gpu: None,
        last: Instant::now(),
        cursors: std::collections::HashMap::new(),
        cursor_key: None,
        frame: Vec::new(),
        window_res: (0, 0),
        replay: std::env::var_os("OPENSOW_CPU_DRAW").is_none(),
        fullscreen: !has("--windowed"),
        focused: true,
        confined: None,
        last_pointer: (0.0, 0.0),
        perf: std::env::var_os("OPENSOW_PERF").filter(|_| DEV).map(|_| Perf::default()),
    };
    event_loop.run_app(&mut app).expect("run");
}

#[cfg(test)]
mod tests;
