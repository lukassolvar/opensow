//! The `Game`'s main flow: start-up, progress, sounds and movies, menu actions, and the per-frame `update` and `draw`.

use super::*;

impl Game {
    /// The game at its start: main menu, settings and progress read from disk, nothing playing.
    pub(super) fn new(assets: Assets) -> Game {
        let levels = opensow_viewer::scene::Scene::list(&assets.game);
        let mut g = Game {
            assets,
            numpad: false,
            digit: None,
            code: None,
            game_speed: 1.0,
            settings: Settings::default(),
            levels,
            screen: Some(Screen::Main),
            back: Vec::new(),
            play: None,
            carry_counter: 0,
            hover: None,
            pressed: None,
            shift: false,
            quick_save: false,
            autosave: false,
            quick_save_auto: false,
            scale: 1.0,
            cursor: (0, 0),
            quit: false,
            intro: None,
            boot: None,
            seen_intros: edition::intros_seen(),
            then: Then::Menu,
            language: 0,
            movies: false,
            mute: false,
            lands: None,
            options_menu: None,
            saveui: None,
            net_menu: None,
            skirmish_menu: None,
            map_editor: None,
            ctrl: false,
            attack_key: false,
            skirmish: None,
            stars: stars::Stars::load(),
            ingame: None,
            results: None,
            options: options::GameOptions::load(),
            progress: opensow_formats::config::PassedLevels::initial(),
            campaign: false,
            music: music::Music::new(),
            sfx: sfx::Sfx::new(),
            dev: devtools::DevTools::default(),
        };
        g.apply_audio();
        g.progress = g.load_progress();
        g
    }

    /// The saved progress of the selected campaign, or its start (tests never read the disk).
    pub(super) fn load_progress(&self) -> opensow_formats::config::PassedLevels {
        use opensow_formats::{config::PassedLevels, Edition};
        let saved = if cfg!(test) { None } else { saveui::Store::locate(&self.assets.game).load_progress() };
        saved.unwrap_or_else(|| if self.assets.game.edition == Edition::Warmonger { PassedLevels::initial_warmonger() } else { PassedLevels::initial() })
    }

    /// Switch the campaign (docs/warmonger.md): mission data, skirmish maps, progress and saves follow the edition.
    pub(super) fn set_edition(&mut self, e: opensow_formats::Edition) {
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
    pub(super) fn save_progress(&self) {
        if !cfg!(test) {
            if let Err(e) = saveui::Store::locate(&self.assets.game).save_progress(&self.progress) {
                eprintln!("progress not saved: {e}");
            }
        }
    }

    /// The buttons of the open menu screen for this window size.
    pub(super) fn buttons(&mut self, w: usize, h: usize) -> Vec<menu::Button> {
        match &self.screen {
            Some(s) => menu::layout(s, &mut self.assets, &self.levels, &self.settings, w, h),
            None => Vec::new(),
        }
    }

    /// Play a sound effect by the exe's id (silent unless the real game turned sound on).
    pub(super) fn play_sfx(&mut self, id: u8) {
        self.sfx.play(&self.assets.game, id, 1.0);
    }

    /// Switch the background music (only in the real game: movies on and not muted).
    pub(super) fn play_music(&mut self, name: &str) {
        if self.movies && !self.mute {
            self.music.play(&self.assets.game, name);
        }
    }

    /// The movies' soundtrack volume (intro, briefings, outro): the MUSIC row, so music off silences them (not in the exe, whose
    /// DirectShow player never sets their volume).
    pub(super) fn movie_gain(&self) -> f32 {
        if self.mute {
            0.0
        } else {
            sfx::gain(self.music.on, self.music.volume)
        }
    }

    /// The green BRIEFING animation's sound (`mA`, after New game): the FX row (not in the exe, which plays it at full volume).
    pub(super) fn anim_gain(&self) -> f32 {
        if self.mute || !self.sfx.enabled {
            0.0
        } else {
            sfx::gain(self.sfx.fx_on, self.sfx.fx_volume)
        }
    }

    /// Show a movie, then do `then`; without a movie (missing file, movies off) `then` happens at once.
    pub(super) fn play_movie(&mut self, name: &str, then: Then) {
        self.then = then;
        if self.movies {
            match intro::Intro::open(&self.assets.game, name, self.language) {
                Ok(mut m) => {
                    self.music.stop();
                    let gain = self.movie_gain();
                    if gain > 0.0 && !m.start_sound(gain) {
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

    /// The exe restarts `uvod.wav` whenever a boot page starts.
    pub(super) fn boot_music(&mut self) {
        if self.boot.as_mut().is_some_and(|b| b.take_changed()) {
            self.music.stop();
            self.play_music("uvod");
        }
    }

    /// Leaves the boot pictures for the menu.
    pub(super) fn end_boot(&mut self) {
        self.boot = None;
        self.play_music("Menu");
    }

    /// A movie ended or was skipped: goes on to what was to follow it.
    pub(super) fn end_movie(&mut self) {
        self.intro = None;
        match std::mem::replace(&mut self.then, Then::Menu) {
            Then::Menu => self.play_music("Menu"),
            Then::Level(l) => self.start(&l),
            Then::Screen(s) => self.screen = Some(s),
            Then::Lands(video) => self.open_lands(video),
        }
    }

    /// Shows the selected campaign's map; the first time (ever) with movies on, that campaign's intro `mI` plays before it.
    pub(super) fn campaign_map(&mut self, video: bool) {
        let i = usize::from(self.assets.game.edition == opensow_formats::Edition::Warmonger);
        if self.movies && !self.seen_intros[i] {
            self.seen_intros[i] = true;
            edition::remember_intro(self.assets.game.edition);
            self.screen = None;
            self.lands = None;
            self.play_movie("mI", Then::Lands(video));
        } else {
            self.open_lands(video);
        }
    }

    /// Start a mission the way the exe does: its briefing movie first.
    pub(super) fn begin(&mut self, level: &str) {
        self.play_movie(level, Then::Level(level.to_owned()));
    }

    /// Starts campaign level `level`.
    pub(super) fn start(&mut self, level: &str) {
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

    /// Does what a menu button does.
    pub(super) fn act(&mut self, a: Action) {
        match a {
            Action::NewGame => self.campaign_map(true),
            Action::Tutorial => {
                // state 5: no briefing movie, the level (0) loads at once and the tutorial script starts at step 0
                self.campaign = false;
                self.start(TUTORIAL_LEVEL);
            }
            Action::Options => self.open_options(),
            Action::LoadGame => self.open_saveui(saveui::Mode::Load),
            Action::Multiplayer => self.open_multiplayer(),
            Action::Skirmish => self.open_skirmish(),
            Action::MapEditor => self.open_map_editor(),
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
                    if let Some(script) = self.skirmish_script(&run) {
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
    pub(super) fn open_lands(&mut self, video: bool) {
        self.screen = None;
        self.back.clear();
        self.play_music(if music::pick(2) == 0 { "Brief2" } else { "Brief" }); // 0x441cf9/0x441c4b: random 0 -> brief2.wav
        let progress = self.effective_progress();
        let gain = self.anim_gain();
        self.lands = Some(lands::Lands::new(&mut self.assets, progress, video && self.movies, gain));
    }

    /// The campaign progress the map shows: with the dev tools' UNLOCK ALL every locked land is open.
    pub(super) fn effective_progress(&self) -> opensow_formats::config::PassedLevels {
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
    pub(super) fn gpu_drawable(&self) -> bool {
        // the save thumbnails are taken from the drawn picture: those frames stay on the CPU
        !self.quick_save && !self.saveui.as_ref().is_some_and(|m| m.needs_capture())
    }

    /// Whether a mission is on screen with nothing above it.
    pub(super) fn in_mission(&self) -> bool {
        self.play.is_some()
            && self.screen.is_none()
            && self.intro.is_none()
            && self.lands.is_none()
            && self.options_menu.is_none()
            && self.saveui.is_none()
            && self.net_menu.is_none()
            && self.skirmish_menu.is_none()
            && self.map_editor.is_none()
            && self.results.is_none()
    }

    /// Hand the menu's current values to the sound players (the exe applies them as they change).
    pub(super) fn apply_audio(&mut self) {
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
        self.settings.turret_range = o.extra.turret_range;
        self.settings.zoom = o.extra.zoom;
        self.settings.swap_buttons = o.extra.swap_buttons;
    }

    /// Above zoom 1 the world is drawn from 2x enhanced copies of the original art (viewer `upscale`, docs/enhance.md): the Options
    /// switch on page 2, unless `--no-enhance` / `OPENSOW_ENHANCE=0` forces it off for this run.
    pub(super) fn apply_enhance(&self) {
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

    /// Opens a screen, remembering the current one to return to.
    pub(super) fn push(&mut self, next: Screen) {
        if let Some(cur) = self.screen.replace(next) {
            self.back.push(cur);
        }
    }

    /// Draws one frame into the `w` x `h` buffer.
    pub(super) fn draw(&mut self, buf: &mut [u32], w: usize, h: usize) {
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

    /// Draws whatever is open: boot pictures, a movie, a menu screen or the mission.
    pub(super) fn draw_screen(&mut self, buf: &mut [u32], w: usize, h: usize) {
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
        if let Some(m) = self.map_editor.as_mut() {
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
    pub(super) fn custom_pointer(&self) -> bool {
        self.screen.is_none()
            && self.intro.is_none()
            && self.lands.is_none()
            && self.options_menu.is_none()
            && self.saveui.is_none()
            && self.net_menu.is_none()
            && self.skirmish_menu.is_none()
            && self.map_editor.is_none()
            && self.play.as_ref().is_some_and(|p| p.edge_scroll)
    }

    /// Advances everything by `dt` seconds: music, movies, the open screen, the mission.
    pub(super) fn update(&mut self, dt: f32, w: usize, h: usize) {
        // a mission started while the key was already held sees it too
        if let Some(p) = self.play.as_mut() {
            p.attack_key = self.attack_key;
        }
        crate::audio::tick();
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
        if let Some(m) = self.map_editor.as_mut() {
            m.update(dt, crate::options::scroll_px_per_second(self.settings.scroll_keys));
            return;
        }
        if self.options_menu.is_some() || self.net_menu.is_some() {
            return;
        }
        if self.screen.is_none() {
            if let Some(p) = self.play.as_mut() {
                p.world.cheats = self.dev.cheats();
                let settings = menu::Settings {
                    ticks_per_second: (self.settings.ticks_per_second as f32 * self.game_speed).round() as u32 * self.dev.speed,
                    ..self.settings.clone()
                };
                p.update(dt, &settings, w, h);
                // the automatic save (not in the original): on the first frame of a mission, then every AUTOSAVE_TICKS of game
                // time, into the quick save slot; the grab happens on the next drawn frame like F5's
                if self.autosave && !self.quick_save && p.world.tick >= p.autosave_at && p.world.outcome.is_none() {
                    p.autosave_at = p.world.tick + crate::AUTOSAVE_TICKS;
                    self.quick_save = true;
                    self.quick_save_auto = true;
                }
                self.carry_counter = p.world.carry_counter;
                // HUD sounds raised while updating: icon hover (50), info box open / close (51 / 52)
                p.ui_sounds.append(&mut p.infobox.sounds);
                // the player's own actions raised by the world (air strike launch, turret placed): priority, heard anywhere
                p.ui_sounds.append(&mut p.world.player_sounds);
                let hud_sounds: Vec<u8> = std::mem::take(&mut p.ui_sounds);
                for id in hud_sounds {
                    self.play_sfx(id);
                }
                let p = self.play.as_mut().unwrap();
                let (zoom, vx, vy) = (p.view.zoom, p.view.x, p.view.y);
                // the exe pans by position and fades with the distance from the screen centre (`sfx::pan` / `sfx::volume`)
                let (cx, cy) = (vx + w as f32 / zoom / 2.0, vy + h as f32 / zoom / 2.0);
                let heard: Vec<(u8, bool, i32, i32, i32)> = p
                    .world
                    .sounds
                    .drain(..)
                    .map(|(id, x, y, j)| (id, j, (x as f32 - cx) as i32, (y as f32 - cy) as i32, (x as f32 - vx) as i32 - 320))
                    .collect();
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
                    // a skirmish won with the map's default setup earns the star of its level (stars.rs); custom maps have none
                    if let Some(run) = self.skirmish.clone().filter(|r| won && r.default_setup && r.custom.is_none()) {
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
}
