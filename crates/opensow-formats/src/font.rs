//! Bitmap fonts: `extras\fonts\{small,medium,big,msg}_{i,o}.msk` (coverage masks) and the coloured
//! `font02_*`, `font_msg2`, `font_briefing_*` `.ps6` files. Every file is a normal sprite file
//! ([`crate::sprite`]) in which frame `k` is the glyph of character `32 + k`.
//!
//! Layout rules recovered from the exe (text width at `0x45ed50`, string draw at `0x461ac0`):
//! - the glyph table is indexed by `font * 100 + char` from a base 32 slots before the first glyph, so only
//!   characters 32..=125 exist (94 glyphs). Characters outside that range have no glyph;
//! - a space is never drawn and advances 4 px (spacing is not added);
//! - any other character draws its glyph with its left edge at the cursor and `-hot_y` pixels below the line top
//!   (hot_x/hot_y are negative bearings; all glyphs then share a baseline), then advances by `width + spacing`;
//!   a character without a glyph advances by `spacing - 1`;
//! - the game draws the `_o` (outline) mask first in one colour and the `_i` (inner) mask over it in another.
//!   The two masks nest only if both are placed at `pen - hot` in x and y (inner `A` is 17x16 with hot (-2,-2), outer
//!   21x20 with hot (0,0)): [`Placed::left`] is that x. The exe's own x arithmetic (`pen + hot_x - hot_x`) cancels the
//!   hotspot, so unoutlined text starts at the pen ([`Placed::x`]); how it offsets the outline pass is not traced;
//! - `|` (0x7c) is drawn 2 px to the right of the cursor.

use crate::sprite::Frame;

pub const FIRST_CHAR: u32 = 32;
pub const SPACE_ADVANCE: i32 = 4;
/// Characters drawn 2 px further right than the cursor (without changing the advance).
pub const PIPE: char = '|';

pub struct Font {
    glyphs: Vec<Frame>,
}

/// One glyph to draw: its frame and the top-left corner relative to the start of the line.
pub struct Placed<'a> {
    pub ch: char,
    pub glyph: &'a Frame,
    /// Pen position (the exe's left edge for plain text).
    pub x: i32,
    /// Left edge when masks are aligned by their hotspots (`pen - hot_x`), needed to nest `_i` in `_o`.
    pub left: i32,
    pub y: i32,
}

impl Font {
    pub fn new(glyphs: Vec<Frame>) -> Font {
        Font { glyphs }
    }

    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    pub fn glyph(&self, c: char) -> Option<&Frame> {
        let k = (c as u32).checked_sub(FIRST_CHAR)?;
        self.glyphs.get(k as usize)
    }

    /// Horizontal advance of one character with the given extra spacing.
    pub fn advance(&self, c: char, spacing: i32) -> i32 {
        if c == ' ' {
            return SPACE_ADVANCE;
        }
        match self.glyph(c) {
            Some(g) => g.width as i32 + spacing,
            None => spacing - 1,
        }
    }

    pub fn measure(&self, text: &str, spacing: i32) -> i32 {
        text.chars().map(|c| self.advance(c, spacing)).sum()
    }

    /// Height of the tallest glyph's bottom edge below the line top (a usable line height).
    pub fn line_height(&self) -> i32 {
        self.glyphs.iter().map(|g| -(g.hot_y as i32) + g.height as i32).max().unwrap_or(0)
    }

    /// Glyph positions for `text`, relative to the line start.
    pub fn layout<'a>(&'a self, text: &str, spacing: i32) -> Vec<Placed<'a>> {
        let mut x = 0;
        let mut out = Vec::new();
        for ch in text.chars() {
            if ch != ' ' {
                if let Some(glyph) = self.glyph(ch) {
                    let dx = if ch == PIPE { 2 } else { 0 };
                    out.push(Placed { ch, glyph, x: x + dx, left: x + dx - glyph.hot_x as i32, y: -(glyph.hot_y as i32) });
                }
            }
            x += self.advance(ch, spacing);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(w: u16, h: u16, hot_y: i16) -> Frame {
        Frame { width: w, height: h, hot_x: -1, hot_y, extra: [0; 4], pixels: vec![0; (w * h) as usize], mask: vec![true; (w * h) as usize] }
    }

    fn font() -> Font {
        // frame k = char 32+k: ' ' (k=0), '!' (1), '"' (2), '#' (3)
        Font::new(vec![glyph(4, 8, -2), glyph(3, 8, -2), glyph(5, 2, -2), glyph(6, 9, -1)])
    }

    #[test]
    fn indexing_and_advance() {
        let f = font();
        assert_eq!(f.glyph('!').unwrap().width, 3);
        assert_eq!(f.glyph('#').unwrap().width, 6);
        assert!(f.glyph('$').is_none() && f.glyph('\u{1f}').is_none() && f.glyph('č').is_none());
        assert_eq!(f.advance(' ', 5), 4);
        assert_eq!(f.advance('!', 1), 4);
        assert_eq!(f.advance('$', 1), 0);
        assert_eq!(f.measure("! #", 1), 4 + 4 + 7);
    }

    #[test]
    fn layout_positions() {
        let f = font();
        let l = f.layout("!  #", 1);
        assert_eq!((l[0].x, l[0].left, l[0].y), (0, 1, 2));
        assert_eq!((l[1].x, l[1].y), (4 + 4 + 4, 1));
        assert_eq!(f.line_height(), 10);
    }

    #[test]
    fn pipe_is_shifted_but_does_not_widen() {
        let mut g = vec![glyph(1, 1, 0); 100];
        g[(('|' as u32) - FIRST_CHAR) as usize] = glyph(3, 3, 0);
        let f = Font::new(g);
        let l = f.layout("a|", 0);
        assert_eq!(l[1].x, 1 + 2);
        assert_eq!(f.measure("a|", 0), 1 + 3);
    }
}
