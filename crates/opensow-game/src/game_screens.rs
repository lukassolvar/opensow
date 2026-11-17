//! What each screen's events do to the `Game`: campaign map, pause menu, multiplayer, skirmish, map editor, options, save/load and results.

use super::*;

impl Game {
    /// Dev tools: a pressed tool button took effect.
    pub(super) fn dev_event(&mut self, item: devtools::Item) {
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
    pub(super) fn lands_sounds(&mut self) {
        let ids: Vec<u8> = self.lands.as_mut().map(|l| std::mem::take(&mut l.sounds)).unwrap_or_default();
        for id in ids {
            self.play_sfx(id);
        }
    }

    /// What the map decided: BACK to the main menu, or ACCEPT a land's mission.
    pub(super) fn lands_event(&mut self, e: lands::Event) {
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
                self.lands = Some(lands::Lands::new(&mut self.assets, progress, false, 0.0));
            }
            lands::Event::Accept(land) => {
                self.lands = None;
                self.campaign = true;
                self.begin(&format!("M{land:02}"));
            }
        }
    }

    /// Esc in a mission: the in-game menu opens (sound 35).
    pub(super) fn open_pause(&mut self) {
        self.screen = Some(Screen::Pause);
        self.ingame = None;
        self.pause_menu();
    }

    /// The in-game menu of the pause screen (made on demand, e.g. when the options menu returns to it).
    pub(super) fn pause_menu(&mut self) -> &mut ingame::InGameMenu {
        if self.ingame.is_none() {
            let tutorial = self.play.as_ref().is_some_and(|p| p.world.tutorial.is_some());
            self.ingame = Some(ingame::InGameMenu::new(&mut self.assets, tutorial));
        }
        self.ingame.as_mut().unwrap()
    }

    /// Plays the sounds the pause menu raised.
    pub(super) fn ingame_sounds(&mut self) {
        let ids: Vec<u8> = self.ingame.as_mut().map(|m| std::mem::take(&mut m.sounds)).unwrap_or_default();
        for id in ids {
            self.play_sfx(id);
        }
    }

    /// Does what the pause menu asked for.
    pub(super) fn ingame_event(&mut self, e: ingame::Event) {
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
    pub(super) fn open_multiplayer(&mut self) {
        self.net_menu = Some(multiplayer::NetMenu::new(&mut self.assets));
        self.screen = None;
    }

    /// Main menu SKIRMISH: the multiplayer lobby against the computer (docs/skirmish.md).
    pub(super) fn open_skirmish(&mut self) {
        self.skirmish_menu = Some(skirmish_ui::SkirmishMenu::new(&mut self.assets, &self.stars));
        self.screen = None;
    }

    /// Main menu MAP EDITOR (docs/map-editor-ingame.md).
    pub(super) fn open_map_editor(&mut self) {
        let mut ed = mapeditor_ui::MapEditor::new(&mut self.assets);
        ed.set_ctrl(self.ctrl);
        self.map_editor = Some(ed);
        self.screen = None;
    }

    /// Does what the map editor asked for.
    pub(super) fn map_editor_event(&mut self, e: mapeditor_ui::Event) {
        let ids: Vec<u8> = self.map_editor.as_mut().map(|m| std::mem::take(&mut m.sounds)).unwrap_or_default();
        for id in ids {
            self.play_sfx(id);
        }
        match e {
            mapeditor_ui::Event::None => {}
            mapeditor_ui::Event::Leave => {
                self.map_editor = None;
                self.screen = Some(Screen::Main);
            }
            mapeditor_ui::Event::Skirmish => {
                self.map_editor = None;
                self.open_skirmish();
                if let Some(m) = self.skirmish_menu.as_mut() {
                    m.set_tab(skirmish_ui::Tab::Custom);
                }
            }
        }
    }

    /// `--skirmish`: picks the map by (a prefix of) its name in the skirmish lobby and starts it.
    pub(super) fn headless_skirmish(
        &mut self,
        map: &str,
        human: usize,
        credits: Option<(i32, i32)>,
        research: Option<(i32, i32)>,
        difficulty: sim::Difficulty,
        spectate: Option<sim::Difficulty>,
    ) {
        let mut menu = skirmish_ui::SkirmishMenu::new(&mut self.assets, &self.stars);
        // `custom/<file stem>`: a map of the maps folder (docs/map-editor-ingame.md)
        if map.to_lowercase().starts_with("custom/") {
            menu.set_tab(skirmish_ui::Tab::Custom);
        }
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
            let run = SkirmishRun {
                map: menu.maps[i].clone(),
                setup: menu.setup.clone(),
                human,
                difficulty,
                spectate,
                default_setup: menu.is_default(),
                custom: menu.custom_path(),
            };
            self.start_skirmish(run, script);
        }
    }

    /// The map script of a skirmish run: the custom map's level or the shipped `.net`.
    pub(super) fn skirmish_script(&self, run: &SkirmishRun) -> Option<opensow_formats::level::Level> {
        match &run.custom {
            Some(p) => custom_maps::load(p).ok().map(|m| m.level),
            None => skirmish::load_map(&self.assets.game, &run.map),
        }
    }

    /// The running skirmish as a save stores it (empty in a mission).
    pub(super) fn skirmish_bytes(&self) -> Vec<u8> {
        self.skirmish.as_ref().and_then(|r| serde_json::to_vec(r).ok()).unwrap_or_default()
    }

    /// The skirmish a save belongs to and its map script; `None` for a mission. Saves made before the run was stored only have the
    /// map's name (`custom/<stem>` or a shipped `.net` map): they get the lobby's default setup for that map.
    /// `maps`: the maps folder (`custom_maps::dir`; tests use their own).
    pub(super) fn saved_skirmish(&self, saved: &saveui::Saved, maps: &std::path::Path) -> Option<(SkirmishRun, opensow_formats::level::Level)> {
        // a custom map by its key, in the maps folder
        let custom_file =
            |map: &str| map.strip_prefix("custom/").map(|stem| maps.join(format!("{stem}.{}", opensow_formats::custommap::EXTENSION))).filter(|p| p.is_file());
        let stored = serde_json::from_slice::<SkirmishRun>(&saved.skirmish).ok();
        let mut run = match stored.clone() {
            Some(run) => run,
            None => {
                let custom = custom_file(&saved.level);
                let game = &self.assets.game;
                let shipped = || {
                    game.resolve(&format!("Data\\Levels\\{}.edt", saved.level)).is_none()
                        && game.resolve(&format!("Data\\Levels\\{}.net", saved.level)).is_some()
                };
                if custom.is_none() && !shipped() {
                    return None;
                }
                SkirmishRun {
                    map: saved.level.clone(),
                    setup: Default::default(),
                    human: 0,
                    difficulty: Default::default(),
                    spectate: None,
                    default_setup: false,
                    custom,
                }
            }
        };
        // the maps folder may have moved since: find the custom map by its key
        if run.custom.as_ref().is_some_and(|p| !p.is_file()) {
            run.custom = custom_file(&run.map);
        }
        let script = self.skirmish_script(&run)?;
        if stored.is_none() {
            // what the lobby would have offered (`SkirmishMenu::select_map`)
            run.setup = if run.custom.is_some() { multiplayer::Setup::of_map(&script) } else { multiplayer::Setup::skirmish_default(&script) };
        }
        Some((run, script))
    }

    /// Starts a skirmish: the map's script with the lobby's choices written in and the sides arranged so the human is player 0.
    pub(super) fn start_skirmish(&mut self, run: SkirmishRun, script: opensow_formats::level::Level) {
        // a custom map: its own terrain (base game or Warmonger mission), additions and mission (docs/map-editor-ingame.md);
        // the human always plays blue there
        let loaded = run.custom.as_ref().and_then(|p| custom_maps::load(p).ok());
        let mission = loaded.as_ref().map(|m| m.mission);
        let reinforcement = loaded.as_ref().map(|m| m.reinforcement.clone());
        let escort = loaded.as_ref().map(|m| m.escort.clone());
        let human = if mission.is_some() { 0 } else { run.human };
        let (map, level) = (run.map.clone(), skirmish::prepare(&script, &run.setup, human));
        let custom = loaded.map(|m| (m.ground(), m.ext));
        match Play::new_skirmish(&mut self.assets, &map, level, &self.settings, run.difficulty, custom) {
            Ok(mut p) => {
                if let Some(m) = mission {
                    p.world.objective.destroy_all = m.win == opensow_formats::custommap::WinRule::Everything;
                    // an escort: the pods walk the waypoints, then to the goal
                    if let Some(goal) = escort.as_ref().and_then(|e| e.goal).filter(|_| m.win == opensow_formats::custommap::WinRule::Escort) {
                        let mut route = escort.as_ref().map(|e| e.route.clone()).unwrap_or_default();
                        route.push(goal);
                        p.world.set_convoy(route);
                    }
                }
                if let Some(r) = reinforcement {
                    p.world.reinforcement = Some(sim::Reinforcement {
                        credits: r.credits,
                        research: r.research,
                        air: r.air.map(u32::from),
                        drops: r.drops.iter().map(|d| (d.cell, usize::from(d.squad))).collect(),
                    });
                }
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

    /// Does what the skirmish screen asked for.
    pub(super) fn skirmish_event(&mut self, e: skirmish_ui::Event) {
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
            skirmish_ui::Event::Edit(path) => {
                let mut ed = mapeditor_ui::MapEditor::new(&mut self.assets);
                ed.set_ctrl(self.ctrl);
                match ed.open_from_skirmish(&path) {
                    Ok(()) => {
                        self.skirmish_menu = None;
                        self.map_editor = Some(ed);
                    }
                    Err(e) => eprintln!("cannot open {}: {e}", path.display()),
                }
            }
            skirmish_ui::Event::Start => {
                let Some(m) = self.skirmish_menu.take() else { return };
                let map = m.maps.get(m.map).cloned().unwrap_or_default();
                match m.map_level() {
                    Some(script) => self.start_skirmish(
                        SkirmishRun {
                            map,
                            setup: m.setup.clone(),
                            human: m.local,
                            difficulty: m.difficulty,
                            spectate: None,
                            default_setup: m.is_default(),
                            custom: m.custom_path(),
                        },
                        script,
                    ),
                    None => self.screen = Some(Screen::Message(format!("Cannot read the map {map}."))),
                }
            }
        }
    }

    /// Does what the multiplayer menu asked for.
    pub(super) fn net_event(&mut self, e: multiplayer::Event) {
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

    /// Opens the options screen over the current one.
    pub(super) fn open_options(&mut self) {
        if let Some(cur) = self.screen.take() {
            self.back.push(cur);
        }
        self.options_menu = Some(options::OptionsMenu::new(&mut self.assets, self.options.clone()));
    }

    /// Open the load or save page (state `0xc` / `0xd` in a mission, `0x11` from the main menu).
    pub(super) fn open_saveui(&mut self, mode: saveui::Mode) {
        if let Some(cur) = self.screen.take() {
            self.back.push(cur);
        }
        let level = self.play.as_ref().map(|p| p.level.clone()).unwrap_or_default();
        let store = saveui::Store::locate(&self.assets.game);
        let mut ui = saveui::SaveUi::new(&mut self.assets, mode, &level, store);
        if let (saveui::Mode::Save, Some(p)) = (mode, self.play.as_mut()) {
            p.progress = self.progress.clone();
            ui.snapshot = p.snapshot();
            ui.skirmish = self.skirmish_bytes();
        }
        self.saveui = Some(ui);
    }

    /// F5 in a mission (`0x416811`): grab the screen (`0x445070`) and save slot 0 with the title "Quick Save" (`0x41a9c0`), then the
    /// "game saved" speech (`0x428bb0`).
    pub(super) fn do_quick_save(&mut self, buf: &[u32], w: usize, h: usize) {
        let Some(p) = self.play.as_mut() else { return };
        p.progress = self.progress.clone();
        let (level, snapshot) = (p.level.clone(), p.snapshot());
        let thumb = opensow_formats::save::grab_thumbnail(buf, w, h);
        let store = saveui::Store::locate(&self.assets.game);
        let auto = std::mem::take(&mut self.quick_save_auto);
        match store.write(0, "Quick Save", &level, &snapshot, &self.skirmish_bytes(), &thumb) {
            Ok(()) if auto => {}
            Ok(()) => self.sfx.say(&self.assets.game, "01-game-saved.wav"),
            Err(e) => eprintln!("quick save failed: {e}"),
        }
    }

    /// Shift + F5 in a mission (`0x416850` -> `0x41a850(0)` -> `0x425d00`): load slot 0; nothing happens when it cannot be read.
    pub(super) fn quick_load(&mut self) {
        let store = saveui::Store::locate(&self.assets.game);
        if let Some(saved) = store.level_of(0) {
            self.saveui_event(saveui::Event::Load(saved));
        }
    }

    /// Play the sounds the load / save page asked for.
    pub(super) fn saveui_sounds(&mut self) {
        let ids: Vec<u8> = self.saveui.as_mut().map(|m| std::mem::take(&mut m.sounds)).unwrap_or_default();
        for id in ids {
            self.play_sfx(id);
        }
    }

    /// What the page decided. In a mission every way out resumes the game (`0x42c260(7)`); from the main menu CANCEL goes back.
    pub(super) fn saveui_event(&mut self, e: saveui::Event) {
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
            saveui::Event::Load(saved) => {
                self.saveui = None;
                self.ingame = None;
                self.back.clear();
                self.campaign = false;
                // a slot knows its campaign (docs/warmonger.md): its missions are only there while that edition is selected
                self.set_edition(saved.edition);
                if self.assets.game.edition != saved.edition {
                    self.act(Action::ToMain);
                    self.push(Screen::Message("This savegame belongs to Warmonger,\nwhich was not found.".into()));
                    return;
                }
                // the running game is replaced (a failed start must not leave the old one to take the snapshot)
                self.play = None;
                match self.saved_skirmish(&saved, &custom_maps::dir()) {
                    Some((run, script)) => self.start_skirmish(run, script),
                    None => self.start(&saved.level),
                }
                if !saved.snapshot.is_empty() {
                    if let Some(p) = self.play.as_mut() {
                        if let Err(e) = p.restore(&saved.snapshot) {
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

    /// Does what the options screen asked for.
    pub(super) fn options_event(&mut self, e: options::Event) {
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
    pub(super) fn options_sounds(&mut self) {
        let ids: Vec<u8> = self.options_menu.as_mut().map(|m| std::mem::take(&mut m.sounds)).unwrap_or_default();
        for id in ids {
            self.play_sfx(id);
        }
    }

    /// The banner or statistics screen decided something (`0x4357a0`, `0x44d770`).
    pub(super) fn results_event(&mut self, e: results::Event) {
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
}
