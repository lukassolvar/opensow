//! The tutorial's instruction panel (docs/tutorial.md): word wrap `0x431900`, layout and draw `0x430fd0`, the arrow button's
//! slide `0x456300` / `0x4377e0` and its spin (`0x432c4a`). Coordinates are the exe's native pixels; the caller scales them.
//!
//! The text is wrapped with the bitmap font 2 (`medium_i`) measure, drawn with the GDI font of the campaign map at x = 48, 20 px per
//! line, the last line 25 px above the bottom edge; behind it a darkened box (`0x460a00`), the spinning "i" / "!" coin at the left
//! and the arrow button that slides in from the left edge to the right of the box and spins while the pointer is over it.

use opensow_formats::font::Font;

/// Widest line the wrap allows before it breaks (`cmp eax, 0x12c`).
pub const WRAP_WIDTH: i32 = 300;
/// The exe keeps ten line strings; line 0 holds the marker, so at most nine text lines.
pub const MAX_LINES: usize = 9;
/// The text's left edge and the line pitch.
pub const TEXT_X: i32 = 48;
/// Pixels from one text line to the next.
pub const LINE_PITCH: i32 = 20;
/// Distance of the last line's top from the bottom edge.
pub const LAST_LINE_UP: i32 = 25;
/// The box's left edge; its right edge is the widest line + [`BOX_RIGHT_PAD`] (`[0x70f2e4] + 8`, with `[0x70f2e4]` = widest + 50).
pub const BOX_LEFT: i32 = 31;
/// The coin's x and the arrow's start x (`0x456470(-0x50, ..)`), speed (`[+0x14]` = 6 steps per tick) and hit box (`0x456490`).
pub const COIN_X: i32 = 15;
/// X the arrow button starts from, left of the screen.
pub const ARROW_START_X: i32 = -80;
/// Pixels the arrow moves per tick.
pub const ARROW_SPEED: i32 = 6;
/// Size of the arrow's hit box.
pub const ARROW_HIT: (i32, i32) = (21, 19);
/// The arrow button's target x is `[0x70f2e4] + 0x1e` = widest + 80.
pub const ARROW_TARGET_PAD: i32 = 80;
/// Frames of `extras\tutorialgfx`: 16 per animation, "i" coin first, the arrow, then the "!" coin (`0x5e9440`, `0x5e9480`, `0x5e94c0`).
pub const COIN_I: usize = 0;
/// First frame of the arrow's animation.
pub const ARROW: usize = 16;
/// First frame of the "!" coin's animation.
pub const COIN_TASK: usize = 32;
/// Ticks per frame of the coin (`[0x70f30c]` reloads 5) and of the arrow (`[0x70f314]` reloads 3), 16 frames each.
pub const COIN_PERIOD: u32 = 6;
/// Ticks per frame of the arrow.
pub const ARROW_PERIOD: u32 = 4;
/// Sound of the arrow click (`[+0x58] = 0x21`, played by `0x456410`).
pub const CLICK_SOUND: u8 = 33;

/// A step's text, wrapped (`0x431900`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrapped {
    /// The first character of the string: `i` information, `!` task (anything else draws no coin).
    pub marker: char,
    /// Text lines in draw order (top first).
    pub lines: Vec<String>,
    /// `[0x70f2e4]`: the widest line measured with spacing 2, plus 50.
    pub width: i32,
}

impl Wrapped {
    /// `[0x70f2e8]`: the number of line breaks (text lines - 1).
    pub fn breaks(&self) -> i32 {
        self.lines.len() as i32 - 1
    }
}

/// Word wrap of `text` (`marker`, one separator, then the words), byte by byte like the exe's `CString::GetAt`.
/// `measure(bytes, spacing)` is the width of `bytes` in font 2 (`0x45ed50`, see [`byte_width`]).
///
/// The exe appends bytes to a word up to and including a space, adds the word's width (spacing 0) to the line and breaks before
/// the word when the line reaches 300 px; after the last word it measures every line with spacing 2. With more than nine lines it
/// stops without computing the widths (width stays 0). Breaks only come after spaces, so UTF-8 sequences are never split.
pub fn wrap(text: &str, measure: &dyn Fn(&[u8], i32) -> i32) -> Wrapped {
    let bytes = text.as_bytes();
    let marker = bytes.first().map_or(' ', |&b| b as char);
    let mut lines: Vec<Vec<u8>> = vec![Vec::new()];
    let (mut i, mut line_w) = (2, 0);
    let mut overflow = false;
    loop {
        let mut word = Vec::new();
        loop {
            let c = bytes.get(i).copied().unwrap_or(b' ');
            i += 1;
            word.push(c);
            if c == b' ' || i >= bytes.len() {
                break;
            }
        }
        let ww = measure(&word, 0);
        line_w += ww;
        if line_w < WRAP_WIDTH {
            lines.last_mut().unwrap().extend_from_slice(&word);
        } else {
            if lines.len() >= MAX_LINES {
                overflow = true;
                break;
            }
            lines.push(word);
            line_w = ww;
        }
        if i >= bytes.len() {
            break;
        }
    }
    let width = if overflow { 0 } else { lines.iter().map(|l| measure(l, 2)).max().unwrap_or(0) + 50 };
    let lines = lines.iter().map(|l| String::from_utf8_lossy(l).into_owned()).collect();
    Wrapped { marker, lines, width }
}

/// The exe's width of `bytes` in font 2 (`0x45ed50(text, spacing, 2)`): a space is 4 px, any other byte reads the glyph table slot
/// `font * 100 + signed byte`, so bytes >= 0x80 reach back into fonts 0 and 1 (slot 72..=125 is font 0's glyph `slot - 32`, 126..=131
/// is empty, 132..=199 is font 1's glyph `slot - 132`); a glyph adds its width + spacing, a missing one `spacing - 1`.
/// `small` is `(small_i, small_o)` = fonts 0 and 1, `medium` font 2.
pub fn byte_width(small: &(Font, Font), medium: &Font, bytes: &[u8], spacing: i32) -> i32 {
    bytes
        .iter()
        .map(|&b| {
            if b == b' ' {
                return 4;
            }
            let glyph = |f: &Font, k: i32| f.glyph(char::from_u32(32 + k as u32).unwrap_or('\0')).map(|g| g.width as i32);
            let w = if b < 0x80 {
                glyph(medium, b as i32 - 32)
            } else {
                let slot = 200 + b as i8 as i32;
                match slot {
                    72..=125 => glyph(&small.0, slot - 32),
                    132..=199 => glyph(&small.1, slot - 132),
                    _ => None,
                }
            };
            w.map_or(spacing - 1, |w| w + spacing)
        })
        .sum()
}

/// The panel while a step's text is shown.
#[derive(Debug, Clone)]
pub struct Panel {
    /// The step's text, wrapped into lines.
    pub text: Wrapped,
    /// The arrow button's x (native pixels from the left edge; its y is [`Panel::arrow_y`] from the bottom).
    pub arrow_x: i32,
    /// Coin and arrow frame counters.
    coin: (usize, u32),
    arrow: (usize, u32),
}

impl Panel {
    /// A panel for a new text: the arrow starts off screen.
    pub fn new(text: Wrapped) -> Panel {
        Panel { text, arrow_x: ARROW_START_X, coin: (0, COIN_PERIOD - 1), arrow: (0, ARROW_PERIOD - 1) }
    }

    /// One tick (`0x432c4a` counters, `0x456300` while the button is up): the coin spins always, the arrow slides towards its
    /// target 6 pixels a tick while it waits for a click, and spins only while the pointer is on it (its frame is 0 otherwise).
    pub fn tick(&mut self, arrow_up: bool, hover: bool) {
        self.coin = advance(self.coin, COIN_PERIOD);
        if hover {
            self.arrow = advance(self.arrow, ARROW_PERIOD);
        } else {
            self.arrow = (0, ARROW_PERIOD - 1);
        }
        if arrow_up {
            self.arrow_x = (self.arrow_x + ARROW_SPEED).min(self.arrow_target());
        }
    }

    /// The x the arrow button moves to.
    pub fn arrow_target(&self) -> i32 {
        self.text.width + ARROW_TARGET_PAD - 50
    }

    /// Vertical centre of the panel relative to the bottom edge (negative): coin and arrow sit on it.
    pub fn arrow_y(&self) -> i32 {
        -20 - 10 * self.text.breaks()
    }

    /// The coin's frame, `None` when the text has no marker.
    pub fn coin_frame(&self) -> Option<usize> {
        match self.text.marker {
            'i' => Some(COIN_I + self.coin.0),
            '!' => Some(COIN_TASK + self.coin.0),
            _ => None,
        }
    }

    /// The arrow's frame.
    pub fn arrow_frame(&self) -> usize {
        ARROW + self.arrow.0
    }

    /// The dark box: `(left, top from the bottom, right, bottom from the bottom)` (all negative from the bottom edge for y).
    pub fn box_rect(&self) -> (i32, i32, i32, i32) {
        (BOX_LEFT, -(37 + LINE_PITCH * self.text.breaks()), self.text.width + 8, -5)
    }

    /// The coin's centre, relative to the bottom edge for y.
    pub fn coin_pos(&self) -> (i32, i32) {
        (COIN_X, -(21 + 10 * self.text.breaks()))
    }

    /// Top of text line `k` (0 = first), relative to the bottom edge.
    pub fn line_y(&self, k: usize) -> i32 {
        -(LAST_LINE_UP + LINE_PITCH * (self.text.lines.len() as i32 - 1 - k as i32))
    }

    /// Whether native pointer position `(x, y_from_bottom)` is on the arrow button.
    pub fn on_arrow(&self, x: i32, y: i32) -> bool {
        let (hx, hy) = ARROW_HIT;
        (x - self.arrow_x).abs() < hx && (y - self.arrow_y()).abs() < hy
    }
}

/// `[counter] -= 1; if < 0 { frame = (frame + 1) % 16; counter = period - 1 }` (`0x432c70`).
fn advance((frame, count): (usize, u32), period: u32) -> (usize, u32) {
    if count == 0 {
        ((frame + 1) % 16, period - 1)
    } else {
        (frame, count - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed-pitch stand-in for the bitmap font: 7 px per glyph + spacing, space 4.
    fn measure(s: &[u8], spacing: i32) -> i32 {
        s.iter().map(|&c| if c == b' ' { 4 } else { 7 + spacing }).sum()
    }

    #[test]
    fn short_text_is_one_line_with_its_marker_cut_off() {
        let w = wrap("i Hello there ", &measure);
        assert_eq!(w.marker, 'i');
        assert_eq!(w.lines, ["Hello there "]);
        assert_eq!(w.breaks(), 0);
        assert_eq!(w.width, measure(b"Hello there ", 2) + 50);
    }

    #[test]
    fn words_break_before_the_word_that_reaches_300_pixels() {
        let text = format!("! {}", "abcde ".repeat(30));
        let w = wrap(&text, &measure);
        assert!(w.lines.len() > 2);
        for l in &w.lines[..w.lines.len() - 1] {
            assert!(measure(l.as_bytes(), 0) < WRAP_WIDTH, "{l:?}");
        }
        // the break happens before the word that would make the line 300 or wider
        let first = measure(w.lines[0].as_bytes(), 0);
        assert!(first + measure(b"abcde ", 0) >= WRAP_WIDTH);
        assert_eq!(w.marker, '!');
        assert_eq!(w.lines.concat(), "abcde ".repeat(30));
    }

    #[test]
    fn non_ascii_text_is_measured_per_byte_and_kept_whole() {
        // "č" is C4 8D: two bytes, each a glyph of the earlier fonts in the exe, so each counts as one character
        let w = wrap("i č č č ", &|s, sp| s.iter().map(|&c| if c == b' ' { 4 } else { 7 + sp }).sum());
        assert_eq!(w.lines, ["č č č "]);
        assert_eq!(w.width, 4 * 3 + 6 * 9 + 50, "6 non-space bytes of 9 px + 3 spaces");
    }

    #[test]
    fn the_layout_hangs_from_the_bottom_edge() {
        let mut p = Panel::new(Wrapped { marker: 'i', lines: vec!["a".into(), "b".into(), "c".into()], width: 200 });
        assert_eq!(p.text.breaks(), 2);
        assert_eq!(p.line_y(2), -25);
        assert_eq!(p.line_y(0), -65);
        assert_eq!(p.box_rect(), (31, -77, 208, -5));
        assert_eq!(p.coin_pos(), (15, -41));
        assert_eq!(p.arrow_y(), -40);
        // the arrow slides 6 px a tick from x = -80 to the widest line + 80 and stops there
        assert_eq!(p.arrow_x, -80);
        p.tick(true, false);
        assert_eq!(p.arrow_x, -74);
        for _ in 0..200 {
            p.tick(true, false);
        }
        assert_eq!(p.arrow_x, 230);
        assert!(p.on_arrow(230, -40) && !p.on_arrow(252, -40) && !p.on_arrow(230, -21));
    }

    #[test]
    fn coin_turns_every_six_ticks_and_the_arrow_only_while_hovered() {
        let mut p = Panel::new(Wrapped { marker: '!', lines: vec!["x".into()], width: 60 });
        assert_eq!(p.coin_frame(), Some(COIN_TASK));
        for _ in 0..6 {
            p.tick(true, false);
        }
        assert_eq!(p.coin_frame(), Some(COIN_TASK + 1));
        assert_eq!(p.arrow_frame(), ARROW);
        for _ in 0..4 {
            p.tick(true, true);
        }
        assert_eq!(p.arrow_frame(), ARROW + 1);
        p.tick(true, false);
        assert_eq!(p.arrow_frame(), ARROW, "the frame goes back to 0 when the pointer leaves");
        for _ in 0..(16 * 6) {
            p.tick(false, false);
        }
        assert_eq!(p.coin_frame(), Some(COIN_TASK + 1), "16 frames, then round again");
    }
}
