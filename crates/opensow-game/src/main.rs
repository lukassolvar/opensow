//! `opensow-game`: a playable prototype with the original menus and a skirmish on the campaign levels.
//!
//! ```text
//! opensow-game [--no-intro] [--mute] [--lang SK|CZ|PL|HU|RU|CHINNESSE]   start the game window (plays the intro movie first; any key or click skips it)
//! opensow-game --screenshot out.png [--size WxH] [--screen main|levels|options|pause|message|results]
//!              [--mute] [--intro-frame N [--movie mI|mO|M01..M22]] [--hover N] [--play M01 [--ticks N] [--select-all] [--select-building CX,CY] [--move CX,CY] [--zoom Z] [--center CX,CY] [--range] [--fx CX,CY,KIND,AGE]... [--no-mask]]
//! ```

mod assets;
mod gdi;
mod hud;
mod infobox;
mod intro;
mod ingame;
mod lands;
mod options;
mod menu;
mod multiplayer;
mod music;
mod play;
mod saveui;
mod sfx;
mod results;
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
    /// The options menu (original art) while it is open; it replaces everything else on screen.
    options_menu: Option<options::OptionsMenu>,
    /// The load / save page (original art) while it is open; it replaces everything else on screen.
    saveui: Option<saveui::SaveUi>,
    /// The multiplayer screens (docs/multiplayer-menus.md).
    net_menu: Option<multiplayer::NetMenu>,
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
}

impl Game {
    fn new(assets: Assets) -> Game {
        let levels = opensow_viewer::scene::Scene::list(&assets.game);
        let mut g = Game { assets, settings: Settings::default(), levels, screen: Some(Screen::Main), back: Vec::new(), play: None, hover: None, pressed: None, shift: false, scale: 1.0, cursor: (0, 0), quit: false, intro: None, then: Then::Menu, language: 0, movies: false, mute: false, lands: None, options_menu: None, saveui: None, net_menu: None, ingame: None, results: None, options: options::GameOptions::load(), progress: opensow_formats::config::PassedLevels::initial(), campaign: false, music: music::Music::new(), sfx: sfx::Sfx::new() };
        g.apply_audio();
        g
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
            Action::Tutorial => {
                // state 5: no briefing movie, the level (0) loads at once and the tutorial script starts at step 0
                self.campaign = false;
                self.start(TUTORIAL_LEVEL);
            }
            Action::Unavailable(msg) => self.push(Screen::Message(msg.to_owned())),
            Action::Options => self.open_options(),
            Action::LoadGame => self.open_saveui(saveui::Mode::Load),
            Action::Multiplayer => self.open_multiplayer(),
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
                if let Some(l) = self.play.as_ref().map(|p| p.level.clone()) {
                    self.start(&l);
                }
            }
            Action::ToMain => {
                self.ingame = None;
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
            ui.snapshot = p.snapshot();
        }
        self.saveui = Some(ui);
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
            saveui::Event::Load(level, snapshot) => {
                self.saveui = None;
                self.ingame = None;
                self.back.clear();
                self.campaign = false;
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
        if let Some(r) = self.results.as_ref().filter(|r| r.in_statistics()) {
            r.draw_statistics(buf, w, h);
            return;
        }
        if let Some(play) = self.play.as_mut() {
            play.draw(&mut self.assets, &self.settings, buf, w, h, self.scale);
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
        self.screen.is_none() && self.intro.is_none() && self.lands.is_none() && self.options_menu.is_none() && self.saveui.is_none() && self.net_menu.is_none() && self.play.as_ref().is_some_and(|p| p.edge_scroll)
    }

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
        if self.options_menu.is_some() || self.net_menu.is_some() {
            return;
        }
        if self.screen.is_none() {
            if let Some(p) = self.play.as_mut() {
                p.update(dt, &self.settings, w, h);
                // HUD sounds raised while updating: icon hover (50), info box open / close (51 / 52)
                p.ui_sounds.extend(p.infobox.sounds.drain(..));
                let hud_sounds: Vec<u8> = std::mem::take(&mut p.ui_sounds);
                for id in hud_sounds {
                    self.play_sfx(id);
                }
                let p = self.play.as_mut().unwrap();
                let (zoom, vx, vy) = (p.view.zoom, p.view.x, p.view.y);
                // the exe pans by position and fades with the distance from the screen centre (`sfx::pan` / `sfx::volume`)
                let (cx, cy) = (vx + w as f32 / zoom / 2.0, vy + h as f32 / zoom / 2.0);
                let heard: Vec<(u8, bool, i32, i32)> = p.world.sounds.drain(..).map(|(id, x, y, j)| (id, j, (x as f32 - cx) as i32, (y as f32 - cy) as i32)).collect();
                for (id, jitter, dx, dy) in heard {
                    self.sfx.play_at(&self.assets.game, id, if jitter { sfx::jitter() } else { 1.0 }, dx, dy);
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
                if let Some(o) = outcome {
                    // 0x4357a0: the mission-end speech, then the banner over the frozen frame
                    let won = o == sim::Outcome::Victory;
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
                    self.progress.pass(n);
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
        if let Some(l) = self.lands.as_mut() {
            if down && button == MouseButton::Left {
                let e = l.click(x, y, w, h);
                self.progress = l.progress.clone();
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
        if let Some(r) = self.results.as_mut() {
            // banner: any button leaves after two seconds; statistics: the left button (`[player+0xa]`)
            if down && (button == MouseButton::Left || !r.in_statistics()) {
                let e = r.input(false, true);
                self.results_event(e);
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

    fn key(&mut self, key: &Key, down: bool, w: usize, h: usize) {
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
            // the pause (`0x435330`) only waits for Space again (`[player+0x7d]`)
            if down && matches!(key, Key::Named(NamedKey::Space)) {
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
            Key::Named(NamedKey::Space) if self.screen.is_none() => {
                if let Some(p) = self.play.as_mut() {
                    p.paused ^= true;
                }
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
            Key::Character(c) if self.screen.is_none() && matches!(c.as_str(), "1" | "2" | "3" | "4" | "5") => {
                if let Some(p) = self.play.as_mut() {
                    let tier: u32 = c.parse().unwrap_or(1);
                    p.placing = (p.world.turret_flags[0][tier as usize - 1] && p.world.credits[0] >= 200 * tier as i32).then_some(tier);
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
                if let Some(win) = &self.window {
                    win.set_cursor_visible(!self.game.custom_pointer());
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
                if self.game.screen.is_none() && self.game.results.is_none() {
                    if let Some(p) = self.game.play.as_mut().filter(|p| !p.paused) {
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
                if has("--select-advancer") {
                    for u in p.world.units.iter_mut().filter(|u| u.player == 0 && u.kind == 40) {
                        u.selected = true;
                    }
                }
                if let Some((cx, cy)) = get("--mouse-cell").and_then(pair) {
                    // headless: the pointer sprite hovers this cell (its type follows the selection and the object there)
                    let (sx, sy) = p.view.to_screen(cx * 32.0 + 16.0, cy * 32.0 + 16.0);
                    p.cursor = (sx, sy);
                    p.edge_scroll = true;
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
                for spec in args.iter().enumerate().filter(|(_, a)| *a == "--unit").filter_map(|(i, _)| args.get(i + 1)) {
                    // headless: --unit KIND,PLAYER,PX,PY puts a unit on a map pixel (repeatable)
                    let f: Vec<i32> = spec.split(',').filter_map(|v| v.parse().ok()).collect();
                    if let [kind, player, x, y] = f[..] {
                        p.world.add_unit(kind, player, x, y);
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
                for _ in 0..get("--ticks").and_then(|t| t.parse().ok()).unwrap_or(0) {
                    p.world.step();
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
                    if let (Some(m), Some(hit)) = (game.options_menu.as_mut(), get("--hover").and_then(|n| n.parse::<usize>().ok())) {
                        let p = m.centre(if hit < 14 { options::Hit::Arrow(hit) } else { options::Hit::Label(hit - 14) });
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
                        l.update(get("--clock").and_then(|n| n.parse().ok()).unwrap_or(0.0));
                    }
                    Screen::Main
                }
                _ => Screen::Main,
            });
        }
        if game.lands.is_some() || game.options_menu.is_some() || game.saveui.is_some() || game.net_menu.is_some() || game.results.is_some() {
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
        // Load game opens the load page; CANCEL goes back to the main menu
        let load = g.buttons(w, h)[1].rect;
        g.mouse_button(MouseButton::Left, true, load.x + 5, load.y + 5, w, h);
        g.mouse_button(MouseButton::Left, false, load.x + 5, load.y + 5, w, h);
        assert!(g.saveui.is_some() && g.screen.is_none());
        g.key(&Key::Named(NamedKey::Escape), true, w, h);
        assert!(g.saveui.is_none());
        assert_eq!(g.screen, Some(Screen::Main));
        let mut buf = vec![0u32; w * h];
        g.draw(&mut buf, w, h);
        assert!(buf.iter().any(|&p| p != 0));
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
        let store = saveui::Store { write: dir.clone(), also: None };
        store.write(3, "t", "M05", &a, &vec![0u16; 160 * 120]).unwrap();
        let (level, snap) = store.level_of(3).unwrap();
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
    fn any_building_shows_its_slots_when_hovered_or_clicked() {
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
            // hovering alone shows nothing
            g.mouse_moved(x, y, w, h);
            g.update(0.0, w, h);
            let mut buf = vec![0u32; w * h];
            g.draw(&mut buf, w, h);
            assert!(g.play.as_ref().unwrap().hud_layout().unwrap().slots.is_empty());
            assert!(g.play.as_ref().unwrap().infobox.kind.is_none(), "hover alone opens no info box");
            // a click selects it: slots and info box appear
            g.mouse_button(MouseButton::Left, true, x, y, w, h);
            g.mouse_button(MouseButton::Left, false, x, y, w, h);
            assert_eq!(g.play.as_ref().unwrap().sel_building, Some(target));
            g.update(0.0, w, h);
            g.draw(&mut buf, w, h);
            let n = g.play.as_ref().unwrap().hud_layout().unwrap().slots.len();
            assert!(n > 0 && (target != own || n == 2), "M01's blue medium factory has two slots, got {n}");
            assert!(g.play.as_ref().unwrap().infobox.visible());
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
    fn space_pauses_and_only_space_resumes() {
        let Some(mut g) = game() else { return };
        let (w, h) = (640, 480);
        g.mute = true;
        g.start("M01");
        g.update(0.5, w, h);
        let tick = g.play.as_ref().unwrap().world.tick;
        g.key(&Key::Named(NamedKey::Space), true, w, h);
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
        g.key(&Key::Named(NamedKey::Space), true, w, h);
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
