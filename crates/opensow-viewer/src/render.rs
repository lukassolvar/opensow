use crate::art::Source;
use crate::scene::{Scene, CELL};
use opensow_formats::effects::Blit;
use opensow_formats::level::{Object, ObjectKind, Owner, UnitKind};
use opensow_formats::sprite::{rgb565_to_rgb8, Frame};

#[derive(Clone, Copy)]
pub struct Overlays {
    pub blocked: bool,
    pub grid: bool,
    pub objects: bool,
    pub markers: bool,
    pub sprites: bool,
    pub muzzles: bool,
    /// Show chassis mid-walk after this many game ticks (`None` = standing still).
    pub walk: Option<u32>,
    /// Show weapon tops at this tick of their fire sequence (1 = first tick), looping.
    pub fire: Option<u32>,
    /// Draw the foreground (tree tops etc.) over the sprites; a caller that draws units itself does it after them.
    pub foreground: bool,
    /// A minimap in the top-right corner ([`draw_minimap`]): terrain, objects by owner, the camera.
    pub minimap: bool,
}

impl Default for Overlays {
    fn default() -> Self {
        Overlays { blocked: false, grid: false, objects: true, markers: true, sprites: true, muzzles: false, walk: None, fire: None, foreground: true, minimap: false }
    }
}

/// Camera: `x`,`y` are the map pixel at the top-left of the screen, `zoom` is screen px per map px.
#[derive(Clone, Copy)]
pub struct View {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
}

impl View {
    pub fn clamp(&mut self, scene: &Scene, w: usize, h: usize) {
        self.zoom = self.zoom.clamp(0.1, 4.0);
        let (vw, vh) = (w as f32 / self.zoom, h as f32 / self.zoom);
        let (mw, mh) = (scene.bg_w as f32, scene.bg_h as f32);
        self.x = if vw >= mw { (mw - vw) / 2.0 } else { self.x.clamp(0.0, mw - vw) };
        self.y = if vh >= mh { (mh - vh) / 2.0 } else { self.y.clamp(0.0, mh - vh) };
    }

    pub fn to_screen(&self, mx: f32, my: f32) -> (i32, i32) {
        (((mx - self.x) * self.zoom).round() as i32, ((my - self.y) * self.zoom).round() as i32)
    }
}

#[inline(always)]
pub fn pack(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}

pub fn blend(dst: u32, r: u8, g: u8, b: u8, a: u32) -> u32 {
    let mix = |d: u32, s: u8| (d * (255 - a) + s as u32 * a) / 255;
    pack(mix(dst >> 16 & 255, r) as u8, mix(dst >> 8 & 255, g) as u8, mix(dst & 255, b) as u8)
}

pub fn kind_color(kind: ObjectKind) -> (u8, u8, u8) {
    match kind {
        ObjectKind::Turret(_) => (235, 235, 235),
        ObjectKind::Headquarters => (255, 210, 40),
        ObjectKind::LightFactory => (255, 160, 60),
        ObjectKind::MediumFactory => (240, 120, 40),
        ObjectKind::HeavyFactory => (200, 70, 30),
        ObjectKind::WalkerFactory => (220, 40, 80),
        ObjectKind::AirBase => (120, 200, 255),
        ObjectKind::GoldMine => (255, 245, 90),
        ObjectKind::ResearchLab => (190, 110, 255),
        ObjectKind::Wind => (60, 230, 200),
        ObjectKind::Unknown(_) => (255, 0, 255),
    }
}

pub fn owner_color(owner: Owner) -> (u8, u8, u8) {
    match owner {
        Owner::Player => (40, 90, 255),
        Owner::Enemy => (30, 200, 60),
        Owner::Neutral => (170, 170, 170),
        Owner::Unknown(_) => (255, 0, 255),
    }
}

/// Half-size of an object's square in map pixels.
fn object_radius(o: &Object) -> f32 {
    if o.object_kind().is_building() { 0.8 * CELL as f32 } else { 0.35 * CELL as f32 }
}

/// Topmost object whose square contains the map pixel (mx, my).
pub fn object_at<'a>(scene: &'a Scene, mx: f32, my: f32) -> Option<&'a Object> {
    let level = scene.level.as_ref()?;
    level.objects.iter().rev().find(|o| {
        let (cx, cy) = ((o.cell_x as f32 + 0.5) * CELL as f32, (o.cell_y as f32 + 0.5) * CELL as f32);
        let r = object_radius(o).max(CELL as f32 * 0.5);
        (mx - cx).abs() <= r && (my - cy).abs() <= r
    })
}

pub fn rect(buf: &mut [u32], w: usize, h: usize, x0: i32, y0: i32, x1: i32, y1: i32, c: (u8, u8, u8), a: u32) {
    for y in y0.max(0)..y1.min(h as i32) {
        for x in x0.max(0)..x1.min(w as i32) {
            let p = &mut buf[y as usize * w + x as usize];
            *p = blend(*p, c.0, c.1, c.2, a);
        }
    }
}

/// Nearest-neighbour blit of `f` with its hotspot at map pixel (ax, ay); `shadow` darkens instead of copying.
pub fn blit(buf: &mut [u32], w: usize, h: usize, view: &View, f: &Frame, ax: i32, ay: i32, shadow: bool) {
    blit_clip(buf, w, h, view, f, ax, ay, shadow, (0, 0, w as i32, h as i32));
}

/// [`blit`] restricted to the screen rectangle `clip = (x0, y0, x1, y1)`.
pub fn blit_clip(buf: &mut [u32], w: usize, h: usize, view: &View, f: &Frame, ax: i32, ay: i32, shadow: bool, clip: (i32, i32, i32, i32)) {
    let z = view.zoom;
    let (x0, y0) = view.to_screen((ax - f.hot_x as i32) as f32, (ay - f.hot_y as i32) as f32);
    let (x1, y1) = view.to_screen((ax - f.hot_x as i32 + f.width as i32) as f32, (ay - f.hot_y as i32 + f.height as i32) as f32);
    for dy in y0.max(0).max(clip.1)..y1.min(h as i32).min(clip.3) {
        let sy = (((dy - y0) as f32 / z) as usize).min(f.height as usize - 1);
        for dx in x0.max(0).max(clip.0)..x1.min(w as i32).min(clip.2) {
            let sx = (((dx - x0) as f32 / z) as usize).min(f.width as usize - 1);
            let i = sy * f.width as usize + sx;
            if !f.mask[i] {
                continue;
            }
            let p = &mut buf[dy as usize * w + dx as usize];
            if shadow {
                *p = blend(*p, 0, 0, 0, 110);
            } else {
                let [r, g, b] = rgb565_to_rgb8(f.pixels[i]);
                *p = pack(r, g, b);
            }
        }
    }
}

/// A shadow blit ([`blit`] with `shadow`) that skips the screen pixels `mask` marks: the exe's masked shade blit `0x462110` for the
/// shadows of weapon tops (docs/effects.md, "The screen mask").
pub fn blit_shadow_masked(buf: &mut [u32], w: usize, h: usize, view: &View, f: &Frame, ax: i32, ay: i32, mask: &[bool]) {
    let z = view.zoom;
    let (x0, y0) = view.to_screen((ax - f.hot_x as i32) as f32, (ay - f.hot_y as i32) as f32);
    let (x1, y1) = view.to_screen((ax - f.hot_x as i32 + f.width as i32) as f32, (ay - f.hot_y as i32 + f.height as i32) as f32);
    for dy in y0.max(0)..y1.min(h as i32) {
        let sy = (((dy - y0) as f32 / z) as usize).min(f.height as usize - 1);
        for dx in x0.max(0)..x1.min(w as i32) {
            let sx = (((dx - x0) as f32 / z) as usize).min(f.width as usize - 1);
            let idx = dy as usize * w + dx as usize;
            if f.mask[sy * f.width as usize + sx] && !mask[idx] {
                buf[idx] = blend(buf[idx], 0, 0, 0, 110);
            }
        }
    }
}

/// [`blit_clip`] that only draws the pixels of `f` where the building mask `m` (same hotspot frame of `builds\\mask`) is set.
pub fn blit_masked(buf: &mut [u32], w: usize, h: usize, view: &View, f: &Frame, m: &Frame, ax: i32, ay: i32, clip: (i32, i32, i32, i32)) {
    let z = view.zoom;
    let (x0, y0) = view.to_screen((ax - f.hot_x as i32) as f32, (ay - f.hot_y as i32) as f32);
    let (x1, y1) = view.to_screen((ax - f.hot_x as i32 + f.width as i32) as f32, (ay - f.hot_y as i32 + f.height as i32) as f32);
    for dy in y0.max(0).max(clip.1)..y1.min(h as i32).min(clip.3) {
        let sy = (((dy - y0) as f32 / z) as usize).min(f.height as usize - 1);
        for dx in x0.max(0).max(clip.0)..x1.min(w as i32).min(clip.2) {
            let sx = (((dx - x0) as f32 / z) as usize).min(f.width as usize - 1);
            let i = sy * f.width as usize + sx;
            if !f.mask[i] {
                continue;
            }
            // the same map pixel in the mask frame
            let (mx, my) = (sx as i32 - f.hot_x as i32 + m.hot_x as i32, sy as i32 - f.hot_y as i32 + m.hot_y as i32);
            if mx < 0 || my < 0 || mx >= m.width as i32 || my >= m.height as i32 || !m.mask[(my * m.width as i32 + mx) as usize] {
                continue;
            }
            let [r, g, b] = rgb565_to_rgb8(f.pixels[i]);
            buf[dy as usize * w + dx as usize] = pack(r, g, b);
        }
    }
}

/// Like [`blit`] but combines the sprite with the picture the way the exe's blit routines do ([`Blit`], exact RGB565
/// arithmetic: plain copy, additive, half-additive, 50 % average, darkening, shade, lens).
pub fn blit_blend(buf: &mut [u32], w: usize, h: usize, view: &View, f: &Frame, ax: i32, ay: i32, mode: Blit) {
    blit_blend_masked(buf, w, h, view, f, ax, ay, mode, None);
}

/// [`blit_blend`] that skips the screen pixels `mask` marks (the exe's screen mask, docs/effects.md: `0x45b5e0` and `0x45f4d0` draw
/// only where the byte is below the frame's stamp, i.e. where no building or foreground tile was stamped this frame).
#[allow(clippy::too_many_arguments)]
pub fn blit_blend_masked(buf: &mut [u32], w: usize, h: usize, view: &View, f: &Frame, ax: i32, ay: i32, mode: Blit, mask: Option<&[bool]>) {
    let z = view.zoom;
    let (x0, y0) = view.to_screen((ax - f.hot_x as i32) as f32, (ay - f.hot_y as i32) as f32);
    let (x1, y1) = view.to_screen((ax - f.hot_x as i32 + f.width as i32) as f32, (ay - f.hot_y as i32 + f.height as i32) as f32);
    let (xs, xe, ys, ye) = (x0.max(0), x1.min(w as i32), y0.max(0), y1.min(h as i32));
    if xs >= xe || ys >= ye {
        return;
    }
    let fw = f.width as usize;
    // the source column of every screen column, once (the picture is sampled nearest-neighbour)
    let cols: Vec<usize> = (xs..xe).map(|dx| (((dx - x0) as f32 / z) as usize).min(fw - 1)).collect();
    // the loop is written once and instantiated per blend mode, so the mode is not tested per pixel; big sprites (the clouds, at a
    // high resolution) are split into bands of rows over several threads
    let parallel = (xe - xs) as usize * (ye - ys) as usize > 150_000;
    let zi = z.round().max(1.0) as usize;
    macro_rules! run {
        ($pixel:expr) => {{
            par_bands(buf, w, ys as usize, ye as usize, parallel, |band, r0| {
                for (k, out) in band.chunks_mut(w).enumerate() {
                    let dy = (r0 + k) as i32;
                    let row = (((dy - y0) as f32 / z) as usize).min(f.height as usize - 1) * fw;
                    let drow = dy as usize * w;
                    for (kx, dx) in (xs..xe).enumerate() {
                        let i = row + cols[kx];
                        if !f.mask[i] {
                            continue;
                        }
                        if mask.is_some_and(|m| m[drow + dx as usize]) {
                            continue;
                        }
                        out[dx as usize] = $pixel(out[dx as usize], f.pixels[i]);
                    }
                }
            })
        }};
    }
    // the exe works on RGB565 words; do the same and expand the result back to 8 bits
    macro_rules! blend {
        ($m:expr) => {
            run!(|q: u32, s: u16| {
                let d = ((q >> 19 & 31) << 11 | (q >> 10 & 63) << 5 | (q >> 3 & 31)) as u16;
                let o = $m.apply565(d, s);
                let (or, og, ob) = ((o >> 11) as u32, (o >> 5 & 63) as u32, (o & 31) as u32);
                pack((or << 3 | or >> 2) as u8, (og << 2 | og >> 4) as u8, (ob << 3 | ob >> 2) as u8)
            })
        };
    }
    match mode {
        Blit::Plain => run!(|_q: u32, s: u16| {
            let [r, g, b] = rgb565_to_rgb8(s);
            pack(r, g, b)
        }),
        Blit::Additive => blend!(Blit::Additive),
        Blit::AddHalf => blend!(Blit::AddHalf),
        Blit::Average => blend!(Blit::Average),
        Blit::Smoke => blend!(Blit::Smoke),
        Blit::Darken => blend!(Blit::Darken),
        Blit::Shade => blend!(Blit::Shade),
        Blit::Lens => {
            // the lens mode brightens the picture pixel 2 columns right and 3 rows down instead of the covered one
            for dy in ys..ye {
                let row = (((dy - y0) as f32 / z) as usize).min(f.height as usize - 1) * fw;
                for (k, dx) in (xs..xe).enumerate() {
                    let i = row + cols[k];
                    if !f.mask[i] {
                        continue;
                    }
                    let idx = dy as usize * w + dx as usize;
                    if mask.is_some_and(|m| m[idx]) {
                        continue;
                    }
                    let (qx, qy) = ((dx as usize + 2 * zi).min(w - 1), (dy as usize + 3 * zi).min(h - 1));
                    let q = buf[qy * w + qx];
                    let d = ((q >> 19 & 31) << 11 | (q >> 10 & 63) << 5 | (q >> 3 & 31)) as u16;
                    let o = Blit::Lens.apply565(d, f.pixels[i]);
                    let (or, og, ob) = ((o >> 11) as u32, (o >> 5 & 63) as u32, (o & 31) as u32);
                    buf[idx] = pack((or << 3 | or >> 2) as u8, (og << 2 | og >> 4) as u8, (ob << 3 | ob >> 2) as u8);
                }
            }
        }
    }
}

/// Marks the screen pixels whose map pixel has something standing in front of the ground (the foreground tiles: tree tops, rock tops).
/// The exe stamps the same pixels into its screen mask while it draws the tiles (`0x459666`, `0x460120`).
pub fn stamp_foreground(scene: &Scene, view: &View, mask: &mut [bool], w: usize, h: usize) {
    if scene.foreground.is_empty() {
        return;
    }
    let inv = 1.0 / view.zoom;
    let cols = column_map(view, w, scene.bg_w);
    let (bg_w, bg_h, foreground, view_y) = (scene.bg_w, scene.bg_h, &scene.foreground[..], view.y);
    par_bands(mask, w, 0, h, w * h > 400_000, |band, r0| {
        for (k, out) in band.chunks_mut(w).enumerate() {
            let my = view_y + (r0 + k) as f32 * inv;
            if my < 0.0 || my >= bg_h as f32 {
                continue;
            }
            let row = my as usize * bg_w;
            for (sx, &mx) in cols.iter().enumerate() {
                if mx >= 0 && foreground[row + mx as usize] {
                    out[sx] = true;
                }
            }
        }
    });
}

/// Marks the screen pixels covered by the set pixels of `f` (a building's `builds\mask` frame, `0x460120`).
pub fn stamp_frame(mask: &mut [bool], w: usize, h: usize, view: &View, f: &Frame, ax: i32, ay: i32) {
    let z = view.zoom;
    let (x0, y0) = view.to_screen((ax - f.hot_x as i32) as f32, (ay - f.hot_y as i32) as f32);
    let (x1, y1) = view.to_screen((ax - f.hot_x as i32 + f.width as i32) as f32, (ay - f.hot_y as i32 + f.height as i32) as f32);
    for dy in y0.max(0)..y1.min(h as i32) {
        let sy = (((dy - y0) as f32 / z) as usize).min(f.height as usize - 1);
        for dx in x0.max(0)..x1.min(w as i32) {
            let sx = (((dx - x0) as f32 / z) as usize).min(f.width as usize - 1);
            if f.mask[sy * f.width as usize + sx] {
                mask[dy as usize * w + dx as usize] = true;
            }
        }
    }
}

/// Copies the background over everything drawn so far wherever the level's tiles say something stands in front of the ground.
pub fn draw_foreground(scene: &Scene, view: &View, buf: &mut [u32], w: usize, h: usize) {
    foreground_rows(scene, view, buf, w, h, 0, scene.bg_h);
}

/// [`draw_foreground`] for the map pixel rows of one cell row only.
pub fn draw_foreground_row(scene: &Scene, view: &View, buf: &mut [u32], w: usize, h: usize, row: usize) {
    foreground_rows(scene, view, buf, w, h, row * CELL as usize, (row + 1) * CELL as usize);
}

/// Runs `f(band, first_row)` over horizontal bands of rows `y0..y1` of a `w`-wide picture, on several threads when `parallel` (the
/// per-pixel passes over a whole high-resolution screen are the frame's main cost). Every band is disjoint, so the result is the
/// same as one sequential pass.
pub fn par_bands<T: Send, F: Fn(&mut [T], usize) + Sync>(buf: &mut [T], w: usize, y0: usize, y1: usize, parallel: bool, f: F) {
    let rows = y1.saturating_sub(y0);
    if rows == 0 {
        return;
    }
    let threads = if parallel { std::thread::available_parallelism().map_or(1, |n| n.get()).min(8).min(rows) } else { 1 };
    let region = &mut buf[y0 * w..y1 * w];
    if threads <= 1 {
        f(region, y0);
        return;
    }
    let per = rows.div_ceil(threads);
    std::thread::scope(|s| {
        for (k, band) in region.chunks_mut(per * w).enumerate() {
            let f = &f;
            s.spawn(move || f(band, y0 + k * per));
        }
    });
}

/// For every screen column the background pixel column it shows (`-1` outside the picture): computed once per call instead of per pixel.
fn column_map(view: &View, w: usize, bg_w: usize) -> Vec<i32> {
    let inv = 1.0 / view.zoom;
    (0..w)
        .map(|sx| {
            let mx = view.x + sx as f32 * inv;
            if mx >= 0.0 && (mx as usize) < bg_w {
                mx as i32
            } else {
                -1
            }
        })
        .collect()
}

fn foreground_rows(scene: &Scene, view: &View, buf: &mut [u32], w: usize, h: usize, y0: usize, y1: usize) {
    if scene.foreground.is_empty() {
        return;
    }
    let inv = 1.0 / view.zoom;
    let cols = column_map(view, w, scene.bg_w);
    let (s0, s1) = (((y0 as f32 - view.y) * view.zoom).floor().max(0.0) as usize, (((y1 as f32 - view.y) * view.zoom).ceil().max(0.0) as usize).min(h));
    for sy in s0..s1 {
        let my = view.y + sy as f32 * inv;
        if my < y0 as f32 || my >= y1.min(scene.bg_h) as f32 {
            continue;
        }
        let row = my as usize * scene.bg_w;
        let out = &mut buf[sy * w..(sy + 1) * w];
        for (sx, &mx) in cols.iter().enumerate() {
            if mx >= 0 && scene.foreground[row + mx as usize] {
                let i = (row + mx as usize) * 3;
                out[sx] = pack(scene.bg[i], scene.bg[i + 1], scene.bg[i + 2]);
            }
        }
    }
}

pub fn render(scene: &Scene, view: &View, ov: Overlays, buf: &mut [u32], w: usize, h: usize) {
    buf.fill(pack(12, 12, 16));
    let inv = 1.0 / view.zoom;
    let cols = column_map(view, w, scene.bg_w);
    let (bg_w, bg_h, bg, view_y) = (scene.bg_w, scene.bg_h, &scene.bg[..], view.y);
    par_bands(buf, w, 0, h, w * h > 400_000, |band, r0| {
        for (k, out) in band.chunks_mut(w).enumerate() {
            let my = view_y + (r0 + k) as f32 * inv;
            if my < 0.0 || my >= bg_h as f32 {
                continue;
            }
            let row = my as usize * bg_w;
            for (sx, &mx) in cols.iter().enumerate() {
                if mx >= 0 {
                    let i = (row + mx as usize) * 3;
                    out[sx] = pack(bg[i], bg[i + 1], bg[i + 2]);
                }
            }
        }
    });

    let cell = CELL as f32;
    if ov.blocked {
        for c in scene.map.cells.iter().filter(|c| c.blocked) {
            let (x0, y0) = view.to_screen(c.x as f32 * cell, c.y as f32 * cell);
            let (x1, y1) = view.to_screen((c.x as f32 + 1.0) * cell, (c.y as f32 + 1.0) * cell);
            rect(buf, w, h, x0, y0, x1, y1, (220, 40, 40), 90);
        }
    }
    if ov.grid {
        for gx in 0..=scene.map.width {
            let (x, _) = view.to_screen(gx as f32 * cell, 0.0);
            rect(buf, w, h, x, 0, x + 1, h as i32, (255, 255, 255), 40);
        }
        for gy in 0..=scene.map.height {
            let (_, y) = view.to_screen(0.0, gy as f32 * cell);
            rect(buf, w, h, 0, y, w as i32, y + 1, (255, 255, 255), 40);
        }
    }
    // Target-search ranges: cells of rings min+1..=max around a centre cell, alternating tints per ring.
    if !scene.ranges.is_empty() {
        let rings = opensow_formats::rings::Rings::generate();
        for &(cx, cy, min, max) in &scene.ranges {
            for r in min + 1..=max {
                let tint = if r % 2 == 0 { (60, 160, 255) } else { (255, 200, 60) };
                for &(dx, dy) in rings.ring(r) {
                    let (x, y) = (cx + dx as i32, cy + dy as i32);
                    let (x0, y0) = view.to_screen(x as f32 * cell, y as f32 * cell);
                    let (x1, y1) = view.to_screen((x + 1) as f32 * cell, (y + 1) as f32 * cell);
                    rect(buf, w, h, x0, y0, x1, y1, tint, 80);
                }
            }
        }
    }
    // Bullet path overlay: ground track (grey), flight line (yellow) with the lift, start (green), end (red).
    for path in &scene.shots {
        for (i, p) in path.points.iter().enumerate() {
            let (gx, gy) = view.to_screen(p.0 as f32, (p.1 + path.lift[i]) as f32);
            rect(buf, w, h, gx, gy, gx + 2, gy + 2, (190, 190, 190), 255);
            let (fx, fy) = view.to_screen(p.0 as f32, p.1 as f32);
            rect(buf, w, h, fx - 1, fy - 1, fx + 2, fy + 2, (255, 230, 40), 255);
        }
        if let (Some(a), Some(b)) = (path.points.first(), path.points.last()) {
            let (gx, gy) = view.to_screen(b.0 as f32, (b.1 + path.lift[path.points.len() - 1]) as f32);
            rect(buf, w, h, gx - 3, gy - 3, gx + 4, gy + 4, (255, 40, 40), 255);
            let (sx, sy) = view.to_screen(a.0 as f32, (a.1 + path.lift[0]) as f32);
            rect(buf, w, h, sx - 3, sy - 3, sx + 4, sy + 4, (40, 255, 40), 255);
        }
    }
    let Some(level) = &scene.level else { return };
    let mut has_sprite = vec![false; level.objects.len()];
    let mut marker_sprite = vec![false; level.markers.len()];
    if ov.sprites {
        // Tick inside a looping fire sequence (a short pause between volleys).
        let fire_tick = |r: &crate::art::Recoil| ov.fire.map(|t| t % (r.seq.duration() + 2 * (r.seq.reload + 1)) + 1);
        let frame_of = |p: &crate::art::Placed| match (p.walk, ov.walk, &p.recoil) {
            (None, Some(t), _) if p.anim.is_some() => p.anim.map_or(p.frame, |a| a.working_frame(t)).min(p.set.len() - 1),
            (Some(wk), Some(t), _) => wk.frame_at(t).min(p.set.len() - 1),
            (_, _, Some(r)) if ov.fire.is_some() => {
                (r.seq.top_frame(fire_tick(r).unwrap_or(0), r.facing as u32, r.steps as u32) as usize).min(p.set.len() - 1)
            }
            _ => p.frame,
        };
        for p in &scene.placed {
            if let Some((set, i)) = &p.shadow {
                blit(buf, w, h, view, &set[frame_of(p).min(set.len() - 1).max(if p.walk.is_some() || p.anim.is_some() { 0 } else { *i })], p.ax, p.ay, true);
            }
        }
        for p in &scene.placed {
            blit(buf, w, h, view, &p.set[frame_of(p)], p.ax, p.ay, false);
            if let (Some(r), Some(_)) = (&p.recoil, ov.fire) {
                if let (Some(flash), Some(t)) = (&r.flash, fire_tick(r)) {
                    if let Some(f) = r.seq.flash_frame(t, r.facing as u32, r.steps as u32) {
                        if let Some(fr) = flash.get(f as usize) {
                            blit_blend(buf, w, h, view, fr, p.ax, p.ay, opensow_formats::effects::Blit::Additive);
                        }
                    }
                }
            }
            match p.source {
                Source::Object(i) => has_sprite[i] = true,
                Source::Marker(i) => marker_sprite[i] = true,
            }
        }
    }
    if ov.foreground {
        draw_foreground(scene, view, buf, w, h);
    }
    if ov.muzzles {
        for p in &scene.placed {
            for &(mx, my) in &p.muzzles {
                let (cx, cy) = view.to_screen(mx as f32, my as f32);
                rect(buf, w, h, cx - 2, cy - 2, cx + 3, cy + 3, (255, 40, 40), 255);
            }
        }
    }
    if ov.objects {
        // Small things first so buildings never hide turrets next to them.
        let mut order: Vec<(usize, &Object)> = level.objects.iter().enumerate().filter(|(i, _)| !has_sprite[*i]).collect();
        order.sort_by_key(|(_, o)| o.object_kind().is_building());
        for (_, o) in order {
            let r = ((object_radius(o) * view.zoom) as i32).max(2);
            let (cx, cy) = view.to_screen((o.cell_x as f32 + 0.5) * cell, (o.cell_y as f32 + 0.5) * cell);
            rect(buf, w, h, cx - r - 2, cy - r - 2, cx + r + 2, cy + r + 2, owner_color(o.owner()), 255);
            rect(buf, w, h, cx - r, cy - r, cx + r, cy + r, kind_color(o.object_kind()), 255);
        }
    }
    if ov.markers {
        let r = ((cell * 0.3 * view.zoom) as i32).max(2);
        for (i, m) in level.markers.iter().enumerate().filter(|(i, _)| !marker_sprite[*i]) {
            let (cx, cy) = view.to_screen(m.x as f32, m.y as f32);
            let team = if m.player == 0 { (40, 90, 255) } else { (30, 200, 60) };
            let fill = match m.unit_kind() {
                UnitKind::Ground { class, tier } => {
                    let base = [(230, 60, 60), (240, 150, 40), (220, 220, 40), (40, 200, 210), (190, 90, 240)][class as usize % 5];
                    let k = [0.7f32, 1.0, 1.0][(tier as usize - 1).min(2)];
                    ((base.0 as f32 * k) as u8, (base.1 as f32 * k) as u8, (base.2 as f32 * k) as u8)
                }
                UnitKind::Special(_) => (245, 245, 245),
                _ => (255, 0, 255),
            };
            let _ = i;
            // Diamond-ish badge: team-colour square with a smaller class-colour square inside.
            rect(buf, w, h, cx - r - 1, cy - r - 1, cx + r + 2, cy + r + 2, team, 255);
            rect(buf, w, h, cx - r + 1, cy - r + 1, cx + r, cy + r, fill, 255);
        }
    }
    if ov.minimap {
        draw_minimap(scene, view, buf, w, h);
    }
}

/// Pixels per map cell of the minimap: as large as fits a quarter of the shorter screen side, at least 1.
pub fn minimap_scale(scene: &Scene, w: usize, h: usize) -> usize {
    let cells = scene.map.width.max(scene.map.height).max(1) as usize;
    (w.min(h) / 4 / cells).clamp(1, 6)
}

/// The minimap (the exe's radar draws 1 px per cell, docs/hud.md): blocked terrain, every building and turret in its owner's
/// colour, unit markers as single dots, and the camera rectangle. Top-right corner with a 1 px frame.
pub fn draw_minimap(scene: &Scene, view: &View, buf: &mut [u32], w: usize, h: usize) {
    let (mw, mh) = (scene.map.width as i32, scene.map.height as i32);
    let k = minimap_scale(scene, w, h) as i32;
    let (pw, ph) = (mw * k, mh * k);
    if pw + 4 > w as i32 || ph + 4 > h as i32 {
        return;
    }
    let (ox, oy) = (w as i32 - pw - 3, 3);
    rect(buf, w, h, ox - 1, oy - 1, ox + pw + 1, oy + ph + 1, (230, 230, 230), 255);
    rect(buf, w, h, ox, oy, ox + pw, oy + ph, (16, 24, 20), 255);
    for c in scene.map.cells.iter().filter(|c| c.blocked) {
        let (x, y) = (ox + c.x as i32 * k, oy + c.y as i32 * k);
        rect(buf, w, h, x, y, x + k, y + k, (70, 80, 96), 255);
    }
    if let Some(level) = &scene.level {
        for o in &level.objects {
            let (x, y) = (ox + o.cell_x * k, oy + o.cell_y * k);
            let big = o.object_kind().is_building();
            let n = if big { (k * 2).max(2) } else { k.max(1) };
            rect(buf, w, h, x, y, x + n, y + n, owner_color(o.owner()), 255);
        }
        for m in &level.markers {
            let (x, y) = (ox + m.x.div_euclid(CELL as i32) * k, oy + m.y.div_euclid(CELL as i32) * k);
            let team = if m.player == 0 { (110, 150, 255) } else { (110, 240, 130) };
            rect(buf, w, h, x, y, x + k.max(1), y + k.max(1), team, 255);
        }
    }
    // camera rectangle: the visible map area, clamped to the map
    let cell = CELL as f32;
    let (x0, y0) = (view.x / cell * k as f32, view.y / cell * k as f32);
    let (x1, y1) = ((view.x + w as f32 / view.zoom) / cell * k as f32, (view.y + h as f32 / view.zoom) / cell * k as f32);
    let (x0, y0, x1, y1) = ((x0.max(0.0)) as i32, (y0.max(0.0)) as i32, (x1.min(pw as f32)) as i32, (y1.min(ph as f32)) as i32);
    for (ax, ay, bx, by) in [(x0, y0, x1, y0 + 1), (x0, y1 - 1, x1, y1), (x0, y0, x0 + 1, y1), (x1 - 1, y0, x1, y1)] {
        rect(buf, w, h, ox + ax, oy + ay, ox + bx, oy + by, (255, 255, 255), 255);
    }
}

#[cfg(test)]
mod mask_tests {
    use super::*;
    use opensow_formats::sprite::Frame;

    fn frame(w: u16, h: u16, px: u16) -> Frame {
        Frame { width: w, height: h, hot_x: 0, hot_y: 0, extra: [0; 4], pixels: vec![px; (w * h) as usize], mask: vec![true; (w * h) as usize] }
    }

    /// The screen mask (docs/effects.md): a masked blit leaves the marked pixels alone, an unmasked one draws them; stamping a frame
    /// marks exactly its set pixels.
    #[test]
    fn masked_blits_skip_stamped_pixels() {
        let (w, h) = (8usize, 8usize);
        let view = View { x: 0.0, y: 0.0, zoom: 1.0 };
        let mut stamp = vec![false; w * h];
        let mut cover = frame(3, 2, 0);
        cover.mask[1] = false; // one transparent pixel
        stamp_frame(&mut stamp, w, h, &view, &cover, 2, 3);
        assert_eq!(stamp.iter().filter(|&&m| m).count(), 5);
        assert!(stamp[3 * w + 2] && !stamp[3 * w + 3] && stamp[4 * w + 4]);

        let sprite = frame(6, 6, 0x0821); // a little of every channel
        let mut plain = vec![0u32; w * h];
        blit_blend(&mut plain, w, h, &view, &sprite, 0, 0, Blit::Additive);
        let mut masked = vec![0u32; w * h];
        blit_blend_masked(&mut masked, w, h, &view, &sprite, 0, 0, Blit::Additive, Some(&stamp));
        for i in 0..w * h {
            let inside = i % w < 6 && i / w < 6;
            assert_eq!(plain[i] != 0, inside, "plain {i}");
            assert_eq!(masked[i] != 0, inside && !stamp[i], "masked {i}");
        }
        // shadows: the same rule
        let shadow = frame(6, 6, 0);
        let (mut a, mut b) = (vec![0x00ff_ffffu32; w * h], vec![0x00ff_ffffu32; w * h]);
        blit(&mut a, w, h, &view, &shadow, 0, 0, true);
        blit_shadow_masked(&mut b, w, h, &view, &shadow, 0, 0, &stamp);
        for i in 0..w * h {
            assert_eq!(a[i] == b[i], !stamp[i] || !(i % w < 6 && i / w < 6), "shadow {i}");
        }
    }
}
