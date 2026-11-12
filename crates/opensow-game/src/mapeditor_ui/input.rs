//! Mouse, wheel, keyboard and text input of the editor, and what each widget does when pressed.

use super::*;

impl MapEditor {
    /// Lets go of the camera keys (the window lost the focus: their releases go elsewhere).
    pub fn release_keys(&mut self) {
        self.held = [false; 4];
    }

    /// The camera direction of a key: 0 left, 1 right, 2 up, 3 down (arrows, or W A S D without Ctrl/Cmd).
    pub(super) fn camera_key(&self, key: &winit::keyboard::Key) -> Option<usize> {
        use winit::keyboard::{Key, NamedKey};
        match key {
            Key::Named(NamedKey::ArrowLeft) => Some(0),
            Key::Named(NamedKey::ArrowRight) => Some(1),
            Key::Named(NamedKey::ArrowUp) => Some(2),
            Key::Named(NamedKey::ArrowDown) => Some(3),
            Key::Character(c) if !self.ctrl => ["a", "d", "w", "s"].iter().position(|k| c.eq_ignore_ascii_case(k)),
            _ => None,
        }
    }

    /// The action of the topmost widget at a window position.
    pub(super) fn hit(&self, x: i32, y: i32, w: usize, h: usize) -> Option<Hit> {
        self.widgets(w, h).into_iter().rev().find(|wd| wd.hit.is_some() && wd.r.contains(x, y)).and_then(|wd| wd.hit)
    }

    /// Window centre of a control (tests).
    #[cfg(test)]
    pub(super) fn centre_of(&self, hit: Hit, w: usize, h: usize) -> Option<(i32, i32)> {
        self.controls(w, h).into_iter().find(|(_, k)| *k == hit).map(|(r, _)| (r.x + r.w / 2, r.y + r.h / 2))
    }

    /// Every control with its window rectangle (tests).
    #[cfg(test)]
    pub(super) fn controls(&self, w: usize, h: usize) -> Vec<(Rect, Hit)> {
        self.widgets(w, h).into_iter().filter_map(|wd| wd.hit.map(|hit| (wd.r, hit))).collect()
    }

    /// Whether a window position is over the map area of the editor page.
    pub(super) fn in_map(&self, x: i32, y: i32, w: usize, h: usize) -> bool {
        self.s.is_some() && x >= 0 && y >= 0 && (x as usize) < map_width(w, h) && (y as usize) < h
    }

    /// Map pixel under window `(x, y)`.
    pub(super) fn map_px(&self, x: i32, y: i32) -> Option<(f32, f32)> {
        let s = self.s.as_ref()?;
        Some((s.view.x + x as f32 / s.view.zoom, s.view.y + y as f32 / s.view.zoom))
    }

    /// The map cell under a window position.
    pub(super) fn cell_at(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        self.map_px(x, y).map(|(mx, my)| ((mx / 32.0).floor() as i32, (my / 32.0).floor() as i32))
    }

    /// The pointer moved: hover, dragging a selection or scrolling the map.
    pub fn mouse_moved(&mut self, x: i32, y: i32, w: usize, h: usize) {
        self.cursor = (x, y);
        let cell = self.cell_at(x, y);
        if let Some(s) = self.s.as_mut() {
            match s.grab {
                Some(Grab::Pan(px, py)) => {
                    s.view.x -= (x - px) as f32 / s.view.zoom;
                    s.view.y -= (y - py) as f32 / s.view.zoom;
                    s.view.clamp(&s.scene, map_width(w, h), h);
                    s.grab = Some(Grab::Pan(x, y));
                }
                Some(Grab::Move(off, _)) => {
                    if let Some(c) = cell {
                        s.grab = Some(Grab::Move(off, (c.0 - off.0, c.1 - off.1)));
                    }
                }
                None => {}
            }
        }
        let hit = self.hit(x, y, w, h);
        if hit.is_some() && hit != self.hover && !matches!(hit, Some(Hit::Row(_) | Hit::Palette(_) | Hit::Terrain(_))) {
            self.sounds.push(30);
        }
        self.hover = hit;
    }

    /// A mouse button went down or up at a window position.
    pub fn mouse_button(&mut self, left: bool, down: bool, x: i32, y: i32, w: usize, h: usize) -> Event {
        if !left {
            // right button: stop placing, else deselect
            if down {
                if let Some(s) = self.s.as_mut() {
                    if s.tool.is_some() {
                        s.tool = None;
                    } else if s.ed.placing {
                        s.ed.placing = false;
                    } else {
                        s.ed.sel = None;
                        s.scroll = 0;
                    }
                }
            }
            return Event::None;
        }
        self.pressed = down;
        if !down {
            // the end of a drag: a move lands
            let tr = self.tr;
            if let Some(s) = self.s.as_mut() {
                if let Some(Grab::Move(_, at)) = s.grab.take() {
                    if s.ed.sel.is_some_and(|sel| s.ed.cell_of(sel) != at) {
                        let moved = s.ed.move_sel(at);
                        if moved {
                            self.refresh();
                        } else if let Some(s) = self.s.as_mut() {
                            s.message = Some((tr.cannot_place().into(), WARN));
                        }
                    }
                }
                if let Some(s) = self.s.as_mut() {
                    s.grab = None;
                }
            }
            return Event::None;
        }
        if let Some(hit) = self.hit(x, y, w, h) {
            return self.press(hit);
        }
        if let Some(s) = self.s.as_mut() {
            s.name.focused = false;
        }
        if self.in_map(x, y, w, h) {
            self.map_click(x, y);
        }
        Event::None
    }

    /// A click on the map: places the armed tool, sets a point that is being picked, or selects.
    pub(super) fn map_click(&mut self, x: i32, y: i32) {
        let tr = self.tr;
        let Some((mx, my)) = self.map_px(x, y) else { return };
        let cell = ((mx / 32.0).floor() as i32, (my / 32.0).floor() as i32);
        let Some(s) = self.s.as_mut() else { return };
        if let Some(tool @ (MapTool::Goal | MapTool::Waypoint)) = s.tool {
            // the convoy's goal (replaced) or the next waypoint: a cell a unit can stand on
            if s.ed.can_place(Thing::Unit(1), cell, None).is_err() {
                s.message = Some((tr.cannot_place().into(), WARN));
                return;
            }
            if tool == MapTool::Goal {
                s.escort.goal = Some(cell);
                s.tool = None;
            } else if s.escort.route.len() < custommap::MAX_WAYPOINTS {
                s.escort.route.push(cell);
                if s.escort.route.len() == custommap::MAX_WAYPOINTS {
                    s.tool = None;
                }
            }
            s.message = None;
            self.sounds.push(33);
            return;
        }
        if s.tool == Some(MapTool::Landing) {
            // a carryall landing point: a whole squad must land there, apart from the other points
            match landing_refusal(s, cell, None) {
                None if s.reinforcement.drops.len() < custommap::MAX_DROPS => {
                    // the new point brings the squad after the last one's (the first: squad 1)
                    let squad = s.reinforcement.drops.last().map_or(0, |d| (d.squad + 1) % custommap::SQUADS);
                    s.reinforcement.drops.push(custommap::Landing { cell, squad });
                    s.message = None;
                    self.sounds.push(33);
                }
                None => {}
                Some(why) => s.message = Some((tr.cannot_land(why).into(), WARN)),
            }
            if s.reinforcement.drops.len() >= custommap::MAX_DROPS {
                s.tool = None;
            }
            return;
        }
        if s.ed.placing {
            let thing = editor::palette()[s.ed.palette];
            if matches!(thing, Thing::Unit(_)) && s.ed.side == Side::Neutral {
                s.message = Some((tr.units_never_neutral().into(), WARN));
                return;
            }
            let at = Editor::add_cell(thing, cell);
            if s.ed.add(at) {
                // keep placing: the new object is not kept selected
                s.ed.sel = None;
                self.sounds.push(33);
                self.refresh();
            } else if let Some(s) = self.s.as_mut() {
                s.message = Some((tr.cannot_place().into(), WARN));
            }
            return;
        }
        match s.ed.pick(mx, my) {
            Some(sel) => {
                if s.ed.sel != Some(sel) {
                    s.scroll = 0;
                }
                s.ed.sel = Some(sel);
                let c = s.ed.cell_of(sel);
                s.grab = Some(Grab::Move((cell.0 - c.0, cell.1 - c.1), c));
            }
            None => {
                s.ed.sel = None;
                s.scroll = 0;
                s.grab = Some(Grab::Pan(x, y));
            }
        }
    }

    /// The wheel: zoom the map around the pointer, scroll the lists and the sidebar.
    pub fn wheel(&mut self, dy: f32, w: usize, h: usize) {
        let (x, y) = self.cursor;
        let mw = map_width(w, h);
        let in_map = self.in_map(x, y, w, h);
        match self.s.as_mut() {
            Some(s) if in_map => {
                // 8 % a wheel notch (a trackpad's small deltas zoom by their share), eased in over a few frames by `update`
                let from = s.zoom_to.map_or(s.view.zoom, |(z, _)| z);
                let target = (from * ZOOM_STEP.powf(dy.clamp(-4.0, 4.0))).clamp(fit_zoom(&s.scene, mw, h), MAX_ZOOM);
                s.zoom_to = Some((target, (x, y)));
            }
            Some(s) => {
                s.scroll = (s.scroll - (dy * 40.0) as i32).max(0);
                // clamp to the content: measure it
                let max = self.sidebar_overflow(w, h);
                if let Some(s) = self.s.as_mut() {
                    s.scroll = s.scroll.min(max);
                }
            }
            None => {
                let rows = start_rows(&Geo::for_window(w, h));
                let max = self.maps.len().saturating_sub(rows);
                self.list_scroll = (self.list_scroll as i32 - dy.round() as i32).clamp(0, max as i32) as usize;
            }
        }
    }

    /// How far the selection's card reaches below the window (design pixels).
    pub(super) fn sidebar_overflow(&self, w: usize, h: usize) -> i32 {
        let Some(s) = self.s.as_ref() else { return 0 };
        self.widgets(w, h)
            .iter()
            .find_map(|wd| match wd.look {
                Look::Scroll(_, view, total) => Some(total - view),
                _ => None,
            })
            .map_or(if s.scroll > 0 { s.scroll } else { 0 }, |v| v.max(0))
    }

    /// Typed text (composed by the window: dead keys, input methods) for the name field.
    pub fn text_input(&mut self, text: &str) {
        if let Some(s) = self.s.as_mut().filter(|s| s.name.focused) {
            if s.name.insert(text) {
                s.overwrite_armed = None;
            }
        }
    }

    /// Control or Command held, from the window's modifier state (`WindowEvent::ModifiersChanged`, cleared when the window loses
    /// the focus). Counting the keys' own presses and releases went wrong when a release went to another window (Cmd+Tab,
    /// Cmd+Shift+4): the editor then took every letter for a shortcut and W A S D did nothing.
    pub fn set_ctrl(&mut self, held: bool) {
        self.ctrl = held;
    }

    /// A key; Ctrl/Cmd shortcuts use [`MapEditor::set_ctrl`].
    pub fn key(&mut self, key: &winit::keyboard::Key, down: bool, w: usize, h: usize) -> Event {
        use winit::keyboard::{Key, NamedKey};
        let _ = (w, h);
        // camera keys: held while down (a release always counts, also of a key pressed before the name field took the keyboard)
        if let Some(k) = self.camera_key(key).filter(|_| self.s.is_some()) {
            if !down || !self.s.as_ref().is_some_and(|s| s.name.focused) {
                self.held[k] = down;
                if !self.s.as_ref().is_some_and(|s| s.name.focused) {
                    return Event::None;
                }
            }
        }
        if !down {
            return Event::None;
        }
        let focused = self.s.as_ref().is_some_and(|s| s.name.focused);
        if focused {
            let s = self.s.as_mut().unwrap();
            let edit = match key {
                Key::Named(NamedKey::Backspace) => Some(textinput::Edit::Backspace),
                Key::Named(NamedKey::Delete) => Some(textinput::Edit::Delete),
                Key::Named(NamedKey::ArrowLeft) => Some(textinput::Edit::Left),
                Key::Named(NamedKey::ArrowRight) => Some(textinput::Edit::Right),
                Key::Named(NamedKey::Home) => Some(textinput::Edit::Home),
                Key::Named(NamedKey::End) => Some(textinput::Edit::End),
                Key::Named(NamedKey::Enter | NamedKey::Escape | NamedKey::Tab) => {
                    s.name.focused = false;
                    None
                }
                _ => None,
            };
            if let Some(e) = edit {
                if s.name.edit(e) {
                    s.overwrite_armed = None;
                }
            }
            return Event::None;
        }
        match key {
            Key::Named(NamedKey::Escape) => {
                if let Some(s) = self.s.as_mut() {
                    if s.tool.is_some() {
                        s.tool = None;
                        return Event::None;
                    }
                    if s.ed.placing {
                        s.ed.placing = false;
                        return Event::None;
                    }
                    if s.ed.sel.is_some() {
                        s.ed.sel = None;
                        s.scroll = 0;
                        return Event::None;
                    }
                }
                self.press(Hit::Back)
            }
            Key::Named(NamedKey::Delete | NamedKey::Backspace) if self.s.as_ref().is_some_and(|s| s.ed.sel.is_some()) => self.press(Hit::Delete),
            Key::Named(NamedKey::Enter) if self.s.is_none() => self.press(Hit::Create),
            Key::Character(c) if !self.ctrl && c.eq_ignore_ascii_case("b") && self.s.is_some() => self.press(Hit::BuildAreas),
            Key::Character(c) if self.ctrl => {
                match c.to_lowercase().as_str() {
                    "z" => {
                        self.press(Hit::Undo);
                    }
                    "y" => {
                        self.press(Hit::Redo);
                    }
                    "s" => {
                        self.press(Hit::Save);
                    }
                    _ => {}
                }
                Event::None
            }
            _ => Event::None,
        }
    }

    /// Does what a widget does when pressed.
    pub fn press(&mut self, hit: Hit) -> Event {
        let tr = self.tr;
        // the start page
        if self.s.is_none() {
            match hit {
                Hit::Row(i) => {
                    self.list_sel = Some(i);
                    self.delete_armed = false;
                    self.sounds.push(39);
                }
                Hit::DeleteMap => {
                    self.sounds.push(39);
                    if !self.delete_armed {
                        self.delete_armed = true;
                    } else {
                        self.delete_armed = false;
                        if let Some(p) = self.list_sel.and_then(|i| self.maps.get(i)).map(|e| e.path.clone()) {
                            if let Err(e) = std::fs::remove_file(&p) {
                                eprintln!("cannot delete {}: {e}", p.display());
                            }
                            self.rescan();
                        }
                    }
                }
                Hit::OpenMap => {
                    if let Some(p) = self.list_sel.and_then(|i| self.maps.get(i)).map(|e| e.path.clone()) {
                        self.sounds.push(39);
                        if let Err(e) = self.open_file(&p) {
                            eprintln!("cannot open {}: {e}", p.display());
                        }
                    }
                }
                Hit::OpenFolder => {
                    self.sounds.push(39);
                    if !cfg!(test) {
                        custom_maps::open_folder();
                    }
                }
                Hit::Edition(wm) => {
                    self.warmonger_tab = wm;
                    if let Some(&first) = self.shown_terrains().first() {
                        self.terrain = first;
                    }
                    self.sounds.push(33);
                }
                Hit::Terrain(i) => {
                    self.terrain = i;
                    self.sounds.push(33);
                }
                Hit::Create => {
                    self.sounds.push(38);
                    if let Err(e) = self.create(self.terrain) {
                        eprintln!("cannot create a map: {e}");
                    }
                }
                Hit::Back => {
                    self.sounds.push(39);
                    return Event::Leave;
                }
                _ => {}
            }
            return Event::None;
        }
        let s = self.s.as_mut().unwrap();
        let mut changed = false;
        let mut sound = 33;
        if hit != Hit::Name {
            s.name.focused = false;
        }
        if hit != Hit::Back {
            s.leave_armed = false;
        }
        let lv = s.ed.level().clone();
        let side_values = |side: usize| Sides::read(&lv, side);
        let write_sides = |ed: &mut Editor, side: usize, v: Sides| {
            let mut l = ed.level().clone();
            v.write(&mut l, side);
            ed.set_header(l.header)
        };
        match hit {
            Hit::Name => {
                s.name.focused = true;
                s.name.caret = s.name.text.chars().count();
                sound = 39;
            }
            Hit::Save => {
                sound = 38;
                let _ = self.save();
                self.sounds.push(sound);
                return Event::None;
            }
            Hit::Undo => changed = s.ed.undo(),
            Hit::Redo => changed = s.ed.redo(),
            Hit::Back => {
                sound = 39;
                if s.dirty() && !s.leave_armed {
                    s.leave_armed = true;
                    s.message = Some((tr.leave_unsaved().into(), WARN));
                    self.sounds.push(sound);
                    return Event::None;
                }
                self.s = None;
                self.rescan();
                self.sounds.push(sound);
                if std::mem::take(&mut self.from_skirmish) {
                    return Event::Skirmish;
                }
                return Event::None;
            }
            Hit::Content(k) => {
                s.content = k;
                s.scroll = 0;
                s.tool = None;
            }
            Hit::RCredits(plus) => {
                let v = &mut s.reinforcement.credits;
                let new = (*v + if plus { 500 } else { -500 }).clamp(0, custommap::MAX_REINFORCEMENT);
                changed = std::mem::replace(v, new) != new;
            }
            Hit::RResearch(plus) => {
                let v = &mut s.reinforcement.research;
                let new = (*v + if plus { 500 } else { -500 }).clamp(0, custommap::MAX_REINFORCEMENT);
                changed = std::mem::replace(v, new) != new;
            }
            Hit::RAir(k, plus) => {
                let v = &mut s.reinforcement.air[AIR_GROUP[k.min(4)]];
                let new = if plus { (*v + 1).min(custommap::MAX_REINFORCEMENT_AIR) } else { v.saturating_sub(1) };
                changed = std::mem::replace(v, new) != new;
            }
            Hit::DropAdd | Hit::GoalSet | Hit::WaypointAdd => {
                let (tool, text) = match hit {
                    Hit::DropAdd => (MapTool::Landing, tr.click_landing()),
                    Hit::GoalSet => (MapTool::Goal, tr.click_goal()),
                    _ => (MapTool::Waypoint, tr.click_waypoint()),
                };
                s.tool = if s.tool == Some(tool) { None } else { Some(tool) };
                s.ed.placing = false;
                if s.tool.is_some() {
                    s.message = Some((text.into(), page::GOLD));
                }
            }
            Hit::WaypointRemove => changed = s.escort.route.pop().is_some(),
            Hit::DropSquad(k, plus) => {
                if let Some(d) = s.reinforcement.drops.get_mut(k) {
                    let n = custommap::SQUADS;
                    d.squad = if plus { (d.squad + 1) % n } else { (d.squad + n - 1) % n };
                    changed = true;
                }
            }
            Hit::DropDelete(k) => {
                if k < s.reinforcement.drops.len() {
                    s.reinforcement.drops.remove(k);
                    changed = true;
                }
            }
            Hit::WinRule(k) => {
                let win = [custommap::WinRule::Headquarters, custommap::WinRule::Everything, custommap::WinRule::Escort][k.min(2)];
                if win != custommap::WinRule::Escort && matches!(s.tool, Some(MapTool::Goal | MapTool::Waypoint)) {
                    s.tool = None;
                }
                changed = s.mission.win != win;
                s.mission.win = win;
            }
            Hit::Countdown(_) | Hit::Minutes(_) => {
                use custommap::{Timer, TimerEnd};
                let old = Timer::read(s.ed.level());
                let minutes = old.map_or(DEFAULT_MINUTES, |t| t.minutes);
                let new = match hit {
                    Hit::Countdown(0) => None,
                    Hit::Countdown(k) => Some(Timer { minutes, end: [TimerEnd::Victory, TimerEnd::Defeat, TimerEnd::Reinforcements][(k - 1).min(2)] }),
                    // whole steps of five (a file's odd value snaps to the next one)
                    Hit::Minutes(plus) => old.map(|t| Timer {
                        minutes: if plus { (t.minutes / MINUTE_STEP + 1) * MINUTE_STEP } else { (t.minutes - 1) / MINUTE_STEP * MINUTE_STEP }
                            .clamp(MINUTE_STEP, MAX_MINUTES),
                        ..t
                    }),
                    _ => old,
                };
                let mut level = s.ed.level().clone();
                Timer::write(new, &mut level);
                changed = s.ed.set_header(level.header);
                if countdown_mode(new) != 3 && s.tool == Some(MapTool::Landing) {
                    s.tool = None;
                }
                s.scroll = 0;
            }
            Hit::BuildAreas => s.areas = !s.areas,
            Hit::Owner(k) => s.ed.side = SIDES3[k],
            Hit::Tab(k) => {
                s.tab = k;
                s.ed.placing = false;
            }
            Hit::Palette(i) => {
                let idx = editor::palette().iter().position(|&t| t == items()[i]).unwrap_or(0);
                if s.ed.placing && s.ed.palette == idx {
                    s.ed.placing = false;
                } else {
                    s.ed.palette = idx;
                    s.ed.placing = true;
                    s.tool = None;
                    s.ed.sel = None;
                    // units and headquarters belong to a side
                    if s.ed.side == Side::Neutral && (matches!(items()[i], Thing::Unit(_)) || items()[i] == Thing::Object(100)) {
                        s.ed.side = Side::Blue;
                    }
                }
            }
            Hit::Credits(side, plus) => {
                let mut v = side_values(side);
                v.credits = (v.credits + if plus { 500 } else { -500 }).clamp(0, MAX_CREDITS);
                changed = write_sides(&mut s.ed, side, v);
            }
            Hit::Research(side, plus) => {
                let mut v = side_values(side);
                v.research = (v.research + if plus { 100 } else { -100 }).clamp(0, MAX_RESEARCH);
                changed = write_sides(&mut s.ed, side, v);
            }
            Hit::Air(row, side, plus) => {
                let mut v = side_values(side);
                let g = AIR_GROUP[row];
                v.air[g] = (v.air[g] + if plus { 1 } else { -1 }).clamp(0, MAX_AIR);
                changed = write_sides(&mut s.ed, side, v);
            }
            Hit::Turret(t, side) => {
                let mut v = side_values(side);
                v.turrets[t] = !v.turrets[t];
                changed = write_sides(&mut s.ed, side, v);
            }
            Hit::Done => {
                s.ed.sel = None;
                s.scroll = 0;
                sound = 39;
            }
            Hit::Delete => {
                // an escort map may leave blue without a headquarters (its convoy is the game)
                let keep: &[i32] = if s.mission.win == custommap::WinRule::Escort { &[1] } else { &[0, 1] };
                changed = s.ed.remove_sel_keeping(keep);
                if !changed {
                    s.message = Some((tr.last_headquarters().into(), WARN));
                }
                s.scroll = 0;
            }
            Hit::Faction(k) => {
                changed = s.ed.set_owner(SIDES3[k]);
                if !changed && s.ed.sel.is_some_and(|sel| s.ed.side_of(sel) != SIDES3[k]) {
                    s.message = Some((tr.last_headquarters().into(), WARN));
                }
            }
            Hit::Hp(plus) => {
                if let Some(Sel::Object(i)) = s.ed.sel {
                    let v = lv.objects[i].raw[18];
                    changed = s.ed.set_start_hp(i, v + if plus { 5 } else { -5 });
                }
            }
            Hit::HpDefault => {
                if let Some(Sel::Object(i)) = s.ed.sel {
                    changed = s.ed.set_start_hp(i, if lv.objects[i].raw[14] == 2 { 50 } else { 100 });
                }
            }
            Hit::TurretType(tier) => changed = s.ed.set_turret_type(tier),
            Hit::Satellite(on) => {
                if let Some(Sel::Object(i)) = s.ed.sel {
                    changed = s.ed.set_protected(i, on);
                }
            }
            Hit::HqAdvancer(on) => {
                if let Some(Sel::Object(i)) = s.ed.sel {
                    let mut ext = s.ed.ext.clone();
                    if let Some(e) = ext.get_mut(i) {
                        e.no_advancer = !on;
                    }
                    changed = s.ed.set_ext(ext);
                }
            }
            Hit::LineKind(k, up) => changed = s.ed.change_lines(LineChange::Kind(k, up)),
            Hit::LineTiers(k) => changed = s.ed.change_lines(LineChange::Tiers(k)),
            Hit::LineLock(k) => changed = s.ed.change_lines(LineChange::Lock(k)),
            Hit::LineRemove(k) => {
                changed = match s.ed.lines() {
                    Some((Lines::Levels(_), _)) => s.ed.change_lines(LineChange::Levels(false)),
                    _ => s.ed.change_lines(LineChange::Remove(k)),
                }
            }
            Hit::AddLine => {
                changed = match s.ed.lines() {
                    Some((Lines::Levels(_), _)) => s.ed.change_lines(LineChange::Levels(true)),
                    _ => s.ed.change_lines(LineChange::Add),
                }
            }
            Hit::LineUnlock(k, plus) => {
                if let (Some(Sel::Object(i)), Some((_, p))) = (s.ed.sel, s.ed.lines()) {
                    let kind = p.lines.get(k).map_or(7, |l| l.0);
                    let default = opensow_formats::production::slot_target(kind).unwrap_or(2000) as i32;
                    let cur = s.ed.ext.get(i).map_or(0, |e| e.unlock[k.min(custommap::LINES - 1)]);
                    let cur = if cur > 0 { cur } else { default };
                    let next = (cur + if plus { 500 } else { -500 }).clamp(500, 20000);
                    changed = s.ed.set_unlock(k, if next == default { 0 } else { next });
                }
            }
            _ => {}
        }
        self.sounds.push(sound);
        if changed {
            self.refresh();
        }
        Event::None
    }
}
