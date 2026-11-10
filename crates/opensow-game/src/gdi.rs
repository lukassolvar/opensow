//! Stand-in for the exe's GDI text on the menus (`0x467060`: white text with a black shadow).
//!
//! The campaign map's text is `CreateFontA(13, 6, 0, 0, 400, ..., "Times New Roman")` (`0x422740`, font `[0x5ddf30]`): a cell height of 13 px and a
//! requested average character width of 6 px, which GDI reaches by widening the face. The game cannot ship the font, so it is read from the
//! system when it exists (macOS, Windows, Liberation on Linux); otherwise callers fall back to the bitmap fonts.

use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont};
use opensow_viewer::render::{blend, pack};

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

/// A glyph pixel's coverage as an 8-bit alpha (integer mixing, so the GPU replay gives the same pixels).
fn alpha(cov: f32) -> u32 {
    (cov.clamp(0.0, 1.0) * 255.0).round() as u32
}

/// Faces with CJK glyphs, tried for characters the Times face lacks (Windows substitutes a font the same way).
const FALLBACKS: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/System/Library/Fonts/STHeiti Medium.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\simsun.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
];

pub struct Gdi {
    font: FontVec,
    fallback: Option<FontVec>,
    height: f32,
    xscale: std::cell::Cell<f32>,
}

impl Gdi {
    pub fn load() -> Option<Gdi> {
        Self::load_sized(HEIGHT, XSCALE)
    }

    /// The same face with another cell height and horizontal stretch (`CreateFontA(height, width, ...)`; width 0 = natural = 1.0).
    pub fn load_sized(height: f32, xscale: f32) -> Option<Gdi> {
        CANDIDATES.iter().find_map(|p| std::fs::read(p).ok().and_then(|d| FontVec::try_from_vec(d).ok())).map(|font| {
            let fallback = FALLBACKS.iter().find_map(|p| std::fs::read(p).ok().and_then(|d| FontVec::try_from_vec(d).ok()));
            Gdi { font, fallback, height, xscale: xscale.into() }
        })
    }

    fn scale(&self) -> PxScale {
        PxScale { x: self.height * self.xscale.get(), y: self.height }
    }

    /// Advance of one character in whole pixels (GDI's hinted advances are integers).
    fn advance(&self, c: char) -> i32 {
        let f = self.face(c);
        let s = f.as_scaled(self.scale());
        s.h_advance(s.glyph_id(c)).round() as i32
    }

    /// The face that draws `c`: the Times face, or the CJK fallback when only that has the glyph.
    fn face(&self, c: char) -> &FontVec {
        match &self.fallback {
            Some(f) if self.font.glyph_id(c).0 == 0 && f.glyph_id(c).0 != 0 => f,
            _ => &self.font,
        }
    }

    /// Arial Bold (the info box's face, `[0x5ddf24]` = `CreateFontA(9, 5, ..., 700, "Arial")`) for non-English languages; the width request of 5 px
    /// is Arial's natural average at 9 px, so no stretch.
    pub fn arial_for_language(l: opensow_formats::lang::Language, height: f32) -> Option<Gdi> {
        const ARIAL: &[&str] = &[
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            "C:\\Windows\\Fonts\\arialbd.ttf",
            "/usr/share/fonts/truetype/msttcorefonts/Arial_Bold.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
            "/usr/share/fonts/liberation/LiberationSans-Bold.ttf",
            "/usr/share/fonts/TTF/LiberationSans-Bold.ttf",
        ];
        if l == opensow_formats::lang::Language::English {
            return None;
        }
        let font = ARIAL.iter().find_map(|p| std::fs::read(p).ok().and_then(|d| FontVec::try_from_vec(d).ok()))?;
        let fallback = FALLBACKS.iter().find_map(|p| std::fs::read(p).ok().and_then(|d| FontVec::try_from_vec(d).ok()));
        Some(Gdi { font, fallback, height, xscale: 1.0.into() })
    }

    /// [`Gdi::draw_flat`] squeezed horizontally (down to 60 %) so that `whole` (the full line, of which `shown` may be only the typed
    /// part) is at most `max_w` natural pixels wide: translated lines are longer than the box the English ones were laid out for.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_fit(&self, buf: &mut [u32], w: usize, h: usize, whole: &str, shown: &str, x: f32, y: f32, z: f32, rgb: (u8, u8, u8), max_w: i32) {
        let base = self.xscale.get();
        let natural = self.width(whole).max(1);
        if natural > max_w {
            self.xscale.set(base * (max_w as f32 / natural as f32).max(0.6));
        }
        self.blit(buf, w, h, shown, x, y, rgb, z);
        self.xscale.set(base);
    }

    /// Text without a shadow, top-left at `(x, y)` window pixels, zoom `z`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_flat(&self, buf: &mut [u32], w: usize, h: usize, text: &str, x: f32, y: f32, z: f32, rgb: (u8, u8, u8)) {
        self.blit(buf, w, h, text, x, y, rgb, z);
    }

    /// Menu text for non-English languages: Times New Roman bold (every menu font of the exe) of `height` px, `None` for English
    /// (the bitmap stand-ins stay) or when no system font is found.
    pub fn for_language(l: opensow_formats::lang::Language, height: f32) -> Option<Gdi> {
        (l != opensow_formats::lang::Language::English).then(|| Gdi::load_sized(height, 1.0)).flatten()
    }

    pub fn width(&self, text: &str) -> i32 {
        text.chars().map(|c| self.advance(c)).sum()
    }

    /// Plain text at zoom `z` with its top-left corner at `(x, y)` (window pixels); advances are the natural whole-pixel ones times `z`.
    fn blit(&self, buf: &mut [u32], w: usize, h: usize, text: &str, x: f32, y: f32, rgb: (u8, u8, u8), z: f32) {
        let scale = PxScale { x: self.height * self.xscale.get() * z, y: self.height * z };
        let base = y + self.font.as_scaled(scale).ascent();
        let mut pen = x;
        for c in text.chars() {
            let face = self.face(c);
            let glyph = face.glyph_id(c).with_scale_and_position(scale, ab_glyph::point(pen, base));
            if let Some(o) = face.outline_glyph(glyph) {
                let b = o.px_bounds();
                if opensow_viewer::record::active() {
                    // the coverage grid of the glyph, replayed with the same mix
                    let (bw, bh) = (b.width().ceil() as usize + 1, b.height().ceil() as usize + 1);
                    let mut grid = vec![0u32; bw * bh];
                    o.draw(|gx, gy, cov| {
                        if (gx as usize) < bw && (gy as usize) < bh {
                            grid[gy as usize * bw + gx as usize] = alpha(cov);
                        }
                    });
                    opensow_viewer::record::cover(b.min.x as i32, b.min.y as i32, bw, &grid, pack(rgb.0, rgb.1, rgb.2));
                    pen += self.advance(c) as f32 * z;
                    continue;
                }
                o.draw(|gx, gy, cov| {
                    let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                    let a = alpha(cov);
                    if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h && a > 0 {
                        let p = &mut buf[py as usize * w + px as usize];
                        *p = blend(*p, rgb.0, rgb.1, rgb.2, a);
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
        self.draw_shadowed(buf, w, h, text, fx, fy, z, rgb, (0, 0, 0));
    }

    /// [`Gdi::draw_rgb`] with a shadow colour of its own (the HUD messages use a dark red one).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_shadowed(&self, buf: &mut [u32], w: usize, h: usize, text: &str, fx: f32, fy: f32, z: f32, rgb: (u8, u8, u8), shadow: (u8, u8, u8)) {
        self.blit(buf, w, h, text, fx + 2.0 * z, fy + 2.0 * z, shadow, z);
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
