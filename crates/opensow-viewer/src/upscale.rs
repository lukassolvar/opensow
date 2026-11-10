//! Runtime texture enhancement from the user's own files (nothing derived is stored or shipped).
//! Pure functions on RGBA8 buffers so they can be tested and compared headless (`examples/upscale_preview.rs`).
//!
//! * [`scale2x`]: the classic EPX/Scale2x pixel-art scaler (exact colour equality, no new colours).
//! * [`smooth2x`]: sprites - the colour is dilated into the transparent border (no dark halo), resampled with
//!   Lanczos3, and the 1-bit coverage is resampled and cut at 0.5 so the outline stays a crisp mask (the game
//!   only has on/off pixels).
//! * [`lanczos`]: backgrounds - plain Lanczos3 resampling plus an optional unsharp mask.

use std::f32::consts::PI;

fn sinc(x: f32) -> f32 {
    if x.abs() < 1e-6 { 1.0 } else { (PI * x).sin() / (PI * x) }
}

fn lanczos3(x: f32) -> f32 {
    if x.abs() >= 3.0 { 0.0 } else { sinc(x) * sinc(x / 3.0) }
}

/// Separable Lanczos3 resample of an interleaved f32 image with `c` channels; edges are clamped.
pub fn resample(src: &[f32], w: usize, h: usize, c: usize, nw: usize, nh: usize) -> Vec<f32> {
    fn pass(src: &[f32], w: usize, h: usize, c: usize, nw: usize, horizontal: bool) -> Vec<f32> {
        let (len, nlen) = if horizontal { (w, nw) } else { (h, nw) };
        let scale = nlen as f32 / len as f32;
        let support = if scale < 1.0 { 3.0 / scale } else { 3.0 };
        let fscale = scale.min(1.0);
        let (ow, oh) = if horizontal { (nlen, h) } else { (w, nlen) };
        let mut out = vec![0.0f32; ow * oh * c];
        // weights per output coordinate
        let weights: Vec<(isize, Vec<f32>)> = (0..nlen)
            .map(|o| {
                let centre = (o as f32 + 0.5) / scale - 0.5;
                let first = (centre - support).ceil() as isize;
                let last = (centre + support).floor() as isize;
                let mut wts: Vec<f32> = (first..=last).map(|i| lanczos3((i as f32 - centre) * fscale)).collect();
                let sum: f32 = wts.iter().sum();
                wts.iter_mut().for_each(|v| *v /= sum);
                (first, wts)
            })
            .collect();
        let threads = if ow * oh * c > 200_000 { std::thread::available_parallelism().map_or(1, |n| n.get()).min(8) } else { 1 };
        let rows_per = oh.div_ceil(threads).max(1);
        std::thread::scope(|sc| {
            for (k, band) in out.chunks_mut(rows_per * ow * c).enumerate() {
                let weights = &weights;
                sc.spawn(move || {
                    for (r, orow) in band.chunks_mut(ow * c).enumerate() {
                        let y = k * rows_per + r;
                        for x in 0..ow {
                            let o = if horizontal { x } else { y };
                            let (first, wts) = &weights[o];
                            for ch in 0..c {
                                let mut acc = 0.0;
                                for (j, wt) in wts.iter().enumerate() {
                                    let i = (first + j as isize).clamp(0, len as isize - 1) as usize;
                                    let (sx, sy) = if horizontal { (i, y) } else { (x, i) };
                                    acc += wt * src[(sy * w + sx) * c + ch];
                                }
                                orow[x * c + ch] = acc;
                            }
                        }
                    }
                });
            }
        });
        out
    }
    let tmp = pass(src, w, h, c, nw, true);
    pass(&tmp, nw, h, c, nh, false)
}

/// EPX / Scale2x: each pixel becomes 2x2, corners take a neighbour's colour when two adjacent neighbours agree.
pub fn scale2x(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
    let px = |x: isize, y: isize| -> [u8; 4] {
        let (x, y) = (x.clamp(0, w as isize - 1) as usize, y.clamp(0, h as isize - 1) as usize);
        rgba[(y * w + x) * 4..][..4].try_into().unwrap()
    };
    // transparent pixels compare equal whatever their colour
    let same = |a: [u8; 4], b: [u8; 4]| a == b || (a[3] == 0 && b[3] == 0);
    let mut out = vec![0u8; w * h * 4 * 4];
    for y in 0..h as isize {
        for x in 0..w as isize {
            let (p, a, b, c, d) = (px(x, y), px(x, y - 1), px(x + 1, y), px(x - 1, y), px(x, y + 1));
            let e = if !same(c, b) && !same(a, d) {
                [
                    if same(a, c) { a } else { p },
                    if same(a, b) { b } else { p },
                    if same(d, c) { c } else { p },
                    if same(d, b) { d } else { p },
                ]
            } else {
                [p; 4]
            };
            for (i, v) in e.iter().enumerate() {
                let (ox, oy) = (x as usize * 2 + i % 2, y as usize * 2 + i / 2);
                out[(oy * w * 2 + ox) * 4..][..4].copy_from_slice(v);
            }
        }
    }
    out
}

/// Copies colour from opaque neighbours into transparent pixels (`passes` rings) so resampling does not bleed black.
fn dilate(rgba: &[u8], w: usize, h: usize, passes: usize) -> Vec<f32> {
    let mut col: Vec<f32> = rgba.chunks(4).flat_map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let mut known: Vec<bool> = rgba.chunks(4).map(|p| p[3] != 0).collect();
    for _ in 0..passes {
        let (c0, k0) = (col.clone(), known.clone());
        for y in 0..h {
            for x in 0..w {
                if k0[y * w + x] {
                    continue;
                }
                let (mut n, mut s) = (0.0, [0.0f32; 3]);
                for (dx, dy) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                    let (nx, ny) = (x as isize + dx, y as isize + dy);
                    if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize || !k0[ny as usize * w + nx as usize] {
                        continue;
                    }
                    n += 1.0;
                    for ch in 0..3 {
                        s[ch] += c0[(ny as usize * w + nx as usize) * 3 + ch];
                    }
                }
                if n > 0.0 {
                    known[y * w + x] = true;
                    for ch in 0..3 {
                        col[(y * w + x) * 3 + ch] = s[ch] / n;
                    }
                }
            }
        }
    }
    col
}

/// Sprite enhancement: `factor`x size, colour resampled, mask re-cut from the smoothed coverage.
pub fn smooth(rgba: &[u8], w: usize, h: usize, factor: usize, sharpen: f32) -> Vec<u8> {
    let (nw, nh) = (w * factor, h * factor);
    let colour = resample(&dilate(rgba, w, h, 3), w, h, 3, nw, nh);
    let colour = if sharpen > 0.0 { unsharp(&colour, nw, nh, 3, sharpen) } else { colour };
    let cover: Vec<f32> = rgba.chunks(4).map(|p| (p[3] != 0) as u8 as f32).collect();
    let cover = resample(&cover, w, h, 1, nw, nh);
    let mut out = vec![0u8; nw * nh * 4];
    for i in 0..nw * nh {
        if cover[i] >= 0.5 {
            for ch in 0..3 {
                out[i * 4 + ch] = colour[i * 3 + ch].round().clamp(0.0, 255.0) as u8;
            }
            out[i * 4 + 3] = 255;
        }
    }
    out
}

pub fn smooth2x(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
    smooth(rgba, w, h, 2, 0.0)
}

fn unsharp(img: &[f32], w: usize, h: usize, c: usize, amount: f32) -> Vec<f32> {
    // 3x3 binomial blur as the low-pass
    let blur = |x: isize, y: isize, ch: usize| -> f32 {
        let mut acc = 0.0;
        for (dy, wy) in [(-1isize, 1.0f32), (0, 2.0), (1, 1.0)] {
            for (dx, wx) in [(-1isize, 1.0f32), (0, 2.0), (1, 1.0)] {
                let (sx, sy) = ((x + dx).clamp(0, w as isize - 1) as usize, (y + dy).clamp(0, h as isize - 1) as usize);
                acc += wx * wy * img[(sy * w + sx) * c + ch];
            }
        }
        acc / 16.0
    };
    let mut out = img.to_vec();
    for y in 0..h {
        for x in 0..w {
            for ch in 0..c {
                let i = (y * w + x) * c + ch;
                out[i] = (img[i] + amount * (img[i] - blur(x as isize, y as isize, ch))).clamp(0.0, 255.0);
            }
        }
    }
    out
}

/// Opaque RGB (e.g. a decoded background) resampled `factor`x with Lanczos3 and an optional unsharp mask.
pub fn lanczos(rgb: &[u8], w: usize, h: usize, factor: usize, sharpen: f32) -> Vec<u8> {
    let f: Vec<f32> = rgb.iter().map(|&v| v as f32).collect();
    let up = resample(&f, w, h, 3, w * factor, h * factor);
    let up = if sharpen > 0.0 { unsharp(&up, w * factor, h * factor, 3, sharpen) } else { up };
    up.iter().map(|v| v.round().clamp(0.0, 255.0) as u8).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// Integration: what the renderer asks for. Everything is made from the user's own files at run time, in memory.
// ---------------------------------------------------------------------------------------------------------------

use opensow_formats::sprite::{rgb565_to_rgb8, Frame};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Master switch (`--no-enhance`, `OPENSOW_ENHANCE=0`). Enhancement only ever applies above zoom 1.0, so a 1:1 picture stays the exe's.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

thread_local! {
    static WORLD: Cell<bool> = const { Cell::new(false) };
}

/// The game sets this while it draws the world (ground, units, buildings, effects); the HUD and menus keep the exe's pixels.
pub fn set_world(on: bool) {
    WORLD.with(|w| w.set(on));
}

/// The game is drawing the world right now (see [`set_world`]); the lighting effects of `fx` use the same scope.
pub fn in_world() -> bool {
    WORLD.with(Cell::get)
}

/// Enhancement is wanted for a blit/ground at this camera zoom.
pub fn wanted(zoom: f32) -> bool {
    zoom > 1.001 && enabled() && WORLD.with(Cell::get)
}

/// Frames bigger than this (clouds, big explosions) are soft already and costly to filter.
const MAX_FRAME_PIXELS: usize = 24_000;

fn to_565(r: u8, g: u8, b: u8) -> u16 {
    let q = |v: u8, max: u32| ((v as u32 * max + 127) / 255) as u16;
    q(r, 31) << 11 | q(g, 63) << 5 | q(b, 31)
}

/// The 2x version of `f` (hotspot doubled), made on first use and kept in the frame. `None` for frames that are not worth it.
pub fn enhanced(f: &Frame) -> Option<&Frame> {
    let (w, h) = (f.width as usize, f.height as usize);
    if w == 0 || h == 0 || w * h > MAX_FRAME_PIXELS || f.hot_x.unsigned_abs() > 16000 || f.hot_y.unsigned_abs() > 16000 {
        return None;
    }
    Some(f.enhanced.get_or_init(|| {
        let rgba: Vec<u8> = f
            .pixels
            .iter()
            .zip(&f.mask)
            .flat_map(|(&c, &m)| if m { let [r, g, b] = rgb565_to_rgb8(c); [r, g, b, 255] } else { [0, 0, 0, 0] })
            .collect();
        let up = smooth(&rgba, w, h, 2, 0.35);
        let (pixels, mask): (Vec<u16>, Vec<bool>) = up.chunks(4).map(|p| (if p[3] != 0 { to_565(p[0], p[1], p[2]) } else { 0 }, p[3] != 0)).unzip();
        Arc::new(Frame { width: f.width * 2, height: f.height * 2, hot_x: f.hot_x * 2, hot_y: f.hot_y * 2, extra: f.extra, pixels, mask, enhanced: OnceLock::new() })
    }))
}

/// A finished 2x background: colour, and the foreground mask (tree tops...) at the same size.
pub struct HiGround {
    pub bg: Vec<u8>,
    pub fg: Vec<bool>,
    pub w: usize,
    pub h: usize,
}

/// 2x ground from the level's background `rgb` (`w*h*3`) and foreground mask (empty = none).
pub fn enhance_ground(rgb: &[u8], fg: &[bool], w: usize, h: usize) -> HiGround {
    let bg = lanczos(rgb, w, h, 2, 0.5);
    let fg2 = if fg.is_empty() {
        Vec::new()
    } else {
        let cover: Vec<f32> = fg.iter().map(|&b| b as u8 as f32).collect();
        resample(&cover, w, h, 1, w * 2, h * 2).iter().map(|&v| v >= 0.5).collect()
    };
    HiGround { bg, fg: fg2, w: w * 2, h: h * 2 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_images_stay_flat() {
        let rgb = vec![90u8; 8 * 8 * 3];
        assert!(lanczos(&rgb, 8, 8, 2, 0.5).iter().all(|&v| v == 90));
    }

    #[test]
    fn scale2x_keeps_a_single_pixel_and_rounds_a_corner() {
        let (a, b) = ([255, 0, 0, 255], [0, 0, 255, 255]);
        // lone pixel: nothing agrees, all four copies stay
        let mut img = vec![0u8; 3 * 3 * 4];
        for i in 0..9 {
            img[i * 4..][..4].copy_from_slice(&b);
        }
        img[4 * 4..][..4].copy_from_slice(&a);
        let up = scale2x(&img, 3, 3);
        assert_eq!(&up[(2 * 6 + 2) * 4..][..4], &a);
        assert_eq!(&up[(2 * 6 + 3) * 4..][..4], &a);
        // a straight edge is not changed
        let mut e = vec![0u8; 4 * 4 * 4];
        for y in 0..4 {
            for x in 0..4 {
                e[(y * 4 + x) * 4..][..4].copy_from_slice(if x < 2 { &a } else { &b });
            }
        }
        let up = scale2x(&e, 4, 4);
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(&up[(y * 8 + x) * 4..][..4], if x < 4 { &a } else { &b });
            }
        }
    }

    #[test]
    fn smooth_keeps_the_mask_one_bit_and_no_dark_halo() {
        // 6x6 opaque light-grey disc-ish square on transparent black
        let mut img = vec![0u8; 6 * 6 * 4];
        for y in 1..5 {
            for x in 1..5 {
                img[(y * 6 + x) * 4..][..4].copy_from_slice(&[200, 200, 200, 255]);
            }
        }
        let up = smooth2x(&img, 6, 6);
        assert_eq!(up.len(), 12 * 12 * 4);
        for p in up.chunks(4) {
            assert!(p[3] == 0 || p[3] == 255);
            if p[3] == 255 {
                assert!(p[0] >= 190, "halo: {}", p[0]);
            }
        }
        // the square's centre stays opaque, the corner transparent
        assert_eq!(up[(6 * 12 + 6) * 4 + 3], 255);
        assert_eq!(up[3], 0);
    }

    #[test]
    fn enhanced_frame_doubles_geometry_and_is_cached() {
        let mut f = Frame { width: 3, height: 2, hot_x: 1, hot_y: -2, extra: [1, 2, 3, 4], pixels: vec![0xF800; 6], mask: vec![true; 6], enhanced: OnceLock::new() };
        f.mask[0] = false;
        let e = enhanced(&f).unwrap();
        assert_eq!((e.width, e.height, e.hot_x, e.hot_y, e.extra), (6, 4, 2, -4, [1, 2, 3, 4]));
        assert_eq!(e.pixels.len(), 24);
        assert!(std::ptr::eq(e, enhanced(&f).unwrap()));
        // opaque pixels keep the colour (no black bleed from the transparent corner)
        assert!(e.mask.iter().zip(&e.pixels).all(|(&m, &p)| !m || p == 0xF800));
    }

    #[test]
    fn ground_doubles() {
        let rgb = vec![50u8; 4 * 4 * 3];
        let g = enhance_ground(&rgb, &[true; 16], 4, 4);
        assert_eq!((g.w, g.h, g.bg.len(), g.fg.len()), (8, 8, 8 * 8 * 3, 64));
        assert!(g.bg.iter().all(|&v| v == 50) && g.fg.iter().all(|&b| b));
    }
}
