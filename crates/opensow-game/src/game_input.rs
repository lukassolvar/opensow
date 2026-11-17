//! Mouse, keyboard and text input, routed to the screen that is open.

use super::*;

impl Game {
    /// Whether typed text goes to a text field (the map editor's name): the window then enables the input method too.
    pub(super) fn wants_text(&self) -> bool {
        self.map_editor.as_ref().is_some_and(|m| m.wants_text())
    }

    /// Control or Command held (the window's modifier state).
    pub(super) fn set_ctrl(&mut self, held: bool) {
        self.ctrl = held;
        if let Some(m) = self.map_editor.as_mut() {
            m.set_ctrl(held);
        }
    }

    /// Typed text goes to the screen that has a text field open.
    pub(super) fn text_input(&mut self, text: &str) {
        if let Some(m) = self.map_editor.as_mut() {
            m.text_input(text);
        }
    }

    /// The pointer moved: passes it to the open screen or the mission.
    pub(super) fn mouse_moved(&mut self, x: i32, y: i32, w: usize, h: usize) {
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
        if let Some(m) = self.map_editor.as_mut() {
            m.mouse_moved(x, y, w, h);
            self.map_editor_event(mapeditor_ui::Event::None);
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

    /// A mouse button went down or up: passes it to the open screen or the mission.
    pub(super) fn mouse_button(&mut self, button: MouseButton, down: bool, x: i32, y: i32, w: usize, h: usize) {
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
            if down && m.capturing() && !matches!(button, MouseButton::Left | MouseButton::Right) {
                // a Controls slot waits: the middle, back, forward and other buttons are bound like keys
                m.capture(button);
                self.options_sounds();
            } else if down && button == MouseButton::Left {
                let e = m.click(x, y, w, h);
                self.options_sounds();
                self.apply_audio();
                self.options_event(e);
            } else if down && button == MouseButton::Right {
                m.right_click(x, y, w, h);
                self.options_sounds();
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
        if let Some(m) = self.map_editor.as_mut() {
            if matches!(button, MouseButton::Left | MouseButton::Right) {
                let e = m.mouse_button(button == MouseButton::Left, down, x, y, w, h);
                self.map_editor_event(e);
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
                    if let Some(released_on) = hit.filter(|_| hit == self.pressed) {
                        let a = buttons[released_on].action.clone();
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
        if !matches!(button, MouseButton::Left | MouseButton::Right) {
            // the other buttons do what they are bound to (Options, Controls; not in the exe)
            self.bound_input(keybinds::Input::Mouse(button), down);
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

    /// The physical key of the event being handled: the window's, else mapped from the logical key (tests, headless runs).
    pub(super) fn code_of(&self, key: &Key) -> Option<winit::keyboard::KeyCode> {
        self.code.or_else(|| keybinds::code_of(key, self.numpad, self.digit))
    }

    /// The mission action the key is bound to (Options, Controls pages; docs/hotkeys.md).
    pub(super) fn action_of(&self, key: &Key) -> Option<keybinds::Act> {
        self.code_of(key).and_then(|c| self.options.keys.action(c))
    }

    /// Whether this key or button press toggles fullscreen: the one bound to it, unless a text line or the Controls page is
    /// waiting for a key.
    pub(super) fn fullscreen_key(&self, code: impl Into<keybinds::Input>) -> bool {
        let typing = self.saveui.is_some()
            || self.net_menu.is_some()
            || self.play.as_ref().is_some_and(|p| p.chat.is_some())
            || self.wants_text()
            || self.options_menu.as_ref().is_some_and(|m| m.capturing());
        !typing && self.options.keys.action(code) == Some(keybinds::Act::Fullscreen)
    }

    /// A key went down or up: global keys first, then the open screen or the mission.
    pub(super) fn key(&mut self, key: &Key, down: bool, w: usize, h: usize) {
        if down && self.intro.is_none() {
            let typing = self.saveui.is_some() || self.net_menu.is_some() || self.play.as_ref().is_some_and(|p| p.chat.is_some()) || self.wants_text();
            match key {
                Key::Named(NamedKey::F12) if crate::DEV => {
                    self.dev.toggle_visible();
                    return;
                }
                Key::Character(c) if crate::DEV && c.as_str() == "`" && !typing => {
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
                let e = if matches!(key, Key::Named(NamedKey::Escape)) || l.playing_video() { l.back() } else { lands::Event::None };
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
            if down && m.capturing() {
                // the Controls page waits for a key: Esc gives up, any other key is bound (or refused)
                if matches!(key, Key::Named(NamedKey::Escape)) {
                    m.stop_capture();
                } else if let Some(c) = self.code.or_else(|| keybinds::code_of(key, self.numpad, self.digit)) {
                    m.capture(c);
                }
                self.options_sounds();
            } else if down && matches!(key, Key::Named(NamedKey::Escape)) {
                let e = m.escape();
                self.options_sounds();
                self.options_event(e);
            }
            return;
        }
        if let Some(m) = self.map_editor.as_mut() {
            let e = m.key(key, down, w, h);
            self.map_editor_event(e);
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
            if down && self.action_of(key) == Some(keybinds::Act::Pause) {
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
        self.scroll_input(self.action_of(key), down);
        if let Key::Named(NamedKey::Shift) = key {
            self.shift = down;
        }
        if !down {
            return;
        }
        self.hotkey(key, w, h);
    }

    /// A key press that no open screen, movie or chat line took: Esc, then the mission's hotkeys through the key bindings
    /// (docs/hotkeys.md; Options, Controls pages).
    fn hotkey(&mut self, key: &Key, w: usize, h: usize) {
        match key {
            Key::Named(NamedKey::Escape) => {
                match self.screen {
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
                }
                return;
            }
            // developer shortcut on the menus (fixed, not a mission key)
            Key::Character(c) if c.eq_ignore_ascii_case("p") && matches!(self.screen, Some(Screen::Main | Screen::Pause)) => {
                self.act(Action::Prototype);
                return;
            }
            _ => {}
        }
        if self.screen.is_some() {
            return;
        }
        if let Some(act) = self.action_of(key) {
            self.run_action(act);
        }
        let _ = (w, h);
    }

    /// A held scroll action (keys or buttons) starts or stops moving the camera.
    fn scroll_input(&mut self, act: Option<keybinds::Act>, down: bool) {
        use keybinds::Act;
        let name = match act {
            Some(Act::ScrollLeft) => "left",
            Some(Act::ScrollRight) => "right",
            Some(Act::ScrollUp) => "up",
            Some(Act::ScrollDown) => "down",
            _ => return,
        };
        if let Some(p) = self.play.as_mut() {
            p.key(name, down);
        }
    }

    /// A bound mouse button (middle, back, forward, others) in a mission: the same as its action's key, with the pause and
    /// chat rules of the keys (docs/hotkeys.md).
    pub(super) fn bound_input(&mut self, input: keybinds::Input, down: bool) {
        if self.screen.is_some() || self.results.is_some() {
            return;
        }
        let act = self.options.keys.action(input);
        let Some(p) = self.play.as_mut() else { return };
        if p.chat.is_some() {
            return;
        }
        if p.paused {
            // the pause waits only for the pause action
            if down && act == Some(keybinds::Act::Pause) {
                p.paused = false;
            }
            return;
        }
        self.scroll_input(act, down);
        if let (true, Some(act)) = (down, act) {
            self.run_action(act);
        }
    }

    /// What a mission action does (docs/hotkeys.md), whichever key or button it is bound to.
    fn run_action(&mut self, act: keybinds::Act) {
        use keybinds::Act;
        if self.screen.is_some() {
            return;
        }
        let shift = self.shift;
        match act {
            // quick save / quick load: key flag `[player+0xa3]` = scan code 0x3f = F5, with a shift key (`[+0x8e]` / `[+0x9a]`) it loads
            Act::QuickSave if self.play.is_some() => {
                if shift {
                    self.quick_load();
                } else {
                    self.quick_save = true;
                }
            }
            // F2 (`0x415c05`, key flag `[player+0xa0]`): in a numbered mission without remote players it plays sound 0x32 and leaves for the
            // campaign map (state 0x1a = `0x433d90`, "after a mission") with the mission number as the result code `[0x5baa90]`
            Act::Abandon if self.play.as_ref().is_some_and(|p| p.world.mission_no.is_some_and(|n| n != 0)) => {
                self.play_sfx(0x32);
                self.play = None;
                self.open_lands(false);
            }
            // F3 / F4 (`0x415c49` / `0x415cb1`): game speed -/+ 0.5 between 0.5 and 3.0, not in a network game
            Act::SpeedDown | Act::SpeedUp if self.play.is_some() => {
                let step = if act == Act::SpeedDown { -0.5 } else { 0.5 };
                self.game_speed = (self.game_speed + step).clamp(0.5, 3.0);
                let speed = self.game_speed;
                if let Some(p) = self.play.as_mut() {
                    p.show_speed(speed);
                }
            }
            // F1 (`0x415bda`): sound 0x20 and the key help overlay on / off (`[0x5baa9b]`, drawn by `0x420a40`); the FPS counter
            // is the "draw fps" cheat's own flag `[0x5baad5]`
            Act::KeyHelp if self.play.is_some() => {
                if let Some(p) = self.play.as_mut() {
                    p.show_keys ^= true;
                }
                self.play_sfx(0x20);
            }
            Act::SelectAll => {
                if let Some(p) = self.play.as_mut() {
                    p.world.select_all();
                    p.sel_building = None;
                    let ids = p.select_all_sounds();
                    for id in ids {
                        self.play_sfx(id);
                    }
                }
            }
            _ => {
                let Some(p) = self.play.as_mut() else { return };
                match act {
                    // P pauses (`0x415d05`: `[player+0x7d]`, scan code 0x19; docs/hotkeys.md)
                    Act::Pause => p.paused ^= true,
                    // Space and numpad 0 (`0x416177`): the view goes to the position of the last message event
                    Act::LookAtEvent => p.look_at_event(),
                    Act::NextUnit => p.select_next_unit(),
                    Act::LookAtHq => p.look_at_hq(),
                    Act::Chat => p.chat_open(),
                    Act::CentreSelection => p.centre_on_selection(),
                    // Shift+H (not in the original): the next idle Advancer, and the view goes to it
                    Act::NextAdvancer if shift => p.select_next_idle_advancer(),
                    Act::NextAdvancer => p.select_next_advancer(),
                    Act::SelectVisible => p.select_visible(None),
                    Act::SelectVisibleAntiAir => p.select_visible(Some(true)),
                    Act::SelectVisibleGround => p.select_visible(Some(false)),
                    // numpad 1-9: camera bookmarks (shift sets)
                    Act::Bookmark(k) => {
                        p.bookmark(k as usize, shift);
                    }
                    // X (`0x4167e1` -> `0x418dc0`): the selected units scatter away from the middle of their box (docs/hotkeys.md)
                    Act::Scatter => {
                        p.world.scatter_selected();
                    }
                    Act::Turret(k) => {
                        p.turret_key(k as u32 + 1);
                    }
                    Act::StrikeBomber | Act::StrikeAsteroid | Act::StrikeCarryall | Act::StrikeTripler | Act::StrikeFighter => {
                        use opensow_formats::air::AirForce as F;
                        let f = match act {
                            Act::StrikeBomber => F::Bomber,
                            Act::StrikeAsteroid => F::Asteroid,
                            Act::StrikeCarryall => F::Carryall,
                            Act::StrikeTripler => F::Tripler,
                            _ => F::Fighter,
                        };
                        if p.world.strike_lock == 0 {
                            p.strike = Some(f);
                        }
                    }
                    Act::TechUp => {
                        p.world.tech_up_all(0);
                    }
                    // `0x416769`: R with a selected own factory arms the rally point mode, else it toggles the Advancer's repair mode
                    Act::RallyOrRepair if !p.arm_rally() => p.world.repair_mode ^= true,
                    _ => {}
                }
            }
        }
    }
}
