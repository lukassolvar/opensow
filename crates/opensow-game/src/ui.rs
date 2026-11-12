//! Software drawing primitives for the menus and the HUD (nearest-neighbour scaling, game bitmap fonts).

use opensow_formats::font::Font;
use opensow_formats::sprite::{rgb565_to_rgb8, Frame};
use opensow_viewer::record::{self, op};
use opensow_viewer::render::{blend, pack};

/// A decoded RGB picture (the menu backdrop).
pub struct Image {
    /// Width in pixels.
    pub w: usize,
    /// Height in pixels.
    pub h: usize,
    /// The pixels, three bytes each, row by row.
    pub rgb: Vec<u8>,
}

/// Draws `img` scaled to cover the whole buffer, centred.
pub fn backdrop(buf: &mut [u32], w: usize, h: usize, img: &Image) {
    let s = (w as f32 / img.w as f32).max(h as f32 / img.h as f32);
    let (ox, oy) = ((img.w as f32 * s - w as f32) / 2.0, (img.h as f32 * s - h as f32) / 2.0);
    if record::active() {
        let col = |x: i32| (((x as f32 + ox) / s) as usize).min(img.w - 1) as i32;
        let row = |y: i32| (((y as f32 + oy) / s) as usize).min(img.h - 1) as i32;
        record::picture_rgb8(&img.rgb, img.w, 0, 0, w as i32, h as i32, col, row, false);
        return;
    }
    for y in 0..h {
        let sy = (((y as f32 + oy) / s) as usize).min(img.h - 1);
        for x in 0..w {
            let sx = (((x as f32 + ox) / s) as usize).min(img.w - 1);
            let i = (sy * img.w + sx) * 3;
            buf[y * w + x] = pack(img.rgb[i], img.rgb[i + 1], img.rgb[i + 2]);
        }
    }
}

/// The main menu's backdrop: scaled to cover the buffer and anchored at the left edge, so the logo and the button column (drawn
/// in the picture's own fractions, see `menu::layout`) stay on it when the window is narrower than 16:9. Returns the scale.
pub fn backdrop_left(buf: &mut [u32], w: usize, h: usize, img: &Image) -> f32 {
    let s = (w as f32 / img.w as f32).max(h as f32 / img.h as f32);
    let oy = (img.h as f32 * s - h as f32) / 2.0;
    if record::active() {
        let col = |x: i32| ((x as f32 / s) as usize).min(img.w - 1) as i32;
        let row = |y: i32| (((y as f32 + oy) / s) as usize).min(img.h - 1) as i32;
        record::picture_rgb8(&img.rgb, img.w, 0, 0, w as i32, h as i32, col, row, false);
        return s;
    }
    for y in 0..h {
        let sy = (((y as f32 + oy) / s) as usize).min(img.h - 1);
        for x in 0..w {
            let sx = ((x as f32 / s) as usize).min(img.w - 1);
            let i = (sy * img.w + sx) * 3;
            buf[y * w + x] = pack(img.rgb[i], img.rgb[i + 1], img.rgb[i + 2]);
        }
    }
    s
}

/// The whole buffer in one colour (recorded as a fill while a frame is recorded).
pub fn clear(buf: &mut [u32], w: usize, h: usize, rgb: u32) {
    if record::active() {
        record::fill(op::FILL, 0, 0, w as i32, h as i32, rgb, 255);
    } else {
        buf.fill(rgb);
    }
}

/// Copies the `0xRRGGBB` page `src` (`sw` x `sh`) scaled by `s` with its top-left at `(ox, oy)` (the 640x480 menu pages:
/// source pixel `(dx - ox) / s`), clipped to the buffer.
pub fn page(buf: &mut [u32], w: usize, h: usize, src: &[u32], sw: usize, sh: usize, ox: usize, oy: usize, s: f32) {
    let (dw, dh) = ((sw as f32 * s) as usize, (sh as f32 * s) as usize);
    if record::active() {
        let (x0, y0) = (ox as i32, oy as i32);
        let col = |x: i32| (((x - x0) as f32 / s) as usize).min(sw - 1) as i32;
        let row = |y: i32| (((y - y0) as f32 / s) as usize).min(sh - 1) as i32;
        record::picture_words(src, sw, x0, y0, x0 + dw as i32, y0 + dh.min(h) as i32, col, row);
        return;
    }
    for y in 0..dh.min(h) {
        let sy = ((y as f32 / s) as usize).min(sh - 1);
        let dy = y + oy;
        if dy >= h {
            break;
        }
        for x in 0..dw {
            let dx = x + ox;
            if dx >= w {
                break;
            }
            buf[dy * w + dx] = src[sy * sw + ((x as f32 / s) as usize).min(sw - 1)];
        }
    }
}

/// `img` stretched over the whole buffer (`x * img.w / w`), every channel halved when `halve`.
pub fn stretch(buf: &mut [u32], w: usize, h: usize, img: &Image, halve: bool) {
    if record::active() {
        let col = |x: i32| (x as usize * img.w / w).min(img.w - 1) as i32;
        let row = |y: i32| (y as usize * img.h / h).min(img.h - 1) as i32;
        record::picture_rgb8(&img.rgb, img.w, 0, 0, w as i32, h as i32, col, row, halve);
        return;
    }
    for y in 0..h {
        let sy = (y * img.h / h).min(img.h - 1);
        for x in 0..w {
            let sx = (x * img.w / w).min(img.w - 1);
            let i = (sy * img.w + sx) * 3;
            buf[y * w + x] =
                if halve { pack(img.rgb[i] / 2, img.rgb[i + 1] / 2, img.rgb[i + 2] / 2) } else { pack(img.rgb[i], img.rgb[i + 1], img.rgb[i + 2]) };
        }
    }
}

/// Blends a colour over a rectangle with opacity `alpha` (0..=255).
pub fn fill(buf: &mut [u32], w: usize, h: usize, rect: Rect, c: (u8, u8, u8), alpha: u32) {
    opensow_viewer::render::rect(buf, w, h, rect.x, rect.y, rect.x + rect.w, rect.y + rect.h, c, alpha);
}

/// Records a `ui` sprite command (the loops' own sampling: frame pixel `(dx - x) / scale`).
fn rec(code: u32, f: &Frame, x: i32, y: i32, scale: f32, p0: u32) {
    let (dw, dh) = ((f.width as f32 * scale).round() as i32, (f.height as f32 * scale).round() as i32);
    let (fw, fh) = (f.width as usize, f.height as usize);
    record::sprite(
        code,
        f,
        x,
        y,
        x + dw,
        y + dh,
        |dx| (((dx - x) as f32 / scale) as usize).min(fw - 1),
        |dy| (((dy - y) as f32 / scale) as usize).min(fh - 1),
        [p0, 0, 0, 0, 0, 0, 0],
        None,
    );
}

/// Colour frame with its top-left corner at screen pixel `(x, y)`, scaled by `scale`.
pub fn sprite(buf: &mut [u32], w: usize, h: usize, f: &Frame, x: i32, y: i32, scale: f32) {
    sprite_lit(buf, w, h, f, x, y, scale, 0)
}

/// Like [`sprite`], with every colour moved `lift`/255 of the way towards white (the dark button art on a dark backdrop).
pub fn sprite_lit(buf: &mut [u32], w: usize, h: usize, f: &Frame, x: i32, y: i32, scale: f32, lift: u32) {
    if record::active() {
        rec(op::LIT, f, x, y, scale, lift);
        return;
    }
    let (dw, dh) = ((f.width as f32 * scale).round() as i32, (f.height as f32 * scale).round() as i32);
    for dy in y.max(0)..(y + dh).min(h as i32) {
        let sy = (((dy - y) as f32 / scale) as usize).min(f.height as usize - 1);
        for dx in x.max(0)..(x + dw).min(w as i32) {
            let sx = (((dx - x) as f32 / scale) as usize).min(f.width as usize - 1);
            let i = sy * f.width as usize + sx;
            if f.mask[i] {
                let [r, g, b] = rgb565_to_rgb8(f.pixels[i]).map(|c| (c as u32 + (255 - c as u32) * lift / 255) as u8);
                buf[dy as usize * w + dx as usize] = pack(r, g, b);
            }
        }
    }
}

/// The main menu's highlighted button frames (`001_buttons.tsp` frames 0..5): the exe copies them plain (`0x445620` -> `0x45edf0`)
/// because the art has the original 640x480 backdrop baked in behind the green glow; on another backdrop (the Steam art) that
/// shows as a grey rectangle. Here only the green part is kept: the weight of a pixel is how far its green exceeds red and blue.
pub fn sprite_glow(buf: &mut [u32], w: usize, h: usize, f: &Frame, x: i32, y: i32, scale: f32) {
    if record::active() {
        rec(op::GLOW, f, x, y, scale, 0);
        return;
    }
    let (dw, dh) = ((f.width as f32 * scale).round() as i32, (f.height as f32 * scale).round() as i32);
    for dy in y.max(0)..(y + dh).min(h as i32) {
        let sy = (((dy - y) as f32 / scale) as usize).min(f.height as usize - 1);
        for dx in x.max(0)..(x + dw).min(w as i32) {
            let sx = (((dx - x) as f32 / scale) as usize).min(f.width as usize - 1);
            let i = sy * f.width as usize + sx;
            if !f.mask[i] {
                continue;
            }
            let [r, g, b] = rgb565_to_rgb8(f.pixels[i]).map(|c| c as u32);
            let a = (g.saturating_sub(r.max(b)) * 2).min(255);
            if a == 0 {
                continue;
            }
            let p = &mut buf[dy as usize * w + dx as usize];
            let mix = |shift: u32, s: u32| ((((*p >> shift) & 255) * (255 - a) + s * a) / 255) as u8;
            *p = pack(mix(16, r), mix(8, g), mix(0, b));
        }
    }
}

/// Additive blit (`0x45cde0`: `min(dst + src, max)` per channel); `half` is mode 2 of the widget draw (`0x45c220`),
/// which halves the source first.
pub fn sprite_add(buf: &mut [u32], w: usize, h: usize, f: &Frame, x: i32, y: i32, scale: f32, half: bool) {
    if record::active() {
        rec(op::ADD8, f, x, y, scale, half as u32);
        return;
    }
    let (dw, dh) = ((f.width as f32 * scale).round() as i32, (f.height as f32 * scale).round() as i32);
    for dy in y.max(0)..(y + dh).min(h as i32) {
        let sy = (((dy - y) as f32 / scale) as usize).min(f.height as usize - 1);
        for dx in x.max(0)..(x + dw).min(w as i32) {
            let sx = (((dx - x) as f32 / scale) as usize).min(f.width as usize - 1);
            let i = sy * f.width as usize + sx;
            if f.mask[i] {
                let src = rgb565_to_rgb8(f.pixels[i]).map(|c| if half { c as u32 / 2 } else { c as u32 });
                let p = &mut buf[dy as usize * w + dx as usize];
                let add = |shift: u32, s: u32| (((*p >> shift) & 255) + s).min(255) as u8;
                *p = pack(add(16, src[0]), add(8, src[1]), add(0, src[2]));
            }
        }
    }
}

/// Full-strength additive draw (`0x45cde0`) when `additive`, else the plain copy (`0x45edf0`).
pub fn sprite_add_or_plain_mode(buf: &mut [u32], w: usize, h: usize, f: &Frame, x: i32, y: i32, scale: f32, additive: bool) {
    if additive {
        sprite_add(buf, w, h, f, x, y, scale, false)
    } else {
        sprite(buf, w, h, f, x, y, scale)
    }
}

/// Plain copy (`0x45edf0`), or the half-strength additive draw when `dimmed` (widget state 2).
pub fn sprite_add_or_plain(buf: &mut [u32], w: usize, h: usize, f: &Frame, x: i32, y: i32, scale: f32, dimmed: bool) {
    if dimmed {
        sprite_add(buf, w, h, f, x, y, scale, true)
    } else {
        sprite(buf, w, h, f, x, y, scale)
    }
}

/// Darkens only the opaque pixels of `f` drawn at `(x, y)` (a disabled icon).
pub fn sprite_dim(buf: &mut [u32], w: usize, h: usize, f: &Frame, x: i32, y: i32, scale: f32, alpha: u32) {
    if record::active() {
        rec(op::DIM, f, x, y, scale, alpha);
        return;
    }
    let (dw, dh) = ((f.width as f32 * scale).round() as i32, (f.height as f32 * scale).round() as i32);
    for dy in y.max(0)..(y + dh).min(h as i32) {
        let sy = (((dy - y) as f32 / scale) as usize).min(f.height as usize - 1);
        for dx in x.max(0)..(x + dw).min(w as i32) {
            let sx = (((dx - x) as f32 / scale) as usize).min(f.width as usize - 1);
            if f.mask[sy * f.width as usize + sx] {
                let p = &mut buf[dy as usize * w + dx as usize];
                *p = blend(*p, 0, 0, 0, alpha);
            }
        }
    }
}

/// Text in a game font: the `_o` outline mask first, the `_i` inner mask over it (both at `pen - hot`).
pub fn text(buf: &mut [u32], w: usize, h: usize, fonts: &(Font, Font), s: &str, x: i32, y: i32, scale: f32, fg: (u8, u8, u8), outline: (u8, u8, u8)) {
    let (inner, outer) = (&fonts.0, &fonts.1);
    for pass_outline in [true, false] {
        for p in inner.layout(s, 1) {
            let (glyph, color) = if pass_outline {
                match outer.glyph(p.ch) {
                    Some(g) => (g, outline),
                    None => continue,
                }
            } else {
                (p.glyph, fg)
            };
            let (gx0, gy0) = (x + ((p.x - glyph.hot_x as i32) as f32 * scale) as i32, y + ((-(glyph.hot_y as i32)) as f32 * scale) as i32);
            let (dw, dh) = ((glyph.width as f32 * scale).ceil() as i32, (glyph.height as f32 * scale).ceil() as i32);
            if record::active() {
                let (gw, gh) = (glyph.width as usize, glyph.height as usize);
                record::sprite(
                    op::GLYPH,
                    glyph,
                    gx0,
                    gy0,
                    gx0 + dw,
                    gy0 + dh,
                    |dx| (((dx - gx0) as f32 / scale) as usize).min(gw - 1),
                    |dy| (((dy - gy0) as f32 / scale) as usize).min(gh - 1),
                    [pack(color.0, color.1, color.2), 0, 0, 0, 0, 0, 0],
                    None,
                );
                continue;
            }
            for dy in gy0.max(0)..(gy0 + dh).min(h as i32) {
                let sy = (((dy - gy0) as f32 / scale) as usize).min(glyph.height as usize - 1);
                for dx in gx0.max(0)..(gx0 + dw).min(w as i32) {
                    let sx = (((dx - gx0) as f32 / scale) as usize).min(glyph.width as usize - 1);
                    if glyph.mask[sy * glyph.width as usize + sx] {
                        buf[dy as usize * w + dx as usize] = pack(color.0, color.1, color.2);
                    }
                }
            }
        }
    }
}

/// Width of `s` in pixels at the given scale.
pub fn text_width(fonts: &(Font, Font), s: &str, scale: f32) -> i32 {
    (fonts.0.measure(s, 1) as f32 * scale) as i32
}

/// Height of a text line in pixels at the given scale.
pub fn text_height(fonts: &(Font, Font), scale: f32) -> i32 {
    (fonts.0.line_height() as f32 * scale) as i32
}

/// A rectangle in window pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
}

impl Rect {
    /// Whether the point lies inside the rectangle.
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// The exe's darkening table (`0x45d5d1`): every colour channel loses `amount` (clamped at 0), as in the hint boxes.
pub fn dim_sub(buf: &mut [u32], w: usize, h: usize, r: Rect, amount: u32) {
    if record::active() {
        record::fill(op::DIM_SUB, r.x, r.y, r.x + r.w, r.y + r.h, amount, 0);
        return;
    }
    for y in r.y.max(0)..(r.y + r.h).min(h as i32) {
        for x in r.x.max(0)..(r.x + r.w).min(w as i32) {
            let p = &mut buf[y as usize * w + x as usize];
            let c = |sh: u32| (((*p >> sh) & 0xff).saturating_sub(amount)) << sh;
            *p = (*p & 0xff00_0000) | c(16) | c(8) | c(0);
        }
    }
}

/// Darkens the area (menu panels).
pub fn dim(buf: &mut [u32], w: usize, h: usize, r: Rect, alpha: u32) {
    if record::active() {
        record::fill(op::FILL, r.x, r.y, r.x + r.w, r.y + r.h, 0, alpha);
        return;
    }
    for y in r.y.max(0)..(r.y + r.h).min(h as i32) {
        for x in r.x.max(0)..(r.x + r.w).min(w as i32) {
            let p = &mut buf[y as usize * w + x as usize];
            *p = blend(*p, 0, 0, 0, alpha);
        }
    }
}
