//! Where everything on the editor's two pages sits: the list of widgets (buttons, steppers, cards) for the current window size and state.

use super::*;

impl MapEditor {
    /// Every widget of the page that is open, for a `w` x `h` window.
    pub(super) fn widgets(&self, w: usize, h: usize) -> Vec<W> {
        match &self.s {
            None => self.start_widgets(w, h),
            Some(s) => self.edit_widgets(s, w, h),
        }
    }

    /// The start page: the player's maps (left), the terrain picker of a new map (right), BACK and CREATE.
    pub(super) fn start_widgets(&self, w: usize, h: usize) -> Vec<W> {
        let g = Geo::for_window(w, h);
        let t = T::new(g.d, w, h);
        let tr = self.tr;
        let mut out: Vec<W> = Vec::new();
        let mut push = |r: Rect, hit: Option<Hit>, look: Look| out.push(W { r, hit, look });
        let (list, grid, back, create, row_h, hh, cols) = if g.wide {
            (rect(60, 128, 420, 800), rect(510, 128, 1350, 800), rect(60, 982, 220, 64), rect(1540, 976, 320, 76), 50, 58, 7)
        } else {
            (rect(16, 72, 300, 608), rect(332, 72, 676, 608), rect(16, 704, 150, 50), rect(824, 700, 184, 58), 40, 44, 6)
        };
        // your maps
        push(t.r(list), None, Look::Panel(tr.your_maps().into(), t.len(hh), t.px(g.head), t.len(g.pad)));
        let foot = row_h + g.pad;
        let foot2 = foot + row_h;
        let rows = start_rows(&g);
        for row in 0..rows {
            let i = self.list_scroll + row;
            let Some(e) = self.maps.get(i) else { break };
            let r = t.r(rect(list.x + 10, list.y + hh + row as i32 * row_h, list.w - 20, row_h - 4));
            push(r, Some(Hit::Row(i)), Look::Row(self.list_sel == Some(i)));
            let col = if e.blocker().is_some() { WARN } else { TEXT };
            push(Rect { x: r.x + t.len(12), y: r.y, w: r.w - t.len(24), h: r.h }, None, Look::Text(e.name.clone(), t.px(g.body - 2), col));
        }
        if self.maps.len() > rows {
            push(
                t.r(rect(list.x + list.w - 8, list.y + hh, 4, rows as i32 * row_h)),
                None,
                Look::Scroll(self.list_scroll as i32, rows as i32, self.maps.len() as i32),
            );
        }
        if self.maps.is_empty() {
            push(t.r(rect(list.x + g.pad, list.y + hh + 6, list.w - 2 * g.pad, row_h * 3)), None, Look::Text(tr.no_maps_yet().into(), t.px(g.small), MUTED));
        }
        let bw = (list.w - 2 * g.pad - 8) / 2;
        let by = list.y + list.h - foot;
        let by2 = list.y + list.h - foot2;
        let sel = self.list_sel.and_then(|i| self.maps.get(i));
        if sel.is_some_and(|e| e.map.is_ok()) {
            push(t.r(rect(list.x + g.pad, by2, bw, row_h - 6)), Some(Hit::OpenMap), Look::Button(tr.open().into(), t.px(g.small + 2), false));
        }
        if sel.is_some() {
            let label = if self.delete_armed { tr.delete_sure() } else { tr.delete() };
            push(t.r(rect(list.x + g.pad + bw + 8, by2, bw, row_h - 6)), Some(Hit::DeleteMap), Look::Button(label.into(), t.px(g.small), self.delete_armed));
        }
        push(t.r(rect(list.x + g.pad, by, list.w - 2 * g.pad, row_h - 6)), Some(Hit::OpenFolder), Look::Button(tr.open_folder().into(), t.px(g.small), false));
        // new map: the edition tabs and the terrains
        push(t.r(grid), None, Look::Panel(tr.new_map().into(), t.len(hh), t.px(g.head), t.len(g.pad)));
        if self.has_warmonger() {
            let tw = 200.min(grid.w / 4);
            for (k, wm) in [false, true].into_iter().enumerate() {
                let r = t.r(rect(grid.x + grid.w - g.pad - 2 * tw - 8 + k as i32 * (tw + 8), grid.y + 8, tw, hh - 16));
                push(
                    r,
                    Some(Hit::Edition(wm)),
                    Look::Chip(if wm { "Warmonger" } else { "State of War" }.into(), t.px(g.small), self.warmonger_tab == wm, ACCENT),
                );
            }
        }
        let gap = 12;
        let tile_w = (grid.w - 2 * g.pad - (cols - 1) * gap) / cols;
        let pic_h = tile_w * 7 / 10;
        let tile_h = pic_h + g.small + 12;
        let shown = self.shown_terrains();
        for k in 0..shown.len() {
            let (col, row) = (k as i32 % cols, k as i32 / cols);
            let r = rect(grid.x + g.pad + col * (tile_w + gap), grid.y + hh + 6 + row * (tile_h + gap), tile_w, tile_h);
            if r.y + r.h > grid.y + grid.h - g.pad {
                break;
            }
            let pic = t.r(rect(r.x, r.y, r.w, pic_h));
            let label = t.r(rect(r.x, r.y + pic_h + 4, r.w, g.small + 6));
            let i = shown[k];
            push(t.r(r), Some(Hit::Terrain(i)), Look::Card(self.terrain == i));
            push(pic, None, Look::Pic(PicRef::Thumb(i), false));
            push(
                label,
                None,
                Look::TextCentred(tr.mission(self.terrains[i].mission), t.px(g.small), if self.terrain == i { (255, 255, 255) } else { TEXT }),
            );
        }
        // bottom bar: BACK, the chosen terrain's facts, CREATE
        push(t.r(back), Some(Hit::Back), Look::Button(tr.back().into(), t.px(g.body + 2), false));
        if let Some(th) = self.thumbs.get(self.terrain).and_then(|t| t.as_ref()) {
            let tt = self.terrains[self.terrain];
            let fact = format!("{} · {} × {}{}", tr.mission(tt.mission), th.full_w / 32, th.full_h / 32, if tt.warmonger { " · Warmonger" } else { "" });
            let x = back.x + back.w + 30;
            push(t.r(rect(x, back.y, create.x - x - 20, back.h)), None, Look::TextRight(fact, t.px(g.body - 2), TEXT));
        }
        push(t.r(create), Some(Hit::Create), Look::Button(tr.create().into(), t.px(if g.wide { 34 } else { 24 }), true));
        out
    }

    /// The editor page's sidebar (the map takes the rest of the window).
    pub(super) fn edit_widgets(&self, s: &Session, w: usize, h: usize) -> Vec<W> {
        let g = Geo::for_window(w, h);
        let t = side_t(&g, w, h);
        let tr = self.tr;
        let design_h = (h as f32 / t.s) as i32;
        let mut l = L { out: Vec::new(), t: &t, g, x0: g.pad, inner: g.side - 2 * g.pad, y: 10 };
        // on the map's top-left corner: the build-areas switch (the map's own control, shown whatever the sidebar shows)
        let px = t.px(g.small + 1);
        let label = tr.build_areas();
        let bw = self.ink.width(label, px) + t.len(28);
        let mw = map_width(w, h) as i32;
        if bw + t.len(16) < mw {
            l.push(Rect { x: t.len(10), y: t.len(10), w: bw, h: t.len(g.body + 14) }, Some(Hit::BuildAreas), Look::Chip(label.into(), px, s.areas, page::WARN));
        }
        // the panel behind everything
        l.push(t.r(rect(0, 0, g.side, design_h + 2)), None, Look::Panel(String::new(), 8, 10, 8));
        // title and save state
        let dirty = s.dirty();
        let (state, scol) = if s.saved.is_empty() {
            (tr.state_new(), WARN)
        } else if dirty {
            (tr.state_unsaved(), WARN)
        } else {
            (tr.state_saved(), ACCENT)
        };
        let r = l.r(l.x0, l.y, l.inner, g.head + 6);
        l.push(r, None, Look::Text(tr.editor_title().into(), l.px(g.head), page::GOLD));
        l.push(r, None, Look::TextRight(state.into(), l.px(g.small), scol));
        l.y += g.head + 14;
        // name field and the file it saves to
        let fh = g.body + 18;
        let r = l.r(l.x0, l.y, l.inner, fh);
        l.push(r, Some(Hit::Name), Look::Field);
        l.y += fh + 4;
        let file = format!("{}.{}", custommap::file_stem(&s.name.text), custommap::EXTENSION);
        let r = l.r(l.x0, l.y, l.inner, g.small + 6);
        l.push(r, None, Look::Text(tr.saves_as(&file), l.px(g.small - 1), MUTED));
        l.y += g.small + 12;
        // Save, Undo, Redo, Back
        let bh = g.body + 20;
        let weights = [36, 14, 14, 30];
        let total: i32 = weights.iter().sum();
        let mut x = l.x0;
        for (k, wt) in weights.iter().enumerate() {
            let bw = (l.inner - 18) * wt / total;
            let r = l.r(x, l.y, bw, bh);
            match k {
                0 => l.push(r, Some(Hit::Save), Look::Button(tr.save().into(), l.px(g.body), true)),
                1 => l.push(r, s.ed.can_undo().then_some(Hit::Undo), Look::Glyph(Glyph::Undo, if s.ed.can_undo() { ACCENT } else { (70, 90, 74) })),
                2 => l.push(r, s.ed.can_redo().then_some(Hit::Redo), Look::Glyph(Glyph::Redo, if s.ed.can_redo() { ACCENT } else { (70, 90, 74) })),
                _ => l.push(r, Some(Hit::Back), Look::Button(capitalised(&tr.back().to_lowercase()), l.px(g.body), false)),
            }
            x += bw + 6;
        }
        l.y += bh + 6;
        // the message line
        if let Some((m, col)) = &s.message {
            let r = l.r(l.x0, l.y, l.inner, g.small + 6);
            l.push(r, None, Look::Text(m.clone(), l.px(g.small), *col));
        }
        l.y += g.small + 12;
        let top = l.y;
        match s.ed.sel {
            Some(sel) => {
                l.y -= s.scroll;
                let first = l.out.len();
                self.selection(&mut l, s, sel);
                // keep only what starts inside the content area (the card's frame stays); the scroll bar tells there is more
                let bottom = l.y + s.scroll;
                let limit = t.y(top) - 1;
                let card = l.out.split_off(first);
                l.out.extend(card.into_iter().enumerate().filter(|(k, wd)| *k == 0 || wd.r.y >= limit).map(|(_, wd)| wd));
                if bottom > design_h {
                    let r = t.r(rect(g.side - 6, top, 4, design_h - top - 4));
                    l.push(r, None, Look::Scroll(s.scroll, design_h - top, bottom - top));
                }
            }
            None => {
                let place_h = self.place_height(&l, s);
                let sides_h = self.sides_height(&l);
                let checks_h = self.checks_height(&l, s);
                let mission_h = self.mission_height(&l, s);
                let chips_h = g.body + 22;
                let mut scrolled = None;
                let all = top + place_h + sides_h + mission_h + checks_h + 4 * g.pad <= design_h;
                // the palette and the sides together with the mission behind a chip, when those two fit
                let pair = top + chips_h + place_h + sides_h + checks_h + 3 * g.pad <= design_h;
                if all {
                    self.place(&mut l, s);
                    self.sides(&mut l, s);
                    self.mission(&mut l, s);
                } else if pair {
                    let both = format!("{} · {}", tr.place(), tr.sides());
                    for (k, r) in l.cells(2, l.y, g.body + 14, 6).into_iter().enumerate() {
                        let (label, hit, on) = if k == 0 {
                            (both.clone(), Hit::Content(0), s.content != 2)
                        } else {
                            (tr.mission_card().to_string(), Hit::Content(2), s.content == 2)
                        };
                        l.push(r, Some(hit), Look::Chip(label, l.px(g.small + 1), on, ACCENT));
                    }
                    l.y += chips_h;
                    if s.content == 2 {
                        scrolled = Some((l.out.len(), l.y));
                        l.y -= s.scroll;
                        self.mission(&mut l, s);
                    } else {
                        self.place(&mut l, s);
                        self.sides(&mut l, s);
                    }
                } else {
                    let cells = l.cells(3, l.y, g.body + 14, 6);
                    for (k, r) in cells.into_iter().enumerate() {
                        let label = [tr.place(), tr.sides(), tr.mission_card()][k];
                        l.push(r, Some(Hit::Content(k)), Look::Chip(label.into(), l.px(g.small + 1), s.content == k, ACCENT));
                    }
                    l.y += chips_h;
                    match s.content {
                        0 => self.place(&mut l, s),
                        1 => self.sides(&mut l, s),
                        _ => {
                            scrolled = Some((l.out.len(), l.y));
                            l.y -= s.scroll;
                            self.mission(&mut l, s);
                        }
                    }
                }
                self.checks(&mut l, s);
                // the mission card alone may be taller than the window: it scrolls like a selection's card
                if let Some((first, top)) = scrolled {
                    let bottom = l.y + s.scroll;
                    let limit = t.y(top) - 1;
                    let card = l.out.split_off(first);
                    l.out.extend(card.into_iter().enumerate().filter(|(k, wd)| *k == 0 || wd.r.y >= limit).map(|(_, wd)| wd));
                    if bottom > design_h {
                        let r = t.r(rect(g.side - 6, top, 4, design_h - top - 4));
                        l.push(r, None, Look::Scroll(s.scroll, design_h - top, bottom - top));
                    }
                }
            }
        }
        l.out
    }

    /// Height of the palette card.
    pub(super) fn place_height(&self, l: &L, s: &Session) -> i32 {
        let g = l.g;
        let (cols, cell) = self.grid(l);
        let n = items().iter().filter(|&&t| tab_of(t) == s.tab).count() as i32;
        let rows = (n + cols - 1) / cols;
        // units: the tallest tab, so the cards below do not jump between tabs
        let rows = rows.max((21 + cols - 1) / cols);
        42 + 2 * (g.body + 20) + rows * (cell + 6) + g.pad
    }

    /// Columns and cell size of the palette grid.
    pub(super) fn grid(&self, l: &L) -> (i32, i32) {
        let cols = if l.g.wide { 7 } else { 6 };
        (cols, (l.inner - 16 - (cols - 1) * 6) / cols)
    }

    /// The palette card: owner, tab, the pictures.
    pub(super) fn place(&self, l: &mut L, s: &Session) {
        let g = l.g;
        let tr = self.tr;
        let card_top = l.y;
        let h = self.place_height(l, s);
        let r = l.r(l.x0, card_top, l.inner, h);
        l.push(r, None, Look::Panel(tr.place().into(), l.t.len(42), l.px(g.head), l.t.len(g.pad)));
        l.y += 42;
        let x0 = l.x0;
        l.x0 += 8;
        l.inner -= 16;
        let ch = g.body + 14;
        let owner = side_index(s.ed.side);
        for (k, r) in l.cells(3, l.y, ch, 6).into_iter().enumerate() {
            let col = if k == 0 { NEUTRAL } else { SIDE[k - 1] };
            l.push(r, Some(Hit::Owner(k)), Look::Chip(tr.owner(k).into(), l.px(g.small + 1), owner == k, col));
        }
        l.y += ch + 6;
        for (k, r) in l.cells(3, l.y, ch, 6).into_iter().enumerate() {
            l.push(r, Some(Hit::Tab(k)), Look::Chip(tr.palette_tab(k).into(), l.px(g.small + 1), s.tab == k, ACCENT));
        }
        l.y += ch + 8;
        let (cols, cell) = self.grid(l);
        let all = items();
        let armed = s.ed.placing.then(|| editor::palette()[s.ed.palette]);
        for (k, (i, &t)) in all.iter().enumerate().filter(|(_, &t)| tab_of(t) == s.tab).enumerate() {
            let (c, row) = (k as i32 % cols, k as i32 / cols);
            let r = l.r(l.x0 + c * (cell + 6), l.y + row * (cell + 6), cell, cell);
            l.push(r, Some(Hit::Palette(i)), Look::Card(armed == Some(t)));
            let pic = Rect { x: r.x + 3, y: r.y + 3, w: r.w - 6, h: r.h - 6 };
            // units and headquarters are never neutral: their pictures show blue then
            let side = if owner == 0 && (matches!(t, Thing::Unit(_)) || t == Thing::Object(100)) { 1 } else { owner };
            l.push(pic, None, Look::Pic(PicRef::Palette(i, side), false));
        }
        l.x0 = x0;
        l.inner += 16;
        l.y = card_top + h + g.pad;
    }

    /// Height of the starting-resources card.
    pub(super) fn sides_height(&self, l: &L) -> i32 {
        let g = l.g;
        let row = g.body + 18;
        42 + 3 * row + 2 * (g.small + 10) + (44 + 2 * (g.small + 12) + 10) + (40 + g.small + 14) + g.pad
    }

    /// Both sides' starting resources, Advancers, air forces and turret tiers.
    pub(super) fn sides(&self, l: &mut L, s: &Session) {
        let g = l.g;
        let tr = self.tr;
        let lv = s.ed.level();
        let sd = [Sides::read(lv, 0), Sides::read(lv, 1)];
        let card_top = l.y;
        let h = self.sides_height(l);
        let r = l.r(l.x0, card_top, l.inner, h);
        l.push(r, None, Look::Panel(tr.sides().into(), l.t.len(42), l.px(g.head), l.t.len(g.pad)));
        let colw = l.inner * 26 / 100;
        let colx = [l.x0 + l.inner * 47 / 100, l.x0 + l.inner * 47 / 100 + colw + 6];
        for side in 0..2 {
            let r = l.r(colx[side], card_top + 8, colw, 26);
            l.push(r, None, Look::TextCentred(tr.owner(side + 1).into(), l.px(g.small), SIDE[side]));
        }
        l.y += 42;
        let row = g.body + 18;
        let adv = [custommap::advancers(lv, &s.ed.ext, 0), custommap::advancers(lv, &s.ed.ext, 1)];
        let hqs = [custommap::headquarters(lv, 0).len(), custommap::headquarters(lv, 1).len()];
        // credits and research are steppers; the Advancers are only counted (a headquarters' own switch sets them)
        // label, picture, both sides' values, and the stepper's action (side, up) when the row can be changed
        type Row = (String, PicRef, [String; 2], Option<fn(usize, bool) -> Hit>);
        let rows: [Row; 3] = [
            (capitalised(&self.string(CREDITS)), PicRef::Gold, [sd[0].credits.to_string(), sd[1].credits.to_string()], Some(Hit::Credits)),
            (capitalised(&self.string(RESEARCH)), PicRef::Bulb, [sd[0].research.to_string(), sd[1].research.to_string()], Some(Hit::Research)),
            (self.string(ADVANCER), PicRef::Advancer, [format!("{}/{}", adv[0], hqs[0]), format!("{}/{}", adv[1], hqs[1])], None),
        ];
        for (label, pic, values, hit) in rows {
            let x = l.x0 + 8;
            let r = l.r(x, l.y + 4, 34, row - 8);
            l.push(r, None, Look::Pic(pic, false));
            let r = l.r(x + 40, l.y, colx[0] - x - 44, row);
            l.push(r, None, Look::Text(label, l.px(g.small + 1), TEXT));
            for side in 0..2 {
                let r = l.r(colx[side], l.y + 3, colw, row - 6);
                match hit {
                    Some(hit) => l.stepper(r, values[side].clone(), SIDE[side], hit(side, false), hit(side, true)),
                    None => l.push(r, None, Look::TextCentred(values[side].clone(), l.px(g.body), (255, 255, 255))),
                }
            }
            l.y += row;
        }
        // air forces: a picture and a stepper per side
        l.y += 4;
        let x0 = l.x0;
        l.x0 += 8;
        l.inner -= 16;
        l.label(&capitalised(&tr.air_forces().to_lowercase()));
        let ch = 44 + 2 * (g.small + 12);
        for (k, r) in l.cells(5, l.y, ch, 6).into_iter().enumerate() {
            l.push(r, None, Look::Card(false));
            let pic = Rect { x: r.x + 3, y: r.y + 3, w: r.w - 6, h: l.t.len(40) };
            l.push(pic, None, Look::Pic(PicRef::Air(k), false));
            for side in 0..2 {
                let y = l.y + 44 + side as i32 * (g.small + 12);
                let rr = Rect { x: r.x + 2, y: l.t.y(y), w: r.w - 4, h: l.t.len(g.small + 10) };
                let v = sd[side].air[AIR_GROUP[k]];
                let (a, b) = page::stepper_arrows(rr);
                l.push(rr, None, Look::Stepper(v.to_string(), l.px(g.small), SIDE[side], Hit::Air(k, side, false), Hit::Air(k, side, true)));
                l.push(a, Some(Hit::Air(k, side, false)), Look::Card(false));
                l.push(b, Some(Hit::Air(k, side, true)), Look::Card(false));
            }
        }
        l.y += ch + 10;
        l.label(&capitalised(&tr.turrets().to_lowercase()));
        let ch = 40 + g.small + 8;
        for (k, r) in l.cells(5, l.y, ch, 6).into_iter().enumerate() {
            l.push(r, None, Look::Card(false));
            let pic = Rect { x: r.x + 3, y: r.y + 2, w: r.w - 6, h: l.t.len(38) };
            l.push(pic, None, Look::Pic(PicRef::Turret(k), false));
            let y = l.t.y(l.y + 42);
            let half = (r.w - 6) / 2;
            for side in 0..2 {
                let tr_ = Rect { x: r.x + 2 + side as i32 * (half + 2), y, w: half, h: l.t.len(g.small + 4) };
                l.push(tr_, Some(Hit::Turret(k, side)), Look::Toggle(sd[side].turrets[k], SIDE[side]));
            }
        }
        l.x0 = x0;
        l.inner += 16;
        l.y = card_top + h + g.pad;
    }

    /// Height of the mission card.
    pub(super) fn mission_height(&self, l: &L, s: &Session) -> i32 {
        let g = l.g;
        let (ch, lab) = (g.body + 14, g.small + 8);
        let drops = s.reinforcement.drops.len() as i32 * DROP_ROW;
        let reinforce = if countdown_mode(custommap::Timer::read(s.ed.level())) == 3 {
            lab + 2 * (g.body + 18) + (44 + g.small + 12 + 10) + lab + drops + ch + 8
        } else {
            0
        };
        let escort = if s.mission.win == custommap::WinRule::Escort { (ch + 4) + (ch + 8) + lab + ch + 8 } else { 0 };
        42 + (lab + 2 * (ch + 6) + 2) + (ch + 8) + reinforce + (lab + ch + 8) + escort + g.pad
    }

    /// The mission: the countdown, what it brings and what wins (docs/map-editor-ingame.md "Missions").
    pub(super) fn mission(&self, l: &mut L, s: &Session) {
        let g = l.g;
        let tr = self.tr;
        let card_top = l.y;
        let h = self.mission_height(l, s);
        let r = l.r(l.x0, card_top, l.inner, h);
        l.push(r, None, Look::Panel(tr.mission_card().into(), l.t.len(42), l.px(g.head), l.t.len(g.pad)));
        l.y += 42;
        let x0 = l.x0;
        l.x0 += 8;
        l.inner -= 16;
        let ch = g.body + 14;
        let timer = custommap::Timer::read(s.ed.level());
        l.label(tr.countdown());
        let mode = countdown_mode(timer);
        for row in 0..2 {
            for (k, r) in l.cells(2, l.y, ch, 6).into_iter().enumerate() {
                let k = row * 2 + k;
                l.push(r, Some(Hit::Countdown(k)), Look::Chip(tr.countdown_mode(k).into(), l.px(g.small + 1), mode == k, ACCENT));
            }
            l.y += ch + 6;
        }
        l.y += 2;
        // the minutes: the label left, the stepper right
        let half = l.inner / 2;
        let r = l.r(l.x0, l.y, half - 6, ch);
        l.push(r, None, Look::Text(tr.minutes().into(), l.px(g.small + 1), if timer.is_some() { TEXT } else { MUTED }));
        let r = l.r(l.x0 + half, l.y, l.inner - half, ch);
        match timer {
            Some(t) => l.stepper(r, t.minutes.to_string(), ACCENT, Hit::Minutes(false), Hit::Minutes(true)),
            None => l.push(r, None, Look::TextCentred("-".into(), l.px(g.body), MUTED)),
        }
        l.y += ch + 8;
        if mode == 3 {
            let f = &s.reinforcement;
            l.label(tr.arrives());
            let row = g.body + 18;
            // label, picture, value, and the stepper's action (up)
            type Row = (String, PicRef, i32, fn(bool) -> Hit);
            let rows: [Row; 2] = [
                (capitalised(&self.string(CREDITS)), PicRef::Gold, f.credits, Hit::RCredits),
                (capitalised(&self.string(RESEARCH)), PicRef::Bulb, f.research, Hit::RResearch),
            ];
            for (label, pic, value, hit) in rows {
                let r = l.r(l.x0, l.y + 4, 34, row - 8);
                l.push(r, None, Look::Pic(pic, false));
                let r = l.r(l.x0 + 40, l.y, half - 46, row);
                l.push(r, None, Look::Text(label, l.px(g.small + 1), TEXT));
                let r = l.r(l.x0 + half, l.y + 3, l.inner - half, row - 6);
                l.stepper(r, value.to_string(), page::GOLD, hit(false), hit(true));
                l.y += row;
            }
            let cards = 44 + g.small + 12;
            for (k, r) in l.cells(5, l.y, cards, 6).into_iter().enumerate() {
                l.push(r, None, Look::Card(false));
                let pic = Rect { x: r.x + 3, y: r.y + 3, w: r.w - 6, h: l.t.len(40) };
                l.push(pic, None, Look::Pic(PicRef::Air(k), false));
                let rr = Rect { x: r.x + 2, y: l.t.y(l.y + 44), w: r.w - 4, h: l.t.len(g.small + 10) };
                let (a, b) = page::stepper_arrows(rr);
                l.push(rr, None, Look::Stepper(f.air[AIR_GROUP[k]].to_string(), l.px(g.small), page::GOLD, Hit::RAir(k, false), Hit::RAir(k, true)));
                l.push(a, Some(Hit::RAir(k, false)), Look::Card(false));
                l.push(b, Some(Hit::RAir(k, true)), Look::Card(false));
            }
            l.y += cards + 10;
            l.label(&tr.landings(f.drops.len(), custommap::MAX_DROPS));
            // one row per landing point: number and cell (orange when no whole squad can land), the squad, delete; under it
            // the squad's units as their pictures with counts
            let units = items();
            for (k, d) in f.drops.iter().enumerate() {
                let bad = landing_issue(s, k).is_some();
                let top = l.y;
                l.push(l.r(l.x0, top, l.inner, DROP_ROW - 6), None, Look::Card(false));
                let r = l.r(l.x0 + 8, top + 4, l.inner * 30 / 100, ch);
                l.push(r, None, Look::Text(format!("{}  ({}, {})", k + 1, d.cell.0, d.cell.1), l.px(g.small + 1), if bad { WARN } else { TEXT }));
                let del = 28;
                let r = l.r(l.x0 + l.inner * 34 / 100, top + 4, l.inner * 66 / 100 - del - 14, ch);
                l.stepper(r, tr.squad(usize::from(d.squad) + 1), page::GOLD, Hit::DropSquad(k, false), Hit::DropSquad(k, true));
                let r = l.r(l.x0 + l.inner - del - 6, top + 4 + (ch - del) / 2, del, del);
                l.push(r, Some(Hit::DropDelete(k)), Look::Glyph(Glyph::Cross, (220, 120, 100)));
                let mut counts: Vec<(i32, usize)> = Vec::new();
                for (_, _, kind) in opensow_formats::aircraft::squad(usize::from(d.squad)).unwrap_or_default() {
                    match counts.iter_mut().find(|c| c.0 == kind) {
                        Some(c) => c.1 += 1,
                        None => counts.push((kind, 1)),
                    }
                }
                let (pw, y) = (34, top + ch + 6);
                let mut x = l.x0 + 8;
                for (kind, n) in counts {
                    if let Some(i) = units.iter().position(|&t| t == Thing::Unit(kind)) {
                        l.push(l.r(x, y, pw, pw), None, Look::Pic(PicRef::Palette(i, 1), false));
                    }
                    l.push(l.r(x + pw + 2, y, 34, pw), None, Look::Text(format!("×{n}"), l.px(g.small), TEXT));
                    x += pw + 40;
                }
                l.y += DROP_ROW;
            }
            let r = l.r(l.x0, l.y, l.inner / 2 - 3, ch);
            let free = f.drops.len() < custommap::MAX_DROPS;
            l.push(r, free.then_some(Hit::DropAdd), Look::Chip(tr.add_on_map().into(), l.px(g.small + 1), s.tool == Some(MapTool::Landing), page::GOLD));
            l.y += ch + 8;
        }
        l.label(tr.to_win());
        for (k, r) in l.cells(3, l.y, ch, 6).into_iter().enumerate() {
            let on = win_index(s.mission.win) == k;
            l.push(r, Some(Hit::WinRule(k)), Look::Chip(tr.win_rule(k).into(), l.px(g.small + 1), on, ACCENT));
        }
        if s.mission.win == custommap::WinRule::Escort {
            l.y += ch + 8;
            // the convoy: the blue pods placed on the map
            let n = custommap::pods(s.ed.level(), 0);
            let half = l.inner / 2;
            let r = l.r(l.x0, l.y, half - 6, ch);
            l.push(r, None, Look::Text(tr.convoy().into(), l.px(g.small + 1), TEXT));
            let r = l.r(l.x0 + half, l.y, l.inner - half, ch);
            let (text, col) = if n == 0 { (tr.no_pods_hint().to_string(), WARN) } else { (format!("{n} × {}", self.string(POD_NAME)), TEXT) };
            l.push(r, None, Look::Text(text, l.px(g.small), col));
            l.y += ch + 4;
            // the goal: its cell and the tool that sets it
            let goal = s.escort.goal.map_or("-".to_string(), |(x, y)| format!("({x}, {y})"));
            let r = l.r(l.x0, l.y, half - 6, ch);
            l.push(r, None, Look::Text(format!("{}  {goal}", tr.goal()), l.px(g.small + 1), if s.escort.goal.is_some() { TEXT } else { WARN }));
            let r = l.r(l.x0 + half, l.y, l.inner - half, ch);
            l.push(r, Some(Hit::GoalSet), Look::Chip(tr.set_on_map().into(), l.px(g.small + 1), s.tool == Some(MapTool::Goal), ACCENT));
            l.y += ch + 8;
            l.label(&tr.route(s.escort.route.len(), custommap::MAX_WAYPOINTS));
            for (k, r) in l.cells(2, l.y, ch, 6).into_iter().enumerate() {
                if k == 0 {
                    let free = s.escort.route.len() < custommap::MAX_WAYPOINTS;
                    l.push(
                        r,
                        free.then_some(Hit::WaypointAdd),
                        Look::Chip(tr.add_on_map().into(), l.px(g.small + 1), s.tool == Some(MapTool::Waypoint), ACCENT),
                    );
                } else {
                    l.push(r, (!s.escort.route.is_empty()).then_some(Hit::WaypointRemove), Look::Button(tr.remove_last().into(), l.px(g.small + 1), false));
                }
            }
        }
        l.x0 = x0;
        l.inner += 16;
        l.y = card_top + h + g.pad;
    }

    /// Height of the checks card (one line per problem found).
    pub(super) fn checks_height(&self, l: &L, s: &Session) -> i32 {
        let n = (s.map().problems().len() + landing_issues(s).len() + escort_issues(s).len()).max(1) as i32;
        n * (l.g.small + 8) + 18
    }

    /// What keeps the map from a match (or that it is ready).
    pub(super) fn checks(&self, l: &mut L, s: &Session) {
        let g = l.g;
        let problems = s.map().problems();
        let landings = landing_issues(s);
        let route = escort_issues(s);
        let h = self.checks_height(l, s);
        let r = l.r(l.x0, l.y, l.inner, h);
        l.push(r, None, Look::Card(false));
        let mut y = l.y + 9;
        // a landing point an edit has covered (a building placed on it, a point loaded from a hand-made file): warning only
        for (k, why) in &landings {
            let r = l.r(l.x0 + 10, y, l.inner - 20, g.small + 6);
            l.push(r, None, Look::Text(format!("! {}", self.tr.landing_problem(k + 1, *why)), l.px(g.small), (220, 200, 120)));
            y += g.small + 8;
        }
        for leg in &route {
            let r = l.r(l.x0 + 10, y, l.inner - 20, g.small + 6);
            l.push(r, None, Look::Text(format!("! {}", self.tr.route_problem(*leg)), l.px(g.small), (220, 200, 120)));
            y += g.small + 8;
        }
        if problems.is_empty() && landings.is_empty() && route.is_empty() {
            let r = l.r(l.x0 + 10, y, l.inner - 20, g.small + 6);
            l.push(r, None, Look::Text(format!("✓ {}", self.tr.ready()), l.px(g.small), ACCENT));
        }
        for p in problems {
            let r = l.r(l.x0 + 10, y, l.inner - 20, g.small + 6);
            let col = if p.blocks_play() { WARN } else { (220, 200, 120) };
            l.push(r, None, Look::Text(format!("! {}", self.tr.problem(p)), l.px(g.small), col));
            y += g.small + 8;
        }
        l.y += h + g.pad;
    }

    /// The selected object's card.
    pub(super) fn selection(&self, l: &mut L, s: &Session, sel: Sel) {
        let g = l.g;
        let tr = self.tr;
        let thing = s.ed.thing_of(sel);
        let side = side_index(s.ed.side_of(sel));
        let (cx, cy) = s.ed.cell_of(sel);
        let card_top = l.y;
        // the card's frame is drawn first with a height fixed at the end
        let frame_index = l.out.len();
        l.push(l.r(l.x0, card_top, l.inner, 10), None, Look::Panel(String::new(), 8, 10, 8));
        l.y += 10;
        let pic_i = items().iter().position(|&t| t == thing);
        if let Some(i) = pic_i {
            let r = l.r(l.x0 + 8, l.y, 70, 60);
            l.push(r, None, Look::Pic(PicRef::Palette(i, if side == 0 && matches!(thing, Thing::Unit(_)) { 1 } else { side }), false));
        } else if thing == Thing::Unit(40) {
            let r = l.r(l.x0 + 8, l.y, 70, 60);
            l.push(r, None, Look::Pic(PicRef::Advancer, false));
        }
        let r = l.r(l.x0 + 86, l.y + 4, l.inner - 94, g.body + 6);
        l.push(r, None, Look::Text(self.thing_name(thing), l.px(g.body), page::GOLD));
        let r = l.r(l.x0 + 86, l.y + g.body + 12, l.inner - 94, g.small + 6);
        l.push(r, None, Look::Text(tr.cell(cx, cy), l.px(g.small), MUTED));
        l.y += 66;
        let x0 = l.x0;
        l.x0 += 8;
        l.inner -= 16;
        let bh = g.body + 14;
        let cells = l.cells(2, l.y, bh, 8);
        l.push(cells[0], Some(Hit::Delete), Look::Button(tr.delete().into(), l.px(g.small + 1), false));
        l.push(cells[1], Some(Hit::Done), Look::Button(tr.done().into(), l.px(g.small + 1), false));
        l.y += bh + 10;
        // faction
        l.label(tr.faction());
        let only_sides = matches!(sel, Sel::Unit(_)) || thing == Thing::Object(100);
        let owners: Vec<usize> = if only_sides { vec![1, 2] } else { vec![0, 1, 2] };
        for (r, k) in l.cells(owners.len() as i32, l.y, bh, 6).into_iter().zip(owners) {
            let col = if k == 0 { NEUTRAL } else { SIDE[k - 1] };
            l.push(r, Some(Hit::Faction(k)), Look::Chip(tr.owner(k).into(), l.px(g.small + 1), side == k, col));
        }
        l.y += bh + 10;
        if let Sel::Object(i) = sel {
            let o = &s.ed.level().objects[i];
            // start health
            l.label(tr.start_health());
            let max = if kinds::TURRETS.contains(&o.kind) {
                [900, 1250, 1540, 2100, 2600][(o.kind - 25) as usize]
            } else {
                opensow_formats::advancer::building_max_hp(o.kind)
            };
            let pct = o.raw[18];
            let r = l.r(l.x0, l.y, l.inner * 40 / 100, bh);
            l.stepper(r, format!("{pct}%"), TEXT, Hit::Hp(false), Hit::Hp(true));
            let r = l.r(l.x0 + l.inner * 42 / 100, l.y, l.inner * 28 / 100, bh);
            l.push(r, None, Look::Text(format!("{} hp", max * pct / 100), l.px(g.small), MUTED));
            let default = if o.raw[14] == 2 { 50 } else { 100 };
            if pct != default {
                let r = l.r(l.x0 + l.inner * 70 / 100, l.y, l.inner * 30 / 100, bh);
                l.push(r, Some(Hit::HpDefault), Look::Button(tr.default_value().into(), l.px(g.small), false));
            }
            l.y += bh + 10;
            if kinds::TURRETS.contains(&o.kind) {
                l.label(tr.turret_type());
                let ch = 52;
                for (k, r) in l.cells(5, l.y, ch, 6).into_iter().enumerate() {
                    l.push(r, Some(Hit::TurretType(k as i32 + 1)), Look::Card(o.kind == 25 + k as i32));
                    l.push(Rect { x: r.x + 3, y: r.y + 3, w: r.w - 6, h: r.h - 6 }, None, Look::Pic(PicRef::Turret(k), false));
                }
                l.y += ch + 10;
            }
            // satellite protection (language string 145); a headquarters always has it
            if o.kind != kinds::HEADQUARTERS {
                l.label(&self.string(SATELLITE));
                let on = o.raw[15] != 0;
                for (k, r) in l.cells(2, l.y, bh, 6).into_iter().enumerate() {
                    let v = k == 0;
                    l.push(r, Some(Hit::Satellite(v)), Look::Chip(tr.on_off(v).into(), l.px(g.small + 1), on == v, ACCENT));
                }
                l.y += bh + 10;
            }
            if o.kind == kinds::HEADQUARTERS {
                l.label(&self.string(ADVANCER).to_uppercase());
                let on = !s.ed.ext.get(i).is_some_and(|e| e.no_advancer);
                for (k, r) in l.cells(2, l.y, bh, 6).into_iter().enumerate() {
                    let v = k == 0;
                    l.push(r, Some(Hit::HqAdvancer(v)), Look::Chip(tr.on_off(v).into(), l.px(g.small + 1), on == v, ACCENT));
                }
                l.y += bh + 10;
            }
            if let Some((family, p)) = s.ed.lines() {
                self.lines(l, s, i, family, &p);
            }
        }
        l.x0 = x0;
        l.inner += 16;
        l.y += 4;
        let hgt = l.y - card_top;
        l.out[frame_index].r = l.r(l.x0, card_top, l.inner, hgt);
        l.y += g.pad;
    }

    /// A building's production lines (or a mine's, lab's, windtrap's levels) with their tiers, locks and upgrade points.
    pub(super) fn lines(&self, l: &mut L, s: &Session, i: usize, family: Lines, p: &Production) {
        let g = l.g;
        let tr = self.tr;
        let levels = matches!(family, Lines::Levels(_));
        l.label(if levels { tr.levels() } else { tr.production() });
        let ext = s.ed.ext.get(i).copied().unwrap_or_default();
        let ctl = g.small + 12;
        for (k, &(kind, rise)) in p.lines.iter().enumerate() {
            let on = (k as i32) < p.active;
            let top = l.y;
            let rh = g.small + 8 + ctl + 6;
            let r = l.r(l.x0, top, l.inner, rh);
            l.push(r, None, Look::Card(false));
            let icon = l.r(l.x0 + 4, top + 4, rh - 8, rh - 8);
            l.push(icon, None, Look::Pic(PicRef::Line(kind), !on));
            let x = l.x0 + rh + 2;
            let wide = l.inner - rh - 6;
            let name = self.line_name(kind);
            let removable = (!levels && p.lines.len() > 1) || (levels && k + 1 == p.lines.len() && p.lines.len() > 1);
            let rm = g.small + 6;
            let name_w = if removable { wide - rm - 4 } else { wide };
            let r = l.r(x, top + 3, name_w, g.small + 6);
            l.push(r, None, Look::Text(name, l.px(g.small), if on { TEXT } else { MUTED }));
            if removable {
                let r = l.r(x + wide - rm, top + 2, rm, rm);
                l.push(r, Some(Hit::LineRemove(k)), Look::Glyph(Glyph::Cross, (220, 120, 100)));
            }
            // controls: tiers (ground) or kind arrows, the lock, the upgrade points
            let cy = top + g.small + 10;
            let mut cx = x;
            if !levels {
                if kinds::GROUND_UNITS.contains(&kind) {
                    let r = l.r(cx, cy, 64, ctl);
                    let label =
                        if rise > 0 { format!("T{}-{}", (kind - 1) % 3 + 1, ((kind - 1) % 3 + 1 + rise).min(3)) } else { format!("T{}", (kind - 1) % 3 + 1) };
                    l.push(r, Some(Hit::LineTiers(k)), Look::Chip(label, l.px(g.small - 1), false, ACCENT));
                    cx += 68;
                }
                for (up, gl) in [(false, Glyph::Left), (true, Glyph::Right)] {
                    let r = l.r(cx, cy, ctl, ctl);
                    l.push(r, Some(Hit::LineKind(k, up)), Look::Glyph(gl, ACCENT));
                    cx += ctl + 2;
                }
                cx += 4;
            }
            let r = l.r(cx, cy, ctl + 4, ctl);
            l.push(r, Some(Hit::LineLock(k)), Look::Glyph(Glyph::Lock(!on), if on { ACCENT } else { page::GOLD }));
            cx += ctl + 10;
            let rest = x + wide - cx;
            if on {
                let r = l.r(cx, cy, rest, ctl);
                l.push(r, None, Look::Text(tr.unlocked().into(), l.px(g.small - 1), MUTED));
            } else {
                let default = opensow_formats::production::slot_target(kind).unwrap_or(2000) as i32;
                let v = if ext.unlock[k.min(custommap::LINES - 1)] > 0 { ext.unlock[k.min(custommap::LINES - 1)] } else { default };
                let r = l.r(cx, cy, rest.min(170), ctl);
                let (a, b) = page::stepper_arrows(r);
                l.push(r, None, Look::Stepper(v.to_string(), l.px(g.small), page::GOLD, Hit::LineUnlock(k, false), Hit::LineUnlock(k, true)));
                l.push(a, Some(Hit::LineUnlock(k, false)), Look::Card(false));
                l.push(b, Some(Hit::LineUnlock(k, true)), Look::Card(false));
            }
            l.y += rh + 4;
        }
        let can_add = if levels { p.lines.len() < 3 } else { p.lines.len() < opensow_formats::maptweak::MAX_LINES };
        if can_add {
            let r = l.r(l.x0, l.y, l.inner, g.body + 12);
            l.push(r, Some(Hit::AddLine), Look::Button(format!("+ {}", if levels { tr.add_level() } else { tr.add_line() }), l.px(g.small + 1), false));
            l.y += g.body + 18;
        }
        let hint = self.ink.wrap(tr.lines_hint(), l.px(g.small - 2), l.t.len(l.inner));
        for line in hint {
            let r = l.r(l.x0, l.y, l.inner, g.small + 2);
            l.push(r, None, Look::Text(line, l.px(g.small - 2), MUTED));
            l.y += g.small + 2;
        }
    }
}
