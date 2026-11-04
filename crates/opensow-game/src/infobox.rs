//! The info box (docs/hud.md, "Info box"): a terminal that slides in from the top right for a selected building or a hovered
//! icon, flashes blue static, types the unit's stats next to its animated picture, then types its description.
//!
//! State machine of `0x43dfd0` (mode switch), `0x43cb30` (per tick), `0x454f4b` (slide arrival), `0x455b3b` (selection):
//! mode 0 hidden, 1 static while sliding in (sound `infobox-income` 51 on entering from hidden), 2 blue static for two
//! cycles of six frames at two ticks each, 4 stats typed at one character per three ticks, then the picture runs 100 steps
//! of two ticks, 3 the locked-slot page ("No Info Available" typed over the static), 5 description typed, 6 static while sliding out (sound `infobox-outcome` 52 unless coming from hidden).

use crate::assets::Assets;
use crate::ui::{self, Rect};
use opensow_formats::infobox::{self, InfoLines, LINE_POS};

/// Anchor y when hidden and when shown (`0x455b6c..0x455bb8`), speed in px per tick (`0x455b80`); x is -65 from the right edge.
pub const Y_HIDDEN: i32 = -91;
pub const Y_SHOWN: i32 = 99;
const SPEED: i32 = 13;
/// Text width limit of the description (`0x43e2bd`: 0x69), in unscaled pixels.
const WRAP_WIDTH: i32 = 105;

#[derive(Default)]
pub struct InfoBox {
    pub mode: u8,
    /// Current anchor y (the widget position).
    pub y: i32,
    target_y: i32,
    pub kind: Option<i32>,
    /// The hovered icon is a locked production slot (the third argument of `0x43cde0`): mode 3 instead of the stats.
    pub locked: bool,
    /// Text of the locked page (lang string 282, `0x43dea3`).
    pub no_info: String,
    /// Frame index of the static animation and its tick counter.
    idx: i32,
    idx_timer: i32,
    blue_cycles: i32,
    typed: i32,
    type_timer: i32,
    done: bool,
    pic_steps: i32,
    pic_timer: i32,
    /// Picture frame (advances every 7 ticks).
    pic_frame: i32,
    pic_timer2: i32,
    first_line: usize,
    /// Interface sounds raised by mode changes, drained by the game.
    pub sounds: Vec<u8>,
}

impl InfoBox {
    pub fn new() -> InfoBox {
        InfoBox { y: Y_HIDDEN, target_y: Y_HIDDEN, pic_frame: 99, ..Default::default() }
    }

    pub fn visible(&self) -> bool {
        self.mode != 0
    }

    /// Selects what the box shows: `Some(kind)` for a selected building or hovered icon, `None` to hide (`0x455b3b`, `0x43cde0`).
    pub fn show(&mut self, kind: Option<i32>, locked: bool) {
        let locked = locked && kind.is_some();
        if kind == self.kind && locked == self.locked {
            return;
        }
        self.kind = kind;
        self.locked = locked;
        match kind {
            Some(_) => {
                self.target_y = Y_SHOWN;
                match self.mode {
                    0 | 6 => {
                        self.mode = 1;
                        self.sounds.push(51); // infobox-income (0x43e066)
                    }
                    1 => {}
                    _ => self.restart_static(), // 0x43cf20: forced back to the blue static
                }
                self.pic_frame = 99;
                self.pic_timer2 = 0;
            }
            None => {
                if self.mode != 0 && self.mode != 6 {
                    self.mode = 6;
                    self.sounds.push(52); // infobox-outcome (0x43e0a8)
                }
                self.target_y = Y_HIDDEN;
            }
        }
    }

    fn restart_static(&mut self) {
        self.mode = 2;
        self.idx = 0;
        self.idx_timer = 0;
        self.blue_cycles = 1;
    }

    /// One game tick (`0x43cb30` and the slide `0x4560d0`).
    pub fn tick(&mut self, lines: Option<&InfoLines>, total_chars: i32, desc_lines: &[String]) {
        if self.mode == 0 {
            return;
        }
        // slide
        if self.y < self.target_y {
            self.y = (self.y + SPEED).min(self.target_y);
        } else if self.y > self.target_y {
            self.y = (self.y - SPEED).max(self.target_y);
        }
        match self.mode {
            1 if self.y == Y_SHOWN && self.kind.is_some() => self.restart_static(),
            6 if self.y == Y_HIDDEN => {
                self.mode = 0;
                return;
            }
            _ => {}
        }
        // picture animation: a frame every 7 ticks over 32 (or 16) frames
        self.pic_timer2 -= 1;
        if self.pic_timer2 < 0 {
            self.pic_timer2 = 6;
            self.pic_frame += 1;
            let frames = self.kind.map_or(32, |k| infobox::picture_frames(k)) as i32;
            if self.pic_frame >= frames {
                self.pic_frame = 0;
            }
        }
        // static animation index
        let (period, count) = match self.mode {
            2 => (2, 6),
            3 | 4 | 5 => (9, 6),
            _ => (0, 0),
        };
        if period > 0 {
            self.idx_timer += 1;
            if self.idx_timer >= period {
                self.idx_timer = 0;
                self.idx += 1;
                if self.idx >= count {
                    self.idx = 0;
                    if self.mode == 2 {
                        self.blue_cycles -= 1;
                        if self.blue_cycles < 0 {
                            self.start_stats();
                        }
                    }
                }
            }
        }
        match self.mode {
            3 => {
                // `[+0x38]` = 3: a character every four ticks, up to the text length + 4 (`0x43cb58`, `0x43deb7..0x43dedc`)
                self.type_timer += 1;
                if self.type_timer >= 4 {
                    self.type_timer = 0;
                    self.typed = (self.typed + 1).min(self.no_info.chars().count() as i32 + 4);
                }
            }
            4 => {
                self.type_timer += 1;
                if self.type_timer >= 3 {
                    self.type_timer = 0;
                    if self.typed < total_chars {
                        self.typed += 1;
                    } else if !self.done {
                        self.done = true;
                        self.pic_steps = 0;
                        self.pic_timer = 0;
                    }
                }
                if self.done {
                    self.pic_timer += 1;
                    if self.pic_timer >= 2 {
                        self.pic_timer = 0;
                        self.pic_steps += 1;
                        if self.pic_steps >= 100 {
                            self.start_description();
                        }
                    }
                }
            }
            5 => {
                self.type_timer += 1;
                if self.type_timer >= 3 {
                    self.type_timer = 0;
                    let total: i32 = desc_lines.iter().map(|l| l.chars().count() as i32 + 1).sum();
                    if self.typed < total {
                        self.typed += 1;
                    }
                    // scroll: the first visible line goes once it is typed (0x43b8f7..0x43b918)
                    let first_end: i32 = desc_lines.iter().take(self.first_line + 1).map(|l| l.chars().count() as i32 + 1).sum();
                    if self.typed < total - 4 && self.first_line + 5 < desc_lines.len() && self.typed >= first_end - 3 {
                        self.first_line += 1;
                    }
                }
            }
            _ => {}
        }
        let _ = lines;
    }

    fn start_stats(&mut self) {
        // `0x43cc17`: a locked entry (`[+0x58]` == 0) goes to mode 3, everything else to the stats
        self.mode = if self.locked { 3 } else { 4 };
        self.typed = 0;
        self.type_timer = 0;
        self.done = false;
        self.idx = 0;
        self.idx_timer = 0;
    }

    fn start_description(&mut self) {
        self.mode = 5;
        self.typed = 0;
        self.type_timer = 0;
        self.first_line = 0;
        self.idx = 0;
        self.idx_timer = 0;
    }

    /// The frame rectangle on screen (for swallowing clicks), `s` the HUD scale.
    pub fn rect(&self, w: i32, s: f32) -> Option<Rect> {
        if self.mode == 0 {
            return None;
        }
        let u = |v: i32| (v as f32 * s).round() as i32;
        Some(Rect { x: w - u(65) - u(65), y: u(self.y) - u(99), w: u(130), h: u(173) })
    }
}

/// Characters the stat lines hold in typing order (`0x43b2c0` consumes the counter line by line).
pub fn total_chars(l: &InfoLines) -> i32 {
    [&l.name, &l.kind_line, &l.offence, &l.scope, &l.defence, &l.speed, &l.shell].iter().map(|s| s.chars().count() as i32).sum()
}

/// Wraps the description to the box (`0x43e1ba`).
pub fn wrap_description(font: &(opensow_formats::font::Font, opensow_formats::font::Font), text: &str) -> Vec<String> {
    infobox::wrap(text, WRAP_WIDTH, |s| ui::text_width(font, s, 1.0))
}

/// Draws the box at the top right: frame, window (static, picture), text. `w` is the screen width, `s` the HUD scale.
pub fn draw(b: &InfoBox, lines: Option<&InfoLines>, desc: &[String], assets: &mut Assets, buf: &mut [u32], w: usize, h: usize, s: f32) {
    if b.mode == 0 {
        return;
    }
    let u = |v: i32| (v as f32 * s).round() as i32;
    let (ax, ay) = (w as i32 - u(65), u(b.y));
    let boxes = assets.art.load("extras\\infobox.ps6");
    let Some(set) = boxes else { return };
    // the window first (static or picture), the frame over it, then the text on the lower screen
    let static_frame = match b.mode {
        1 | 6 => 2,
        2 => 21 + b.idx.clamp(0, 5) as usize,
        _ => 2 + b.idx.clamp(0, 5) as usize,
    };
    let hot = |f: &opensow_formats::sprite::Frame| (u(f.hot_x as i32), u(f.hot_y as i32));
    if let Some(f) = set.get(static_frame) {
        let (hx, hy) = hot(f);
        ui::sprite(buf, w, h, f, ax - hx, ay - hy, s);
    }
    if b.mode == 4 {
        if let Some((path, off)) = b.kind.and_then(infobox::picture) {
            if let Some(pic) = assets.infobox_picture(path) {
                if let Some(f) = pic.get(b.pic_frame.clamp(0, pic.len() as i32 - 1) as usize) {
                    let (hx, hy) = hot(f);
                    ui::sprite(buf, w, h, f, ax - hx, ay + u(off) - hy, s);
                }
            }
        }
    }
    if let Some(f) = set.first() {
        let (hx, hy) = hot(f);
        ui::sprite(buf, w, h, f, ax - hx, ay - hy, s);
    }
    let font = assets.font("small");
    let white = (255, 255, 255);
    let put = |buf: &mut [u32], text: &str, dx: i32, dy: i32, typed_left: &mut i32, caret: &mut bool| {
        if *typed_left <= 0 {
            return;
        }
        let n = text.chars().count() as i32;
        let shown: String = text.chars().take((*typed_left).min(n) as usize).collect();
        let mut out = shown;
        if *typed_left < n && !*caret {
            out.push('|');
            *caret = true;
        }
        *typed_left -= n;
        ui::text(buf, w, h, &font, &out, ax + u(dx), ay + u(dy), s, white, (0, 0, 0));
    };
    match (b.mode, lines) {
        (4, Some(l)) => {
            let order = [&l.name, &l.kind_line, &l.offence, &l.scope, &l.defence, &l.speed, &l.shell];
            let (mut left, mut caret) = (b.typed, false);
            for (text, &(dx, dy)) in order.iter().zip(LINE_POS.iter()) {
                put(buf, text, dx, dy, &mut left, &mut caret);
            }
        }
        (5, Some(_)) => {
            let (mut left, mut caret) = (b.typed, false);
            let before: i32 = desc.iter().take(b.first_line).map(|l| l.chars().count() as i32 + 1).sum();
            left -= before;
            for (i, text) in desc.iter().skip(b.first_line).take(5).enumerate() {
                put(buf, text, -54, 13 + 9 * i as i32, &mut left, &mut caret);
                left -= 1;
            }
        }
        _ => {}
    }
    if b.mode == 3 {
        // `0x43b45c`: the typed prefix at (x - 50, y - 50), with a caret while it is typing
        let n = b.no_info.chars().count() as i32;
        let mut out: String = b.no_info.chars().take(b.typed.min(n).max(0) as usize).collect();
        if b.typed < n {
            out.push('|');
        }
        ui::text(buf, w, h, &font, &out, ax - u(50), ay - u(50), s, white, (0, 0, 0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(b: &mut InfoBox, n: usize, total: i32) {
        for _ in 0..n {
            b.tick(None, total, &[]);
        }
    }

    #[test]
    fn opens_with_a_sound_slides_flashes_types_and_closes_with_a_sound() {
        let mut b = InfoBox::new();
        assert!(!b.visible());
        b.show(Some(100), false);
        assert_eq!(b.mode, 1);
        assert_eq!(b.sounds, vec![51]); // infobox-income
        run(&mut b, 15, 60); // 190 px at 13 per tick
        assert_eq!(b.y, Y_SHOWN);
        assert_eq!(b.mode, 2, "blue static after arriving");
        run(&mut b, 24, 60);
        assert_eq!(b.mode, 4, "stats after two blue cycles");
        run(&mut b, 3 * 60 + 2, 60);
        assert!(b.done, "all 60 characters typed");
        run(&mut b, 200, 60);
        assert_eq!(b.mode, 5, "description after the picture's 100 steps");
        b.sounds.clear();
        b.show(None, false);
        assert_eq!(b.mode, 6);
        assert_eq!(b.sounds, vec![52]); // infobox-outcome
        run(&mut b, 20, 60);
        assert_eq!(b.mode, 0);
        assert!(!b.visible());
    }

    #[test]
    fn a_locked_entry_types_its_no_info_text_instead_of_the_stats() {
        let mut b = InfoBox::new();
        b.no_info = "No Info Available".into();
        b.show(Some(7), true);
        run(&mut b, 15 + 24, 60);
        assert_eq!(b.mode, 3);
        run(&mut b, 4 * 5, 60);
        assert_eq!(b.typed, 5, "one character every four ticks");
        run(&mut b, 4 * 100, 60);
        assert_eq!(b.typed, 21, "text length 17 + 4, then it stays");
        assert_eq!(b.mode, 3, "mode 3 never ends by itself");
        // hovering an unlocked slot again goes back to the stats
        b.show(Some(7), false);
        assert_eq!(b.mode, 2);
    }

    #[test]
    fn switching_kinds_while_open_restarts_the_static() {
        let mut b = InfoBox::new();
        b.show(Some(1), false);
        run(&mut b, 100, 60);
        assert!(b.mode >= 2);
        b.sounds.clear();
        b.show(Some(2), false);
        assert_eq!(b.mode, 2);
        assert!(b.sounds.is_empty(), "no new open sound");
    }
}
