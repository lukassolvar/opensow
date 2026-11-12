//! Lighting effects for the world, made from the original's pictures at run time (docs/fx.md). Each one is expressed with the
//! drawing primitives the GPU replay already has (shadow blits and coverage grids, `record::cover`), so the CPU and GPU pictures
//! stay bit-identical:
//!
//! * **soft shadows**: every shadow blit becomes nine blits one to a few screen pixels apart whose alphas add up to the original's
//!   110 (a blurred edge, the same darkness inside);
//! * **contact shadows**: a faint soft dark halo hugging a building's silhouette (its own sprite drawn as a faint shade on a few rings)
//!   and a soft dark pool under each ground unit;
//! * **glow**: an additive effect sprite (explosion, muzzle flash) also lights the ground around it.
//!
//! All three only act while the game draws the world ([`crate::upscale::set_world`]); the HUD and menus keep the exe's pixels.

use crate::render::{blend, View};
use crate::{record, upscale};
use opensow_formats::sprite::{rgb565_to_rgb8, Frame};
use std::sync::atomic::{AtomicBool, Ordering};

static SOFT_SHADOWS: AtomicBool = AtomicBool::new(true);
static CONTACT: AtomicBool = AtomicBool::new(true);
static GLOW: AtomicBool = AtomicBool::new(true);
static TURRET_SHADOWS: AtomicBool = AtomicBool::new(false);

/// Switches (Options page 2; `--no-fx` forces all off).
pub fn set(soft_shadows: bool, contact: bool, glow: bool, turret_shadows: bool) {
    SOFT_SHADOWS.store(soft_shadows, Ordering::Relaxed);
    CONTACT.store(contact, Ordering::Relaxed);
    GLOW.store(glow, Ordering::Relaxed);
    TURRET_SHADOWS.store(turret_shadows, Ordering::Relaxed);
}

/// Whether soft shadows are drawn: the switch is on and the world is being drawn enhanced.
pub fn soft_shadows() -> bool {
    SOFT_SHADOWS.load(Ordering::Relaxed) && upscale::in_world()
}

/// Whether contact shadows are drawn: the switch is on and the world is being drawn enhanced.
pub fn contact() -> bool {
    CONTACT.load(Ordering::Relaxed) && upscale::in_world()
}

/// Turrets without shadow art get a made-up one (see [`turret_shadow_offset`]); the original draws only the A.A. turret's.
pub fn turret_shadows() -> bool {
    TURRET_SHADOWS.load(Ordering::Relaxed) && upscale::in_world()
}

/// Map-pixel offset of a made-up turret shadow: the sprite's own silhouette moved right and up in proportion to its height. Measured
/// on the original shadows of the A.A. turret and the unit tops (`2AA`, `1A`, `2A`: their visible shadow lies about 0.4 heights to
/// the right of the sprite and 0.14 heights up; the made-up shadow uses 0.32 / 0.11, a little closer, by preference).
pub fn turret_shadow_offset(sprite_height: u16) -> (i32, i32) {
    let h = sprite_height as f32;
    ((0.32 * h).round() as i32, -((0.11 * h).round() as i32))
}

/// Whether the explosion glow is drawn: the switch is on and the world is being drawn enhanced.
pub fn glow_on() -> bool {
    GLOW.load(Ordering::Relaxed) && upscale::in_world()
}

/// The exe's shadow darkening (`blend(.., 0, 0, 0, 110)`).
pub const SHADOW_ALPHA: u32 = 110;

/// Per-tap alphas of the nine soft-shadow blits: laid over each other they darken like one blit of [`SHADOW_ALPHA`] (within 4 of 255).
pub const TAP_ALPHA: [u32; 9] = [15, 14, 15, 14, 15, 14, 15, 14, 15];

/// Screen offsets of the nine taps at camera zoom `z`: a 3x3 grid with a step of about 0.8 screen pixels per map pixel of zoom.
pub fn taps(z: f32) -> [(i32, i32); 9] {
    let r = ((z * 0.8).round() as i32).max(1);
    [(0, 0), (-r, 0), (r, 0), (0, -r), (0, r), (-r, -r), (r, -r), (-r, r), (r, r)]
}

/// `view` moved so everything drawn with it lands `(dx, dy)` screen pixels further right / down.
pub fn shifted(view: &View, dx: i32, dy: i32) -> View {
    View { x: view.x - dx as f32 / view.zoom, y: view.y - dy as f32 / view.zoom, zoom: view.zoom }
}

/// Blends `rgb` over the screen through an alpha grid (`bw` wide, 0..=255 per cell) whose top-left is screen `(bx, by)`: recorded as
/// one coverage command for the GPU replay, otherwise mixed with the same integer formula.
pub fn cover_grid(buf: &mut [u32], w: usize, h: usize, bx: i32, by: i32, bw: usize, grid: &[u32], rgb: (u8, u8, u8)) {
    if bw == 0 || grid.is_empty() {
        return;
    }
    if record::active() {
        record::cover(bx, by, bw, grid, crate::render::pack(rgb.0, rgb.1, rgb.2));
        return;
    }
    let bh = grid.len() / bw;
    for gy in 0..bh {
        let y = by + gy as i32;
        if y < 0 || y >= h as i32 {
            continue;
        }
        for gx in 0..bw {
            let a = grid[gy * bw + gx];
            let x = bx + gx as i32;
            if a > 0 && x >= 0 && x < w as i32 {
                let p = &mut buf[y as usize * w + x as usize];
                *p = blend(*p, rgb.0, rgb.1, rgb.2, a);
            }
        }
    }
}

/// `(1 - t)^2` for `t` in 0..1 (0 outside): a soft falloff.
fn falloff(t: f32) -> f32 {
    if t >= 1.0 {
        0.0
    } else {
        (1.0 - t) * (1.0 - t)
    }
}

/// A soft dark ellipse centred on map pixel `(mx, my)` with half-sizes `(rx, ry)` map pixels and peak alpha `peak`.
pub fn blob(buf: &mut [u32], w: usize, h: usize, view: &View, mx: f32, my: f32, rx: f32, ry: f32, peak: f32) {
    let (cx, cy) = ((mx - view.x) * view.zoom, (my - view.y) * view.zoom);
    let (rxs, rys) = ((rx * view.zoom).max(1.0), (ry * view.zoom).max(1.0));
    let (x0, y0) = ((cx - rxs).floor() as i32, (cy - rys).floor() as i32);
    let (bw, bh) = ((rxs * 2.0).ceil() as usize + 2, (rys * 2.0).ceil() as usize + 2);
    if bw * bh > 160_000 || x0 >= w as i32 || y0 >= h as i32 || x0 + (bw as i32) < 0 || y0 + (bh as i32) < 0 {
        return;
    }
    let mut grid = vec![0u32; bw * bh];
    for gy in 0..bh {
        for gx in 0..bw {
            let (dx, dy) = ((x0 + gx as i32) as f32 + 0.5 - cx, (y0 + gy as i32) as f32 + 0.5 - cy);
            let d = ((dx / rxs).powi(2) + (dy / rys).powi(2)).sqrt();
            grid[gy * bw + gx] = (peak * falloff(d)).round() as u32;
        }
    }
    cover_grid(buf, w, h, x0, y0, bw, &grid, (0, 0, 0));
}

/// Screen offsets and alphas of the blits that make a building's contact shadow: the building's own silhouette drawn as a faint shade
/// on three rings (centre, half radius, full radius), shifted slightly down. Where the rings overlap near the silhouette the ground
/// is darkest and it fades to nothing at the outer radius (about 4 screen pixels per zoom step), so the shape follows the building
/// itself rather than its footprint rectangle, and no edge or square can show.
pub fn contact_taps(z: f32) -> Vec<(i32, i32, u32)> {
    let r = (4.0 * z).max(2.0);
    let mut taps = vec![(0, (0.2 * r).round() as i32, 5)];
    for (ring, rad) in [(8, r * 0.5), (12, r)] {
        for k in 0..ring {
            let a = k as f32 / ring as f32 * std::f32::consts::TAU;
            taps.push(((rad * a.cos()).round() as i32, (rad * a.sin() + 0.2 * r).round() as i32, 3));
        }
    }
    taps
}

/// Glow of an additive effect sprite: a warm radial light on the ground around its centre, as strong as the sprite is bright.
pub fn glow(buf: &mut [u32], w: usize, h: usize, view: &View, f: &Frame, ax: i32, ay: i32) {
    let (fw, fh) = (f.width as usize, f.height as usize);
    if fw * fh < 64 || fw * fh > 24_000 {
        return; // markers / big soft sprites (clouds)
    }
    let (mut sum, mut cx, mut cy, mut rgb) = (0.0f32, 0.0f32, 0.0f32, [0.0f32; 3]);
    for y in 0..fh {
        for x in 0..fw {
            let i = y * fw + x;
            if !f.mask[i] {
                continue;
            }
            let c = rgb565_to_rgb8(f.pixels[i]);
            let l = (c[0] as f32 * 0.3 + c[1] as f32 * 0.59 + c[2] as f32 * 0.11) / 255.0;
            sum += l;
            cx += l * x as f32;
            cy += l * y as f32;
            for k in 0..3 {
                rgb[k] += l * c[k] as f32;
            }
        }
    }
    let brightness = sum / (fw * fh) as f32;
    if sum <= 0.0 || brightness < 0.06 {
        return;
    }
    let mean = rgb.map(|v| v / sum);
    // pull the colour towards a warm light and keep it bright
    let warm = [255.0, 175.0, 80.0];
    let tint = |k: usize| (mean[k] * 0.6 + warm[k] * 0.4).clamp(0.0, 255.0) as u8;
    let (mx, my) = ((ax - f.hot_x as i32) as f32 + cx / sum + 0.5, (ay - f.hot_y as i32) as f32 + cy / sum + 0.5);
    let radius = fw.max(fh) as f32 * 1.1;
    let peak = (brightness * 300.0).min(150.0);
    let (scx, scy) = ((mx - view.x) * view.zoom, (my - view.y) * view.zoom);
    let rs = (radius * view.zoom).min(360.0);
    let (x0, y0) = ((scx - rs).floor() as i32, (scy - rs).floor() as i32);
    let side = (rs * 2.0).ceil() as usize + 2;
    if x0 >= w as i32 || y0 >= h as i32 || x0 + side as i32 <= 0 || y0 + side as i32 <= 0 {
        return;
    }
    let mut grid = vec![0u32; side * side];
    for gy in 0..side {
        for gx in 0..side {
            let (dx, dy) = ((x0 + gx as i32) as f32 + 0.5 - scx, (y0 + gy as i32) as f32 + 0.5 - scy);
            grid[gy * side + gx] = (peak * falloff((dx * dx + dy * dy).sqrt() / rs)).round() as u32;
        }
    }
    cover_grid(buf, w, h, x0, y0, side, &grid, (tint(0), tint(1), tint(2)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turret_shadow_offset_grows_with_the_sprite() {
        assert_eq!(turret_shadow_offset(36), (12, -4));
        assert_eq!(turret_shadow_offset(17), (5, -2));
        assert!(turret_shadow_offset(60).0 > turret_shadow_offset(30).0);
    }

    #[test]
    fn nine_taps_darken_like_one_shadow() {
        for d in 0..=255u32 {
            let one = d * (255 - SHADOW_ALPHA) / 255;
            let nine = TAP_ALPHA.iter().fold(d, |p, &a| p * (255 - a) / 255);
            assert!(one.abs_diff(nine) <= 6, "{d}: {one} vs {nine}");
        }
    }

    #[test]
    fn shifted_view_moves_the_picture_by_about_whole_pixels() {
        let v = View { x: 100.0, y: 50.0, zoom: 1.5 };
        for (dx, dy) in [(1, 0), (-2, 3)] {
            let s = shifted(&v, dx, dy);
            for (mx, my) in [(120.0, 70.0), (121.0, 71.0)] {
                let (a, b) = (v.to_screen(mx, my), s.to_screen(mx, my));
                // exact except where the unshifted position sits on a rounding tie (e.g. 31.5): then it may be one pixel off
                assert!((b.0 - a.0 - dx).abs() <= 1 && (b.1 - a.1 - dy).abs() <= 1, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn cover_grid_mixes_like_blend() {
        let (w, h) = (4, 3);
        let mut buf = vec![crate::render::pack(200, 100, 50); w * h];
        cover_grid(&mut buf, w, h, 1, 1, 2, &[255, 0, 128, 64], (0, 0, 0));
        assert_eq!(buf[w + 1], crate::render::pack(0, 0, 0));
        assert_eq!(buf[w + 2], crate::render::pack(200, 100, 50));
        assert_eq!(buf[2 * w + 1], blend(crate::render::pack(200, 100, 50), 0, 0, 0, 128));
        // partly off screen is clipped, not a panic
        cover_grid(&mut buf, w, h, -1, -1, 3, &[10; 9], (255, 0, 0));
    }

    #[test]
    fn contact_taps_fade_out_and_stay_light() {
        let taps = contact_taps(3.0);
        // at most the darkness of a faint shade where all rings overlap (the silhouette's inside, hidden by the building)
        let worst = taps.iter().fold(200u32, |p, &(_, _, a)| p * (255 - a) / 255);
        assert!(worst > 200 * 60 / 100, "darkest overlap stays above 60 % ({worst})");
        assert!(taps.iter().all(|&(dx, dy, _)| dx.abs() <= 13 && dy.abs() <= 16), "radius about 4 px per zoom step");
        assert_eq!(contact_taps(1.0).len(), taps.len());
    }

    #[test]
    fn soft_shadow_keeps_the_darkness_and_blurs_the_edge() {
        use opensow_formats::sprite::Frame;
        let (w, h) = (60, 60);
        let frame =
            Frame { width: 20, height: 20, hot_x: 0, hot_y: 0, extra: [0; 4], pixels: vec![0; 400], mask: vec![true; 400], enhanced: Default::default() };
        let view = View { x: 0.0, y: 0.0, zoom: 1.0 };
        let grey = crate::render::pack(200, 200, 200);
        let mut hard = vec![grey; w * h];
        crate::render::blit(&mut hard, w, h, &view, &frame, 20, 20, true);
        let (mut soft, _) = (vec![grey; w * h], 0);
        upscale::set_world(true);
        set(true, true, true, false);
        crate::render::blit(&mut soft, w, h, &view, &frame, 20, 20, true);
        upscale::set_world(false);
        let c = |b: &Vec<u32>, x: usize, y: usize| (b[y * w + x] & 255) as i32;
        assert!((c(&hard, 30, 30) - c(&soft, 30, 30)).abs() <= 6, "same darkness inside: {} vs {}", c(&hard, 30, 30), c(&soft, 30, 30));
        assert_eq!(c(&hard, 19, 30), 200, "the hard shadow stops at the edge");
        assert!(c(&soft, 19, 30) < 200 && c(&soft, 19, 30) > c(&soft, 30, 30), "the soft one bleeds out a little");
        assert_eq!(c(&soft, 10, 30), 200, "and not far");
        assert!(c(&soft, 20, 30) > c(&soft, 30, 30), "and is lighter on the edge");
    }
}
