//! Software drawing primitives for the menus and the HUD (nearest-neighbour scaling, game bitmap fonts).

use opensow_formats::font::Font;
use opensow_formats::sprite::{rgb565_to_rgb8, Frame};
use opensow_viewer::render::{blend, pack};

/// A decoded RGB picture (the menu backdrop).
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
}

/// Draws `img` scaled to cover the whole buffer, centred.
pub fn backdrop(buf: &mut [u32], w: usize, h: usize, img: &Image) {
    let s = (w as f32 / img.w as f32).max(h as f32 / img.h as f32);
    let (ox, oy) = ((img.w as f32 * s - w as f32) / 2.0, (img.h as f32 * s - h as f32) / 2.0);
    for y in 0..h {
        let sy = (((y as f32 + oy) / s) as usize).min(img.h - 1);
        for x in 0..w {
            let sx = (((x as f32 + ox) / s) as usize).min(img.w - 1);
            let i = (sy * img.w + sx) * 3;
            buf[y * w + x] = pack(img.rgb[i], img.rgb[i + 1], img.rgb[i + 2]);
        }
    }
}

pub fn fill(buf: &mut [u32], w: usize, h: usize, rect: Rect, c: (u8, u8, u8), alpha: u32) {
    opensow_viewer::render::rect(buf, w, h, rect.x, rect.y, rect.x + rect.w, rect.y + rect.h, c, alpha);
}

/// Colour frame with its top-left corner at screen pixel `(x, y)`, scaled by `scale`.
pub fn sprite(buf: &mut [u32], w: usize, h: usize, f: &Frame, x: i32, y: i32, scale: f32) {
    sprite_lit(buf, w, h, f, x, y, scale, 0)
}

/// Like [`sprite`], with every colour moved `lift`/255 of the way towards white (the dark button art on a dark backdrop).
pub fn sprite_lit(buf: &mut [u32], w: usize, h: usize, f: &Frame, x: i32, y: i32, scale: f32, lift: u32) {
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

/// Additive blit (`0x45cde0`: `min(dst + src, max)` per channel); `half` is mode 2 of the widget draw (`0x45c220`),
/// which halves the source first.
pub fn sprite_add(buf: &mut [u32], w: usize, h: usize, f: &Frame, x: i32, y: i32, scale: f32, half: bool) {
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

pub fn text_width(fonts: &(Font, Font), s: &str, scale: f32) -> i32 {
    (fonts.0.measure(s, 1) as f32 * scale) as i32
}

pub fn text_height(fonts: &(Font, Font), scale: f32) -> i32 {
    (fonts.0.line_height() as f32 * scale) as i32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// The exe's darkening table (`0x45d5d1`): every colour channel loses `amount` (clamped at 0), as in the hint boxes.
pub fn dim_sub(buf: &mut [u32], w: usize, h: usize, r: Rect, amount: u32) {
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
    for y in r.y.max(0)..(r.y + r.h).min(h as i32) {
        for x in r.x.max(0)..(r.x + r.w).min(w as i32) {
            let p = &mut buf[y as usize * w + x as usize];
            *p = blend(*p, 0, 0, 0, alpha);
        }
    }
}
