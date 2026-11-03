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
}

impl Default for Overlays {
    fn default() -> Self {
        Overlays { blocked: false, grid: false, objects: true, markers: true, sprites: true, muzzles: false, walk: None, fire: None }
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
    let z = view.zoom;
    let (x0, y0) = view.to_screen((ax - f.hot_x as i32) as f32, (ay - f.hot_y as i32) as f32);
    let (x1, y1) = view.to_screen((ax - f.hot_x as i32 + f.width as i32) as f32, (ay - f.hot_y as i32 + f.height as i32) as f32);
    for dy in y0.max(0)..y1.min(h as i32) {
        let sy = (((dy - y0) as f32 / z) as usize).min(f.height as usize - 1);
        for dx in x0.max(0)..x1.min(w as i32) {
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

/// Like [`blit`] but combines the sprite with the picture the way the exe's blit routines do ([`Blit`], exact RGB565
/// arithmetic: plain copy, additive, half-additive, 50 % average, darkening, shade, lens).
pub fn blit_blend(buf: &mut [u32], w: usize, h: usize, view: &View, f: &Frame, ax: i32, ay: i32, mode: Blit) {
    let z = view.zoom;
    let (x0, y0) = view.to_screen((ax - f.hot_x as i32) as f32, (ay - f.hot_y as i32) as f32);
    let (x1, y1) = view.to_screen((ax - f.hot_x as i32 + f.width as i32) as f32, (ay - f.hot_y as i32 + f.height as i32) as f32);
    for dy in y0.max(0)..y1.min(h as i32) {
        let sy = (((dy - y0) as f32 / z) as usize).min(f.height as usize - 1);
        for dx in x0.max(0)..x1.min(w as i32) {
            let sx = (((dx - x0) as f32 / z) as usize).min(f.width as usize - 1);
            let i = sy * f.width as usize + sx;
            if !f.mask[i] {
                continue;
            }
            let [r, g, b] = rgb565_to_rgb8(f.pixels[i]);
            let idx = dy as usize * w + dx as usize;
            let out = match mode {
                Blit::Plain => pack(r, g, b),
                // the exe works on RGB565 words; do the same and expand the result back to 8 bits
                _ => {
                    // the lens mode brightens the picture pixel 2 columns right and 3 rows down instead of the covered one
                    let q = if mode == Blit::Lens {
                        let zi = z.round().max(1.0) as usize;
                        let (qx, qy) = ((dx as usize + 2 * zi).min(w - 1), (dy as usize + 3 * zi).min(h - 1));
                        buf[qy * w + qx]
                    } else {
                        buf[dy as usize * w + dx as usize]
                    };
                    let d = ((q >> 19 & 31) << 11 | (q >> 10 & 63) << 5 | (q >> 3 & 31)) as u16;
                    let s = f.pixels[i];
                    let o = mode.apply565(d, s);
                    let (or, og, ob) = ((o >> 11) as u32, (o >> 5 & 63) as u32, (o & 31) as u32);
                    pack((or << 3 | or >> 2) as u8, (og << 2 | og >> 4) as u8, (ob << 3 | ob >> 2) as u8)
                }
            };
            buf[idx] = out;
        }
    }
}

pub fn render(scene: &Scene, view: &View, ov: Overlays, buf: &mut [u32], w: usize, h: usize) {
    buf.fill(pack(12, 12, 16));
    let inv = 1.0 / view.zoom;
    for sy in 0..h {
        let my = view.y + sy as f32 * inv;
        if my < 0.0 || my >= scene.bg_h as f32 {
            continue;
        }
        let row = my as usize * scene.bg_w;
        for sx in 0..w {
            let mx = view.x + sx as f32 * inv;
            if mx >= 0.0 && (mx as usize) < scene.bg_w {
                let i = (row + mx as usize) * 3;
                buf[sy * w + sx] = pack(scene.bg[i], scene.bg[i + 1], scene.bg[i + 2]);
            }
        }
    }

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
            (Some(wk), Some(t), _) => wk.frame_at(t).min(p.set.len() - 1),
            (_, _, Some(r)) if ov.fire.is_some() => {
                (r.seq.top_frame(fire_tick(r).unwrap_or(0), r.facing as u32, r.steps as u32) as usize).min(p.set.len() - 1)
            }
            _ => p.frame,
        };
        for p in &scene.placed {
            if let Some((set, i)) = &p.shadow {
                blit(buf, w, h, view, &set[frame_of(p).min(set.len() - 1).max(if p.walk.is_some() { 0 } else { *i })], p.ax, p.ay, true);
            }
        }
        for p in &scene.placed {
            blit(buf, w, h, view, &p.set[frame_of(p)], p.ax, p.ay, false);
            if let (Some(r), Some(_)) = (&p.recoil, ov.fire) {
                if let (Some(flash), Some(t)) = (&r.flash, fire_tick(r)) {
                    if let Some(f) = r.seq.flash_frame(t, r.facing as u32, r.steps as u32) {
                        if let Some(fr) = flash.get(f as usize) {
                            blit(buf, w, h, view, fr, p.ax, p.ay, false);
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
}
