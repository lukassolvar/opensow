//! The look of the port's own full-window pages (skirmish setup, map editor; docs/skirmish.md, docs/map-editor-ingame.md): colours,
//! pictures composed from the install's sprites, the GDI text in the main menu's face, panels, buttons, steppers and the small flat
//! shapes (arrows, stars, ticks). Everything draws into a plain `0x00RRGGBB` canvas.

use crate::ui::{self, Rect};
use opensow_formats::maptweak::{template, Side, Thing};
use opensow_formats::sprite::Frame;
use opensow_viewer::render::pack;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Normal text.
pub const TEXT: (u8, u8, u8) = (226, 236, 226);
/// Less important text.
pub const MUTED: (u8, u8, u8) = (150, 172, 150);
/// Gold: headings and credit amounts.
pub const GOLD: (u8, u8, u8) = (247, 239, 99);
/// Highlight colour.
pub const ACCENT: (u8, u8, u8) = (120, 230, 120);
/// The blue side.
pub const BLUE: (u8, u8, u8) = (110, 150, 255);
/// The green side.
pub const GREEN: (u8, u8, u8) = (90, 230, 110);
/// Neutral things.
pub const NEUTRAL: (u8, u8, u8) = (200, 200, 200);
/// Warnings.
pub const WARN: (u8, u8, u8) = (240, 170, 90);
/// The two sides' colours, blue then green.
pub const SIDE: [(u8, u8, u8); 2] = [BLUE, GREEN];
/// Panel fill and edges.
pub const FILL: (u8, u8, u8) = (4, 14, 8);
/// Fill of a list row.
pub const ROW: (u8, u8, u8) = (10, 30, 16);
/// Panel edge.
pub const EDGE: (u8, u8, u8) = (70, 150, 80);
/// Separator line.
pub const RULE: (u8, u8, u8) = (60, 120, 70);
/// Fill of the chosen row or card.
pub const CHOSEN: (u8, u8, u8) = (40, 110, 50);

/// A rectangle from its top-left corner and size.
pub fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect { x, y, w, h }
}

/// A card picture: one or more frames placed in a common space (top-left offsets), e.g. a turret's base and gun top.
pub struct Picture {
    layers: Vec<(Frame, i32, i32)>,
    x0: i32,
    y0: i32,
    /// Width of the space all layers fit in.
    pub w: i32,
    /// Height of the space all layers fit in.
    pub h: i32,
}

impl Picture {
    /// A picture from frames and their top-left offsets.
    pub fn of(layers: Vec<(Frame, i32, i32)>) -> Picture {
        let x0 = layers.iter().map(|l| l.1).min().unwrap_or(0);
        let y0 = layers.iter().map(|l| l.2).min().unwrap_or(0);
        let x1 = layers.iter().map(|l| l.1 + l.0.width as i32).max().unwrap_or(0);
        let y1 = layers.iter().map(|l| l.2 + l.0.height as i32).max().unwrap_or(0);
        Picture { layers, x0, y0, w: x1 - x0, h: y1 - y0 }
    }

    /// Scaled to fit `area` (at most `max_scale`), centred; enlarged pictures use the 2x Lanczos copies (`upscale::enhanced`).
    pub fn draw(&self, c: &mut [u32], w: usize, h: usize, area: Rect, max_scale: f32) {
        if self.w <= 0 || self.h <= 0 || area.w <= 0 || area.h <= 0 {
            return;
        }
        let s = (area.w as f32 / self.w as f32).min(area.h as f32 / self.h as f32).min(max_scale);
        let (ox, oy) = (area.x as f32 + (area.w as f32 - self.w as f32 * s) / 2.0, area.y as f32 + (area.h as f32 - self.h as f32 * s) / 2.0);
        for (f, x, y) in &self.layers {
            let (dx, dy) = ((ox + (x - self.x0) as f32 * s).round() as i32, (oy + (y - self.y0) as f32 * s).round() as i32);
            match opensow_viewer::upscale::enhanced(f).filter(|_| s > 1.2) {
                Some(big) => ui::sprite(c, w, h, big, dx, dy, s * f.width as f32 / big.width.max(1) as f32),
                None => ui::sprite(c, w, h, f, dx, dy, s),
            }
        }
    }

    /// Drawn darker (a locked production line): the picture, then a dark veil over its area.
    pub fn draw_dim(&self, c: &mut [u32], w: usize, h: usize, area: Rect, max_scale: f32) {
        self.draw(c, w, h, area, max_scale);
        ui::dim(c, w, h, area, 150);
    }
}

/// A turret as it stands on the map (docs/art.md): the finished base `builds\blue\sentry0N` at the building's art offset and the
/// blue gun top (`buildings::turret_art`) at its own position, turned south-east so the barrel shows.
pub fn turret_picture(art: &mut opensow_viewer::art::Art, kind: i32) -> Picture {
    let mut layers = Vec::new();
    let Some(t) = opensow_formats::buildings::turret_art(kind) else { return Picture::of(layers) };
    if let Some(f) = art.load(&format!("builds\\blue\\{}.ps6", t.base)).and_then(|set| set.get(2).or(set.first()).cloned()) {
        let (ax, ay) = opensow_formats::buildings::art_offset(kind);
        layers.push((f.clone(), ax - f.hot_x as i32, ay - f.hot_y as i32));
    }
    if let Some(f) = art.load(&format!("units\\{}_blue.ps6", t.top)).and_then(|set| set.get(12).or(set.first()).cloned()) {
        let (tx, ty) = opensow_formats::buildings::turret_top_pos(kind, (0, 0));
        layers.push((f.clone(), tx - f.hot_x as i32, ty + t.top_dy - f.hot_y as i32));
    }
    Picture::of(layers)
}

/// A building, turret or unit as `side` owns it on the map: its sprites as the map draws them (building, or turret base and gun,
/// or unit chassis and top), cut to their extent.
pub fn thing_picture(art: &mut opensow_viewer::art::Art, t: Thing, side: Side) -> Picture {
    let placed = match t {
        Thing::Object(k) => art.place(0, &template(k, (2, 2), side)),
        Thing::Unit(k) => {
            art.place_marker(0, &opensow_formats::level::Marker { kind: k, player: if side == Side::Green { 1 } else { 0 }, x: 80, y: 80 }, 12, None)
        }
    };
    Picture::of(placed.iter().filter_map(|p| p.set.get(p.frame).map(|f| (f.clone(), p.ax - f.hot_x as i32, p.ay - f.hot_y as i32))).collect())
}

/// HUD icon `k` without its tile: every frame of the same size shares the tile, so a pixel belongs to the icon when it differs
/// from the colour most of those frames have there; the result is cropped to the icon.
pub fn hud_icon(set: &[Frame], k: usize) -> Option<Frame> {
    let f = set.get(k)?;
    let peers: Vec<&Frame> = set.iter().filter(|o| o.width == f.width && o.height == f.height && o.hot_x == f.hot_x && o.hot_y == f.hot_y).collect();
    if peers.len() < 3 {
        return None;
    }
    let rgb = |p: u16| opensow_formats::sprite::rgb565_to_rgb8(p);
    let near = |a: u16, b: u16| {
        let (a, b) = (rgb(a), rgb(b));
        (0..3).map(|i| (a[i] as i32 - b[i] as i32).abs()).sum::<i32>() < 36
    };
    let (w, h) = (f.width as usize, f.height as usize);
    // pixels showing the tile: the value the most peers agree on
    let tile_like: Vec<bool> = (0..w * h)
        .map(|i| {
            !f.mask[i] || {
                let tile =
                    peers.iter().filter(|o| o.mask[i]).map(|o| o.pixels[i]).max_by_key(|&p| peers.iter().filter(|o| o.mask[i] && near(o.pixels[i], p)).count());
                tile.is_none_or(|t| near(f.pixels[i], t))
            }
        })
        .collect();
    // the tile is what of it is reachable from the frame's edge: what shows through the icon (the bulb's glass) stays
    let mut outside = vec![false; w * h];
    let mut stack: Vec<usize> = (0..w * h).filter(|&i| (i % w == 0 || i % w == w - 1 || i / w == 0 || i / w == h - 1) && tile_like[i]).collect();
    while let Some(i) = stack.pop() {
        if outside[i] {
            continue;
        }
        outside[i] = true;
        let (x, y) = (i % w, i / w);
        for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
            if nx < w && ny < h && tile_like[ny * w + nx] && !outside[ny * w + nx] {
                stack.push(ny * w + nx);
            }
        }
    }
    let keep: Vec<bool> = (0..w * h).map(|i| f.mask[i] && !outside[i]).collect();
    // drop lone specks (noise where the tile itself varies)
    let solid: Vec<bool> = (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            keep[i]
                && [(-1, 0), (1, 0), (0, -1), (0, 1)]
                    .iter()
                    .filter(|(dx, dy)| {
                        let (nx, ny) = (x + dx, y + dy);
                        nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h && keep[ny as usize * w + nx as usize]
                    })
                    .count()
                    >= 2
        })
        .collect();
    let xs = (0..w * h).filter(|&i| solid[i]).map(|i| i % w);
    let ys = (0..w * h).filter(|&i| solid[i]).map(|i| i / w);
    let (x0, x1, y0, y1) = (xs.clone().min()?, xs.max()?, ys.clone().min()?, ys.max()?);
    let (cw, ch) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut pixels = vec![0u16; cw * ch];
    let mut mask = vec![false; cw * ch];
    for y in 0..ch {
        for x in 0..cw {
            let i = (y + y0) * w + x + x0;
            if solid[i] {
                pixels[y * cw + x] = f.pixels[i];
                mask[y * cw + x] = true;
            }
        }
    }
    Some(Frame { width: cw as u16, height: ch as u16, hot_x: 0, hot_y: 0, extra: [0; 4], pixels, mask, enhanced: Default::default() })
}

/// Design -> window transform: a design area scaled to fit the window and centred.
#[derive(Clone, Copy, Debug)]
pub struct T {
    /// Window pixels per design pixel.
    pub s: f32,
    /// Window x of the design space's left edge.
    pub ox: f32,
    /// Window y of the design space's top edge.
    pub oy: f32,
}

impl T {
    /// The transform that fits a `design` area into a `w` x `h` window, centred.
    pub fn new(design: (i32, i32), w: usize, h: usize) -> T {
        let s = (w as f32 / design.0 as f32).min(h as f32 / design.1 as f32);
        T { s, ox: (w as f32 - design.0 as f32 * s) / 2.0, oy: (h as f32 - design.1 as f32 * s) / 2.0 }
    }
    /// Window x of a design x.
    pub fn x(&self, v: i32) -> i32 {
        (self.ox + v as f32 * self.s).round() as i32
    }
    /// Window y of a design y.
    pub fn y(&self, v: i32) -> i32 {
        (self.oy + v as f32 * self.s).round() as i32
    }
    /// A design length in window pixels.
    pub fn len(&self, v: i32) -> i32 {
        (v as f32 * self.s).round() as i32
    }
    /// A design rectangle in window pixels.
    pub fn r(&self, r: Rect) -> Rect {
        let (x0, y0) = (self.x(r.x), self.y(r.y));
        Rect { x: x0, y: y0, w: self.x(r.x + r.w) - x0, h: self.y(r.y + r.h) - y0 }
    }
    /// A font size: design pixels scaled, never below 9.
    pub fn px(&self, v: i32) -> i32 {
        self.len(v).max(9)
    }
}

/// Text in the main menu's face (GDI, any language), sizes cached; the game's bitmap font when no face is available.
pub struct Ink {
    fonts: RefCell<HashMap<i32, Rc<crate::gdi::Gdi>>>,
    bitmap: Rc<(opensow_formats::font::Font, opensow_formats::font::Font)>,
}

impl Ink {
    /// Text drawing with the given pair of bitmap fonts as the fallback.
    pub fn new(bitmap: Rc<(opensow_formats::font::Font, opensow_formats::font::Font)>) -> Ink {
        Ink { fonts: RefCell::new(HashMap::new()), bitmap }
    }

    fn font(&self, px: i32) -> Option<Rc<crate::gdi::Gdi>> {
        let px = px.max(8);
        if let Some(f) = self.fonts.borrow().get(&px) {
            return Some(f.clone());
        }
        let f = Rc::new(crate::gdi::Gdi::load_sized(px as f32, 1.0)?);
        self.fonts.borrow_mut().insert(px, f.clone());
        Some(f)
    }

    /// Width of `s` in pixels at text height `px`.
    pub fn width(&self, s: &str, px: i32) -> i32 {
        match self.font(px) {
            Some(g) => g.width(s),
            None => ui::text_width(&self.bitmap, s, px as f32 / 16.0),
        }
    }

    /// Text with its top-left at `(x, y)`, `px` high, with a dark shadow.
    pub fn text(&self, c: &mut [u32], w: usize, h: usize, s: &str, x: i32, y: i32, px: i32, rgb: (u8, u8, u8)) {
        match self.font(px) {
            // the face is already `px` high: no zoom; small text gets no shadow (it would smear)
            Some(g) if px < 14 => g.draw_flat(c, w, h, s, x as f32, y as f32, 1.0, rgb),
            Some(g) => g.draw_shadowed(c, w, h, s, x as f32, y as f32, 1.0, rgb, (0, 0, 0)),
            None => ui::text(c, w, h, &self.bitmap, s, x, y, px as f32 / 16.0, rgb, (0, 0, 0)),
        }
    }

    /// Text squeezed to at most `max_w` pixels wide (long translations).
    pub fn fit(&self, c: &mut [u32], w: usize, h: usize, s: &str, x: i32, y: i32, px: i32, rgb: (u8, u8, u8), max_w: i32) {
        match self.font(px) {
            Some(g) => g.draw_fit(c, w, h, s, s, x as f32, y as f32, 1.0, rgb, max_w),
            None => self.text(c, w, h, s, x, y, px, rgb),
        }
    }

    /// Draws `s` centred in the rectangle `r`.
    pub fn centred(&self, c: &mut [u32], w: usize, h: usize, s: &str, r: Rect, px: i32, rgb: (u8, u8, u8)) {
        let tw = self.width(s, px).min(r.w);
        self.fit(c, w, h, s, r.x + (r.w - tw) / 2, r.y + (r.h - px) / 2 - px / 10, px, rgb, r.w);
    }

    /// Text vertically centred in `r`, from its left edge (plus `inset`).
    pub fn left(&self, c: &mut [u32], w: usize, h: usize, s: &str, r: Rect, inset: i32, px: i32, rgb: (u8, u8, u8)) {
        self.fit(c, w, h, s, r.x + inset, r.y + (r.h - px) / 2 - px / 10, px, rgb, (r.w - 2 * inset).max(1));
    }

    /// Splits `s` at spaces into lines of at most `max_w` pixels.
    pub fn wrap(&self, s: &str, px: i32, max_w: i32) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        for word in s.split(' ') {
            let next = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if self.width(&next, px) > max_w && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                cur = word.to_string();
            } else {
                cur = next;
            }
        }
        out.push(cur);
        out
    }

    /// A framed panel with a bright top edge and, when `title` is not empty, its heading over a rule.
    pub fn panel(&self, c: &mut [u32], w: usize, h: usize, pr: Rect, title: &str, head_px: i32, pad: i32, hh: i32) {
        ui::fill(c, w, h, pr, FILL, 200);
        outline(c, w, h, pr, EDGE, 2);
        ui::fill(c, w, h, Rect { x: pr.x, y: pr.y, w: pr.w, h: (hh / 14).max(2) }, (110, 220, 120), 255);
        if !title.is_empty() {
            self.fit(c, w, h, title, pr.x + pad, pr.y + (hh - head_px) / 2, head_px, GOLD, pr.w - 2 * pad);
            ui::fill(c, w, h, Rect { x: pr.x + pad, y: pr.y + hh - hh / 7, w: pr.w - 2 * pad, h: 1 }, RULE, 255);
        }
    }

    /// A button: `primary` = the screen's main action (filled green), else framed.
    pub fn button(&self, c: &mut [u32], w: usize, h: usize, r: Rect, label: &str, px: i32, hot: bool, primary: bool) {
        if primary {
            ui::fill(c, w, h, r, if hot { (80, 200, 90) } else { (60, 170, 70) }, 255);
            outline(c, w, h, r, (170, 255, 170), 2);
            self.centred(c, w, h, label, r, px, (5, 30, 10));
        } else {
            ui::fill(c, w, h, r, (0, 0, 0), if hot { 200 } else { 120 });
            outline(c, w, h, r, if hot { (255, 255, 255) } else { ACCENT }, 2);
            self.centred(c, w, h, label, r, px, if hot { (255, 255, 255) } else { TEXT });
        }
    }

    /// A toggle chip: filled with `col` when on.
    pub fn chip(&self, c: &mut [u32], w: usize, h: usize, r: Rect, label: &str, px: i32, on: bool, hot: bool, col: (u8, u8, u8)) {
        // chosen: a third of the colour as the fill and the colour as a bright frame (it glows); not chosen: the same colour
        // dimmed in all three (an eighth as the fill, a dark frame, the label at 62 %): neutral grey, blue, green
        let part = |k: u32| ((col.0 as u32 * k / 100) as u8, (col.1 as u32 * k / 100) as u8, (col.2 as u32 * k / 100) as u8);
        ui::fill(c, w, h, r, if on { part(33) } else { part(13) }, 255);
        outline(
            c,
            w,
            h,
            r,
            if hot {
                (255, 255, 255)
            } else if on {
                col
            } else {
                part(38)
            },
            if on { 2 } else { 1 },
        );
        self.centred(c, w, h, label, r, px, if on || hot { (255, 255, 255) } else { part(62) });
    }

    /// Arrow, value, arrow; `hot` = which arrow is under the mouse (0 minus, 1 plus).
    pub fn stepper(&self, c: &mut [u32], w: usize, h: usize, r: Rect, value: &str, px: i32, colour: (u8, u8, u8), hot: Option<bool>) {
        let (lr, rr) = stepper_arrows(r);
        let a = (r.h / 4).clamp(3, 12);
        for (plus, left, ar) in [(false, true, lr), (true, false, rr)] {
            let c2 = if hot == Some(plus) { (255, 255, 255) } else { colour };
            arrow(c, w, h, ar.x + ar.w / 2, ar.y + ar.h / 2, a, left, c2);
        }
        self.centred(c, w, h, value, Rect { x: lr.x + lr.w, y: r.y, w: rr.x - lr.x - lr.w, h: r.h }, px, (255, 255, 255));
    }
}

/// A stepper's "-" and "+" areas at its two ends.
pub fn stepper_arrows(r: Rect) -> (Rect, Rect) {
    // the arrows take at most a third each, and not much wider than tall: the value keeps the middle
    let t = (r.w / 3).min(r.h + r.h / 4);
    (rect(r.x, r.y, t, r.h), rect(r.x + r.w - t, r.y, t, r.h))
}

/// `s` with its first letter in upper case (the rest unchanged).
pub fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

/// A rectangle's border, `t` pixels thick.
pub fn outline(c: &mut [u32], w: usize, h: usize, r: Rect, col: (u8, u8, u8), t: i32) {
    ui::fill(c, w, h, Rect { x: r.x, y: r.y, w: r.w, h: t }, col, 255);
    ui::fill(c, w, h, Rect { x: r.x, y: r.y + r.h - t, w: r.w, h: t }, col, 255);
    ui::fill(c, w, h, Rect { x: r.x, y: r.y, w: t, h: r.h }, col, 255);
    ui::fill(c, w, h, Rect { x: r.x + r.w - t, y: r.y, w: t, h: r.h }, col, 255);
}

/// Sets one pixel, if it is inside the buffer.
pub fn put(c: &mut [u32], w: usize, h: usize, x: i32, y: i32, col: (u8, u8, u8)) {
    if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
        c[y as usize * w + x as usize] = pack(col.0, col.1, col.2);
    }
}

/// A solid triangle pointing left or right, `a` half-height, centred on `(cx, cy)`.
pub fn arrow(c: &mut [u32], w: usize, h: usize, cx: i32, cy: i32, a: i32, left: bool, col: (u8, u8, u8)) {
    for dy in -a..=a {
        let len = a - dy.abs();
        for dx in 0..=len {
            let x = if left { cx + a / 2 - dx } else { cx - a / 2 + dx };
            put(c, w, h, x, cy + dy, col);
        }
    }
}

/// A five-pointed star of outer radius `r` with a dark rim.
pub fn star_shape(c: &mut [u32], w: usize, h: usize, cx: i32, cy: i32, r: i32, col: (u8, u8, u8)) {
    let pts: Vec<(f32, f32)> = (0..10)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let rad = if i % 2 == 0 { r as f32 } else { r as f32 * 0.42 };
            (cx as f32 + rad * a.cos(), cy as f32 + rad * a.sin())
        })
        .collect();
    let inside = |x: f32, y: f32, scale: f32| {
        let mut hit = false;
        for i in 0..10 {
            let (a, b) = (pts[i], pts[(i + 1) % 10]);
            let (ax, ay) = (cx as f32 + (a.0 - cx as f32) * scale, cy as f32 + (a.1 - cy as f32) * scale);
            let (bx, by) = (cx as f32 + (b.0 - cx as f32) * scale, cy as f32 + (b.1 - cy as f32) * scale);
            if (ay > y) != (by > y) && x < (bx - ax) * (y - ay) / (by - ay) + ax {
                hit = !hit;
            }
        }
        hit
    };
    for y in cy - r - 2..=cy + r + 2 {
        for x in cx - r - 2..=cx + r + 2 {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            if inside(fx, fy, 1.0) {
                put(c, w, h, x, y, col);
            } else if inside(fx, fy, 1.0 + 1.5 / r as f32 + 0.12) {
                put(c, w, h, x, y, (0, 0, 0));
            }
        }
    }
}

/// A circle outline of radius `r`, `t` thick.
pub fn ring(c: &mut [u32], w: usize, h: usize, cx: i32, cy: i32, r: i32, t: i32, col: (u8, u8, u8)) {
    let (outer, inner) = ((r * r) as f32, ((r - t).max(0) * (r - t).max(0)) as f32);
    for y in -r - 1..=r + 1 {
        for x in -r - 1..=r + 1 {
            let d = (x * x + y * y) as f32;
            if d <= outer + r as f32 && d >= inner - r as f32 {
                put(c, w, h, cx + x, cy + y, col);
            } else if d <= (r + 1) as f32 * (r + 1) as f32 + r as f32 && d > outer {
                put(c, w, h, cx + x, cy + y, (0, 0, 0));
            }
        }
    }
}

/// A line two pixels thick.
pub fn line(c: &mut [u32], w: usize, h: usize, (x0, y0): (i32, i32), (x1, y1): (i32, i32), col: (u8, u8, u8)) {
    let n = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
    for i in 0..=n {
        let (x, y) = (x0 + (x1 - x0) * i / n, y0 + (y1 - y0) * i / n);
        for d in 0..2 {
            put(c, w, h, x, y + d, col);
            put(c, w, h, x + d, y, col);
        }
    }
}

/// A tick (on) or a cross (off) in the middle of a toggle.
pub fn mark(c: &mut [u32], w: usize, h: usize, r: Rect, on: bool) {
    let s = (r.h / 3).max(3);
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    if on {
        line(c, w, h, (cx - s, cy), (cx - s / 3, cy + s * 2 / 3), (0, 0, 0));
        line(c, w, h, (cx - s / 3, cy + s * 2 / 3), (cx + s, cy - s * 2 / 3), (0, 0, 0));
    } else {
        cross(c, w, h, r, (170, 170, 170));
    }
}

/// A cross in the middle of `r`.
pub fn cross(c: &mut [u32], w: usize, h: usize, r: Rect, col: (u8, u8, u8)) {
    let s = (r.h / 3).max(3);
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    line(c, w, h, (cx - s * 2 / 3, cy - s * 2 / 3), (cx + s * 2 / 3, cy + s * 2 / 3), col);
    line(c, w, h, (cx + s * 2 / 3, cy - s * 2 / 3), (cx - s * 2 / 3, cy + s * 2 / 3), col);
}

/// A plus in the middle of `r`.
#[allow(dead_code)]
pub fn plus(c: &mut [u32], w: usize, h: usize, r: Rect, col: (u8, u8, u8)) {
    let s = (r.h / 3).max(3);
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    line(c, w, h, (cx - s, cy), (cx + s, cy), col);
    line(c, w, h, (cx, cy - s), (cx, cy + s), col);
}

/// A curved "undo" (`back`) or "redo" arrow in `r`.
pub fn turn_arrow(c: &mut [u32], w: usize, h: usize, r: Rect, back: bool, col: (u8, u8, u8)) {
    let s = (r.h / 3).max(4);
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2 + s / 4);
    let dir = if back { -1 } else { 1 };
    // a half ring over the top and the arrow head at its start
    for k in 0..=24 {
        let a = std::f32::consts::PI * k as f32 / 24.0;
        let (x, y) = (cx as f32 - dir as f32 * s as f32 * a.cos(), cy as f32 - s as f32 * 0.9 * a.sin());
        for d in 0..2 {
            put(c, w, h, x as i32, y as i32 + d, col);
            put(c, w, h, x as i32 + d, y as i32, col);
        }
    }
    let (hx, hy) = (cx + dir * s, cy);
    line(c, w, h, (hx, hy), (hx - dir * s / 2, hy - s / 2), col);
    line(c, w, h, (hx, hy), (hx + dir * s / 2, hy - s / 2), col);
}

/// A folder (the maps folder's button where a label does not fit).
pub fn folder(c: &mut [u32], w: usize, h: usize, r: Rect, col: (u8, u8, u8)) {
    let s = (r.h * 2 / 5).max(6);
    let (x0, y0) = (r.x + r.w / 2 - s * 3 / 4, r.y + r.h / 2 - s / 2);
    ui::fill(c, w, h, Rect { x: x0, y: y0, w: s * 3 / 5, h: s / 4 }, col, 255);
    ui::fill(c, w, h, Rect { x: x0, y: y0 + s / 5, w: s * 3 / 2, h: s }, col, 255);
    ui::fill(c, w, h, Rect { x: x0 + 2, y: y0 + s / 5 + s / 3, w: s * 3 / 2 - 4, h: 1 }, (0, 0, 0), 120);
}

/// Whether `r` lies inside a `w` x `h` window.
#[cfg(test)]
pub fn inside(r: Rect, w: usize, h: usize) -> bool {
    r.w > 0 && r.h > 0 && r.x >= 0 && r.y >= 0 && r.x + r.w <= w as i32 && r.y + r.h <= h as i32
}

#[cfg(test)]
pub fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}
