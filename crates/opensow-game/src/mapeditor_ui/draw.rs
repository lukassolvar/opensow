//! Drawing the editor: the start page, the map with its markers, and the widgets.

use super::*;

impl MapEditor {
    /// Draws the editor into the `w` x `h` frame buffer.
    pub fn draw(&mut self, buf: &mut [u32], w: usize, h: usize) {
        self.size = (w, h);
        let mut canvas = std::mem::take(&mut self.canvas);
        canvas.resize(w * h, 0);
        opensow_viewer::record::cpu(|| self.render(&mut canvas, w, h));
        ui::page(buf, w, h, &canvas, w, h, 0, 0, 1.0);
        self.canvas = canvas;
    }

    /// Draws the page that is open: the start page, or the map and the sidebar.
    pub(super) fn render(&mut self, c: &mut [u32], w: usize, h: usize) {
        if self.s.is_some() {
            // the sidebar's panels are translucent: they need a fresh background every frame
            ui::clear(c, w, h, 0x00040a06);
            self.render_map(c, w, h);
        } else {
            self.render_start_back(c, w, h);
        }
        let widgets = self.widgets(w, h);
        for wd in &widgets {
            self.draw_widget(c, w, h, wd);
        }
    }

    /// The start page's backdrop and its top bar.
    pub(super) fn render_start_back(&self, c: &mut [u32], w: usize, h: usize) {
        let g = Geo::for_window(w, h);
        let t = T::new(g.d, w, h);
        match &self.bg {
            Some(img) => ui::backdrop(c, w, h, img),
            None => ui::clear(c, w, h, 0x00081008),
        }
        ui::dim(c, w, h, Rect { x: 0, y: 0, w: w as i32, h: h as i32 }, 165);
        let (top_d, bottom_d, title_px, sub_px) = if g.wide { (96, 958, 50, 24) } else { (60, 692, 34, 18) };
        let (top, bottom) = (t.y(top_d), t.y(bottom_d));
        ui::fill(c, w, h, Rect { x: 0, y: 0, w: w as i32, h: top }, (0, 0, 0), 170);
        ui::fill(c, w, h, Rect { x: 0, y: top, w: w as i32, h: 2 }, (90, 200, 100), 255);
        ui::fill(c, w, h, Rect { x: 0, y: bottom, w: w as i32, h: h as i32 - bottom }, (0, 0, 0), 180);
        ui::fill(c, w, h, Rect { x: 0, y: bottom, w: w as i32, h: 2 }, (90, 200, 100), 255);
        let x = t.x(if g.wide { 60 } else { 16 });
        let title = self.tr.editor_title();
        self.ink.text(c, w, h, title, x, t.y((top_d - title_px) / 2 - 4), t.px(title_px), TEXT);
        if g.wide {
            let sx = x + self.ink.width(title, t.px(title_px)) + t.len(36);
            self.ink.text(c, w, h, self.tr.editor_subtitle(), sx, t.y((top_d - sub_px) / 2 + 2), t.px(sub_px), MUTED);
        }
    }

    /// The editor page's map with the editor's marks.
    pub(super) fn render_map(&mut self, c: &mut [u32], w: usize, h: usize) {
        let mw = map_width(w, h);
        let hover_sel = {
            let s = self.s.as_ref().unwrap();
            let (x, y) = self.cursor;
            if self.in_map(x, y, w, h) && s.grab.is_none() && !s.ed.placing {
                self.map_px(x, y).and_then(|(mx, my)| s.ed.pick(mx, my))
            } else {
                None
            }
        };
        let place_cell = {
            let s = self.s.as_ref().unwrap();
            let (x, y) = self.cursor;
            (s.ed.placing && self.in_map(x, y, w, h)).then(|| self.cell_at(x, y)).flatten()
        };
        let drop_cell = {
            let s = self.s.as_ref().unwrap();
            let (x, y) = self.cursor;
            s.tool.filter(|_| self.in_map(x, y, w, h)).and_then(|t| self.cell_at(x, y).map(|c| (t, c)))
        };
        let mut map = std::mem::take(&mut self.map_buf);
        let s = self.s.as_mut().unwrap();
        if !s.fitted || s.view.zoom <= 0.0 {
            // the whole map in view at the start
            s.view = View { x: 0.0, y: 0.0, zoom: fit_zoom(&s.scene, mw, h) };
            s.view.clamp(&s.scene, mw, h);
            s.fitted = true;
        }
        // a window made larger can leave the view zoomed out past the whole map
        if s.view.zoom < fit_zoom(&s.scene, mw, h) {
            s.view.zoom = fit_zoom(&s.scene, mw, h);
            s.view.clamp(&s.scene, mw, h);
        }
        map.clear();
        map.resize(mw * h, 0);
        if mw > 0 {
            opensow_viewer::render::render(&s.scene, &s.view, Overlays::default(), &mut map, mw, h);
            let drag = match s.grab {
                Some(Grab::Move(_, at)) => Some(at),
                _ => None,
            };
            // where nothing can be built: blocked ground red, the no-build edges orange (with a building or turret armed too)
            let armed_building = s.ed.placing && matches!(editor::palette()[s.ed.palette], Thing::Object(_));
            if s.areas || armed_building {
                build_areas(&mut map, mw, h, &s.scene.map, &s.view);
            }
            s.ed.draw_marks(&mut map, mw, h, &s.view, hover_sel, drag, place_cell);
            // the landing points of a reinforcement countdown: the 4x2 cells a carryall fills, and the cell it aims at (orange
            // where no whole squad can land); while the tool is armed, the block under the pointer, green or red
            if countdown_mode(custommap::Timer::read(s.ed.level())) == 3 {
                let at = |v: i32, origin: f32| ((v as f32 * 32.0 - origin) * s.view.zoom).round() as i32;
                // a marked cell and its colour
                type Mark = ((i32, i32), (u8, u8, u8));
                let mut marks: Vec<Mark> = (0..s.reinforcement.drops.len())
                    .map(|k| (s.reinforcement.drops[k].cell, if landing_issue(s, k).is_some() { WARN } else { page::GOLD }))
                    .collect();
                if let Some((MapTool::Landing, c)) = drop_cell {
                    marks.push((c, if landing_refusal(s, c, None).is_none() { ACCENT } else { RED }));
                }
                for ((cx, cy), col) in marks {
                    let (x0, y0, x1, y1) = (at(cx - 1, s.view.x), at(cy, s.view.y), at(cx + 3, s.view.x), at(cy + 2, s.view.y));
                    outline(&mut map, mw, h, Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }, col, 2);
                    let cell = at(cx + 1, s.view.x) - at(cx, s.view.x);
                    page::ring(&mut map, mw, h, at(cx, s.view.x) + cell / 2, y0 + cell / 2, (cell / 3).max(3), 2, col);
                }
            }
            // an escort: the route as dotted legs (red where the convoy cannot walk), the waypoints as rings, the goal as its
            // 3x3 zone; while a tool is armed, the goal zone or waypoint under the pointer, green or red
            if s.mission.win == custommap::WinRule::Escort {
                let (z, vx, vy) = (s.view.zoom, s.view.x, s.view.y);
                let mid = |c: (i32, i32)| ((((c.0 * 32 + 16) as f32 - vx) * z).round() as i32, (((c.1 * 32 + 16) as f32 - vy) * z).round() as i32);
                let px = ((32.0 * z).round() as i32).max(4);
                let bad = escort_issues(s);
                let points: Vec<(i32, i32)> = s.escort.route.iter().copied().chain(s.escort.goal).collect();
                let pods: Vec<(i32, i32)> = s.ed.level().markers.iter().filter(|m| m.kind == custommap::POD && m.player == 0).map(|m| m.cell()).collect();
                for (i, &p) in points.iter().enumerate() {
                    let name = (i < s.escort.route.len()).then_some(i + 1);
                    let col = if bad.contains(&name) { RED } else { ACCENT };
                    let from: Vec<(i32, i32)> = if i == 0 { pods.clone() } else { vec![points[i - 1]] };
                    for f in from {
                        dotted(&mut map, mw, h, mid(f), mid(p), col);
                    }
                }
                for &p in &s.escort.route {
                    let (x, y) = mid(p);
                    page::ring(&mut map, mw, h, x, y, (px / 3).max(3), 2, ACCENT);
                }
                let zone = |map: &mut Vec<u32>, g: (i32, i32), col: (u8, u8, u8)| {
                    let (x, y) = mid(g);
                    outline(map, mw, h, Rect { x: x - px * 3 / 2, y: y - px * 3 / 2, w: px * 3, h: px * 3 }, col, 2);
                    page::ring(map, mw, h, x, y, (px / 3).max(3), 2, col);
                };
                if let Some(g) = s.escort.goal {
                    zone(&mut map, g, ACCENT);
                }
                if let Some((tool @ (MapTool::Goal | MapTool::Waypoint), c)) = drop_cell {
                    let col = if s.ed.can_place(Thing::Unit(1), c, None).is_ok() { ACCENT } else { RED };
                    if tool == MapTool::Goal {
                        zone(&mut map, c, col);
                    } else {
                        let (x, y) = mid(c);
                        page::ring(&mut map, mw, h, x, y, (px / 3).max(3), 2, col);
                    }
                }
            }
            for y in 0..h {
                c[y * w..y * w + mw].copy_from_slice(&map[y * mw..(y + 1) * mw]);
            }
        }
        self.map_buf = map;
    }

    /// The picture a widget refers to.
    pub(super) fn picture(&self, p: PicRef) -> Option<&Picture> {
        match p {
            PicRef::Palette(i, side) => self.pics[side.min(2)].get(i),
            PicRef::Line(k) => self.lines.get(k as usize),
            PicRef::Air(k) => self.air.get(k),
            PicRef::Turret(k) => self.turrets.get(k),
            PicRef::Gold => self.icons[0].as_ref(),
            PicRef::Bulb => self.icons[1].as_ref(),
            PicRef::Advancer => Some(&self.advancer),
            PicRef::Thumb(_) => None,
        }
    }

    /// Draws one widget according to its look.
    pub(super) fn draw_widget(&self, c: &mut [u32], w: usize, h: usize, wd: &W) {
        let r = wd.r;
        let hot = wd.hit.is_some() && self.hover == wd.hit;
        match &wd.look {
            Look::Panel(title, hh, head, pad) => self.ink.panel(c, w, h, r, title, *head, *pad, *hh),
            Look::Text(s, px, col) => self.ink.left(c, w, h, s, r, 0, *px, *col),
            Look::TextCentred(s, px, col) => self.ink.centred(c, w, h, s, r, *px, *col),
            Look::TextRight(s, px, col) => {
                let tw = self.ink.width(s, *px).min(r.w);
                self.ink.fit(c, w, h, s, r.x + r.w - tw, r.y + (r.h - px) / 2 - px / 10, *px, *col, r.w);
            }
            Look::Button(label, px, primary) => self.ink.button(c, w, h, r, label, *px, hot, *primary),
            Look::Chip(label, px, on, col) => self.ink.chip(c, w, h, r, label, *px, *on, hot, *col),
            Look::Stepper(v, px, col, minus, plus) => {
                let hot = if self.hover == Some(*minus) {
                    Some(false)
                } else if self.hover == Some(*plus) {
                    Some(true)
                } else {
                    None
                };
                ui::fill(c, w, h, r, (0, 0, 0), 110);
                self.ink.stepper(c, w, h, r, v, *px, *col, hot);
            }
            Look::Pic(PicRef::Thumb(i), _) => self.draw_thumb(c, w, h, *i, r),
            Look::Pic(p, dim) => {
                if let Some(pic) = self.picture(*p) {
                    if *dim {
                        pic.draw_dim(c, w, h, r, 3.0);
                    } else {
                        pic.draw(c, w, h, r, 3.0);
                    }
                }
            }
            Look::Card(on) => {
                if wd.hit.is_none()
                    || !matches!(
                        wd.hit,
                        Some(
                            Hit::Credits(..)
                                | Hit::Research(..)
                                | Hit::Air(..)
                                | Hit::LineUnlock(..)
                                | Hit::Hp(_)
                                | Hit::Minutes(_)
                                | Hit::RCredits(_)
                                | Hit::RResearch(_)
                                | Hit::RAir(..)
                                | Hit::DropSquad(..)
                        )
                    )
                {
                    ui::fill(c, w, h, r, page::ROW, 230);
                    outline(
                        c,
                        w,
                        h,
                        r,
                        if *on {
                            (255, 230, 90)
                        } else if hot {
                            ACCENT
                        } else {
                            (50, 110, 60)
                        },
                        if *on { 2 } else { 1 },
                    );
                }
            }
            Look::Row(on) => {
                if *on {
                    ui::fill(c, w, h, r, CHOSEN, 230);
                } else if hot {
                    ui::fill(c, w, h, r, CHOSEN, 90);
                }
            }
            Look::Field => {
                let s = self.s.as_ref().unwrap();
                ui::fill(c, w, h, r, (0, 0, 0), 220);
                outline(
                    c,
                    w,
                    h,
                    r,
                    if s.name.focused {
                        (255, 255, 255)
                    } else if hot {
                        ACCENT
                    } else {
                        EDGE
                    },
                    2,
                );
                let px = (r.h * 55 / 100).max(9);
                let inset = (r.h / 4).max(4);
                if s.name.text.is_empty() && !s.name.focused {
                    self.ink.left(c, w, h, self.tr.map_name(), r, inset, px, MUTED);
                } else {
                    self.ink.left(c, w, h, &s.name.text, r, inset, px, (255, 255, 255));
                }
                if s.name.focused {
                    let cx = r.x + inset + self.ink.width(s.name.before_caret(), px).min(r.w - 2 * inset);
                    ui::fill(c, w, h, Rect { x: cx, y: r.y + r.h / 5, w: 2, h: r.h * 3 / 5 }, (255, 255, 255), 255);
                }
            }
            Look::Glyph(gl, col) => {
                let col = if hot { (255, 255, 255) } else { *col };
                ui::fill(c, w, h, r, (0, 0, 0), if hot { 200 } else { 120 });
                outline(c, w, h, r, col, 1);
                match gl {
                    Glyph::Cross => page::cross(c, w, h, r, col),
                    Glyph::Undo => page::turn_arrow(c, w, h, r, true, col),
                    Glyph::Redo => page::turn_arrow(c, w, h, r, false, col),
                    Glyph::Left | Glyph::Right => page::arrow(c, w, h, r.x + r.w / 2, r.y + r.h / 2, (r.h / 4).max(3), *gl == Glyph::Left, col),
                    Glyph::Lock(locked) => lock(c, w, h, r, *locked, col),
                }
            }
            Look::Toggle(on, col) => {
                ui::fill(c, w, h, r, if *on { *col } else { (34, 38, 36) }, 255);
                outline(c, w, h, r, if hot { (255, 255, 255) } else { *col }, 1);
                page::mark(c, w, h, r, *on);
            }
            Look::Scroll(at, view, total) => {
                ui::fill(c, w, h, r, (60, 120, 70), 140);
                let total = (*total).max(1);
                let th = Rect { x: r.x, y: r.y + r.h * at / total, w: r.w, h: (r.h * view / total).max(8) };
                ui::fill(c, w, h, th, ACCENT, 255);
            }
        }
    }

    /// Draws the thumbnail of terrain `i` into rectangle `r`.
    pub(super) fn draw_thumb(&self, c: &mut [u32], w: usize, h: usize, i: usize, r: Rect) {
        ui::fill(c, w, h, r, (0, 0, 0), 255);
        let Some(th) = self.thumbs.get(i).and_then(|t| t.as_ref()) else { return };
        let s = (r.w as f32 / th.w as f32).min(r.h as f32 / th.h as f32);
        let (dw, dh) = ((th.w as f32 * s) as i32, (th.h as f32 * s) as i32);
        let (ox, oy) = (r.x + (r.w - dw) / 2, r.y + (r.h - dh) / 2);
        for y in 0..dh {
            let (sy, dy) = (((y as f32 / s) as usize).min(th.h - 1), oy + y);
            if dy < 0 || dy >= h as i32 {
                continue;
            }
            for x in 0..dw {
                let dx = ox + x;
                if dx >= 0 && dx < w as i32 {
                    c[dy as usize * w + dx as usize] = th.pixels[sy * th.w + ((x as f32 / s) as usize).min(th.w - 1)];
                }
            }
        }
    }
}
