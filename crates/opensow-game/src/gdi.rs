//! Stand-in for the exe's GDI text on the menus (`0x467060`: white text with a black shadow).
//!
//! The campaign map's text is `CreateFontA(13, 6, 0, 0, 400, ..., "Times New Roman")` (`0x422740`, font `[0x5ddf30]`): a cell height of 13 px and a
//! requested average character width of 6 px, which GDI reaches by widening the face. The game cannot ship the font, so it is read from the
//! system when it exists (macOS, Windows, Liberation on Linux); otherwise callers fall back to the bitmap fonts.

use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont};
use opensow_viewer::render::pack;

/// Horizontal stretch GDI applies to reach the requested 6 px average width (calibrated on the Steam 1920x1080 screenshots:
/// "First, seize Research Lab located left of your initial" is about 316 px wide).
const XSCALE: f32 = 1.23;
/// The font's cell height in pixels (`nHeight`).
const HEIGHT: f32 = 13.0;

/// The bold face: the Steam screenshots show heavy strokes (GDI's font mapper picks the wider face for the 6 px width request).
const CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Times New Roman Bold.ttf",
    "C:\\Windows\\Fonts\\timesbd.ttf",
    "/usr/share/fonts/truetype/msttcorefonts/Times_New_Roman_Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSerif-Bold.ttf",
    "/usr/share/fonts/liberation/LiberationSerif-Bold.ttf",
    "/usr/share/fonts/TTF/LiberationSerif-Bold.ttf",
];

pub struct Gdi {
    font: FontVec,
    height: f32,
    xscale: f32,
}

impl Gdi {
    pub fn load() -> Option<Gdi> {
        Self::load_sized(HEIGHT, XSCALE)
    }

    /// The same face with another cell height and horizontal stretch (`CreateFontA(height, width, ...)`; width 0 = natural = 1.0).
    pub fn load_sized(height: f32, xscale: f32) -> Option<Gdi> {
        CANDIDATES.iter().find_map(|p| std::fs::read(p).ok().and_then(|d| FontVec::try_from_vec(d).ok())).map(|font| Gdi { font, height, xscale })
    }

    fn scale(&self) -> PxScale {
        PxScale { x: self.height * self.xscale, y: self.height }
    }

    /// Advance of one character in whole pixels (GDI's hinted advances are integers).
    fn advance(&self, c: char) -> i32 {
        let s = self.font.as_scaled(self.scale());
        s.h_advance(s.glyph_id(c)).round() as i32
    }

    pub fn width(&self, text: &str) -> i32 {
        text.chars().map(|c| self.advance(c)).sum()
    }

    /// Plain text at zoom `z` with its top-left corner at `(x, y)` (window pixels); advances are the natural whole-pixel ones times `z`.
    fn blit(&self, buf: &mut [u32], w: usize, h: usize, text: &str, x: f32, y: f32, rgb: (u8, u8, u8), z: f32) {
        let scale = PxScale { x: self.height * self.xscale * z, y: self.height * z };
        let s = self.font.as_scaled(scale);
        let base = y + s.ascent();
        let mut pen = x;
        for c in text.chars() {
            let glyph = s.glyph_id(c).with_scale_and_position(scale, ab_glyph::point(pen, base));
            if let Some(o) = self.font.outline_glyph(glyph) {
                let b = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h && cov > 0.0 {
                        let p = &mut buf[py as usize * w + px as usize];
                        let mix = |sh: u32, v: u8| (((*p >> sh) & 255) as f32 * (1.0 - cov) + v as f32 * cov) as u8;
                        *p = pack(mix(16, rgb.0), mix(8, rgb.1), mix(0, rgb.2));
                    }
                });
            }
            pen += self.advance(c) as f32 * z;
        }
    }

    /// `0x467060`: a black copy 2 px right and down, then the white text; `(x, y)` is in natural pixels, `z` the zoom to the window.
    pub fn draw(&self, buf: &mut [u32], w: usize, h: usize, text: &str, x: i32, y: i32, z: f32) {
        self.draw_px(buf, w, h, text, x as f32 * z, y as f32 * z, z);
    }

    /// Like [`Gdi::draw`] with the top-left corner already in window pixels.
    pub fn draw_px(&self, buf: &mut [u32], w: usize, h: usize, text: &str, fx: f32, fy: f32, z: f32) {
        self.draw_rgb(buf, w, h, text, fx, fy, z, (255, 255, 255));
    }

    /// [`Gdi::draw_px`] in another text colour (the shadow stays black).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_rgb(&self, buf: &mut [u32], w: usize, h: usize, text: &str, fx: f32, fy: f32, z: f32, rgb: (u8, u8, u8)) {
        self.blit(buf, w, h, text, fx + 2.0 * z, fy + 2.0 * z, (0, 0, 0), z);
        self.blit(buf, w, h, text, fx, fy, rgb, z);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_match_the_steam_screenshots() {
        let Some(g) = Gdi::load() else { return };
        let w = g.width("First, seize Research Lab located left of your initial");
        eprintln!("width {w}");
        assert!((300..=335).contains(&w), "{w}");
    }
}
