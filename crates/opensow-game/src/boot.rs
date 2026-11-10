//! The boot screens shown before the intro movie: the exe's `CMenuCopyright` (screen 23, handler `0x433f50`, update `0x442400`).
//!
//! Every page start restarts the `uvod.wav` music (the exe plays it from the constructor and from each page's branch).
//! Each page is a picture from `Data\PICTURES` drawn centred on black (no fade). The page timer starts at `start` ticks and
//! grows every frame; the page ends at 120 ticks, or at once when a key arrives while the timer is >= 60.
//! Page table, read from the Steam build's state machine: `CS.srf` (Cypronia logo, timer 0: 2 s, skippable after 1 s), then
//! `presents.srf` (timer 0, as the exe's state-6 branch sets it), then `nazov.srf` (the State of War title, timer -80: 3.3 s,
//! skippable after 2.3 s), then the exe leaves for the next screen.
//! The Steam exe's own transitions go 5 -> 7 and skip the `presents.srf` branch (state 6); we show it anyway, as the original
//! did (Cypronia / presents / State of War). `CI.srf` (a PC Mania logo) is referenced by neither exe and is not shown.
//! Warmonger's exe has no `presents.srf`; a missing picture is skipped, so it shows `CS` and `nazov`.

use crate::assets::load_backdrop;
use crate::ui::Image;
use opensow_formats::GameDir;

/// Ticks per second of the exe's frame accumulator.
const TICKS_PER_SEC: f64 = 60.0;
/// The page timer at which a page ends by itself.
const END: i32 = 120;
/// The page timer from which a key or click ends the page.
const SKIP_FROM: i32 = 60;

/// `(picture, timer at page start)` in the order the exe shows them.
pub const PAGES: [(&str, i32); 3] = [("CS", 0), ("presents", 0), ("nazov", -80)];

pub struct Boot {
    pages: Vec<(Image, i32)>,
    index: usize,
    /// The page timer in ticks.
    timer: f64,
    /// A page has just started (its music restarts, see [`Boot::take_changed`]).
    changed: bool,
}

impl Boot {
    /// Load the pages the install has (a missing picture is skipped); `None` when there are none.
    pub fn open(game: &GameDir) -> Option<Boot> {
        let pages: Vec<(Image, i32)> = PAGES.iter().filter_map(|&(name, start)| Some((load_backdrop(game, name)?, start))).collect();
        let timer = pages.first()?.1 as f64;
        Some(Boot { pages, index: 0, timer, changed: true })
    }

    /// Advance by `dt` seconds; returns true once the last page is over.
    pub fn update(&mut self, dt: f64) -> bool {
        self.timer += dt * TICKS_PER_SEC;
        while self.timer >= END as f64 {
            if !self.next() {
                return true;
            }
        }
        false
    }

    /// A key or click: ends the page when its timer allows it; returns true once the last page is over.
    pub fn skip(&mut self) -> bool {
        self.timer >= SKIP_FROM as f64 && !self.next()
    }

    /// Show page `n` from its start (headless screenshots).
    pub fn jump(&mut self, n: usize) {
        self.index = n.min(self.pages.len() - 1);
        self.timer = self.pages[self.index].1 as f64;
    }

    /// The left button ends the page at once, whatever its timer says; returns true once the last page is over.
    pub fn skip_now(&mut self) -> bool {
        !self.next()
    }

    /// True once per page start: the exe's page code (`0x442400`, and the constructor) restarts `uvod.wav` every time a page begins.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Move to the next page; false when there is none.
    fn next(&mut self) -> bool {
        self.index += 1;
        match self.pages.get(self.index) {
            Some(&(_, start)) => {
                self.timer = start as f64;
                self.changed = true;
                true
            }
            None => false,
        }
    }

    /// The picture centred on black, scaled to fit the buffer with its aspect kept.
    pub fn draw(&self, buf: &mut [u32], w: usize, h: usize) {
        crate::ui::clear(buf, w, h, 0);
        let Some((img, _)) = self.pages.get(self.index) else { return };
        let s = (w as f32 / img.w as f32).min(h as f32 / img.h as f32);
        let (dw, dh) = (((img.w as f32 * s) as usize).max(1), ((img.h as f32 * s) as usize).max(1));
        let (ox, oy) = ((w.saturating_sub(dw)) / 2, (h.saturating_sub(dh)) / 2);
        if opensow_viewer::record::active() {
            let (x0, y0) = (ox as i32, oy as i32);
            let col = |x: i32| (((x - x0) as f32 / s) as usize).min(img.w - 1) as i32;
            let row = |y: i32| (((y - y0) as f32 / s) as usize).min(img.h - 1) as i32;
            opensow_viewer::record::picture_rgb8(&img.rgb, img.w, x0, y0, x0 + dw.min(w) as i32, y0 + dh.min(h) as i32, col, row, false);
            return;
        }
        for y in 0..dh.min(h) {
            let sy = ((y as f32 / s) as usize).min(img.h - 1);
            for x in 0..dw.min(w) {
                let sx = ((x as f32 / s) as usize).min(img.w - 1);
                let i = (sy * img.w + sx) * 3;
                buf[(oy + y) * w + ox + x] = opensow_viewer::render::pack(img.rgb[i], img.rgb[i + 1], img.rgb[i + 2]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boot() -> Boot {
        let img = || Image { w: 4, h: 2, rgb: vec![255; 24] };
        Boot { pages: PAGES.iter().map(|&(_, s)| (img(), s)).collect(), index: 0, timer: PAGES[0].1 as f64, changed: true }
    }

    #[test]
    fn pages_last_as_in_the_exe() {
        let mut b = boot();
        assert!(!b.update(1.9) && b.index == 0, "CS lasts 120 ticks = 2 s");
        assert!(!b.update(0.2) && b.index == 1, "presents starts at timer 0 like CS");
        assert!(!b.update(2.0) && b.index == 2 && b.timer < 0.0);
        assert!(!b.update(3.2), "nazov starts at -80, ends at 120: 200 ticks = 3.33 s");
        assert!(b.update(0.2));
    }

    #[test]
    fn a_key_only_counts_from_tick_60() {
        let mut b = boot();
        b.update(0.9);
        assert!(!b.skip() && b.index == 0);
        b.update(0.2);
        assert!(!b.skip() && b.index == 1, "skips CS, lands on presents");
        b.update(1.1);
        assert!(!b.skip() && b.index == 2, "skips presents, lands on nazov");
        b.update(2.2); // timer -80 + 132 = 52
        assert!(!b.skip() && b.index == 2);
        b.update(0.2);
        assert!(b.skip(), "past tick 60 the last page can be skipped");
    }

    #[test]
    fn left_click_skips_at_once_and_each_page_restarts_the_music() {
        let mut b = boot();
        assert!(b.take_changed() && !b.take_changed(), "the first page starts the music once");
        assert!(!b.skip_now() && b.index == 1 && b.take_changed(), "a click at tick 0 still skips");
        b.update(2.1);
        assert!(b.index == 2 && b.take_changed(), "a timed change restarts it too");
        assert!(b.skip_now(), "a click on the last page ends the boot screens");
    }

    #[test]
    fn draw_letterboxes_on_black() {
        let b = boot();
        let (w, h) = (16, 16);
        let mut buf = vec![1u32; w * h];
        b.draw(&mut buf, w, h);
        assert_eq!(buf[0], 0, "bar above the 2:1 picture");
        assert_ne!(buf[8 * w + 8], 0);
        assert_eq!(buf[15 * w + 15], 0);
    }
}
