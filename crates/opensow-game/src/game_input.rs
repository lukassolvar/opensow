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
    pub(super) fn digit_of(&self, key: &Key) -> Option<u8> {
        self.digit.or_else(|| match key {
            Key::Character(c) if c.len() == 1 && c.as_bytes()[0].is_ascii_digit() => Some(c.as_bytes()[0] - b'0'),
            _ => None,
        })
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
            if down && matches!(key, Key::Named(NamedKey::Escape)) {
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
        self.hotkey(key, w, h);
    }

    /// A key press that no open screen, movie or chat line took: the hotkeys of the menus and of a running mission
    /// (docs/hotkeys.md).
    fn hotkey(&mut self, key: &Key, w: usize, h: usize) {
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
            Key::Named(NamedKey::F2) if self.screen.is_none() && self.play.as_ref().is_some_and(|p| p.world.mission_no.is_some_and(|n| n != 0)) => {
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
            // Shift+H (not in the original): the next idle Advancer, and the view goes to it
            Key::Character(c) if self.screen.is_none() && self.shift && c.eq_ignore_ascii_case("h") => {
                if let Some(p) = self.play.as_mut() {
                    p.select_next_idle_advancer();
                }
            }
            Key::Character(c) if self.screen.is_none() && (c.eq_ignore_ascii_case("h") || (self.numpad && c.as_str() == "+")) => {
                if let Some(p) = self.play.as_mut() {
                    p.select_next_advancer();
                }
            }
            Key::Character(c) if self.screen.is_none() && self.numpad && matches!(c.as_str(), "*" | "/" | "-") => {
                if let Some(p) = self.play.as_mut() {
                    p.select_visible(match c.as_str() {
                        "/" => Some(true),
                        "-" => Some(false),
                        _ => None,
                    });
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
            Key::Named(NamedKey::F3 | NamedKey::F4) if self.screen.is_none() && self.play.is_some() => {
                let step = if *key == Key::Named(NamedKey::F3) { -0.5 } else { 0.5 };
                self.game_speed = (self.game_speed + step).clamp(0.5, 3.0);
                let speed = self.game_speed;
                if let Some(p) = self.play.as_mut() {
                    p.show_speed(speed);
                }
            }
            // F1 (`0x415bda`): sound 0x20 and the key help overlay on / off (`[0x5baa9b]`, drawn by `0x420a40`); the FPS counter
            // is the "draw fps" cheat's own flag `[0x5baad5]`
            Key::Named(NamedKey::F1) if self.screen.is_none() && self.play.is_some() => {
                if let Some(p) = self.play.as_mut() {
                    p.show_keys ^= true;
                }
                self.play_sfx(0x20);
            }
            // X (`0x4167e1` -> `0x418dc0`): the selected units scatter away from the middle of their box (docs/hotkeys.md)
            Key::Character(c) if self.screen.is_none() && c.eq_ignore_ascii_case("x") => {
                if let Some(p) = self.play.as_mut() {
                    p.world.scatter_selected();
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
