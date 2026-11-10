//! `opensow-game`: a playable prototype with the original menus and a skirmish on the campaign levels.
//!
//! ```text
//! opensow-game [--no-intro] [--mute] [--windowed] [--lang SK|CZ|CS|CT]   start the game fullscreen (F11 toggles, `--windowed` opens a window; shows the boot pictures and plays the intro movie first; any key or click skips it)
//!   `--no-enhance` (or OPENSOW_ENHANCE=0) turns off the run-time texture enhancement of the zoomed-in world (docs/enhance.md); `--no-fx` (OPENSOW_FX=0) the soft/contact shadows and explosion glow (docs/fx.md); `--no-sound-enhance` (OPENSOW_SOUND_ENHANCE=0) plays the sounds exactly as before: no sinc resampling, fades or limiter (docs/audio.md)
//! opensow-game --screenshot out.png [--size WxH] [--screen main|levels|options|pause|message|results]
//!              [--mute] [--boot-page N] [--intro-frame N [--movie mI|mO|M01..M22]] [--hover N] [--play M01 [--ticks N] [--select-all] [--select-building CX,CY] [--move CX,CY] [--zoom Z] [--center CX,CY] [--range] [--fx CX,CY,KIND,AGE]... [--no-mask]]
//! ```

mod assets;
mod devtools;
mod edition;
mod gdi;
mod gpu;
mod hifi;
mod replay;
mod hud;
mod infobox;
mod boot;
mod intro;
mod ingame;
mod lands;
mod options;
mod menu;
mod multiplayer;
mod music;
mod play;
mod saveui;
mod skirmish;
mod skirmish_ui;
mod tr;
mod stars;
mod sfx;
mod results;
mod sim;
mod tutorial_panel;
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
use winit::window::{CustomCursor, Cursor, Fullscreen, Window, WindowId};

/// What happens when the movie on screen ends or is skipped.
#[derive(Debug, Clone, PartialEq)]
enum Then {
    Menu,
    /// The briefing is over: start the mission.
    Level(String),
    /// The outro is over: show the closing message.
    Screen(Screen),
}

/// Made-up counters for the statistics screen's headless screenshots.
fn sample_stats() -> [[u32; 8]; 2] {
    [[3, 5, 12, 7, 4, 1, 2, 0], [1, 2, 20, 15, 0, 3, 1, 1]]
}

/// The last campaign mission; winning it plays the outro (`mO.sgv`).
const FINAL_LEVEL: &str = "M22";
/// The tutorial is level 0 (`0x41f140(0)`).
const TUTORIAL_LEVEL: &str = "M00";

struct Game {
    assets: Assets,
    /// The key event being handled comes from the numeric keypad (winit `KeyLocation::Numpad`).
    numpad: bool,
    /// The digit of the physical key being handled (top row 1-9, or numpad 0-9), whatever the layout: the original reads scan codes.
    digit: Option<u8>,
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

/// The running skirmish's choices, for RESTART and for the star.
#[derive(Clone)]
struct SkirmishRun {
    map: String,
    setup: multiplayer::Setup,
    human: usize,
    difficulty: sim::Difficulty,
    /// Dev tool (`--spectate`): the computer plays blue too, at this difficulty (docs/skirmish.md).
    spectate: Option<sim::Difficulty>,
    /// The lobby still had the map's own credits, research, air forces and turrets: a win earns a star.
    default_setup: bool,
}

impl Game {
    fn new(assets: Assets) -> Game {
        let levels = opensow_viewer::scene::Scene::list(&assets.game);
        let mut g = Game { assets, numpad: false, digit: None, game_speed: 1.0, settings: Settings::default(), levels, screen: Some(Screen::Main), back: Vec::new(), play: None, carry_counter: 0, hover: None, pressed: None, shift: false, quick_save: false, scale: 1.0, cursor: (0, 0), quit: false, intro: None, boot: None, then: Then::Menu, language: 0, movies: false, mute: false, lands: None, options_menu: None, saveui: None, net_menu: None, skirmish_menu: None, skirmish: None, stars: stars::Stars::load(), ingame: None, results: None, options: options::GameOptions::load(), progress: opensow_formats::config::PassedLevels::initial(), campaign: false, music: music::Music::new(), sfx: sfx::Sfx::new(), dev: devtools::DevTools::default() };
        g.apply_audio();
        g.progress = g.load_progress();
        g
    }

    /// The saved progress of the selected campaign, or its start (tests never read the disk).
    fn load_progress(&self) -> opensow_formats::config::PassedLevels {
        use opensow_formats::{config::PassedLevels, Edition};
        let saved = if cfg!(test) { None } else { saveui::Store::locate(&self.assets.game).load_progress() };
        saved.unwrap_or_else(|| if self.assets.game.edition == Edition::Warmonger { PassedLevels::initial_warmonger() } else { PassedLevels::initial() })
    }

    /// Switch the campaign (docs/warmonger.md): mission data, skirmish maps, progress and saves follow the edition.
    fn set_edition(&mut self, e: opensow_formats::Edition) {
        if self.assets.game.edition == e || (e == opensow_formats::Edition::Warmonger && self.assets.game.warmonger.is_none()) {
            return;
        }
        self.save_progress();
        self.assets.game.edition = e;
        self.levels = opensow_viewer::scene::Scene::list(&self.assets.game);
        self.progress = self.load_progress();
        edition::remember(e);
    }

    /// Writes the campaign progress to `player.cfg` (the exe does it after every change); tests never touch the disk.
    fn save_progress(&self) {
        if !cfg!(test) {
            if let Err(e) = saveui::Store::locate(&self.assets.game).save_progress(&self.progress) {
                eprintln!("progress not saved: {e}");
            }
        }
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

    /// The boot screens are over: the intro movie follows, then the menu.
    /// The exe restarts `uvod.wav` whenever a boot page starts.
    fn boot_music(&mut self) {
        if self.boot.as_mut().is_some_and(|b| b.take_changed()) {
            self.music.stop();
            self.play_music("uvod");
        }
    }

    fn end_boot(&mut self) {
        self.boot = None;
        self.play_movie("mI", Then::Menu);
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
        self.skirmish = None;
        match Play::new(&mut self.assets, level, &self.settings, self.carry_counter) {
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
            Action::Tutorial => {
                // state 5: no briefing movie, the level (0) loads at once and the tutorial script starts at step 0
                self.campaign = false;
                self.start(TUTORIAL_LEVEL);
            }
            Action::Unavailable(msg) => self.push(Screen::Message(msg.to_owned())),
            Action::Options => self.open_options(),
            Action::LoadGame => self.open_saveui(saveui::Mode::Load),
            Action::Multiplayer => self.open_multiplayer(),
            Action::Skirmish => self.open_skirmish(),
            Action::Prototype => self.push(Screen::Options),
            Action::Exit => self.quit = true,
            Action::Level(l) => {
                self.campaign = false;
                self.begin(&l)
            }
            Action::Back => {
                if matches!(self.screen, Some(Screen::Message(_))) && self.play.is_some() && self.back.is_empty() {
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
                self.ingame = None;
                self.screen = None;
                self.back.clear();
            }
            Action::Restart => {
                self.ingame = None;
                if let Some(run) = self.skirmish.clone() {
                    let script = skirmish::load_map(&self.assets.game, &run.map);
                    if let Some(script) = script {
                        self.start_skirmish(run, script);
                    }
                } else if let Some(l) = self.play.as_ref().map(|p| p.level.clone()) {
                    self.start(&l);
                }
            }
            Action::ToMain => {
                self.ingame = None;
                self.skirmish = None;
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
        self.play_music(if music::pick(2) == 0 { "Brief2" } else { "Brief" }); // 0x441cf9/0x441c4b: random 0 -> brief2.wav
        let progress = self.effective_progress();
        self.lands = Some(lands::Lands::new(&mut self.assets, progress, video && self.movies, self.mute));
    }

    /// The campaign progress the map shows: with the dev tools' UNLOCK ALL every locked land is open.
    fn effective_progress(&self) -> opensow_formats::config::PassedLevels {
        let mut p = self.progress.clone();
        let wm = self.assets.game.edition == opensow_formats::Edition::Warmonger;
        if self.dev.unlock_all {
            for l in 1..p.states.len() {
                if p.states[l] == 0 && (!wm || opensow_formats::warmonger::case_of(l).is_some()) {
                    p.states[l] = 2;
                }
            }
        }
        p
    }

    /// Mission running with nothing on top of it (the dev tools may place units).
    /// The frame can be recorded for the GPU (`opensow_viewer::record`): everything but a frame a save thumbnail is taken from.
    fn gpu_drawable(&self) -> bool {
        // the save thumbnails are taken from the drawn picture: those frames stay on the CPU
        !self.quick_save && !self.saveui.as_ref().is_some_and(|m| m.needs_capture())
    }

    fn in_mission(&self) -> bool {
        self.play.is_some() && self.screen.is_none() && self.intro.is_none() && self.lands.is_none() && self.options_menu.is_none() && self.saveui.is_none() && self.net_menu.is_none() && self.skirmish_menu.is_none() && self.results.is_none()
    }

    /// Dev tools: a pressed tool button took effect.
    fn dev_event(&mut self, item: devtools::Item) {
        use devtools::Item;
        self.play_sfx(29);
        match item {
            Item::UnlockAll => {
                let p = self.effective_progress();
                if let Some(l) = self.lands.as_mut() {
                    l.progress = p;
                }
            }
            Item::Kind(k) => {
                if let (true, Some(p)) = (self.in_mission(), self.play.as_mut()) {
                    p.dev_spawn = if p.dev_spawn.map(|d| d.0) == Some(k) { None } else { Some((k, self.dev.side)) };
                }
            }
            Item::Air(g) => {
                if let (true, Some(p)) = (self.in_mission(), self.play.as_mut()) {
                    p.world.air_stock[self.dev.side][g] += 1;
                }
            }
            Item::Side(s) => {
                if let Some(d) = self.play.as_mut().and_then(|p| p.dev_spawn.as_mut()) {
                    d.1 = s;
                }
            }
            _ => {}
        }
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
            lands::Event::Edition(e) => {
                self.set_edition(e);
                // the same map for the other campaign: no animation, the music keeps playing
                let progress = self.effective_progress();
                self.lands = Some(lands::Lands::new(&mut self.assets, progress, false, self.mute));
            }
            lands::Event::Accept(land) => {
                self.lands = None;
                self.campaign = true;
                self.begin(&format!("M{land:02}"));
            }
        }
    }

    /// Esc in a mission: the in-game menu opens (sound 35).
    fn open_pause(&mut self) {
        self.screen = Some(Screen::Pause);
        self.ingame = None;
        self.pause_menu();
    }

    /// The in-game menu of the pause screen (made on demand, e.g. when the options menu returns to it).
    fn pause_menu(&mut self) -> &mut ingame::InGameMenu {
        if self.ingame.is_none() {
            let tutorial = self.play.as_ref().is_some_and(|p| p.world.tutorial.is_some());
            self.ingame = Some(ingame::InGameMenu::new(&mut self.assets, tutorial));
        }
        self.ingame.as_mut().unwrap()
    }

    fn ingame_sounds(&mut self) {
        let ids: Vec<u8> = self.ingame.as_mut().map(|m| std::mem::take(&mut m.sounds)).unwrap_or_default();
        for id in ids {
            self.play_sfx(id);
        }
    }

    fn ingame_event(&mut self, e: ingame::Event) {
        match e {
            ingame::Event::None => {}
            ingame::Event::Continue => {
                self.play_sfx(35);
                self.act(Action::Resume);
            }
            ingame::Event::Options => self.act(Action::Options),
            ingame::Event::Load => self.open_saveui(saveui::Mode::Load),
            ingame::Event::Save => self.open_saveui(saveui::Mode::Save),
            ingame::Event::Restart => self.act(Action::Restart),
            ingame::Event::MainMenu => self.act(Action::ToMain),
        }
    }

    /// Open the options menu; O.K. / CANCEL return to the screen it was opened from.
    fn open_multiplayer(&mut self) {
        self.net_menu = Some(multiplayer::NetMenu::new(&mut self.assets));
        self.screen = None;
    }

    /// Main menu SKIRMISH: the multiplayer lobby against the computer (docs/skirmish.md).
    fn open_skirmish(&mut self) {
        self.skirmish_menu = Some(skirmish_ui::SkirmishMenu::new(&mut self.assets, &self.stars));
        self.screen = None;
    }

    /// `--skirmish`: picks the map by (a prefix of) its name in the skirmish lobby and starts it.
    fn headless_skirmish(&mut self, map: &str, human: usize, credits: Option<(i32, i32)>, research: Option<(i32, i32)>, difficulty: sim::Difficulty, spectate: Option<sim::Difficulty>) {
        let mut menu = skirmish_ui::SkirmishMenu::new(&mut self.assets, &self.stars);
        let wanted = map.to_lowercase();
        let Some(i) = menu.maps.iter().position(|m| m.to_lowercase().starts_with(&wanted)) else {
            eprintln!("no skirmish map {map}; available: {:?}", menu.maps);
            return;
        };
        menu.select_map(i);
        if let Some((a, b)) = credits {
            menu.setup.sides[0].credits = a;
            menu.setup.sides[1].credits = b;
        }
        if let Some((a, b)) = research {
            menu.setup.sides[0].research = a;
            menu.setup.sides[1].research = b;
        }
        if let Some(script) = menu.map_level() {
            let run = SkirmishRun { map: menu.maps[i].clone(), setup: menu.setup.clone(), human, difficulty, spectate, default_setup: menu.is_default() };
            self.start_skirmish(run, script);
        }
    }

    /// Starts a skirmish: the map's script with the lobby's choices written in and the sides arranged so the human is player 0.
    fn start_skirmish(&mut self, run: SkirmishRun, script: opensow_formats::level::Level) {
        let (map, level) = (run.map.clone(), skirmish::prepare(&script, &run.setup, run.human));
        match Play::new_skirmish(&mut self.assets, &map, level, &self.settings, run.difficulty) {
            Ok(mut p) => {
                if let Some(d) = run.spectate {
                    p.world.spectate = true;
                    p.world.difficulty0 = d;
                }
                self.play_music(&format!("Level{:02}", 1 + music::pick(4)));
                self.play = Some(Box::new(p));
                self.screen = None;
                self.back.clear();
                self.campaign = false;
                self.skirmish = Some(run);
            }
            Err(e) => {
                eprintln!("cannot start skirmish {map}: {e}");
                self.screen = Some(Screen::Message(format!("Cannot start the skirmish on {map}:\n{e}")));
            }
        }
    }

    fn skirmish_event(&mut self, e: skirmish_ui::Event) {
        let ids: Vec<u8> = self.skirmish_menu.as_mut().map(|m| std::mem::take(&mut m.sounds)).unwrap_or_default();
        for id in ids {
            self.play_sfx(id);
        }
        match e {
            skirmish_ui::Event::None => {}
            skirmish_ui::Event::Leave => {
                self.skirmish_menu = None;
                self.screen = Some(Screen::Main);
            }
            skirmish_ui::Event::Start => {
                let Some(m) = self.skirmish_menu.take() else { return };
                let map = m.maps.get(m.map).cloned().unwrap_or_default();
                match m.map_level() {
                    Some(script) => self.start_skirmish(SkirmishRun { map, setup: m.setup.clone(), human: m.local, difficulty: m.difficulty, spectate: None, default_setup: m.is_default() }, script),
                    None => self.screen = Some(Screen::Message(format!("Cannot read the map {map}."))),
                }
            }
        }
    }

    fn net_event(&mut self, e: multiplayer::Event) {
        let ids: Vec<u8> = self.net_menu.as_mut().map(|m| std::mem::take(&mut m.sounds)).unwrap_or_default();
        for id in ids {
            self.play_sfx(id);
        }
        match e {
            multiplayer::Event::None => {}
            multiplayer::Event::Leave => {
                self.net_menu = None;
                self.screen = Some(Screen::Main);
            }
            multiplayer::Event::Start { map, .. } => {
                self.net_menu = None;
                self.screen = Some(Screen::Message(format!("The multiplayer match on {map}\nneeds the network game, which is not playable yet.")));
            }
        }
    }

    fn open_options(&mut self) {
        if let Some(cur) = self.screen.take() {
            self.back.push(cur);
        }
        self.options_menu = Some(options::OptionsMenu::new(&mut self.assets, self.options.clone()));
    }

    /// Open the load or save page (state `0xc` / `0xd` in a mission, `0x11` from the main menu).
    fn open_saveui(&mut self, mode: saveui::Mode) {
        if let Some(cur) = self.screen.take() {
            self.back.push(cur);
        }
        let level = self.play.as_ref().map(|p| p.level.clone()).unwrap_or_default();
        let store = saveui::Store::locate(&self.assets.game);
        let mut ui = saveui::SaveUi::new(&mut self.assets, mode, &level, store);
        if let (saveui::Mode::Save, Some(p)) = (mode, self.play.as_mut()) {
            p.progress = self.progress.clone();
            ui.snapshot = p.snapshot();
        }
        self.saveui = Some(ui);
    }

    /// F5 in a mission (`0x416811`): grab the screen (`0x445070`) and save slot 0 with the title "Quick Save" (`0x41a9c0`), then the
    /// "game saved" speech (`0x428bb0`).
    fn do_quick_save(&mut self, buf: &[u32], w: usize, h: usize) {
        let Some(p) = self.play.as_mut() else { return };
        p.progress = self.progress.clone();
        let (level, snapshot) = (p.level.clone(), p.snapshot());
        let thumb = opensow_formats::save::grab_thumbnail(buf, w, h);
        let store = saveui::Store::locate(&self.assets.game);
        match store.write(0, "Quick Save", &level, &snapshot, &thumb) {
            Ok(()) => self.sfx.say(&self.assets.game, "01-game-saved.wav"),
            Err(e) => eprintln!("quick save failed: {e}"),
        }
    }

    /// Shift + F5 in a mission (`0x416850` -> `0x41a850(0)` -> `0x425d00`): load slot 0; nothing happens when it cannot be read.
    fn quick_load(&mut self) {
        let store = saveui::Store::locate(&self.assets.game);
        if let Some((level, edition, snapshot)) = store.level_of(0) {
            self.saveui_event(saveui::Event::Load(level, edition, snapshot));
        }
    }

    /// Play the sounds the load / save page asked for.
    fn saveui_sounds(&mut self) {
        let ids: Vec<u8> = self.saveui.as_mut().map(|m| std::mem::take(&mut m.sounds)).unwrap_or_default();
        for id in ids {
            self.play_sfx(id);
        }
    }

    /// What the page decided. In a mission every way out resumes the game (`0x42c260(7)`); from the main menu CANCEL goes back.
    fn saveui_event(&mut self, e: saveui::Event) {
        self.saveui_sounds();
        let in_mission = self.play.is_some();
        let leave = |g: &mut Game| {
            g.saveui = None;
            if in_mission {
                g.act(Action::Resume);
            } else {
                g.screen = Some(g.back.pop().unwrap_or(Screen::Main));
            }
        };
        match e {
            saveui::Event::None => {}
            saveui::Event::Cancel => leave(self),
            saveui::Event::Saved => {
                leave(self);
                self.sfx.say(&self.assets.game, "01-game-saved.wav");
            }
            saveui::Event::SaveFailed(msg) => {
                leave(self);
                self.push(Screen::Message(format!("Cannot save the game.\n{msg}")));
            }
            saveui::Event::Load(level, edition, snapshot) => {
                self.saveui = None;
                self.ingame = None;
                self.back.clear();
                self.campaign = false;
                // a slot knows its campaign (docs/warmonger.md): its missions are only there while that edition is selected
                self.set_edition(edition);
                if self.assets.game.edition != edition {
                    self.act(Action::ToMain);
                    self.push(Screen::Message("This savegame belongs to Warmonger,\nwhich was not found.".into()));
                    return;
                }
                self.start(&level);
                if !snapshot.is_empty() {
                    if let Some(p) = self.play.as_mut() {
                        if let Err(e) = p.restore(&snapshot) {
                            eprintln!("savegame snapshot not applied ({e}); the level starts over");
                        }
                    }
                }
            }
            saveui::Event::LoadFailed(_) => {
                // the exe falls back to the main menu when the state cannot be loaded
                self.saveui = None;
                self.act(Action::ToMain);
                self.push(Screen::Message("The savegame cannot be loaded.".into()));
            }
        }
    }

    /// Hand the menu's current values to the sound players (the exe applies them as they change).
    fn apply_audio(&mut self) {
        let o = match &self.options_menu {
            Some(m) => m.opts.clone(),
            None => self.options.clone(),
        };
        self.sfx.fx_on = o.fx_on;
        self.sfx.fx_volume = o.fx_volume;
        self.sfx.voice_on = o.voice_on;
        self.sfx.voice_volume = o.voice_volume;
        self.music.set(o.music_on && !self.mute, o.music_volume);
        (self.settings.scroll_keys, self.settings.scroll_mouse) = (o.cfg.scroll_b, o.cfg.scroll_a);
        (self.settings.radar_on, self.settings.quake_on) = (o.cfg.flag_a, o.cfg.flag_b);
        self.settings.cloud_detail = o.cfg.detail;
    }

    /// Above zoom 1 the world is drawn from 2x enhanced copies of the original art (viewer `upscale`, docs/enhance.md): the Options
    /// switch on page 2, unless `--no-enhance` / `OPENSOW_ENHANCE=0` forces it off for this run.
    fn apply_enhance(&self) {
        let forced_off = std::env::args().any(|a| a == "--no-enhance") || std::env::var("OPENSOW_ENHANCE").is_ok_and(|v| v == "0");
        opensow_viewer::upscale::set_enabled(self.options.extra.enhance && !forced_off);
        // the sound enhancement (`hifi`, docs/audio.md): the Options switch on page 2, unless `--no-sound-enhance` /
        // `OPENSOW_SOUND_ENHANCE=0` forces it off for this run
        let sound_off = std::env::args().any(|a| a == "--no-sound-enhance") || std::env::var("OPENSOW_SOUND_ENHANCE").is_ok_and(|v| v == "0");
        hifi::set_enabled(self.options.extra.sound && !sound_off);
        // the lighting effects (docs/fx.md): `--no-fx` / `OPENSOW_FX=0` force all three off
        let fx_off = std::env::args().any(|a| a == "--no-fx") || std::env::var("OPENSOW_FX").is_ok_and(|v| v == "0");
        let e = &self.options.extra;
        opensow_viewer::fx::set(e.soft_shadows && !fx_off, e.contact_shadows && !fx_off, e.glow && !fx_off, e.turret_shadows && !fx_off);
    }

    fn options_event(&mut self, e: options::Event) {
        let done = matches!(e, options::Event::Done(_));
        match e {
            options::Event::None => {}
            options::Event::Done(o) | options::Event::Cancel(o) => {
                if done {
                    o.save();
                }
                self.options = o;
                self.options_menu = None;
                self.screen = self.back.pop();
                self.apply_audio();
                self.apply_enhance();
            }
        }
    }

    /// Play the sounds the options menu asked for.
    fn options_sounds(&mut self) {
        let ids: Vec<u8> = self.options_menu.as_mut().map(|m| std::mem::take(&mut m.sounds)).unwrap_or_default();
        for id in ids {
            self.play_sfx(id);
        }
    }

    fn push(&mut self, next: Screen) {
        if let Some(cur) = self.screen.replace(next) {
            self.back.push(cur);
        }
    }

    fn draw(&mut self, buf: &mut [u32], w: usize, h: usize) {
        self.draw_screen(buf, w, h);
        if self.intro.is_none() && self.boot.is_none() {
            let armed = self.play.as_ref().and_then(|p| p.dev_spawn);
            let in_mission = self.in_mission();
            if let (Some((kind, _)), Some(p), true) = (armed, self.play.as_ref(), in_mission) {
                // the cell under the pointer shows where the unit will land
                let (mx, my) = p.to_map(self.cursor.0, self.cursor.1);
                let (sx, sy) = p.view.to_screen((mx.div_euclid(32) * 32) as f32, (my.div_euclid(32) * 32) as f32);
                let z = p.view.zoom;
                let r = ui::Rect { x: sx, y: sy, w: (32.0 * z) as i32, h: (32.0 * z) as i32 };
                let blocked = p.world.is_blocked(mx.div_euclid(32), my.div_euclid(32));
                ui::fill(buf, w, h, r, if blocked { (255, 80, 60) } else { (60, 255, 120) }, 70);
                let font = self.assets.font("small");
                let hs = (h as f32 / 768.0).max(1.0);
                ui::text(buf, w, h, &font, &devtools::kind_label(kind), r.x + r.w + 6, r.y, hs, (255, 255, 120), (0, 0, 0));
            }
            self.dev.draw(&mut self.assets, buf, w, h, armed.map(|a| a.0), in_mission);
        }
    }

    fn draw_screen(&mut self, buf: &mut [u32], w: usize, h: usize) {
        if let Some(b) = &self.boot {
            b.draw(buf, w, h);
            return;
        }
        if let Some(i) = &self.intro {
            i.draw(buf, w, h);
            return;
        }
        if let Some(l) = self.lands.as_mut() {
            l.draw(buf, w, h);
            return;
        }
        if let Some(m) = self.options_menu.as_mut() {
            m.draw(buf, w, h);
            return;
        }
        if let Some(m) = self.net_menu.as_mut() {
            m.draw(buf, w, h);
            return;
        }
        if let Some(m) = self.skirmish_menu.as_mut() {
            m.draw(buf, w, h);
            return;
        }
        if let Some(r) = self.results.as_ref().filter(|r| r.in_statistics()) {
            r.draw_statistics(buf, w, h);
            return;
        }
        if let Some(play) = self.play.as_mut() {
            play.draw(&mut self.assets, &self.settings, buf, w, h, self.scale);
        }
        if std::mem::take(&mut self.quick_save) {
            self.do_quick_save(buf, w, h);
        }
        if let Some(m) = self.saveui.as_mut() {
            if m.needs_capture() {
                m.capture(buf, w, h);
            }
            m.draw(buf, w, h);
            return;
        }
        if let Some(r) = self.results.as_ref() {
            r.draw_banner(buf, w, h);
            return;
        }
        if matches!(self.screen, Some(Screen::Pause)) {
            self.pause_menu().draw(buf, w, h);
            return;
        }
        let buttons = self.buttons(w, h);
        if let Some(screen) = self.screen.clone() {
            menu::draw(&screen, &buttons, self.hover, &mut self.assets, buf, w, h, self.play.is_some());
        }
    }

    /// Per-frame update; opens the outcome message when the mission ends.
    /// In a running mission (no menu on top) the game draws its own pointer sprite instead of the system one.
    fn custom_pointer(&self) -> bool {
        self.screen.is_none() && self.intro.is_none() && self.lands.is_none() && self.options_menu.is_none() && self.saveui.is_none() && self.net_menu.is_none() && self.skirmish_menu.is_none() && self.play.as_ref().is_some_and(|p| p.edge_scroll)
    }

    fn update(&mut self, dt: f32, w: usize, h: usize) {
        self.music.tick();
        if let Some(b) = self.boot.as_mut() {
            if b.update(dt as f64) {
                self.end_boot();
            } else {
                self.boot_music();
            }
            return;
        }
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
        if let Some(r) = self.results.as_mut() {
            r.update(dt as f64);
            for id in std::mem::take(&mut r.sounds) {
                self.play_sfx(id);
            }
            return;
        }
        if let Some(m) = self.saveui.as_mut() {
            m.update((dt * 60.0).round() as u32);
            return;
        }
        if let Some(m) = self.skirmish_menu.as_mut() {
            m.update((dt * 60.0).round() as u32);
            return;
        }
        if self.options_menu.is_some() || self.net_menu.is_some() {
            return;
        }
        if self.screen.is_none() {
            if let Some(p) = self.play.as_mut() {
                p.world.cheats = self.dev.cheats();
                let settings = menu::Settings { ticks_per_second: (self.settings.ticks_per_second as f32 * self.game_speed).round() as u32 * self.dev.speed, ..self.settings.clone() };
                p.update(dt, &settings, w, h);
                self.carry_counter = p.world.carry_counter;
                // HUD sounds raised while updating: icon hover (50), info box open / close (51 / 52)
                p.ui_sounds.extend(p.infobox.sounds.drain(..));
                // the player's own actions raised by the world (air strike launch, turret placed): priority, heard anywhere
                p.ui_sounds.extend(p.world.player_sounds.drain(..));
                let hud_sounds: Vec<u8> = std::mem::take(&mut p.ui_sounds);
                for id in hud_sounds {
                    self.play_sfx(id);
                }
                let p = self.play.as_mut().unwrap();
                let (zoom, vx, vy) = (p.view.zoom, p.view.x, p.view.y);
                // the exe pans by position and fades with the distance from the screen centre (`sfx::pan` / `sfx::volume`)
                let (cx, cy) = (vx + w as f32 / zoom / 2.0, vy + h as f32 / zoom / 2.0);
                let heard: Vec<(u8, bool, i32, i32, i32)> = p.world.sounds.drain(..).map(|(id, x, y, j)| (id, j, (x as f32 - cx) as i32, (y as f32 - cy) as i32, (x as f32 - vx) as i32 - 320)).collect();
                for (id, jitter, dx, dy, pan_dx) in heard {
                    self.sfx.play_at(&self.assets.game, id, if jitter { sfx::jitter() } else { 1.0 }, dx, dy, pan_dx);
                }
                // the message events are spoken (`0x402ec0`), with the info-box / icon blips that go with some of them
                let news: Vec<u8> = p.world.log[p.heard.min(p.world.log.len())..].iter().map(|m| m.1).collect();
                p.heard = p.world.log.len();
                for id in news {
                    if let Some(file) = opensow_formats::messages::MESSAGES.iter().find(|m| m.0 == id).and_then(|m| m.2) {
                        self.sfx.say(&self.assets.game, file);
                    }
                    match id {
                        11 | 12 => self.sfx.play(&self.assets.game, 51, 1.0), // infobox-income
                        18..=22 => self.sfx.play(&self.assets.game, 49, 1.0), // airicon-come
                        _ => {}
                    }
                }
                for n in std::mem::take(&mut p.tut_voices) {
                    self.sfx.say_in(&self.assets.game, "Tutorial", &format!("{n:02}.wav"));
                }
                let outcome = p.outcome();
                if outcome.is_some() && p.world.tutorial.is_some() {
                    // the script's last step is `0x42c260(0x1c)`: straight back to the main menu, no result screen
                    self.act(Action::ToMain);
                    return;
                }
                if let (Some(o), true) = (outcome, self.skirmish.as_ref().is_some_and(|r| r.spectate.is_some())) {
                    // spectating: no banner, statistics or stars; say who won (blue is player 0, the Defeat of the HQ rules)
                    let who = if o == sim::Outcome::Victory { "Blue" } else { "Green" };
                    let t = self.play.as_ref().map_or(0, |p| p.world.tick);
                    self.screen = Some(Screen::Message(format!("{who} wins after {}:{:02}.", t / 3600, t / 60 % 60)));
                    return;
                }
                if let Some(o) = outcome {
                    // 0x4357a0: the mission-end speech, then the banner over the frozen frame
                    let won = o == sim::Outcome::Victory;
                    // a skirmish won with the map's default setup earns the star of its level (stars.rs)
                    if let Some(run) = self.skirmish.clone().filter(|r| won && r.default_setup) {
                        if self.stars.record(&run.map, run.difficulty) {
                            self.stars.save();
                        }
                    }
                    self.sfx.say(&self.assets.game, if won { "01-mis-complet.wav" } else { "01-mis-failed.wav" });
                    let p = self.play.as_ref().unwrap();
                    let stats = [p.world.stats[0], p.world.stats[1]];
                    let tutorial = p.world.tutorial.is_some();
                    self.results = Some(results::Results::new(&mut self.assets, won, tutorial, stats, self.language));
                }
            }
        }
    }

    /// The banner or statistics screen decided something (`0x4357a0`, `0x44d770`).
    fn results_event(&mut self, e: results::Event) {
        match e {
            results::Event::None => {}
            results::Event::Statistics => self.play_music("Statistic"),
            results::Event::Done => {
                let Some(r) = self.results.take() else { return };
                let level = self.play.as_ref().map(|p| p.level.clone()).unwrap_or_default();
                if !r.won || r.tutorial {
                    // lost (and the tutorial): the main menu (state 0x1c)
                    self.act(Action::ToMain);
                    return;
                }
                if self.campaign {
                    // CPassedLevels::Pass (0x4565a0, called by the statistics screen): this land is done and the next one opens
                    let n = level.trim_start_matches(['M', 'm']).parse().unwrap_or(0);
                    if self.assets.game.edition == opensow_formats::Edition::Warmonger {
                        self.progress.pass_warmonger(n);
                    } else {
                        self.progress.pass(n);
                    }
                    self.save_progress();
                }
                let done = Screen::Message("Campaign complete!\nThe enemy is defeated.".into());
                if level.eq_ignore_ascii_case(FINAL_LEVEL) {
                    self.play_movie("mO", Then::Screen(done));
                } else if self.campaign {
                    // state 0x19: the campaign map
                    self.play = None;
                    self.screen = None;
                    self.open_lands(false);
                } else {
                    self.act(Action::ToMain);
                }
            }
        }
    }

    fn mouse_moved(&mut self, x: i32, y: i32, w: usize, h: usize) {
        let prev = self.cursor;
        self.cursor = (x, y);
        if self.intro.is_none() {
            let fonts = self.assets.font("small");
            if self.dev.mouse_moved(&fonts, x, y, w, h) {
                return;
            }
        }
        if let Some(l) = self.lands.as_mut() {
            l.mouse_moved(x, y, w, h);
            self.lands_sounds();
            return;
        }
        if let Some(m) = self.options_menu.as_mut() {
            m.mouse_moved(x, y, w, h);
            self.options_sounds();
            return;
        }
        if let Some(m) = self.saveui.as_mut() {
            m.mouse_moved(x, y, w, h);
            self.saveui_sounds();
            return;
        }
        if let Some(m) = self.net_menu.as_mut() {
            m.mouse_moved(x, y, w, h);
            self.net_event(multiplayer::Event::None);
            return;
        }
        if let Some(m) = self.skirmish_menu.as_mut() {
            m.mouse_moved(x, y, w, h);
            self.skirmish_event(skirmish_ui::Event::None);
            return;
        }
        if self.results.is_some() || self.play.as_ref().is_some_and(|p| p.paused && self.screen.is_none()) {
            return; // the pause and the mission-end screens ignore the mouse
        }
        if let Some(p) = self.play.as_mut() {
            if p.right_down_now() {
                // the pointer stays put while the right button scrolls the map
                p.right_drag(x - prev.0, y - prev.1);
                return;
            }
            p.cursor = (x, y);
            p.edge_scroll = true;
        }
        if matches!(self.screen, Some(Screen::Pause)) {
            self.pause_menu().mouse_moved(x, y, w, h);
            self.ingame_sounds();
            return;
        }
        let buttons = self.buttons(w, h);
        let hover = menu::hit(&buttons, x, y);
        if hover.is_some() && hover != self.hover && self.screen.is_some() {
            self.play_sfx(30); // mainmenu-move
        }
        self.hover = hover;
    }

    fn mouse_button(&mut self, button: MouseButton, down: bool, x: i32, y: i32, w: usize, h: usize) {
        if self.intro.is_none() {
            // the dev tools panel takes the clicks that land on it
            let fonts = self.assets.font("small");
            if button != MouseButton::Left {
                if self.dev.covers(&fonts, x, y, w, h) {
                    return;
                }
            } else if down {
                if self.dev.press(&fonts, x, y, w, h) {
                    return;
                }
            } else {
                let (consumed, item) = self.dev.release(&fonts, x, y, w, h);
                if let Some(i) = item {
                    self.dev_event(i);
                }
                if consumed {
                    return;
                }
            }
        }
        if let Some(l) = self.lands.as_mut() {
            if down && button == MouseButton::Left {
                let e = l.click(x, y, w, h);
                if !self.dev.unlock_all && self.progress != l.progress {
                    self.progress = l.progress.clone();
                    self.save_progress();
                }
                self.lands_sounds();
                self.lands_event(e);
            }
            return;
        }
        if let Some(m) = self.options_menu.as_mut() {
            if down && button == MouseButton::Left {
                let e = m.click(x, y, w, h);
                self.options_sounds();
                self.apply_audio();
                self.options_event(e);
            }
            return;
        }
        if let Some(m) = self.saveui.as_mut() {
            if down && button == MouseButton::Left {
                let e = m.click(x, y, w, h);
                self.saveui_event(e);
            }
            return;
        }
        if let Some(m) = self.net_menu.as_mut() {
            if down && button == MouseButton::Left {
                let e = m.click(x, y, w, h);
                self.net_event(e);
            }
            return;
        }
        if let Some(m) = self.skirmish_menu.as_mut() {
            if down && button == MouseButton::Left {
                let e = m.click(x, y, w, h);
                self.skirmish_event(e);
            }
            return;
        }
        if let Some(r) = self.results.as_mut() {
            // banner: any button leaves after two seconds; statistics: the left button (`[player+0xa]`)
            if down && (button == MouseButton::Left || !r.in_statistics()) {
                let e = r.input(false, true);
                self.results_event(e);
            }
            return;
        }
        if self.boot.is_some() {
            if down && button == MouseButton::Left {
                if self.boot.as_mut().is_some_and(|b| b.skip_now()) {
                    self.end_boot();
                } else {
                    self.boot_music();
                }
            }
            return;
        }
        if self.intro.is_some() {
            if down {
                self.end_movie();
            }
            return;
        }
        if matches!(self.screen, Some(Screen::Pause)) {
            if down && button == MouseButton::Left {
                let e = self.pause_menu().click(x, y, w, h);
                self.ingame_sounds();
                self.ingame_event(e);
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
        if p.paused {
            return; // the pause (`0x435330`) reads no mouse button
        }
        match (button, down) {
            (MouseButton::Left, true) => p.left_down(x, y),
            (MouseButton::Left, false) => p.left_up(x, y, self.shift),
            (MouseButton::Right, true) => p.right_down(),
            (MouseButton::Right, false) => p.right_up(x, y),
            _ => {}
        }
        let ids: Vec<u8> = std::mem::take(&mut p.ui_sounds);
        for id in ids {
            self.play_sfx(id);
        }
    }

    /// The digit a key stands for: the physical key's (top row or numpad, any layout; Czech needs shift for the logical digits),
    /// else the character itself.
    fn digit_of(&self, key: &Key) -> Option<u8> {
        self.digit.or_else(|| match key {
            Key::Character(c) if c.len() == 1 && c.as_bytes()[0].is_ascii_digit() => Some(c.as_bytes()[0] - b'0'),
            _ => None,
        })
    }

    fn key(&mut self, key: &Key, down: bool, w: usize, h: usize) {
        if down && self.intro.is_none() {
            let typing = self.saveui.is_some() || self.net_menu.is_some() || self.play.as_ref().is_some_and(|p| p.chat.is_some());
            match key {
                Key::Named(NamedKey::F12) => {
                    self.dev.toggle_visible();
                    return;
                }
                Key::Character(c) if c.as_str() == "`" && !typing => {
                    self.dev.toggle_visible();
                    return;
                }
                Key::Named(NamedKey::Escape) if self.in_mission() && self.play.as_ref().is_some_and(|p| p.dev_spawn.is_some()) => {
                    if let Some(p) = self.play.as_mut() {
                        p.dev_spawn = None;
                    }
                    return;
                }
                _ => {}
            }
        }
        if let Some(l) = self.lands.as_mut() {
            if down {
                let e = if matches!(key, Key::Named(NamedKey::Escape)) { l.back() } else if l.playing_video() { l.back() } else { lands::Event::None };
                self.lands_event(e);
            }
            return;
        }
        if let Some(m) = self.saveui.as_mut() {
            if down {
                let e = match key {
                    Key::Named(NamedKey::Escape) => m.cancel(),
                    Key::Named(NamedKey::Enter) => m.confirm(),
                    Key::Named(NamedKey::Backspace) => {
                        m.backspace();
                        saveui::Event::None
                    }
                    Key::Named(NamedKey::Space) => {
                        m.type_text(" ");
                        saveui::Event::None
                    }
                    Key::Character(c) => {
                        m.type_text(c);
                        saveui::Event::None
                    }
                    _ => saveui::Event::None,
                };
                self.saveui_event(e);
            }
            return;
        }
        if let Some(m) = self.options_menu.as_mut() {
            if down && matches!(key, Key::Named(NamedKey::Escape)) {
                let e = m.escape();
                self.options_sounds();
                self.options_event(e);
            }
            return;
        }
        if let Some(m) = self.skirmish_menu.as_mut() {
            if down {
                let e = match key {
                    Key::Named(NamedKey::Escape) => m.escape(),
                    Key::Named(NamedKey::Enter) => m.enter(),
                    Key::Named(NamedKey::ArrowUp) => {
                        m.step_map(false);
                        skirmish_ui::Event::None
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        m.step_map(true);
                        skirmish_ui::Event::None
                    }
                    _ => skirmish_ui::Event::None,
                };
                self.skirmish_event(e);
            }
            return;
        }
        if let Some(m) = self.net_menu.as_mut() {
            if down {
                let e = match key {
                    Key::Named(NamedKey::Escape) => m.escape(),
                    Key::Named(NamedKey::Backspace) => {
                        m.backspace();
                        multiplayer::Event::None
                    }
                    Key::Named(NamedKey::Enter) => {
                        m.enter();
                        multiplayer::Event::None
                    }
                    Key::Named(NamedKey::Space) => {
                        m.typed(' ');
                        multiplayer::Event::None
                    }
                    Key::Character(c) => {
                        c.chars().for_each(|ch| m.typed(ch));
                        multiplayer::Event::None
                    }
                    _ => multiplayer::Event::None,
                };
                self.net_event(e);
            }
            return;
        }
        if let Some(r) = self.results.as_mut() {
            // banner: any key leaves after two seconds; statistics: Esc (`[player+0x65]`), a click leaves too
            if down {
                let e = r.input(matches!(key, Key::Named(NamedKey::Escape)), !r.in_statistics());
                self.results_event(e);
            }
            return;
        }
        if self.screen.is_none() && self.play.as_ref().is_some_and(|p| p.paused) {
            // the pause (`0x435330`) only waits for the pause key again (`[player+0x7d]` = scan code 0x19 = P, docs/hotkeys.md)
            if down && matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("p")) {
                if let Some(p) = self.play.as_mut() {
                    p.paused = false;
                }
            }
            return;
        }
        // developer shortcut: every mission, whether or not the campaign has opened it
        if down && matches!(self.screen, Some(Screen::Main)) && matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("l")) {
            self.push(Screen::Levels);
            return;
        }
        if self.boot.is_some() {
            if down {
                if self.boot.as_mut().is_some_and(|b| b.skip()) {
                    self.end_boot();
                } else {
                    self.boot_music();
                }
            }
            return;
        }
        if self.intro.is_some() {
            if down {
                self.end_movie();
            }
            return;
        }
        // the open chat line takes every key press (Esc closes it, Enter sends, Tab recalls the last line)
        if down && self.screen.is_none() && self.play.as_ref().is_some_and(|p| p.chat.is_some()) {
            if let Some(p) = self.play.as_mut() {
                match key {
                    Key::Named(NamedKey::Escape) => p.chat_cancel(),
                    Key::Named(NamedKey::Enter) => p.chat_send(),
                    Key::Named(NamedKey::Backspace) => p.chat_backspace(),
                    Key::Named(NamedKey::Tab) => p.chat_recall(),
                    Key::Named(NamedKey::Space) => p.chat_type(' '),
                    Key::Character(c) => c.chars().for_each(|ch| p.chat_type(ch)),
                    _ => {}
                }
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
                None if self.play.is_some() => {
                    self.open_pause();
                    self.ingame_sounds();
                }
                Some(Screen::Pause) => {
                    self.play_sfx(35);
                    self.act(Action::Resume)
                }
                Some(Screen::Main) => {}
                Some(_) => self.act(Action::Back),
                None => {}
            },
            Key::Character(c) if c.eq_ignore_ascii_case("p") && matches!(self.screen, Some(Screen::Main | Screen::Pause)) => self.act(Action::Prototype),
            // quick save / quick load: key flag `[player+0xa3]` = scan code 0x3f = F5, with a shift key (`[+0x8e]` / `[+0x9a]`) it loads
            Key::Named(NamedKey::F5) if self.screen.is_none() && self.play.is_some() => {
                if self.shift {
                    self.quick_load();
                } else {
                    self.quick_save = true;
                }
            }
            // F2 (`0x415c05`, key flag `[player+0xa0]`): in a numbered mission without remote players it plays sound 0x32 and leaves for the
            // campaign map (state 0x1a = `0x433d90`, "after a mission") with the mission number as the result code `[0x5baa90]`
            Key::Named(NamedKey::F2) if self.screen.is_none() && self.play.as_ref().map_or(false, |p| p.world.mission_no.map_or(false, |n| n != 0)) => {
                self.play_sfx(0x32);
                self.play = None;
                self.open_lands(false);
            }
            // P pauses (`0x415d05`: `[player+0x7d]`, scan code 0x19; docs/hotkeys.md)
            Key::Character(c) if self.screen.is_none() && c.eq_ignore_ascii_case("p") => {
                if let Some(p) = self.play.as_mut() {
                    p.paused ^= true;
                }
            }
            // Space and numpad 0 (`0x416177`): the view goes to the position of the last message event
            Key::Named(NamedKey::Space) if self.screen.is_none() => {
                if let Some(p) = self.play.as_mut() {
                    p.look_at_event();
                }
            }
            Key::Character(_) | Key::Dead(_) | Key::Unidentified(_) if self.screen.is_none() && self.numpad && self.digit_of(key) == Some(0) => {
                if let Some(p) = self.play.as_mut() {
                    p.look_at_event();
                }
            }
            Key::Named(NamedKey::Tab) if self.screen.is_none() => {
                if let Some(p) = self.play.as_mut() {
                    p.select_next_unit();
                }
            }
            Key::Named(NamedKey::Backspace) if self.screen.is_none() => {
                if let Some(p) = self.play.as_mut() {
                    p.look_at_hq();
                }
            }
            Key::Named(NamedKey::Enter) if self.screen.is_none() && !self.numpad => {
                if let Some(p) = self.play.as_mut() {
                    p.chat_open();
                }
            }
            Key::Named(NamedKey::Enter) if self.screen.is_none() && self.numpad => {
                if let Some(p) = self.play.as_mut() {
                    p.centre_on_selection();
                }
            }
            Key::Character(c) if self.screen.is_none() && (c.eq_ignore_ascii_case("h") || (self.numpad && c.as_str() == "+")) => {
                if let Some(p) = self.play.as_mut() {
                    p.select_next_advancer();
                }
            }
            Key::Character(c) if self.screen.is_none() && self.numpad && matches!(c.as_str(), "*" | "/" | "-") => {
                if let Some(p) = self.play.as_mut() {
                    p.select_visible(match c.as_str() { "/" => Some(true), "-" => Some(false), _ => None });
                }
            }
            // numpad 1-9: camera bookmarks (shift sets); the top-row 1-5 stay the turret keys
            Key::Character(_) | Key::Dead(_) | Key::Unidentified(_) if self.screen.is_none() && self.numpad && matches!(self.digit_of(key), Some(1..=9)) => {
                let (shift, n) = (self.shift, self.digit_of(key).unwrap_or(1) as usize - 1);
                if let Some(p) = self.play.as_mut() {
                    p.bookmark(n, shift);
                }
            }
            // F3 / F4 (`0x415c49` / `0x415cb1`): game speed -/+ 0.5 between 0.5 and 3.0, not in a network game
            Key::Named(NamedKey::F3) if self.screen.is_none() && self.play.is_some() => {
                self.game_speed = (self.game_speed - 0.5).max(0.5);
            }
            Key::Named(NamedKey::F4) if self.screen.is_none() && self.play.is_some() => {
                self.game_speed = (self.game_speed + 0.5).min(3.0);
            }
            Key::Character(c) if self.screen.is_none() && c.eq_ignore_ascii_case("q") => {
                if let Some(p) = self.play.as_mut() {
                    p.world.select_all();
                    p.sel_building = None;
                    let ids = p.select_all_sounds();
                    for id in ids {
                        self.play_sfx(id);
                    }
                }
            }
            Key::Character(_) | Key::Dead(_) | Key::Unidentified(_) if self.screen.is_none() && !self.numpad && matches!(self.digit_of(key), Some(1..=5)) => {
                let tier = self.digit_of(key).unwrap_or(1) as u32;
                if let Some(p) = self.play.as_mut() {
                    p.turret_key(tier);
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
                    if p.world.strike_lock == 0 {
                        p.strike = Some(f);
                    }
                }
            }
            Key::Character(c) if self.screen.is_none() && c.eq_ignore_ascii_case("t") => {
                if let Some(p) = self.play.as_mut() {
                    p.world.tech_up_all(0);
                }
            }
            Key::Character(c) if self.screen.is_none() && c.eq_ignore_ascii_case("r") => {
                if let Some(p) = self.play.as_mut() {
                    // `0x416769`: R with a selected own factory arms the rally point mode, else it toggles the Advancer's repair mode
                    if !p.arm_rally() {
                        p.world.repair_mode ^= true;
                    }
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
}

/// Where a `iw` x `ih` picture lands in a `ww` x `wh` window: the largest centred rectangle of the same aspect `(x, y, w, h)`.
fn fit_rect(ww: usize, wh: usize, iw: usize, ih: usize) -> (usize, usize, usize, usize) {
    let s = (ww as f64 / iw as f64).min(wh as f64 / ih as f64);
    let (dw, dh) = (((iw as f64 * s).round() as usize).clamp(1, ww), ((ih as f64 * s).round() as usize).clamp(1, wh));
    ((ww - dw) / 2, (wh - dh) / 2, dw, dh)
}

/// Scales `src` (`iw` x `ih`) into `dst` (`ww` x `wh`) inside `rect` (nearest neighbour); the rest is black.
fn blit_scaled(dst: &mut [u32], ww: usize, wh: usize, src: &[u32], iw: usize, ih: usize, rect: (usize, usize, usize, usize)) {
    let (ox, oy, dw, dh) = rect;
    if (dw, dh) == (iw, ih) && (ww, wh) == (iw, ih) {
        dst.copy_from_slice(src);
        return;
    }
    dst.fill(0);
    let xs: Vec<usize> = (0..dw).map(|x| (x * iw / dw).min(iw - 1)).collect();
    for y in 0..dh {
        let sy = (y * ih / dh).min(ih - 1);
        let (row, out) = (&src[sy * iw..(sy + 1) * iw], &mut dst[(oy + y) * ww + ox..(oy + y) * ww + ox + dw]);
        for (o, &sx) in out.iter_mut().zip(&xs) {
            *o = row[sx];
        }
    }
}

#[derive(Default)]
struct Perf {
    since: Option<Instant>,
    frames: u32,
    ms: [f64; 4],
}

impl App {
    /// In a mission the pointer is a custom system cursor made from `cursor.ps6` (the same frame and hotspot the software
    /// pointer uses); elsewhere, or when the system refuses the image, the normal arrow (menus) / the frame's own drawing.
    fn update_cursor(&mut self, el: &ActiveEventLoop, h: usize) {
        let Some(win) = self.window.clone() else { return };
        // `OPENSOW_SOFT_POINTER=1` keeps the old pointer drawn into the frame
        let want = self.game.custom_pointer() && std::env::var_os("OPENSOW_SOFT_POINTER").is_none();
        if let Some(p) = self.game.play.as_mut() {
            p.system_pointer = false;
        }
        if !want {
            if self.cursor_key.take().is_some() {
                win.set_cursor(Cursor::default());
            }
            // the software pointer (menus: none) is drawn into the frame, so the system's one is hidden behind it
            win.set_cursor_visible(!self.game.custom_pointer());
            return;
        }
        let ui = (h as f32 / 768.0).max(1.0);
        // macOS sizes a custom cursor in points, the other systems in pixels
        // the picture is stretched into the window: the cursor is as big as the game's pixels are on screen
        let (ww, wh) = self.window_size();
        let (iw, ih) = self.size();
        let shown = fit_rect(ww, wh, iw, ih).3 as f32 / ih as f32;
        let ui = ui * shown;
        let ui_img = if cfg!(target_os = "macos") { ui / self.game.scale.max(1.0) } else { ui };
        let Some(play) = self.game.play.as_ref() else { return };
        let Some((key, hot, w, h2, rgba)) = play.pointer_cursor(&mut self.game.assets, ui_img) else {
            win.set_cursor_visible(false);
            return;
        };
        if self.cursor_key != Some(key) {
            if !self.cursors.contains_key(&key) {
                if let Ok(src) = CustomCursor::from_rgba(rgba, w as u16, h2 as u16, hot.0 as u16, hot.1 as u16) {
                    self.cursors.insert(key, el.create_custom_cursor(src));
                }
            }
            if let Some(c) = self.cursors.get(&key) {
                win.set_cursor(Cursor::Custom(c.clone()));
                self.cursor_key = Some(key);
            }
        }
        win.set_cursor_visible(self.cursor_key.is_some());
        if let Some(p) = self.game.play.as_mut() {
            p.system_pointer = self.cursor_key.is_some();
        }
    }

    /// The window's pixel size.
    fn window_size(&self) -> (usize, usize) {
        self.window.as_ref().map_or((1280, 800), |w| (w.inner_size().width.max(1) as usize, w.inner_size().height.max(1) as usize))
    }

    /// The resolution the game renders at: the Options setting (any window size shows it scaled), or the window's own
    /// pixel size for NATIVE.
    fn size(&self) -> (usize, usize) {
        if self.game.options.native() {
            let (w, h) = self.window_size();
            return (w.clamp(320, 7680), h.clamp(240, 4320));
        }
        let c = &self.game.options.cfg;
        (c.width.clamp(320, 7680) as usize, c.height.clamp(240, 4320) as usize)
    }

    /// A window point (pixels) -> the game's pixel; points on the bars stick to the nearest edge.
    fn to_game(&self, x: f64, y: f64) -> (i32, i32) {
        let (ww, wh) = self.window_size();
        let (iw, ih) = self.size();
        let (ox, oy, dw, dh) = fit_rect(ww, wh, iw, ih);
        let gx = ((x - ox as f64) * iw as f64 / dw as f64).floor().clamp(0.0, iw as f64 - 1.0);
        let gy = ((y - oy as f64) * ih as f64 / dh as f64).floor().clamp(0.0, ih as f64 - 1.0);
        (gx as i32, gy as i32)
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let c = &self.game.options.cfg;
        self.window_res = (c.width, c.height);
        let mut attrs = Window::default_attributes().with_title("OpenSOW");
        if !self.game.options.native() {
            let (rw, rh) = self.size();
            attrs = attrs.with_inner_size(LogicalSize::new(rw as f64, rh as f64));
        }
        if self.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = Arc::new(el.create_window(attrs).expect("window"));
        if std::env::var_os("OPENSOW_SOFTWARE").is_none() {
            self.gpu = gpu::Gpu::new(window.clone());
        }
        if self.gpu.is_none() {
            let ctx = softbuffer::Context::new(window.clone()).expect("softbuffer context");
            self.surface = Some(softbuffer::Surface::new(&ctx, window.clone()).expect("surface"));
        }
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
        if self.gpu.is_some() {
            // the vsync'd present paces the frames at the display's refresh
            el.set_control_flow(ControlFlow::Poll);
        } else {
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(16)));
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let (w, h) = self.size();
        let res = (self.game.options.cfg.width, self.game.options.cfg.height);
        if self.window_res != res {
            // a new resolution from the options menu: the window takes that size (it can still be resized by hand;
            // fullscreen and NATIVE keep the window as it is)
            self.window_res = res;
            if let Some(win) = self.window.as_ref().filter(|w| w.fullscreen().is_none() && !self.game.options.native()) {
                let _ = win.request_inner_size(LogicalSize::new(w as f64, h as f64));
            }
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now.duration_since(self.last).as_secs_f32().min(0.1);
                self.last = now;
                let t0 = Instant::now();
                self.game.update(dt, w, h);
                let t1 = Instant::now();
                self.update_cursor(el, h);
                let t2 = Instant::now();
                let (ww, wh) = self.window_size();
                let (mut t3, mut t4) = (t2, t2);
                self.frame.resize(w * h, 0);
                if let Some(gpu) = self.gpu.as_mut() {
                    // a mission frame is recorded and drawn by the GPU; anything that cannot be recorded is drawn on the CPU
                    let mut replayed = false;
                    if self.game.gpu_drawable() && self.replay {
                        opensow_viewer::record::begin(w, h);
                        self.game.draw(&mut self.frame, w, h);
                        if let Some(rec) = opensow_viewer::record::end().filter(|r| !r.tainted) {
                            t3 = Instant::now();
                            gpu.present_recording(&rec, ww, wh, fit_rect(ww, wh, w, h));
                            replayed = true;
                        }
                    }
                    if !replayed {
                        self.game.draw(&mut self.frame, w, h);
                        t3 = Instant::now();
                        gpu.present(&self.frame, w, h, ww, wh, fit_rect(ww, wh, w, h));
                    }
                    t4 = Instant::now();
                } else {
                let Some(surface) = self.surface.as_mut() else { return };
                if surface.resize(NonZeroU32::new(ww as u32).unwrap(), NonZeroU32::new(wh as u32).unwrap()).is_err() {
                    return;
                }
                self.game.draw(&mut self.frame, w, h);
                if let Ok(mut buf) = surface.buffer_mut() {
                    blit_scaled(&mut buf, ww, wh, &self.frame, w, h, fit_rect(ww, wh, w, h));
                    t3 = Instant::now();
                    let _ = buf.present();
                    t4 = Instant::now();
                }
                }
                if let Some(p) = self.perf.as_mut() {
                    let ms = |a: Instant, b: Instant| b.duration_since(a).as_secs_f64() * 1000.0;
                    for (k, v) in [ms(t0, t1), ms(t1, t2), ms(t2, t3), ms(t3, t4)].into_iter().enumerate() {
                        p.ms[k] += v;
                    }
                    p.frames += 1;
                    let since = *p.since.get_or_insert(now);
                    if now.duration_since(since).as_secs_f64() >= 1.0 {
                        let f = p.frames.max(1) as f64;
                        eprintln!("PERF {w}x{h}: {} fps; per frame update {:.1} ms, cursor {:.1} ms, draw {:.1} ms, present {:.1} ms", p.frames, p.ms[0] / f, p.ms[1] / f, p.ms[2] / f, p.ms[3] / f);
                        *p = Perf { since: Some(now), ..Perf::default() };
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = self.to_game(position.x, position.y);
                self.game.mouse_moved(x, y, w, h)
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let (x, y) = self.game.cursor;
                self.game.mouse_button(button, state == ElementState::Pressed, x, y, w, h);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                if let Some(m) = self.game.skirmish_menu.as_mut() {
                    m.wheel(dy, w, h);
                } else if self.game.screen.is_none() && self.game.results.is_none() {
                    if let Some(p) = self.game.play.as_mut().filter(|p| !p.paused) {
                        p.wheel(dy, w, h);
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && !event.repeat && event.logical_key == Key::Named(NamedKey::F11) => {
                if let Some(win) = &self.window {
                    win.set_fullscreen(if win.fullscreen().is_some() { None } else { Some(Fullscreen::Borderless(None)) });
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.game.numpad = event.location == winit::keyboard::KeyLocation::Numpad;
                use winit::keyboard::{KeyCode as K, PhysicalKey};
                self.game.digit = match event.physical_key {
                    PhysicalKey::Code(c) => [K::Digit0, K::Digit1, K::Digit2, K::Digit3, K::Digit4, K::Digit5, K::Digit6, K::Digit7, K::Digit8, K::Digit9]
                        .iter()
                        .position(|&d| d == c)
                        .or_else(|| [K::Numpad0, K::Numpad1, K::Numpad2, K::Numpad3, K::Numpad4, K::Numpad5, K::Numpad6, K::Numpad7, K::Numpad8, K::Numpad9].iter().position(|&d| d == c))
                        .map(|d| d as u8),
                    _ => None,
                };
                self.game.key(&event.logical_key, event.state == ElementState::Pressed, w, h);
                self.game.numpad = false;
                self.game.digit = None;
            }
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
    let headless = has("--screenshot");
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
    let chosen = get("--lang").and_then(|l| opensow_formats::lang::Language::from_code(&l).or_else(|| (l.eq_ignore_ascii_case("CHINNESSE")).then_some(opensow_formats::lang::Language::ChineseSimplified)));
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

    if let Some(path) = get("--screenshot") {
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
                    for u in p.world.units.iter_mut().filter(|u| u.player == 0 && u.kind == 40) {
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
                        let ids: Vec<u32> = p.world.player_units().filter(|u| u.kind == 40).map(|u| u.id).collect();
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
                            p.world.bullets.push(sim::Bullet::new(10, 1000, 0, path));
                        }
                    } else if let [cx, cy, "missile", _] = f[..] {
                        // an anti-air missile (bullet type 1) flying 250 px right and 60 px up from the cell's centre
                        let (x, y) = (cx.parse::<i32>().unwrap_or(0) * 32 + 16, cy.parse::<i32>().unwrap_or(0) * 32 + 16);
                        if let Some(path) = opensow_formats::projectile::build_path(1, x, y, x + 250, y - 60, 0) {
                            p.world.bullets.push(sim::Bullet::new(1, 0, 0, path));
                        }
                    } else if let [cx, cy, "piece", age] = f[..] {
                        // a destroyed unit's flying piece (bullet type 15) from the cell's centre; AGE = the top variant 0..14
                        let (x, y) = (cx.parse::<i32>().unwrap_or(0) * 32 + 16, cy.parse::<i32>().unwrap_or(0) * 32 + 16);
                        if let Some(path) = opensow_formats::projectile::build_path(15, x, y, x + 50, y + 30, 0) {
                            let mut b = sim::Bullet::new(15, 40, 0, path);
                            b.variant = age.parse().unwrap_or(0);
                            p.world.bullets.push(b);
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
                        println!("unit {} kind {} player {} at ({:.0},{:.0}) cell {:?} hp {}/{} path {} target {:?}", u.id, u.kind, u.player, u.x, u.y, u.cell(), u.hp, u.max_hp, u.path.len(), u.target);
                    }
                    for s in p.world.structures.iter().filter(|s| s.tally != [0; 3] || s.hp != s.max_hp || s.heal > 0 || s.owner == opensow_formats::level::Owner::Neutral || s.kind_id == 106) {
                        println!("structure kind {} cell {:?} owner {:?} hp {}/{} heal {} tally {:?} active {}", s.kind_id, s.cell, s.owner, s.hp, s.max_hp, s.heal, s.tally, s.active);
                    }
                    if let Some(t) = &p.world.tutorial {
                        println!("tutorial step {} arrow {} entered {}", t.step, t.arrow, t.entered);
                    }
                    for b in &p.world.bullets {
                        println!("bullet type {} strength {} owner {} index {}/{} from {:?} to {:?}", b.bullet_type, b.strength, b.owner, b.index, b.path.points.len(), b.path.points.first(), b.path.points.last());
                    }
                    // the positional sounds the run raised (the first 64; nothing drains them headless)
                    println!("sounds {:?}", p.world.sounds.iter().map(|s| s.0).collect::<Vec<_>>());
                    for a in &p.world.aircraft {
                        println!("aircraft {:?} at ({}, {}) alt {} hp {} frame {}", a.force, a.x, a.y, a.altitude, a.hp, a.frame);
                    }
                    println!("air stock {:?} launched {:?}", p.world.air_stock, [p.world.stats[0][6], p.world.stats[1][6]]);
                    println!("credits {:?} research {:?} enemy turrets {:?}", p.world.credits, p.world.research, p.world.structures.iter().filter(|s| s.alive() && (25..=29).contains(&s.kind_id) && format!("{:?}", s.owner) == "Enemy").map(|s| (s.kind_id, s.cell)).collect::<Vec<_>>());
                    let owned = |o: &str| p.world.structures.iter().filter(|s| s.alive() && (100..=108).contains(&s.kind_id) && format!("{:?}", s.owner) == o).count();
                    println!("buildings player {} enemy {} neutral {}", owned("Player"), owned("Enemy"), owned("Neutral"));
                    println!("turret tiers player {:?} enemy {:?}", p.world.turret_flags[0], p.world.turret_flags[1]);
                    println!("bullets {} tick {} outcome {:?}", p.world.bullets.len(), p.world.tick, p.world.outcome);
                }
            }
            if has("--paused") {
                if let Some(p) = game.play.as_mut() {
                    p.paused = true;
                }
            }
            if let Some(m) = get("--results") {
                // --results won|lost shows the banner, won-stats|lost-stats the statistics --age ticks into the count (sample numbers
                // with --sample-stats)
                let stats = if has("--sample-stats") { sample_stats() } else { game.play.as_ref().map(|p| [p.world.stats[0], p.world.stats[1]]).unwrap_or_default() };
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
            game.screen = Some(match get("--screen").as_deref() {
                Some("levels") => Screen::Levels,
                Some("prototype") => Screen::Options,
                Some("options") => {
                    game.open_options();
                    // --page 0|1; --hover N: 0..13 arrow, 14.. label, 100+k row k of page 2, 200 page arrow
                    if let (Some(m), Some(n)) = (game.options_menu.as_mut(), get("--page").and_then(|n| n.parse::<usize>().ok())) {
                        m.page = n.min(options::PAGES - 1);
                    }
                    if let (Some(m), Some(hit)) = (game.options_menu.as_mut(), get("--hover").and_then(|n| n.parse::<usize>().ok())) {
                        let p = m.centre(match hit {
                            0..=13 => options::Hit::Arrow(hit),
                            14..=99 => options::Hit::Label(hit - 14),
                            100..=199 => options::Hit::Extra(hit - 100),
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
        if game.lands.is_some() || game.options_menu.is_some() || game.saveui.is_some() || game.net_menu.is_some() || game.skirmish_menu.is_some() || game.results.is_some() {
            game.screen = None;
        } else {
            game.hover = get("--hover").and_then(|h| h.parse().ok());
        }
        let mut buf = vec![0u32; w * h];
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
        write_png(&path, &buf, w, h);
        println!("-> {path} ({w}x{h})");
        return;
    }

    game.mute = has("--mute");
    game.sfx.enabled = !game.mute;
    // a windowed `--skirmish MAP` goes straight into the game (no intro, no menus)
    let straight = get("--skirmish");
    if let Some(map) = &straight {
        start_skirmish_from_args(&mut game, map, &get, &has);
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
    if let Ok(m) = std::env::var("OPENSOW_START") {
        game.start(&m);
    }
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App { game, window: None, surface: None, gpu: None, last: Instant::now(), cursors: std::collections::HashMap::new(), cursor_key: None, frame: Vec::new(), window_res: (0, 0), replay: std::env::var_os("OPENSOW_CPU_DRAW").is_none(), fullscreen: !has("--windowed"), perf: std::env::var_os("OPENSOW_PERF").map(|_| Perf::default()) };
    event_loop.run_app(&mut app).expect("run");
}

/// `--skirmish MAP [--side green] [--difficulty easy|normal|hard|brutal] [--credits BLUE,GREEN] [--research BLUE,GREEN]
/// [--spectate [--blue-difficulty D]]`: the lobby's choices from the command line (docs/skirmish.md), headless or windowed.
fn start_skirmish_from_args(game: &mut Game, map: &str, get: &dyn Fn(&str) -> Option<String>, has: &dyn Fn(&str) -> bool) {
    let pair = |k: &str| get(k).and_then(|s| s.split_once(',').and_then(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?))));
    let level = |k: &str| sim::Difficulty::ALL.into_iter().find(|d| get(k).is_some_and(|v| v.eq_ignore_ascii_case(d.name())));
    let difficulty = level("--difficulty").unwrap_or_default();
    let spectate = has("--spectate").then(|| level("--blue-difficulty").unwrap_or(difficulty));
    game.headless_skirmish(map, usize::from(get("--side").is_some_and(|s| s.eq_ignore_ascii_case("green"))), pair("--credits"), pair("--research"), difficulty, spectate);
}

/// `--gpu-check`: the frame drawn on the CPU, recorded + replayed on the CPU, and replayed on the GPU; prints how many pixels differ.
fn gpu_check(game: &mut Game, w: usize, h: usize) {
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
    println!("gpu-check {w}x{h}: {} commands, {} lens, {} map words, tainted {}; cpu draw {:.1} ms, record {:.1} ms", rec.cmds.len(), rec.cmds.iter().filter(|c| c.op == record::op::LENS).count(), rec.maps.len(), rec.tainted, t_cpu.as_secs_f64() * 1e3, t_rec.as_secs_f64() * 1e3);
    let mut replayed = vec![0u32; w * h];
    record::replay(&rec, &mut replayed);
    println!("  cpu replay: {}", diff(&cpu, &replayed));
    match gpu::headless() {
        Some((device, queue, name)) => {
            let mut r = replay::Replay::new(&device);
            let out = r.readback(&device, &queue, &rec);
            let t0 = Instant::now();
            let _ = r.readback(&device, &queue, &rec);
            println!("  gpu replay ({name}, {:.1} ms incl. upload + readback): {}; against the cpu replay: {}", t0.elapsed().as_secs_f64() * 1e3, diff(&cpu, &out), diff(&replayed, &out));
        }
        None => println!("  no GPU adapter"),
    }
    gpu_check_synthetic();
}

/// Every recorded operation (all blend modes, the lens, masks, the `ui` and `gdi` helpers) on made-up sprites at several zooms,
/// drawn on the CPU and replayed on the GPU.
fn gpu_check_synthetic() {
    use opensow_formats::effects::Blit;
    use opensow_formats::sprite::Frame;
    use opensow_viewer::record;
    use opensow_viewer::render::{self, View};
    let Some((device, queue, _)) = gpu::headless() else { return };
    let mut r = replay::Replay::new(&device);
    let frame = |w: u16, h: u16, seed: u32| {
        let n = (w as usize) * (h as usize);
        let px: Vec<u16> = (0..n as u32).map(|i| (i.wrapping_mul(2_654_435_761).wrapping_add(seed.wrapping_mul(40_503)) >> 7) as u16).collect();
        Frame { width: w, height: h, hot_x: (w / 3) as i16, hot_y: (h / 2) as i16, extra: [0; 4], pixels: px, mask: (0..n).map(|i| (i * 7 + seed as usize) % 5 != 0).collect(), enhanced: Default::default() }
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
            for (k, mode) in [Blit::Plain, Blit::Additive, Blit::AddHalf, Blit::Average, Blit::Smoke, Blit::Darken, Blit::Shade, Blit::Lens, Blit::Lens].into_iter().enumerate() {
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
        assert_eq!(b.len(), 7, "the six original rows and SKIRMISH");
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
        // Load game opens the load page; CANCEL goes back to the main menu
        let load = g.buttons(w, h)[1].rect;
        g.mouse_button(MouseButton::Left, true, load.x + 5, load.y + 5, w, h);
        g.mouse_button(MouseButton::Left, false, load.x + 5, load.y + 5, w, h);
        assert!(g.saveui.is_some() && g.screen.is_none());
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert!(g.saveui.is_none());
        assert_eq!(g.screen, Some(Screen::Main));
        // Skirmish opens the lobby against the computer; Escape returns
        let sk = g.buttons(w, h)[4].rect;
        g.mouse_button(MouseButton::Left, true, sk.x + 5, sk.y + 5, w, h);
        g.mouse_button(MouseButton::Left, false, sk.x + 5, sk.y + 5, w, h);
        assert!(g.skirmish_menu.is_some() && g.screen.is_none());
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert!(g.skirmish_menu.is_none());
        assert_eq!(g.screen, Some(Screen::Main));
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        assert!(buf.iter().any(|&p| p != 0));
    }

    /// The bridge to the original's state stream (docs/savegame.md "The sim bridge"): a running mission is written as the exe's
    /// stream, read back, applied to a fresh world of the level and written again; the two streams are identical, so everything
    /// the bridge carries survives, and the restored world still runs.
    #[test]
    fn a_mission_round_trips_through_the_original_state_stream() {
        use opensow_formats::{config::PassedLevels, save::State};
        let Some(mut g) = game() else { return };
        for mission in ["M01", "M05", "M10", "M16", "M22"] {
            mission_trip(&mut g, mission);
        }
    }

    fn mission_trip(g: &mut Game, mission: &str) {
        use opensow_formats::{config::PassedLevels, save::State};
        g.start(mission);
        let (mut airborne, mut shots) = (0, 0);
        let (bytes, units, structures, credits) = {
            let p = g.play.as_mut().unwrap();
            let adv = p.world.player_units().find(|u| u.kind == 40).map(|u| u.id);
            if let Some(id) = adv {
                p.world.order_move(id, (20, 20), None);
            }
            // a bomber and a tripler in flight, and let the armies shoot
            p.world.air_stock[0][0] = 1;
            p.world.air_stock[0][3] = 1;
            let (cx, cy) = (p.world.cells.0 * 16, p.world.cells.1 * 16);
            p.world.launch_strike(0, opensow_formats::air::AirForce::Bomber, cx, cy);
            p.world.launch_strike(0, opensow_formats::air::AirForce::Tripler, cx / 2, cy / 2);
            for _ in 0..900 {
                p.world.step();
            }
            airborne += p.world.aircraft.len();
            shots += p.world.bullets.len() + p.world.beams.len();
            let number: u32 = mission[1..].parse().unwrap();
            let st = p.world.to_state(number, &PassedLevels::initial(), (64, 96));
            let bytes = st.to_bytes().expect("the state can be written");
            let key = |w: &sim::World| {
                let mut v: Vec<_> = w.units.iter().map(|u| (u.kind, u.player, u.x as i32, u.y as i32, u.hp)).collect();
                v.sort();
                v
            };
            let s: Vec<_> = p.world.structures.iter().map(|s| (s.cell, s.kind_id, s.hp.max(0), s.active)).collect();
            (bytes, key(&p.world), s, p.world.credits)
        };
        // the stream is valid and re-writes itself
        if let Some(dir) = std::env::var_os("OPENSOW_DUMP_STATES") {
            // scripts/verify_save_load.py runs the exe's load branch on these
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(std::path::Path::new(&dir).join(format!("{mission}.state")), &bytes);
        }
        let parsed = State::parse(&bytes, 1).expect("the written stream parses");
        assert_eq!(parsed.to_bytes().unwrap(), bytes);
        assert_eq!(parsed.squads.iter().map(|g| g.1.len()).sum::<usize>(), airborne, "{mission}: squads");
        // with the install's sprite sheets every top and chassis carries its size words (the exe's initialiser reads them)
        for (_, o) in parsed.units.iter().flat_map(|g| g.1.iter()) {
            if o.get(0x2c).unwrap_or(0) != 0 {
                continue; // corpses have no parts
            }
            if let Some(t) = o.child(4).filter(|t| t.get(0x54).is_some_and(|i| i != u32::MAX)) {
                assert!(t.get(0x64).unwrap_or(0) > 0 && t.get(0x68).unwrap_or(0) > 0, "{mission}: top sprite size of kind {:?}", o.get(0x40));
            }
            if let Some(c) = o.child(8) {
                assert!(c.get(0x20).unwrap_or(0) > 1 && c.get(0x24).unwrap_or(0) > 1, "{mission}: chassis sprite size of kind {:?}", o.get(0x40));
            }
        }
        assert_eq!(parsed.bullets.iter().map(|g| g.1.len()).sum::<usize>(), shots, "{mission}: bullets");
        assert!(!units.is_empty() && !structures.is_empty(), "{mission}: nothing to save");
        assert!(parsed.buildings.iter().map(|b| b.1.len()).sum::<usize>() > 0, "{mission}: no building in the stream");
        assert_eq!((parsed.map.scroll_x, parsed.map.scroll_y, parsed.head.level), (64, 96, mission[1..].parse().unwrap()));
        // a fresh world of the level takes it
        g.start(mission);
        let p = g.play.as_mut().unwrap();
        p.world.apply_state(&parsed).expect("the state applies");
        let mut got: Vec<_> = p.world.units.iter().map(|u| (u.kind, u.player, u.x as i32, u.y as i32, u.hp)).collect();
        got.sort();
        assert_eq!(got, units, "{mission}: units differ after the trip");
        let s: Vec<_> = p.world.structures.iter().map(|s| (s.cell, s.kind_id, s.hp.max(0), s.active)).collect();
        assert_eq!(s, structures, "{mission}");
        assert_eq!((p.world.aircraft.len(), p.world.bullets.len() + p.world.beams.len()), (airborne, shots), "{mission}: shots in flight");
        assert_eq!(p.world.credits, credits);
        let again = p.world.to_state(mission[1..].parse().unwrap(), &PassedLevels::initial(), (64, 96)).to_bytes().unwrap();
        assert_eq!(again.len(), bytes.len());
        assert!(again == bytes, "{mission}: the second stream differs from the first");
        for _ in 0..300 {
            p.world.step();
        }
        // through the save container as a file of the original (no OpenSOW extension): the level comes from the head, the stream applies
        let dir = std::env::temp_dir().join(format!("opensow-bridge-{}-{}", mission, std::process::id()));
        let store = saveui::Store { write: dir.clone(), also: None, edition: opensow_formats::Edition::Original };
        store.write(2, "t", mission, &saveui::pack_payload(&bytes, &[]), &vec![0u16; 160 * 120]).unwrap();
        let (level, edition, payload) = store.level_of(2).unwrap();
        assert_eq!((level.as_str(), edition), (mission, opensow_formats::Edition::Original));
        g.start(mission);
        let p = g.play.as_mut().unwrap();
        p.restore(&payload).unwrap();
        let mut got: Vec<_> = p.world.units.iter().map(|u| (u.kind, u.player, u.x as i32, u.y as i32, u.hp)).collect();
        got.sort();
        assert_eq!(got, units, "{mission}: units differ after loading the file");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The original's stream has no tutorial script (its step `[0x70f2cc]` is outside the saved ranges), so a stream without our
    /// extension loads level 0 as a plain mission; with the extension the script continues.
    #[test]
    fn the_tutorial_script_is_only_in_our_extension() {
        let Some(mut g) = game() else { return };
        g.start("M00");
        let a = {
            let p = g.play.as_mut().unwrap();
            for _ in 0..30 {
                p.world.step();
            }
            assert!(p.world.tutorial.is_some());
            p.snapshot()
        };
        let (stream, ours) = saveui::unpack_payload(&a).unwrap();
        assert!(!ours.is_empty());
        g.start("M00");
        g.play.as_mut().unwrap().restore(&a).unwrap();
        assert!(g.play.as_ref().unwrap().world.tutorial.is_some(), "our extension keeps the script");
        g.start("M00");
        let bare = saveui::pack_payload(stream, &[]);
        g.play.as_mut().unwrap().restore(&bare).unwrap();
        let w = &g.play.as_ref().unwrap().world;
        assert!((w.tutorial.is_none(), w.tutorial_step, w.ai) == (true, None, true), "the bare stream is a plain mission");
    }

    #[test]
    fn a_saved_game_continues_exactly_where_it_was_saved() {
        let Some(mut g) = game() else { return };
        g.start("M05");
        let (a, b) = {
            let p = g.play.as_mut().unwrap();
            // give the player something to do so the state is not just the start
            let id = p.world.player_units().find(|u| u.kind == 40).map(|u| u.id).expect("an Advancer");
            p.world.order_move(id, (20, 20), None);
            for _ in 0..1500 {
                p.world.step();
            }
            (p.snapshot(), p.world.tick)
        };
        let mut orig = g.play.take().unwrap();
        assert_eq!(b, 1500);
        // through the save container: write a slot, read it back
        let dir = std::env::temp_dir().join(format!("opensow-roundtrip-{}", std::process::id()));
        let store = saveui::Store { write: dir.clone(), also: None, edition: opensow_formats::Edition::Original };
        store.write(3, "t", "M05", &a, &vec![0u16; 160 * 120]).unwrap();
        let (level, _, snap) = store.level_of(3).unwrap();
        assert_eq!((level.as_str(), snap.len()), ("M05", a.len()));
        // a fresh play of the level takes the snapshot and both run on identically
        g.start("M05");
        g.play.as_mut().unwrap().restore(&snap).unwrap();
        let mut again = g.play.take().unwrap();
        assert_eq!(again.world.tick, 1500);
        g.start("M05");
        let mut other = g.play.take().unwrap();
        other.restore(&a).unwrap();
        for _ in 0..600 {
            again.world.step();
            other.world.step();
            orig.world.step();
        }
        // (JSON maps are compared as maps: the HashMaps' key order differs from run to run)
        let json = |w: &mut sim::World| serde_json::from_slice::<serde_json::Value>(&w.snapshot()).unwrap();
        assert!(json(&mut again.world) == json(&mut other.world), "the restored games diverged");
        assert!(json(&mut orig.world) == json(&mut other.world), "the restored game differs from the one that was never saved");
        let fresh_after = {
            g.start("M05");
            let mut f = g.play.take().unwrap();
            for _ in 0..2100 {
                f.world.step();
            }
            f.world.snapshot()
        };
        // (a fresh game without the move order differs; the restored one has the order's effect)
        assert!(serde_json::from_slice::<serde_json::Value>(&fresh_after).unwrap() != json(&mut orig.world));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn f5_quick_saves_to_slot_zero_and_shift_f5_loads_it() {
        let Some(mut g) = game() else { return };
        let dir = std::env::temp_dir().join(format!("opensow-quick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("OPENSOW_SAVE_DIR", &dir);
        let (w, h) = (1024, 768);
        g.start("M05");
        g.update(0.0, w, h);
        for _ in 0..50 {
            g.play.as_mut().unwrap().world.step();
        }
        g.key(&Key::Named(NamedKey::F5), true, w, h);
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h); // the grab happens on the next drawn frame
        assert!(dir.join("savegame_048.info").exists() || std::fs::read_dir(&dir).map_or(false, |d| d.count() >= 2), "slot 0 written");
        for _ in 0..70 {
            g.play.as_mut().unwrap().world.step();
        }
        assert_eq!(g.play.as_ref().unwrap().world.tick, 120);
        g.key(&Key::Named(NamedKey::Shift), true, w, h);
        g.key(&Key::Named(NamedKey::F5), true, w, h);
        assert_eq!(g.play.as_ref().unwrap().world.tick, 50, "back at the quick save");
        std::env::remove_var("OPENSOW_SAVE_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_advancers_beam_sparks_are_drawn_under_its_body() {
        // exe order (0x420xxx): ground units, then bullets and effects, then the hovering Advancer's body (0x46a310) last
        let Some(mut g) = game() else { return };
        let (w, h) = (1024, 768);
        g.start("M01");
        g.update(0.0, w, h);
        let (id, centre) = {
            let p = g.play.as_mut().unwrap();
            let t = p.world.structure_at(13 * 32 + 16, 38 * 32 + 16).expect("a building to work on");
            let id = p.world.player_units().find(|u| u.kind == 40).map(|u| u.id).expect("an Advancer");
            assert!(p.world.order_action(id, t));
            for _ in 0..6000 {
                p.world.step();
                if !p.world.beams.is_empty() {
                    break;
                }
            }
            assert!(!p.world.beams.is_empty(), "the Advancer never fired");
            for _ in 0..20 {
                p.world.step();
            }
            p.view.zoom = 2.0;
            let u = p.world.unit(id).unwrap();
            let (ux, uy) = (u.x, u.sprite_y());
            p.view.x = ux - w as f32 / 2.0 / p.view.zoom;
            p.view.y = uy - h as f32 / 2.0 / p.view.zoom;
            (id, p.view.to_screen(ux, uy))
        };
        let mut with = vec![0u32; w * h];
        g.draw(&mut with, w, h);
        {
            let p = g.play.as_mut().unwrap();
            p.world.beams.clear();
            p.world.bullets.clear();
            p.world.effects.clear();
        }
        let mut without = vec![0u32; w * h];
        g.draw(&mut without, w, h);
        let _ = id;
        assert!(with != without, "no sparks were drawn at all");
        for dy in -4i32..=4 {
            for dx in -4i32..=4 {
                let i = ((centre.1 + dy) as usize) * w + (centre.0 + dx) as usize;
                assert_eq!(with[i], without[i], "a spark covers the Advancer at ({dx}, {dy})");
            }
        }
    }

    /// The scripted events of the real missions (docs/ai.md "Win, lose and mission scripts", docs/air.md "Mission-script launches"):
    /// start carryalls and their squads, the countdown messages and rewards of the survival missions, the timer defeats.
    #[test]
    fn mission_scripts_on_the_real_levels() {
        let Some(mut g) = game() else { return };
        // a started mission with the computer switched off (only the scripts are under test) and, when `clock` is given, a
        // shortened countdown of that many seconds (the real ones are 20..50 minutes of ticks)
        let mut start = |m: &str, clock: Option<i32>| {
            g.start(m);
            let mut w = std::mem::take(&mut g.play).unwrap().world;
            w.ai = false;
            if let (Some(sec), Some((_, _, mode))) = (clock, w.timer) {
                w.timer = Some((sec / 60, sec % 60, mode));
            }
            w
        };
        let steps = |w: &mut sim::World, n: u64| (0..n).for_each(|_| w.step());
        let player = |w: &sim::World, kind: i32| w.units.iter().filter(|u| u.player == 0 && u.hp > 0 && u.kind == kind).count();
        let ids = |w: &sim::World, of: &[u8]| w.log.iter().map(|&(_, id)| id).filter(|id| of.contains(id)).collect::<Vec<_>>();

        // the clocks the levels start with (header words 65/66/67: enabled, minutes, end mode)
        for (m, want) in [("M07", (20, 0, 2)), ("M12", (20, 0, 2)), ("M17", (30, 0, 2)), ("M15", (30, 0, 1)), ("M16", (40, 0, 1)), ("M21", (50, 0, 1)), ("M22", (50, 0, 1)), ("M01", (0, 0, 9))] {
            let w = start(m, None);
            assert_eq!(w.timer.unwrap_or((0, 0, 9)), want, "{m} clock");
        }

        // mission 1: two carryalls land six units each (4 x cannon 7 + 2 x flamer 10), mission 4 one carryall with two walkers
        let mut w = start("M01", None);
        steps(&mut w, 1500);
        assert_eq!((player(&w, 7), player(&w, 10)), (8, 4), "mission 1 airlift");
        // each carryall plays `carryall-land` (62) when it arrives and again when it leaves (`0x467592`, flags `[+0x8c]`, `[+0x8d]`)
        assert_eq!(w.sounds.iter().filter(|s| s.0 == 62).count(), 4, "carryall sounds");
        // mission 1: both tech-levels of the medium factory at their limit pay 8 fighters and 3 carryalls once (`0x42e6f5`), message 13
        let stock = w.air_stock[0];
        assert!(ids(&w, &[13]).is_empty());
        for s in w.structures.iter_mut().filter(|s| s.owner == opensow_formats::level::Owner::Player && s.kind_id == 102) {
            s.slots.iter_mut().take(2).for_each(|sl| sl.level = sl.max_level);
        }
        steps(&mut w, 125);
        assert_eq!((w.air_stock[0][4] - stock[4], w.air_stock[0][2] - stock[2]), (8, 3), "mission 1 reward");
        assert_eq!(ids(&w, &[13]), vec![13]);
        steps(&mut w, 300);
        assert_eq!(w.air_stock[0][4] - stock[4], 8, "paid only once");
        let mut w = start("M04", None);
        steps(&mut w, 1500);
        assert_eq!(player(&w, 41), 2, "mission 4 airlift");

        // missions 7 and 17: 5000 credits (17: and 5000 research) and two carryalls with squads 11 / 10, one second after the
        // clock shows 0:00 (`0x42ebd0` looks at the clock, then counts a second off); messages 13, 11 (and 12)
        for (m, squad, research, announced) in [("M07", 11, 0, vec![13, 11]), ("M17", 10, 5000, vec![13, 11, 12])] {
            let mut w = start(m, Some(150));
            steps(&mut w, 150 * 60 - 1);
            assert!(w.aircraft.is_empty() && ids(&w, &[13]).is_empty(), "{m} too early");
            let (credits, rp) = (w.credits[0], w.research[0]);
            steps(&mut w, 62);
            assert_eq!(w.aircraft.iter().map(|a| a.squad).collect::<Vec<_>>(), vec![squad, squad], "{m} carryalls");
            assert!(w.credits[0] - credits >= 5000, "{m} credits");
            assert!(w.research[0] - rp >= research, "{m} research");
            assert_eq!(ids(&w, &[11, 12, 13]), announced, "{m} messages");
            // the 2-minutes message sits at the clock value 2:01
            if let Some(&(t, _)) = w.log.iter().find(|&&(_, id)| id == 17) {
                assert!((t as i64 - 29 * 60).abs() <= 61, "{m} 2 minutes message at tick {t}");
            }
            assert_eq!(w.outcome, None, "{m}: mode 2 ends nothing");
        }
        // mission 12: 10000 credits, 2500 research, 6 asteroids, 3 bombers, 3 carryalls (squad 16), message 13 only
        let mut w = start("M12", Some(150));
        steps(&mut w, 150 * 60 - 1);
        let (credits, rp, stock) = (w.credits[0], w.research[0], w.air_stock[0]);
        steps(&mut w, 62);
        assert_eq!(w.aircraft.iter().map(|a| a.squad).collect::<Vec<_>>(), vec![16, 16, 16]);
        assert!(w.credits[0] - credits >= 10_000 && w.research[0] - rp >= 2500);
        assert_eq!((w.air_stock[0][0] - stock[0], w.air_stock[0][1] - stock[1]), (3, 6));
        assert_eq!(ids(&w, &[11, 12, 13]), vec![13]);

        // the timed defeats (mode 1): M15, M16, M21, M22 are lost when the clock has run out, and not before
        for m in ["M15", "M16", "M21", "M22"] {
            let mut w = start(m, Some(60));
            steps(&mut w, 60 * 60 - 1);
            assert_eq!(w.outcome, None, "{m} must still run");
            steps(&mut w, 125);
            assert_eq!(w.outcome, Some(sim::Outcome::Defeat), "{m} timer defeat");
        }
    }

    /// The dev tools overlay: F12 shows it, its buttons set the cheats and the speed, a unit button plus a map click places a unit,
    /// UNLOCK ALL opens every land of the campaign map without touching the real progress.
    #[test]
    fn dev_tools_panel_flow() {
        use devtools::Item;
        let Some(mut g) = game() else { return };
        let (w, h) = (1024, 768);
        g.start("M01");
        g.update(0.0, w, h);
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        assert!(!g.dev.visible);
        g.key(&Key::Named(NamedKey::F12), true, w, h);
        assert!(g.dev.visible);
        let fonts = g.assets.font("small");
        let click = |g: &mut Game, item: Item| {
            let r = g.dev.rect_of(&devtools::Fonts::clone(&fonts), item, w, h).expect("item");
            let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
            g.mouse_moved(x, y, w, h);
            g.mouse_button(MouseButton::Left, true, x, y, w, h);
            g.mouse_button(MouseButton::Left, false, x, y, w, h);
        };
        click(&mut g, Item::Rich);
        click(&mut g, Item::Invulnerable);
        click(&mut g, Item::Freeze);
        click(&mut g, Item::Speed(10));
        g.update(1.0 / 60.0, w, h);
        let p = g.play.as_ref().unwrap();
        assert_eq!(p.world.cheats, sim::Cheats { invulnerable: true, rich: true, freeze_enemy: true });
        assert_eq!(p.world.credits[0], opensow_formats::production::CREDITS_CAP);
        assert!(p.world.tick >= 8, "10x runs several ticks in one frame (ran {})", p.world.tick);
        // an air force button adds one to the stock of the chosen side
        let stock = g.play.as_ref().unwrap().world.air_stock;
        click(&mut g, Item::Air(4));
        assert_eq!(g.play.as_ref().unwrap().world.air_stock[0][4], stock[0][4] + 1);
        // a unit button arms the placement, a click on open ground puts the unit there (and not on a panel click)
        let before = g.play.as_ref().unwrap().world.units.len();
        click(&mut g, Item::Side(1));
        click(&mut g, Item::Kind(7));
        assert_eq!(g.play.as_ref().unwrap().dev_spawn, Some((7, 1)));
        assert_eq!(g.play.as_ref().unwrap().world.units.len(), before, "pressing the button places nothing");
        let panel_right = g.dev.rect_of(&fonts, Item::Title, w, h).unwrap().x + 400;
        let (x, y) = (panel_right, h as i32 / 2);
        g.mouse_moved(x, y, w, h);
        g.mouse_button(MouseButton::Left, true, x, y, w, h);
        g.mouse_button(MouseButton::Left, false, x, y, w, h);
        let p = g.play.as_ref().unwrap();
        assert!(p.world.units.len() > before || p.world.is_blocked(p.to_map(x, y).0 / 32, p.to_map(x, y).1 / 32), "a unit was placed");
        // Esc stops the placement before it opens the pause menu
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert_eq!(g.play.as_ref().unwrap().dev_spawn, None);
        assert!(g.screen.is_none(), "Esc did not pause");
        // the campaign map: locked lands open with the switch and close again without it
        g.dev.visible = false;
        g.play = None;
        g.open_lands(false);
        assert_eq!(g.lands.as_ref().unwrap().progress.states[3], 0);
        g.dev.unlock_all = true;
        g.dev_event(Item::UnlockAll);
        assert_eq!(g.lands.as_ref().unwrap().progress.states[3], 2);
        assert_eq!(g.progress.states[3], 0, "the saved progress is untouched");
        g.dev.unlock_all = false;
        g.dev_event(Item::UnlockAll);
        assert_eq!(g.lands.as_ref().unwrap().progress.states[3], 0);
    }

    #[test]
    fn playing_a_mission() {
        let Some(mut g) = game() else { return };
        let (w, h) = (1024, 768);
        g.start("M01");
        assert!(g.play.is_some() && g.screen.is_none());
        // the mission starts with the Advancer only (the squad arrives by carryall): give the test four units beside it
        {
            let world = &mut g.play.as_mut().unwrap().world;
            let (x, y) = world.player_units().find(|u| u.kind == 40).map(|u| u.px()).unwrap();
            for _ in 0..4 {
                world.add_unit_near(7, 0, x, y + 48);
            }
        }
        g.update(0.0, w, h);
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        // box-select everything on screen, then order a move
        g.mouse_button(MouseButton::Left, true, 80, 40, w, h);
        g.mouse_button(MouseButton::Left, false, w as i32 - 2, h as i32 - 30, w, h);
        let selected = g.play.as_ref().unwrap().world.units.iter().filter(|u| u.selected).count();
        assert!(selected >= 4, "box selection picked {selected}");
        g.mouse_moved(w as i32 / 2, h as i32 / 2, w, h);
        // the left button commands: with a selection the pointer over open ground is the move pointer (water is "blocked")
        let (mut gx, mut gy) = (0, 0);
        'scan: for y in (100..h as i32 - 100).step_by(20) {
            for x in (100..w as i32 - 100).step_by(20) {
                g.mouse_moved(x, y, w, h);
                if g.play.as_ref().unwrap().pointer() == opensow_formats::cursor::Pointer::Move {
                    (gx, gy) = (x, y);
                    break 'scan;
                }
            }
        }
        assert!(gx > 0, "no open ground on screen");
        g.mouse_button(MouseButton::Left, true, gx, gy, w, h);
        g.mouse_button(MouseButton::Left, false, gx, gy, w, h);
        assert!(g.play.as_ref().unwrap().world.units.iter().any(|u| u.selected && !u.path.is_empty()));
        // a right click deselects, it gives no order
        g.mouse_button(MouseButton::Right, true, w as i32 / 2, h as i32 / 2 - 100, w, h);
        g.mouse_button(MouseButton::Right, false, w as i32 / 2, h as i32 / 2 - 100, w, h);
        assert!(!g.play.as_ref().unwrap().world.units.iter().any(|u| u.selected));
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
    fn multiplayer_button_opens_the_network_screens() {
        let Some(mut g) = game() else { return };
        let (w, h) = (1280, 800);
        let b = g.buttons(w, h)[3].rect;
        g.mouse_button(MouseButton::Left, true, b.x + 5, b.y + 5, w, h);
        g.mouse_button(MouseButton::Left, false, b.x + 5, b.y + 5, w, h);
        assert!(g.net_menu.is_some() && g.screen.is_none());
        assert_eq!(g.net_menu.as_ref().unwrap().page, multiplayer::Page::Sessions);
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert!(g.net_menu.is_none());
        assert_eq!(g.screen, Some(Screen::Main));
    }

    #[test]
    fn options_menu_flow() {
        let Some(mut g) = game() else { return };
        let (w, h) = (1280, 800);
        g.screen = Some(Screen::Pause);
        g.act(Action::Options);
        assert!(g.options_menu.is_some() && g.screen.is_none());
        let click = |g: &mut Game, hit: options::Hit| {
            let c = g.options_menu.as_ref().unwrap().centre(hit);
            let (s, ox, oy) = (h as f32 / 480.0, (w as f32 - 640.0 * h as f32 / 480.0) / 2.0, 0.0);
            let (x, y) = ((c.0 as f32 * s + ox) as i32, (c.1 as f32 * s + oy) as i32);
            g.mouse_moved(x, y, w, h);
            g.mouse_button(MouseButton::Left, true, x, y, w, h);
        };
        g.sfx.played.clear();
        click(&mut g, options::Hit::Arrow(3));
        click(&mut g, options::Hit::Arrow(3));
        click(&mut g, options::Hit::Arrow(2));
        assert_eq!(g.options_menu.as_ref().unwrap().opts.fx_volume, 95);
        assert_eq!(g.sfx.fx_volume, 95, "the volume reaches the player at once");
        click(&mut g, options::Hit::Label(3));
        assert!(!g.sfx.fx_on);
        assert!(g.sfx.played.contains(&33) && g.sfx.played.contains(&34) && g.sfx.played.contains(&30));
        click(&mut g, options::Hit::Ok);
        assert!(g.options_menu.is_none());
        assert_eq!(g.screen, Some(Screen::Pause), "back to the pause menu");
        assert_eq!(g.options.fx_volume, 95);
        assert!(g.sfx.played.contains(&38));
        // Escape cancels
        g.act(Action::Options);
        click(&mut g, options::Hit::Label(0));
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert!(g.options_menu.is_none() && g.options.cfg.flag_a);
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
    }

    #[test]
    fn options_page_two_toggles_the_enhancement() {
        let Some(mut g) = game() else { return };
        let (w, h) = (1280, 800);
        g.screen = Some(Screen::Pause);
        g.act(Action::Options);
        let click = |g: &mut Game, hit: options::Hit| {
            let c = g.options_menu.as_ref().unwrap().centre(hit);
            let (s, ox) = (h as f32 / 480.0, (w as f32 - 640.0 * h as f32 / 480.0) / 2.0);
            let (x, y) = ((c.0 as f32 * s + ox) as i32, (c.1 as f32 * s) as i32);
            g.mouse_moved(x, y, w, h);
            g.mouse_button(MouseButton::Left, true, x, y, w, h);
        };
        click(&mut g, options::Hit::Page(1));
        assert_eq!(g.options_menu.as_ref().unwrap().page, 1);
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        click(&mut g, options::Hit::Extra(0));
        assert!(!g.options_menu.as_ref().unwrap().opts.extra.enhance);
        click(&mut g, options::Hit::Ok);
        assert!(!g.options.extra.enhance);
        assert!(!opensow_viewer::upscale::enabled(), "OK applies the switch");
        // back on, so the other tests are not affected
        g.act(Action::Options);
        click(&mut g, options::Hit::Page(1));
        click(&mut g, options::Hit::Extra(0));
        click(&mut g, options::Hit::Ok);
        assert!(g.options.extra.enhance && opensow_viewer::upscale::enabled());
        // the sound row (the last one) switches the sound enhancement the same way
        g.act(Action::Options);
        click(&mut g, options::Hit::Page(1));
        click(&mut g, options::Hit::Extra(5));
        click(&mut g, options::Hit::Ok);
        assert!(!g.options.extra.sound && !hifi::enabled(), "OK applies the sound switch");
        g.act(Action::Options);
        click(&mut g, options::Hit::Page(1));
        click(&mut g, options::Hit::Extra(5));
        click(&mut g, options::Hit::Ok);
        assert!(g.options.extra.sound && hifi::enabled());
    }

    #[test]
    fn pause_menu_flow() {
        let Some(mut g) = game() else { return };
        let (w, h) = (1280, 800);
        g.start("M01");
        g.sfx.played.clear();
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert_eq!(g.screen, Some(Screen::Pause));
        assert_eq!(g.sfx.played, vec![35], "opening plays ingame-come");
        let click = |g: &mut Game, hit: ingame::Hit| {
            let (x, y) = g.pause_menu().centre(hit, w, h);
            g.mouse_moved(x, y, w, h);
            g.mouse_button(MouseButton::Left, true, x, y, w, h);
        };
        g.sfx.played.clear();
        click(&mut g, ingame::Hit::Item(3));
        assert!(g.options_menu.is_some());
        assert_eq!(g.sfx.played, vec![36, 37], "hover then select");
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert!(g.options_menu.is_none());
        assert_eq!(g.screen, Some(Screen::Pause), "options return to the in-game menu");
        // restart asks first; the X goes back
        click(&mut g, ingame::Hit::Item(4));
        assert_eq!(g.pause_menu().page, ingame::Page::ConfirmRestart);
        click(&mut g, ingame::Hit::No);
        assert_eq!(g.pause_menu().page, ingame::Page::Items);
        // load and save open their pages; CANCEL resumes the mission (state 7), not the oval
        for item in [1, 2] {
            click(&mut g, ingame::Hit::Item(item));
            assert!(g.saveui.is_some() && g.screen.is_none(), "item {item}");
            let c = g.saveui.as_ref().unwrap().centre(saveui::Hit::Cancel);
            g.mouse_button(MouseButton::Left, true, c.0, c.1, 640, 480);
            assert!(g.saveui.is_none() && g.screen.is_none(), "item {item} resumes");
            g.open_pause();
        }
        // main menu with the check mark
        click(&mut g, ingame::Hit::Item(5));
        g.sfx.played.clear();
        click(&mut g, ingame::Hit::Yes);
        assert_eq!(g.screen, Some(Screen::Main));
        assert!(g.play.is_none() && g.ingame.is_none());
        assert!(g.sfx.played.contains(&38));
    }

    #[test]
    fn enter_opens_the_chat_line_and_cheat_phrases_take_effect() {
        let Some(mut g) = game() else { return };
        let (w, h) = (1024, 768);
        g.start("M01");
        g.update(0.0, w, h);
        let chr = |c: &str| Key::Character(SmolStr::new(c));
        let say = |g: &mut Game, text: &str| {
            g.key(&Key::Named(NamedKey::Enter), true, w, h);
            assert!(g.play.as_ref().unwrap().chat.is_some(), "Enter opens the line");
            for c in text.chars() {
                if c == ' ' {
                    g.key(&Key::Named(NamedKey::Space), true, w, h);
                } else {
                    g.key(&chr(&c.to_string()), true, w, h);
                }
            }
            g.key(&Key::Named(NamedKey::Enter), true, w, h);
            assert!(g.play.as_ref().unwrap().chat.is_none(), "Enter sends and closes");
        };
        let before = {
            let wd = &g.play.as_ref().unwrap().world;
            (wd.credits[0], wd.research[0], wd.air_stock[0])
        };
        // keys typed into the line do not reach the game: "2" would arm a turret, "p" would pause
        g.play.as_mut().unwrap().world.turret_flags[0] = [true; 5];
        g.play.as_mut().unwrap().world.credits[0] = 5000;
        g.key(&Key::Named(NamedKey::Enter), true, w, h);
        g.key(&chr("2"), true, w, h);
        g.key(&chr("p"), true, w, h);
        assert_eq!(g.play.as_ref().unwrap().chat.as_deref(), Some("2p"));
        assert!(g.play.as_ref().unwrap().placing.is_none() && !g.play.as_ref().unwrap().paused);
        g.key(&Key::Named(NamedKey::Backspace), true, w, h);
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert!(g.play.as_ref().unwrap().chat.is_none() && g.screen.is_none(), "Esc closes the line, not the game");
        g.play.as_mut().unwrap().world.credits[0] = before.0;
        say(&mut g, "give me money");
        say(&mut g, "speed up my research");
        say(&mut g, "give me fighter");
        say(&mut g, "give me bomber");
        say(&mut g, "give me meteor storm");
        say(&mut g, "give me carryall");
        say(&mut g, "give me tripler");
        let wd = &g.play.as_ref().unwrap().world;
        assert_eq!((wd.credits[0] - before.0, wd.research[0] - before.1), (1000, 200));
        assert!((0..5).all(|i| wd.air_stock[0][i] == before.2[i] + 1), "one of every force");
        assert!(wd.chat.is_empty(), "a cheat is not echoed");
        // a wrong case is only a chat line; Tab brings the last line back
        say(&mut g, "Give me money");
        let wd = &g.play.as_ref().unwrap().world;
        assert_eq!(wd.credits[0] - before.0, 1000);
        assert_eq!(wd.chat.last().map(|c| c.1.as_str()), Some("Give me money"));
        g.key(&Key::Named(NamedKey::Enter), true, w, h);
        g.key(&Key::Named(NamedKey::Tab), true, w, h);
        assert_eq!(g.play.as_ref().unwrap().chat.as_deref(), Some("Give me money"));
        g.key(&Key::Named(NamedKey::Enter), true, w, h);
        // the mission 1 clock does not run in a mission without a countdown; "one minute" adds to a running one only
        g.play.as_mut().unwrap().world.timer = Some((3, 10, 0));
        say(&mut g, "give me one minute");
        assert_eq!(g.play.as_ref().unwrap().world.timer, Some((4, 10, 0)));
        g.play.as_mut().unwrap().world.timer_expired = true;
        say(&mut g, "give me one minute");
        assert_eq!(g.play.as_ref().unwrap().world.timer, Some((4, 10, 0)), "not after the countdown ended");
        say(&mut g, "draw fps");
        assert!(g.play.as_ref().unwrap().show_fps);
    }

    #[test]
    fn mission_hotkeys_follow_the_exes_key_handler() {
        let Some(mut g) = game() else { return };
        let (w, h) = (1024, 768);
        g.start("M01");
        g.update(0.0, w, h);
        let chr = |c: &str| Key::Character(SmolStr::new(c));
        // 1-5 (`0x415d7c`): drop the selection and arm the turret, only with the turret available and 200 * tier credits
        {
            let p = g.play.as_mut().unwrap();
            p.world.turret_flags[0] = [true; 5];
            p.world.credits[0] = 399;
            p.world.select_all();
        }
        g.key(&chr("2"), true, w, h);
        assert_eq!(g.play.as_ref().unwrap().placing, None, "tier 2 costs 400");
        g.play.as_mut().unwrap().world.credits[0] = 400;
        g.key(&chr("2"), true, w, h);
        let p = g.play.as_ref().unwrap();
        assert_eq!(p.placing, Some(2));
        assert!(!p.world.units.iter().any(|u| u.selected), "the key deselects everything");
        // the physical key counts, not the character: Czech layouts make the number row "+ ě š č ř" without shift
        g.play.as_mut().unwrap().placing = None;
        g.digit = Some(3);
        g.key(&chr("š"), true, w, h);
        g.digit = None;
        assert_eq!(g.play.as_ref().unwrap().placing, None, "tier 3 costs 600");
        g.play.as_mut().unwrap().world.credits[0] = 600;
        g.digit = Some(3);
        g.key(&chr("š"), true, w, h);
        g.digit = None;
        assert_eq!(g.play.as_ref().unwrap().placing, Some(3));
        // building one plays sound 72 (turret-place), the player's own: a priority sound, not a positional one
        {
            let wd = &mut g.play.as_mut().unwrap().world;
            let own = wd.structures.iter().find(|s| s.owner == opensow_formats::level::Owner::Player).map(|s| s.cell).unwrap();
            wd.sounds.clear();
            let placed = (-8..8).flat_map(|dy| (-8..8).map(move |dx| (own.0 + dx, own.1 + dy))).any(|c| wd.place_turret(0, 1, c));
            assert!(placed && wd.player_sounds.contains(&72) && !wd.sounds.iter().any(|s| s.0 == 72), "turret-place sound");
        }
        // placement tiles: one verdict per footprint cell (tier 5 is a 1x2 turret), all refused far from the own buildings
        assert_eq!(g.play.as_ref().unwrap().world.turret_cells(0, 5, (3, 3)).len(), 2);
        assert!(g.play.as_ref().unwrap().world.turret_cells(0, 3, (0, 0)).iter().all(|&ok| !ok));
        g.play.as_mut().unwrap().world.credits[0] = 400;
        g.play.as_mut().unwrap().placing = Some(2);
        // P pauses (scan code 0x19) and only P resumes; Space does neither
        g.key(&chr("p"), true, w, h);
        assert!(g.play.as_ref().unwrap().paused);
        g.key(&Key::Named(NamedKey::Space), true, w, h);
        assert!(g.play.as_ref().unwrap().paused);
        g.key(&chr("p"), true, w, h);
        assert!(!g.play.as_ref().unwrap().paused);
        // Tab selects one live own unit at a time and walks the list; H the Advancers
        g.key(&Key::Named(NamedKey::Tab), true, w, h);
        let sel = |g: &Game| g.play.as_ref().unwrap().world.units.iter().enumerate().filter(|(_, u)| u.selected).map(|(i, _)| i).collect::<Vec<_>>();
        let first = sel(&g);
        assert_eq!(first.len(), 1);
        let own = g.play.as_ref().unwrap().world.units.iter().filter(|u| u.player == 0 && u.hp > 0).count();
        g.key(&Key::Named(NamedKey::Tab), true, w, h);
        assert_eq!(sel(&g) != first, own > 1, "{own} own units");
        // Space: nothing without a shown message; with one, the view goes to the last event position
        g.play.as_mut().unwrap().look_at(100.0, 100.0);
        let before = g.play.as_ref().unwrap().view;
        g.key(&Key::Named(NamedKey::Space), true, w, h);
        assert_eq!((g.play.as_ref().unwrap().view.x, g.play.as_ref().unwrap().view.y), (before.x, before.y));
        {
            let wd = &mut g.play.as_mut().unwrap().world;
            wd.last_event = Some((1200, 1000));
            wd.log.push((wd.tick, 3));
        }
        g.key(&Key::Named(NamedKey::Space), true, w, h);
        let v = g.play.as_ref().unwrap().view;
        assert!((v.x + w as f32 / v.zoom / 2.0 - 1200.0).abs() < 2.0 && (v.y + h as f32 / v.zoom / 2.0 - 1000.0).abs() < 2.0, "{:?}", (v.x, v.y));
        // numpad bookmarks: shift sets, plain jumps
        g.numpad = true;
        g.shift = true;
        g.play.as_mut().unwrap().look_at(500.0, 400.0);
        g.key(&chr("3"), true, w, h);
        g.shift = false;
        g.play.as_mut().unwrap().look_at(900.0, 900.0);
        g.key(&chr("3"), true, w, h);
        let v = g.play.as_ref().unwrap().view;
        assert!((v.x + w as f32 / v.zoom / 2.0 - 500.0).abs() < 2.0 || v.x == 0.0 || v.y == 0.0, "the bookmark brings the view back");
        assert_eq!(g.play.as_ref().unwrap().placing, Some(2), "numpad digits are not turret keys");
        // numpad * selects what is on screen, F3 / F4 move the game speed in halves between 0.5 and 3
        g.key(&chr("*"), true, w, h);
        g.numpad = false;
        for _ in 0..6 {
            g.key(&Key::Named(NamedKey::F4), true, w, h);
        }
        assert_eq!(g.game_speed, 3.0);
        for _ in 0..9 {
            g.key(&Key::Named(NamedKey::F3), true, w, h);
        }
        assert_eq!(g.game_speed, 0.5);
    }

    #[test]
    fn hud_clicks_reach_the_panel_not_the_map() {
        let Some(mut g) = game() else { return };
        let (w, h) = (640, 480);
        g.start("M12");
        g.update(0.0, w, h);
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        let l = g.play.as_ref().unwrap().hud_layout().unwrap().clone();
        // the radar moves the camera
        let before = (g.play.as_ref().unwrap().view.x, g.play.as_ref().unwrap().view.y);
        g.mouse_button(MouseButton::Left, true, l.radar.x + 2, l.radar.y + 2, w, h);
        g.mouse_button(MouseButton::Left, false, l.radar.x + 2, l.radar.y + 2, w, h);
        let after = (g.play.as_ref().unwrap().view.x, g.play.as_ref().unwrap().view.y);
        assert_ne!(before, after, "radar click did not scroll");
        // a right click on the radar toggles the black radar
        assert!(!g.play.as_ref().unwrap().radar_black());
        g.mouse_button(MouseButton::Right, true, l.radar.x + 2, l.radar.y + 2, w, h);
        g.mouse_button(MouseButton::Right, false, l.radar.x + 2, l.radar.y + 2, w, h);
        assert!(g.play.as_ref().unwrap().radar_black());
        // a turret icon arms the placement only with enough credits; a right click cancels
        g.play.as_mut().unwrap().world.credits[0] = 1000;
        let t = l.tabs[1].1; // tier 2 (tabs run from the right)
        g.mouse_button(MouseButton::Left, true, t.x + 3, t.y + 3, w, h);
        g.mouse_button(MouseButton::Left, false, t.x + 3, t.y + 3, w, h);
        assert_eq!(g.play.as_ref().unwrap().placing, Some(2));
        g.mouse_button(MouseButton::Right, true, t.x + 3, t.y + 3, w, h);
        g.mouse_button(MouseButton::Right, false, t.x + 3, t.y + 3, w, h);
        assert_eq!(g.play.as_ref().unwrap().placing, None);
        g.play.as_mut().unwrap().world.credits[0] = 0;
        g.mouse_button(MouseButton::Left, true, t.x + 3, t.y + 3, w, h);
        assert_eq!(g.play.as_ref().unwrap().placing, None);
    }

    #[test]
    fn the_rally_button_arms_the_mode_and_a_map_click_sets_the_point() {
        let Some(mut g) = game() else { return };
        for (w, h) in [(640usize, 480usize), (1024, 768), (1920, 1080)] {
            g.start("M01");
            g.update(0.0, w, h);
            let own = g.play.as_ref().unwrap().world.structures.iter().position(|s| s.owner == opensow_formats::level::Owner::Player && s.kind_id == 102).unwrap();
            let p = g.play.as_mut().unwrap();
            let c = p.world.structures[own].cell;
            p.look_at(c.0 as f32 * 32.0 + 16.0, c.1 as f32 * 32.0 + 16.0);
            // no button before the factory is selected
            let mut buf = vec![0u32; w * h];
            g.draw(&mut buf, w, h);
            assert!(g.play.as_ref().unwrap().hud_layout().unwrap().rally.is_none());
            g.play.as_mut().unwrap().sel_building = Some(own);
            g.update(0.0, w, h);
            g.draw(&mut buf, w, h);
            let l = g.play.as_ref().unwrap().hud_layout().unwrap().clone();
            let (r, ..) = l.rally.expect("the selected own factory shows the button");
            // it sits above the top slot icon
            assert!(r.y + r.h <= l.slots[0].0.y, "button above the icon at {w}x{h}");
            let (bx, by) = (r.x + r.w / 2, r.y + r.h / 2);
            assert_eq!(l.hit(bx, by), Some(crate::hud::Hit::Rally));
            g.mouse_button(MouseButton::Left, true, bx, by, w, h);
            g.mouse_button(MouseButton::Left, false, bx, by, w, h);
            assert_eq!(g.play.as_ref().unwrap().rallying, Some(own));
            // a click on free ground sets the point (the cell centre) and ends the mode
            let p = g.play.as_mut().unwrap();
            let target = (c.0 + 4, c.1 + 6);
            p.look_at(target.0 as f32 * 32.0 + 16.0, target.1 as f32 * 32.0 + 16.0);
            let (sx, sy) = (w as i32 / 2 - 100, h as i32 / 2);
            let (mx, my) = p.to_map(sx, sy);
            let free = !p.world.is_blocked(mx.div_euclid(32), my.div_euclid(32));
            g.mouse_moved(sx, sy, w, h);
            g.mouse_button(MouseButton::Left, true, sx, sy, w, h);
            g.mouse_button(MouseButton::Left, false, sx, sy, w, h);
            let p = g.play.as_ref().unwrap();
            if free {
                assert_eq!(p.rallying, None);
                assert_eq!(p.world.structures[own].rally, Some(opensow_formats::rally::snap(mx, my)));
            } else {
                assert_eq!(p.rallying, Some(own), "a blocked cell keeps the mode on");
            }
            g.play.as_mut().unwrap().world.structures[own].rally = None;
        }
    }

    #[test]
    fn any_building_shows_its_slots_when_hovered_and_only_own_ones_open_the_info_box() {
        let Some(mut g) = game() else { return };
        let (w, h) = (1024, 768);
        g.start("M01");
        g.update(0.0, w, h);
        let p = g.play.as_mut().unwrap();
        // an enemy production building and the neutral research lab (slots 19, 20)
        let enemy = p.world.structures.iter().position(|s| s.owner == opensow_formats::level::Owner::Enemy && !s.slots.is_empty()).unwrap();
        let lab = p.world.structures.iter().position(|s| s.owner == opensow_formats::level::Owner::Neutral && !s.slots.is_empty()).unwrap();
        let own = p.world.structures.iter().position(|s| s.owner == opensow_formats::level::Owner::Player && s.kind_id == 102).unwrap();
        for target in [own, enemy, lab] {
            let p = g.play.as_mut().unwrap();
            let c = p.world.structures[target].cell;
            p.look_at(c.0 as f32 * 32.0 + 16.0, c.1 as f32 * 32.0 + 16.0);
            let mut at = None;
            'scan: for y in (80..h as i32 - 80).step_by(6) {
                for x in (120..w as i32 - 120).step_by(6) {
                    if p.world.structure_at(p.to_map(x, y).0, p.to_map(x, y).1) == Some(target) {
                        at = Some((x, y));
                        break 'scan;
                    }
                }
            }
            let (x, y) = at.expect("building on screen");
            // hovering shows the production slots of any building, but no info box
            g.mouse_moved(x, y, w, h);
            g.update(0.0, w, h);
            let mut buf = vec![0u32; w * h];
            g.draw(&mut buf, w, h);
            let n = g.play.as_ref().unwrap().hud_layout().unwrap().slots.len();
            assert!(n > 0 && (target != own || n == 2), "M01's blue medium factory has two slots, got {n}");
            assert!(g.play.as_ref().unwrap().infobox.kind.is_none(), "hover alone opens no info box");
            // a click selects only an own building (info box); an enemy's or the neutral one stays hover-only
            g.mouse_button(MouseButton::Left, true, x, y, w, h);
            g.mouse_button(MouseButton::Left, false, x, y, w, h);
            assert_eq!(g.play.as_ref().unwrap().sel_building, (target == own).then_some(target));
            g.update(0.0, w, h);
            g.draw(&mut buf, w, h);
            assert!(g.play.as_ref().unwrap().hud_layout().unwrap().slots.len() > 0);
            if target == own {
                assert!(g.play.as_ref().unwrap().infobox.visible());
            } else {
                assert!(g.play.as_ref().unwrap().infobox.kind.is_none(), "only the selected own building gets the page");
            }
            g.mouse_button(MouseButton::Right, true, 5, 5, w, h);
            g.mouse_button(MouseButton::Right, false, 5, 5, w, h);
            g.play.as_mut().unwrap().sel_building = None;
        }
    }

    #[test]
    fn info_box_slot_click_buys_a_tech_level() {
        let Some(mut g) = game() else { return };
        let (w, h) = (1280, 800);
        g.start("M12");
        g.update(0.0, w, h);
        let p = g.play.as_mut().unwrap();
        let Some(b) = p.world.structures.iter().position(|s| s.owner == opensow_formats::level::Owner::Player && s.slots.first().is_some_and(|sl| sl.level < sl.max_level && s.active > 0)) else { return };
        p.sel_building = Some(b);
        p.world.research[0] = 5000;
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        let l = g.play.as_ref().unwrap().hud_layout().unwrap().clone();
        let r = l.arrows[0].unwrap().0;
        let before = g.play.as_ref().unwrap().world.structures[b].slots[0].kind;
        g.mouse_button(MouseButton::Left, true, r.x + 3, r.y + 3, w, h);
        g.mouse_button(MouseButton::Left, false, r.x + 3, r.y + 3, w, h);
        assert_ne!(g.play.as_ref().unwrap().world.structures[b].slots[0].kind, before);
        assert!(g.play.as_ref().unwrap().world.research[0] < 5000);
    }

    #[test]
    fn a_win_with_the_default_setup_earns_the_levels_star_and_a_changed_setup_does_not() {
        let Some(mut g) = game() else { return };
        g.mute = true;
        let (w, h) = (640, 480);
        let win = |g: &mut Game| {
            g.results = None; // the banner of the previous match
            g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
            g.update(0.0, w, h);
        };
        assert_eq!(g.stars.get("01-First Spark"), None);
        // a loss earns nothing
        g.headless_skirmish("01-First", 0, None, None, sim::Difficulty::Normal, None);
        g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Defeat);
        g.update(0.0, w, h);
        assert_eq!(g.stars.get("01-First Spark"), None);
        // a win with the map's own setup: silver for Normal
        g.headless_skirmish("01-First", 0, None, None, sim::Difficulty::Normal, None);
        win(&mut g);
        assert_eq!(g.stars.get("01-First Spark"), Some(sim::Difficulty::Normal));
        // changed credits (or research, air, turrets): no star, even on Brutal
        g.headless_skirmish("01-First", 0, Some((801, 800)), None, sim::Difficulty::Brutal, None);
        win(&mut g);
        assert_eq!(g.stars.get("01-First Spark"), Some(sim::Difficulty::Normal));
        // an easier win with the default setup never lowers it; the side is not part of the setup
        g.headless_skirmish("01-First", 1, None, None, sim::Difficulty::Easy, None);
        win(&mut g);
        assert_eq!(g.stars.get("01-First Spark"), Some(sim::Difficulty::Normal));
        g.headless_skirmish("01-First", 1, None, None, sim::Difficulty::Hard, None);
        win(&mut g);
        assert_eq!(g.stars.get("01-First Spark"), Some(sim::Difficulty::Hard));
        // the lobby shows it
        g.open_skirmish();
        assert_eq!(g.skirmish_menu.as_ref().unwrap().names[0], "01-First Spark");
    }

    #[test]
    fn the_main_menu_has_a_skirmish_row_that_opens_the_lobby_against_the_computer() {
        let Some(mut g) = game() else { return };
        let buttons = menu::layout(&Screen::Main, &mut g.assets, &g.levels, &g.settings, 640, 480);
        assert_eq!(buttons.len(), 7);
        let row = buttons.iter().find(|b| b.action == Action::Skirmish).expect("a SKIRMISH row");
        assert!(buttons.iter().all(|b| b.rect.y + b.rect.h <= 480), "all rows fit the lowest resolution");
        g.act(row.action.clone());
        let m = g.skirmish_menu.as_ref().expect("the skirmish setup screen");
        assert!(m.map_level().is_some(), "the first map's picture and script are loaded");
        assert_eq!((m.local, m.map), (0, 0));
    }

    #[test]
    fn a_skirmish_starts_with_the_lobbys_choices_for_either_side_and_restarts() {
        let Some(mut g) = game() else { return };
        g.mute = true;
        // blue: the human keeps the map's player 0 side
        g.headless_skirmish("01-First", 0, Some((1234, 77)), Some((300, 0)), sim::Difficulty::Hard, None);
        let p = g.play.as_ref().expect("the match started");
        assert_eq!((p.world.credits[0], p.world.credits[1]), (1234, 77));
        assert_eq!(p.world.research[0], 300);
        assert_eq!(p.world.difficulty, sim::Difficulty::Hard, "the lobby's difficulty reaches the world");
        assert_eq!(g.skirmish.as_ref().map(|s| s.map.as_str()), Some("01-First Spark"));
        let blue_hq = p.world.structures.iter().find(|s| s.kind_id == 100 && s.owner == opensow_formats::level::Owner::Player).map(|s| s.cell).unwrap();
        assert_eq!(blue_hq, (16, 3), "First Spark's blue HQ");
        // green: the sides are exchanged, so the human (player 0) sits at the green HQ and has green's resources
        g.headless_skirmish("01-First", 1, Some((1234, 77)), None, sim::Difficulty::Normal, None);
        let p = g.play.as_ref().unwrap();
        assert_eq!((p.world.credits[0], p.world.credits[1]), (77, 1234));
        let hq = p.world.structures.iter().find(|s| s.kind_id == 100 && s.owner == opensow_formats::level::Owner::Player).map(|s| s.cell).unwrap();
        assert_eq!(hq, (9, 55), "First Spark's green HQ");
        assert!(p.world.units.iter().filter(|u| u.player == 0).all(|u| u.px().1 > 32 * 30), "the human's units start on the green side");
        assert!(p.world.ai, "the computer plays the other side");
        // RESTART replays the same skirmish, ESC menu -> main menu forgets it
        g.act(Action::Restart);
        assert_eq!(g.play.as_ref().unwrap().world.credits[0], 77);
        assert_eq!(g.play.as_ref().unwrap().world.difficulty, sim::Difficulty::Normal, "RESTART keeps the level");
        g.act(Action::ToMain);
        assert!(g.skirmish.is_none());
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
        // winning the final mission shows the banner and the statistics, then plays the outro and shows the message
        g.start(FINAL_LEVEL);
        g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
        g.update(0.0, w, h);
        assert!(g.results.is_some() && g.intro.is_none());
        pass_results(&mut g, w, h);
        assert_eq!(g.intro.as_ref().map(|i| i.name.as_str()), Some("mO"));
        g.key(&Key::Named(NamedKey::Space), true, w, h);
        assert!(g.intro.is_none() && matches!(g.screen, Some(Screen::Message(_))));
        // winning any other mission (not from the campaign map) returns to the main menu after the statistics
        g.start("M02");
        g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
        g.update(0.0, w, h);
        pass_results(&mut g, w, h);
        assert!(g.intro.is_none() && g.results.is_none() && g.play.is_none() && matches!(g.screen, Some(Screen::Main)));
    }

    #[test]
    fn p_pauses_and_only_p_resumes() {
        let Some(mut g) = game() else { return };
        let (w, h) = (640, 480);
        g.mute = true;
        g.start("M01");
        g.update(0.5, w, h);
        let tick = g.play.as_ref().unwrap().world.tick;
        g.key(&Key::Character(SmolStr::new("p")), true, w, h);
        assert!(g.play.as_ref().unwrap().paused);
        g.update(1.0, w, h);
        assert_eq!(g.play.as_ref().unwrap().world.tick, tick, "the game stands still");
        // Esc and the mouse do nothing while paused
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert!(g.screen.is_none());
        g.mouse_button(MouseButton::Left, true, 100, 100, w, h);
        assert!(g.play.as_ref().unwrap().paused && g.screen.is_none());
        let mut buf = vec![0xff8040u32; w * h];
        g.draw(&mut buf, w, h);
        assert!(buf[100 * w..101 * w].iter().all(|&p| p == 0), "even rows are black");
        assert!(buf[101 * w..102 * w].iter().any(|&p| p != 0));
        g.key(&Key::Character(SmolStr::new("p")), true, w, h);
        assert!(!g.play.as_ref().unwrap().paused);
        g.update(0.5, w, h);
        assert!(g.play.as_ref().unwrap().world.tick > tick);
    }

    /// Click through the banner (after its two seconds) and the statistics screen.
    fn pass_results(g: &mut Game, w: usize, h: usize) {
        assert!(g.results.is_some());
        g.mouse_button(MouseButton::Left, true, 5, 5, w, h);
        assert!(!g.results.as_ref().unwrap().in_statistics(), "the banner waits two seconds");
        g.update(2.5, w, h);
        g.mouse_button(MouseButton::Left, true, 5, 5, w, h);
        assert!(g.results.as_ref().unwrap().in_statistics());
        g.update(0.1, w, h);
        assert!(!g.results.as_ref().unwrap().finished(), "the bars are still counting");
        g.mouse_button(MouseButton::Left, true, 5, 5, w, h); // completes the bars
        assert!(g.results.as_ref().unwrap().finished() && g.results.is_some());
        g.mouse_button(MouseButton::Left, true, 5, 5, w, h); // leaves
        assert!(g.results.is_none());
    }

    #[test]
    fn the_tutorial_button_starts_m00_without_a_movie_and_its_end_returns_to_the_menu() {
        let Some(mut g) = game() else { return };
        let (w, h) = (640, 480);
        g.movies = true;
        g.mute = true;
        g.act(Action::Tutorial);
        assert!(g.intro.is_none() && g.screen.is_none() && !g.campaign);
        let p = g.play.as_ref().unwrap();
        assert_eq!(p.level, "M00");
        assert_eq!(p.world.tutorial.as_ref().map(|t| t.step), Some(0));
        assert!(!p.world.ai);
        // the script's last step goes straight to the main menu (state 0x1c), no result screen
        g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
        g.update(0.0, w, h);
        assert!(g.play.is_none() && matches!(g.screen, Some(Screen::Main)));
    }

    #[test]
    fn the_warmonger_edition_has_its_own_missions_progress_and_saves() {
        use opensow_formats::{config::PassedLevels, Edition};
        let Some(mut g) = game() else { return };
        if g.assets.game.warmonger.is_none() {
            return;
        }
        assert!(g.levels.iter().any(|l| l == "M05"));
        let base_m01 = g.assets.game.resolve("Data\\Levels\\M01.map").unwrap();
        g.set_edition(Edition::Warmonger);
        assert_eq!(g.progress, PassedLevels::initial_warmonger());
        assert!(!g.levels.iter().any(|l| l == "M05"), "{:?}", g.levels);
        assert!(g.levels.iter().any(|l| l == "M07"));
        assert_ne!(g.assets.game.resolve("Data\\Levels\\M01.map").unwrap(), base_m01);
        assert_eq!(saveui::Store::locate(&g.assets.game).edition, Edition::Warmonger, "new saves are flagged");
        // a mission loads from Warmonger's data and passing it follows the Warmonger unlock chain
        g.campaign = true;
        g.start("M03");
        assert!(g.play.as_ref().is_some_and(|p| p.level == "M03" && p.world.units.iter().any(|u| u.kind == 46)), "the convoy mission's Rogon pods");
        g.progress.pass_warmonger(3);
        assert_eq!((g.progress.states[3], g.progress.states[4]), (1, 2));
        g.open_lands(false);
        assert!(g.lands.as_ref().unwrap().headings(3).0.starts_with("TERRITORY : 03"));
        g.set_edition(Edition::Original);
        assert_eq!(g.progress, PassedLevels::initial());
        assert!(g.levels.iter().any(|l| l == "M05"));
    }

    #[test]
    fn a_save_slot_remembers_its_campaign_and_loading_it_switches_the_edition() {
        use opensow_formats::Edition;
        let Some(mut g) = game() else { return };
        if g.assets.game.warmonger.is_none() {
            return;
        }
        g.set_edition(Edition::Warmonger);
        g.start("M03");
        let snapshot = g.play.as_mut().unwrap().snapshot();
        let dir = std::env::temp_dir().join(format!("opensow-edition-save-{}", std::process::id()));
        let mut store = saveui::Store { write: dir.clone(), also: None, edition: Edition::Warmonger };
        let thumb = vec![0u16; 160 * 120];
        store.write(1, "wm", "M03", &snapshot, &thumb).unwrap();
        store.edition = Edition::Original;
        store.write(2, "base", "M03", &snapshot, &thumb).unwrap();
        let (level, edition, payload) = store.level_of(1).unwrap();
        assert_eq!((level.as_str(), edition), ("M03", Edition::Warmonger));
        assert_eq!(store.level_of(2).unwrap().1, Edition::Original);
        // loading it from the base game's campaign selects Warmonger and its mission
        g.set_edition(Edition::Original);
        g.saveui_event(saveui::Event::Load(level, edition, payload));
        assert_eq!(g.assets.game.edition, Edition::Warmonger);
        assert!(g.play.as_ref().is_some_and(|p| p.level == "M03" && p.world.units.iter().any(|u| u.kind == 46)), "Warmonger's convoy mission");
        let _ = std::fs::remove_dir_all(dir);
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
        g.update(3.0, w, h); // the reticle plays first; ACCEPT and BACK appear after it
        g.mouse_moved(40, 450, w, h);
        g.mouse_button(MouseButton::Left, true, 40, 450, w, h);
        assert!(g.lands.is_none() && g.play.is_some(), "ACCEPT starts mission 1");
        assert_eq!(g.play.as_ref().unwrap().level, "M01");
        // winning shows the banner and the statistics; leaving them passes the land, opens the next one in the exe's order and
        // brings the map back
        g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Victory);
        g.update(0.0, w, h);
        assert_eq!(g.progress.states[1], 2, "not passed before the statistics are left");
        pass_results(&mut g, w, h);
        assert_eq!((g.progress.states[1], g.progress.states[6]), (1, 2));
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
        // the map: hovering a land is silent (the headings type with briefing-type02 later); selecting it flashes (46) and says its
        // location (41) on the first frame, and the description box opens (44) once the reticle has played
        g.sfx.played.clear();
        g.mouse_moved(170, 190, w, h);
        g.mouse_button(MouseButton::Left, true, 170, 190, w, h);
        assert!(g.sfx.played.is_empty());
        g.update(0.0, w, h);
        assert_eq!(g.sfx.played, vec![46, 41]);
        g.update(1.5, w, h);
        assert_eq!(g.sfx.played, vec![46, 41, 44]);
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
        // the player's own actions raised by the world (air strike far off screen, turret placed) are always heard
        g.sfx.played.clear();
        g.play.as_mut().unwrap().world.player_sounds.extend([68, 72]);
        g.update(0.0, w, h);
        assert_eq!(g.sfx.played, vec![68, 72]);
        assert!(g.play.as_ref().unwrap().world.player_sounds.is_empty());
    }

    #[test]
    fn clicking_any_building_plays_a_click() {
        let Some(mut g) = game() else { return };
        let (w, h) = (1024, 768);
        g.start("M01");
        g.update(0.0, w, h);
        for owner in [opensow_formats::level::Owner::Player, opensow_formats::level::Owner::Enemy] {
            let p = g.play.as_mut().unwrap();
            p.world.clear_selection();
            p.sel_building = None;
            let b = p.world.structures.iter().position(|s| s.alive() && s.owner == owner && !matches!(s.kind, opensow_formats::level::ObjectKind::Turret(_))).unwrap();
            let (cx, cy) = (p.world.structures[b].cell.0 * sim::CELL + 8, p.world.structures[b].cell.1 * sim::CELL + 8);
            assert_eq!(p.world.structure_at(cx, cy), Some(b));
            p.view.x = cx as f32 - 300.0 / p.view.zoom;
            p.view.y = cy as f32 - 300.0 / p.view.zoom;
            g.sfx.played.clear();
            g.mouse_button(MouseButton::Left, true, 300, 300, w, h);
            g.mouse_button(MouseButton::Left, false, 300, 300, w, h);
            assert!(g.sfx.played.contains(&play::BUILDING_CLICK_SOUND), "{owner:?}: {:?}", g.sfx.played);
            assert_eq!(g.play.as_ref().unwrap().sel_building.is_some(), owner == opensow_formats::level::Owner::Player);
        }
    }

    #[test]
    fn message_events_are_spoken_and_the_mission_end_speaks() {
        let Some(mut g) = game() else { return };
        let (w, h) = (640, 480);
        g.start("M01");
        g.update(0.0, w, h);
        g.sfx.played.clear();
        let p = g.play.as_mut().unwrap();
        let t = p.world.tick;
        p.world.log.push((t, opensow_formats::messages::BUILDING_CAPTURED));
        p.world.log.push((t, 11)); // 5000 credits: speech plus the info-box blip
        p.world.log.push((t, 6)); // "new airstrike ready" has no speech file
        g.update(0.0, w, h);
        assert_eq!(g.sfx.spoken, vec!["06-building-captured.wav", "02-5000-credits-arrived.wav"]);
        assert_eq!(g.sfx.played, vec![51]);
        // spoken once
        g.update(0.0, w, h);
        assert_eq!(g.sfx.spoken.len(), 2);
        // losing says so, shows the banner and the statistics and returns to the main menu
        g.play.as_mut().unwrap().world.outcome = Some(sim::Outcome::Defeat);
        g.update(0.0, w, h);
        assert_eq!(g.sfx.spoken.last().map(String::as_str), Some("01-mis-failed.wav"));
        pass_results(&mut g, w, h);
        assert!(g.play.is_none() && matches!(g.screen, Some(Screen::Main)));
    }
}
