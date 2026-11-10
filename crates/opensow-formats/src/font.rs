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
/// The glyph a **byte >= 0x80** really draws in the exe (`0x461ac0` / `0x45ed50` read the byte with `movsx`, so it is negative, and index the
/// glyph table `font * 100 + c`): the slot falls into the arrays of the fonts before it. The ten fonts are loaded one after the other at slot
/// `f * 100 + 32` with room for 100 frames (`0x425af1..0x425c3e`: 0 small_i, 1 small_o, 2 medium_i, 3 medium_o, 4 big_i, 5 big_o, 6 msg_i,
/// 7 msg_o, 8 font02_dark, 9 font02_light). Returns `(font, char)` of the glyph that is drawn (char 32..=125), or `None` when the slot holds no
/// glyph of any font (slots before font 0 are other sprite arrays, slots 126..131 of a font are empty). Bytes below 0x80 return themselves.
pub fn glyph_of_byte(font: i32, byte: u8) -> Option<(i32, u8)> {
    let slot = font * 100 + (byte as i8) as i32;
    let g = (slot - 32).div_euclid(100);
    let ch = slot - g * 100;
    if g < 0 || !(32..=125).contains(&ch) {
        return None;
    }
    Some((g, ch as u8))
}

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
    #[test]
    fn high_bytes_reach_into_the_previous_fonts() {
        assert_eq!(glyph_of_byte(2, b'A'), Some((2, b'A')));
        assert_eq!(glyph_of_byte(2, 0xff), Some((1, b'c'))); // -1 -> slot 199 = font 1, char 99
        assert_eq!(glyph_of_byte(2, 0xc3), Some((1, 39))); // -61 -> slot 139
        assert_eq!(glyph_of_byte(2, 0xbc), Some((1, 32))); // -68 -> slot 132
        assert_eq!(glyph_of_byte(2, 0xb0), Some((0, 120))); // -80 -> slot 120 = font 0
        assert_eq!(glyph_of_byte(2, 0xbb), None); // -69 -> slot 131: empty room after font 0's glyphs
        assert_eq!(glyph_of_byte(1, 0xff), Some((0, b'c')));
        assert_eq!(glyph_of_byte(0, 0xff), None); // before the table
    }

    use super::*;

    fn glyph(w: u16, h: u16, hot_y: i16) -> Frame {
        Frame { width: w, height: h, hot_x: -1, hot_y, extra: [0; 4], pixels: vec![0; (w * h) as usize], mask: vec![true; (w * h) as usize], enhanced: Default::default() }
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
