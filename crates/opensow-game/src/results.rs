//! What the exe shows after a mission: the "MISSION SUCCESSFULLY COMPLETED / MISSION FAILED" banner over the frozen game (states
//! 9 and 0xa, `0x4357a0`) and then the statistics bar chart (state 0x16, `0x44d770` / `0x44cbd0`). Notes: docs/results.md.

use crate::assets::{load_backdrop, Assets};
use crate::ui::{self, Image};
use opensow_formats::endscreen::{self as es, SideStats};
use opensow_formats::font::Font;
use opensow_formats::lang::LangFile;
use opensow_formats::sprite::{self, rgb565_to_rgb8, Frame, Kind};
use opensow_viewer::render::pack;

/// What the player's click decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    None,
    /// The banner is over: the statistics screen starts (the game switches to the `statistic` music).
    Statistics,
    /// The statistics screen is over: a won mission opens the next land, a lost one returns to the main menu.
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Banner,
    Statistics,
}

/// The mission-end screens: the victory / defeat banner, then the statistics.
pub struct Results {
    /// The mission was won.
    pub won: bool,
    /// The tutorial skips the statistics screen and returns to the main menu (`0x4359f3`: state 0x16 only when the mission number is not 0).
    pub tutorial: bool,
    phase: Phase,
    /// Ticks (60 Hz) since the current phase began.
    ticks: f64,
    done_ticks: i32,
    stats: [SideStats; 2],
    /// Every bar is complete (`[state+0x28]`); the next click leaves.
    finished: bool,
    /// Sound ids to play (the game drains them): 47 a pair is complete, 48 a bar is growing.
    pub sounds: Vec<u8>,
    banner: Vec<Frame>,
    bars: Vec<Frame>,
    bg: Option<Image>,
    panels: Option<Image>,
    numbers: Font,
    labels: std::rc::Rc<(Font, Font)>,
    strings: Option<LangFile>,
    /// Pairs whose "complete" sound has played (`[state+0x14..0x1b]`).
    topped: [bool; 8],
}

fn frames(assets: &Assets, path: &str) -> Vec<Frame> {
    assets.game.resolve(path).and_then(|p| std::fs::read(p).ok()).and_then(|d| sprite::parse(&d, Kind::Color).ok()).unwrap_or_default()
}

impl Results {
    /// `language`: the index into `intro::LANGUAGES` (the banner picture has a copy per language under `DiffData`).
    pub fn new(assets: &mut Assets, won: bool, tutorial: bool, stats: [SideStats; 2], language: usize) -> Results {
        let lang_id = assets.game.language();
        let strings = assets
            .game
            .resolve(&format!("Data\\Language\\{}", lang_id.file_name()))
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| LangFile::parse(&d).ok())
            .map(|f| f.with_supplement(lang_id));
        let dir = crate::intro::LANGUAGES.get(language).copied().unwrap_or("");
        let local = (!dir.is_empty()).then(|| frames(assets, &format!("Data\\DiffData\\{dir}\\Sprites2\\Extras\\WinLoose.tsp"))).filter(|f| f.len() >= 2);
        let banner = local.unwrap_or_else(|| frames(assets, "Data\\Sprites2\\Extras\\WinLoose.tsp"));
        let numbers = Font::new(assets.art.load("extras\\fonts\\font_msg2.ps6").map(|f| (*f).clone()).unwrap_or_default());
        Results {
            won,
            tutorial,
            phase: Phase::Banner,
            ticks: 0.0,
            done_ticks: 0,
            stats,
            finished: false,
            sounds: Vec::new(),
            banner,
            bars: frames(assets, "Data\\Sprites2\\Menu\\statistic.tsp"),
            bg: load_backdrop(&assets.game, "Statistic2"),
            panels: load_backdrop(&assets.game, "Statistic"),
            numbers,
            labels: assets.font("small"),
            strings,
            topped: [false; 8],
        }
    }

    /// Whether the statistics screen is showing (after the banner).
    pub fn in_statistics(&self) -> bool {
        self.phase == Phase::Statistics
    }

    #[cfg(test)]
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Advance by `dt` seconds. Ticks run one at a time so the 4-tick sound period and the 2-tick "complete" window are never skipped.
    pub fn update(&mut self, dt: f64) {
        let before = self.ticks as i32;
        self.ticks += dt * 60.0;
        let after = self.ticks as i32;
        if self.phase == Phase::Banner {
            self.done_ticks = after;
            return;
        }
        for t in before + 1..=after {
            self.tick(t);
        }
    }

    /// One tick of the statistics screen at counter time `t` (`0x44d770` then `0x44cbd0`).
    fn tick(&mut self, t: i32) {
        if self.finished && t < 1_000_000 {
            return;
        }
        let c = es::counter(t, es::speed_sum(&self.stats));
        let (cs, done) = es::pair_counters(c, &self.stats);
        let growing_tick = t % es::SOUND_PERIOD == 0;
        for k in 0..8 {
            for side in 0..2 {
                if cs[k] > 0 && cs[k] < self.stats[side][k] as i32 && growing_tick {
                    self.sounds.push(48);
                }
            }
            if es::pair_top(cs[k], es::pair_max(&self.stats, k)) && !self.topped[k] {
                self.topped[k] = true;
                self.sounds.push(47);
            }
        }
        if done {
            self.finished = true;
        }
    }

    /// A click or key. In the banner anything leaves after `BANNER_TICKS`; in the statistics a click (or Esc, `escape`) first completes
    /// the bars at once and then leaves.
    pub fn input(&mut self, escape: bool, any_key: bool) -> Event {
        match self.phase {
            Phase::Banner => {
                if self.done_ticks < es::BANNER_TICKS {
                    return Event::None;
                }
                if self.tutorial {
                    return Event::Done;
                }
                self.phase = Phase::Statistics;
                self.ticks = 0.0;
                Event::Statistics
            }
            Phase::Statistics => {
                if !(escape || any_key) {
                    return Event::None;
                }
                if !self.finished {
                    self.finished = true;
                    self.ticks = 1_000_000.0;
                    return Event::None;
                }
                Event::Done
            }
        }
    }

    /// The banner: `buf` already holds the frozen game frame; every even row is tinted dark red, every odd row is black, and the
    /// picture (frame 0 won, frame 1 lost) sits at the screen centre minus its hot spot.
    pub fn draw_banner(&self, buf: &mut [u32], w: usize, h: usize) {
        if opensow_viewer::record::active() {
            opensow_viewer::record::fill(opensow_viewer::record::op::TINT_ROWS, 0, 0, w as i32, h as i32, 0, 0);
        } else {
            for y in 0..h {
                let row = &mut buf[y * w..(y + 1) * w];
                if y % 2 == 1 {
                    row.fill(0);
                    continue;
                }
                for px in row.iter_mut() {
                    let (r, g, b) = ((*px >> 16 & 255) as u16, (*px >> 8 & 255) as u16, (*px & 255) as u16);
                    let p = es::tint565((r >> 3) << 11 | (g >> 2) << 5 | (b >> 3));
                    let [r, g, b] = rgb565_to_rgb8(p);
                    *px = pack(r, g, b);
                }
            }
        }
        if let Some(f) = self.banner.get(if self.won { 0 } else { 1 }) {
            let s = (h as f32 / 768.0).max(1.0);
            let (x, y) = (w as i32 / 2 - (f.hot_x as f32 * s) as i32, h as i32 / 2 - (f.hot_y as f32 * s) as i32);
            ui::sprite(buf, w, h, f, x, y, s);
        }
    }

    /// The statistics screen (replaces the frame).
    /// The layout is the exe's 640x480 one; it is drawn at that size and scaled to the largest centred 4:3 rectangle, like the
    /// other screens, over the background covering the whole window.
    pub fn draw_statistics(&self, buf: &mut [u32], w: usize, h: usize) {
        ui::clear(buf, w, h, 0);
        if let Some(bg) = &self.bg {
            ui::backdrop(buf, w, h, bg);
        }
        let recording = opensow_viewer::record::active();
        if (w, h) == (640, 480) && !recording {
            self.draw_layout(buf, w, h);
            return;
        }
        // the 640x480 page is drawn on the CPU (also while a frame is recorded) and handed over scaled
        let mut small = vec![0u32; 640 * 480];
        opensow_viewer::record::cpu(|| {
            if let Some(bg) = &self.bg {
                ui::backdrop(&mut small, 640, 480, bg);
            }
            self.draw_layout(&mut small, 640, 480);
        });
        let s = (w as f32 / 640.0).min(h as f32 / 480.0);
        let (dw, dh) = (((640.0 * s) as usize).max(1), ((480.0 * s) as usize).max(1));
        let (ox, oy) = ((w - dw.min(w)) / 2, (h - dh.min(h)) / 2);
        if recording {
            let (ax, ay) = (ox as i32, oy as i32);
            let col = |x: i32| ((x - ax) as usize * 640 / dw).min(639) as i32;
            let row = |y: i32| ((y - ay) as usize * 480 / dh).min(479) as i32;
            opensow_viewer::record::picture_words(&small, 640, ax, ay, ax + dw.min(w) as i32, ay + dh.min(h) as i32, col, row);
            return;
        }
        for y in 0..dh.min(h) {
            for x in 0..dw.min(w) {
                buf[(oy + y) * w + ox + x] = small[(y * 480 / dh).min(479) * 640 + (x * 640 / dw).min(639)];
            }
        }
    }

    fn draw_layout(&self, buf: &mut [u32], w: usize, h: usize) {
        let (cx, cy) = (w as i32 / 2, h as i32 / 2);
        if let Some(p) = &self.panels {
            for y in 0..p.h as i32 {
                for x in 0..p.w as i32 {
                    let (dx, dy) = (cx - p.w as i32 / 2 + x, cy - p.h as i32 / 2 + y);
                    if dx >= 0 && dy >= 0 && (dx as usize) < w && (dy as usize) < h {
                        let i = (y as usize * p.w + x as usize) * 3;
                        buf[dy as usize * w + dx as usize] = pack(p.rgb[i], p.rgb[i + 1], p.rgb[i + 2]);
                    }
                }
            }
        }
        // labels: 12 px GDI text in the exe (light grey on a dark green shadow); the small bitmap font stands in
        for k in 0..8 {
            let Some(text) = self.strings.as_ref().and_then(|l| l.get(es::PAIR_LABELS[k] as usize - 1)) else { continue };
            let (lx, ly) = es::LABEL_POS[k];
            ui::text(buf, w, h, &self.labels, text.trim(), cx + lx, cy + ly, 1.0, (206, 206, 206), (25, 45, 16));
        }
        let t = if self.finished && self.ticks >= 1_000_000.0 { 1_000_000 } else { self.ticks as i32 };
        let c = es::counter(t, es::speed_sum(&self.stats));
        let (cs, _) = es::pair_counters(c, &self.stats);
        for k in 0..8 {
            let max = es::pane_max(&self.stats, k);
            let top = es::pair_top(cs[k], es::pair_max(&self.stats, k));
            let (xs, row) = (es::PAIR_X[k % 4], k / 4);
            for side in 0..2 {
                let (shown, _) = es::pair_state(cs[k], self.stats[side][k]);
                if cs[k] <= 0 {
                    continue;
                }
                let x = cx + if side == 0 { xs.0 } else { xs.1 };
                let base = cy + es::ROW_BASE[row];
                let bh = es::bar_height(shown, max);
                if let Some(f) = self.bars.get(side) {
                    for row_k in 0..bh {
                        for _ in 0..1 + top as usize {
                            ui::sprite_add(buf, w, h, f, x - f.hot_x as i32, base - row_k - f.hot_y as i32, 1.0, false);
                        }
                    }
                }
                self.draw_number(buf, w, h, &shown.to_string(), x, base - bh - 20);
            }
        }
    }

    /// A number centred on `x` (`0x44d370`: `x - width / 2 - 1`), colour font `font_msg2`, spacing -2.
    fn draw_number(&self, buf: &mut [u32], w: usize, h: usize, text: &str, x: i32, y: i32) {
        let placed = self.numbers.layout(text, -2);
        let width = self.numbers.measure(text, -2);
        let top = placed.iter().map(|p| p.y).min().unwrap_or(0);
        for p in placed {
            let g = p.glyph;
            for gy in 0..g.height as i32 {
                for gx in 0..g.width as i32 {
                    let k = (gy * g.width as i32 + gx) as usize;
                    let (dx, dy) = (x - width / 2 - 1 + p.x + gx, y + p.y - top + gy);
                    if g.mask[k] && dx >= 0 && dy >= 0 && (dx as usize) < w && (dy as usize) < h {
                        let [r, gr, b] = rgb565_to_rgb8(g.pixels[k]);
                        buf[dy as usize * w + dx as usize] = pack(r, gr, b);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensow_formats::GameDir;

    fn sample() -> [SideStats; 2] {
        [[3, 5, 12, 7, 4, 1, 2, 0], [1, 2, 20, 15, 0, 3, 1, 1]]
    }

    fn make(won: bool) -> Option<Results> {
        let game = GameDir::locate()?;
        let mut assets = Assets::load(game).ok()?;
        Some(Results::new(&mut assets, won, false, sample(), 0))
    }

    #[test]
    fn banner_waits_two_seconds_then_leaves_on_input() {
        let Some(mut r) = make(true) else { return };
        r.update(1.0);
        assert_eq!(r.input(false, true), Event::None, "too early");
        r.update(1.1);
        assert_eq!(r.input(false, true), Event::Statistics);
        assert!(r.in_statistics());
    }

    #[test]
    fn statistics_complete_then_leave() {
        let Some(mut r) = make(false) else { return };
        r.update(2.5);
        r.input(false, true);
        assert!(!r.finished());
        r.update(0.5);
        assert_eq!(r.input(false, true), Event::None, "the first click completes the bars");
        assert!(r.finished());
        assert_eq!(r.input(false, true), Event::Done);
    }

    #[test]
    fn the_count_runs_to_the_end_and_plays_each_top_sound_once() {
        let Some(mut r) = make(true) else { return };
        r.update(2.5);
        r.input(false, true);
        r.update(60.0);
        assert!(r.finished());
        assert_eq!(r.sounds.iter().filter(|&&s| s == 47).count(), 8);
        assert!(r.sounds.contains(&48));
    }

    #[test]
    fn tutorial_skips_the_statistics() {
        let Some(mut r) = make(true) else { return };
        r.tutorial = true;
        r.update(2.5);
        assert_eq!(r.input(false, true), Event::Done);
    }

    #[test]
    fn both_screens_draw_at_the_extreme_sizes() {
        let Some(mut r) = make(true) else { return };
        for (w, h) in [(640, 480), (1920, 1080)] {
            let mut buf = vec![0x00336699u32; w * h];
            r.draw_banner(&mut buf, w, h);
            assert!(buf[w..2 * w].iter().all(|&p| p == 0), "odd rows are black");
            r.update(2.5);
            r.input(false, true);
            r.update(30.0);
            r.draw_statistics(&mut buf, w, h);
            assert!(buf.iter().any(|&p| p != 0));
        }
    }
}
