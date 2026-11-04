//! The in-game menu (`CMenuInGame`, `0x442870`): the bronze oval opened with Esc. Notes: docs/pause.md.

use crate::assets::Assets;
use crate::ui::{self, Rect};
use opensow_formats::sprite::Frame;

/// What an item asks the game to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    None,
    Continue,
    Load,
    Save,
    Options,
    /// The check mark on the restart page.
    Restart,
    /// The check mark on the main-menu page.
    MainMenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Items,
    ConfirmRestart,
    ConfirmMain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// Items 0..=5: continue, load, save, options, restart, main menu.
    Item(usize),
    Yes,
    No,
}

pub struct InGameMenu {
    frames: Vec<Frame>,
    pub page: Page,
    hover: Option<Hit>,
    /// Load and save do nothing in the tutorial (`[0x70f2cc] != -1`).
    pub tutorial: bool,
    /// Sound ids for the game to play: 35 open / close, 36 hover, 37 item, 38 yes, 39 no.
    pub sounds: Vec<u8>,
}

impl InGameMenu {
    pub fn new(assets: &mut Assets, tutorial: bool) -> InGameMenu {
        let frames = assets.art.load("menu\\ingame.ps6").map(|f| (*f).clone()).unwrap_or_default();
        InGameMenu::with_frames(frames, tutorial)
    }

    pub fn with_frames(frames: Vec<Frame>, tutorial: bool) -> InGameMenu {
        InGameMenu { frames, page: Page::Items, hover: None, tutorial, sounds: vec![35] }
    }

    /// Menu scale for a window of height `h` (the HUD's rule: height / 768, never below 1).
    fn scale(h: usize) -> f32 {
        (h as f32 / 768.0).max(1.0)
    }

    fn origin(w: usize, h: usize) -> (i32, i32) {
        (w as i32 / 2, h as i32 / 2)
    }

    fn rect(&self, frame: usize, w: usize, h: usize) -> Option<Rect> {
        let f = self.frames.get(frame)?;
        let (cx, cy) = Self::origin(w, h);
        let s = Self::scale(h);
        Some(Rect { x: cx - (f.hot_x as f32 * s) as i32, y: cy - (f.hot_y as f32 * s) as i32, w: (f.width as f32 * s) as i32, h: (f.height as f32 * s) as i32 })
    }

    fn item_enabled(&self, i: usize) -> bool {
        !(self.tutorial && (i == 1 || i == 2))
    }

    /// The control under a window pixel on the current page.
    pub fn hit_at(&self, x: i32, y: i32, w: usize, h: usize) -> Option<Hit> {
        let inside = |frame: usize| self.rect(frame, w, h).is_some_and(|r| r.contains(x, y));
        match self.page {
            Page::Items => (0..6).find(|&i| self.item_enabled(i) && inside(1 + i)).map(Hit::Item),
            _ => {
                if inside(9) {
                    Some(Hit::Yes)
                } else if inside(7) {
                    Some(Hit::No)
                } else {
                    None
                }
            }
        }
    }

    /// Window pixel at the centre of a control (for scripted clicks).
    pub fn centre(&self, hit: Hit, w: usize, h: usize) -> (i32, i32) {
        let frame = match hit {
            Hit::Item(i) => 1 + i,
            Hit::Yes => 9,
            Hit::No => 7,
        };
        self.rect(frame, w, h).map_or((0, 0), |r| (r.x + r.w / 2, r.y + r.h / 2))
    }

    pub fn mouse_moved(&mut self, x: i32, y: i32, w: usize, h: usize) {
        let hover = self.hit_at(x, y, w, h);
        if hover.is_some() && hover != self.hover {
            self.sounds.push(36); // ingame-move
        }
        self.hover = hover;
    }

    pub fn hovered(&self) -> Option<Hit> {
        self.hover
    }

    pub fn click(&mut self, x: i32, y: i32, w: usize, h: usize) -> Event {
        match self.hit_at(x, y, w, h) {
            Some(hit) => self.press(hit),
            None => Event::None,
        }
    }

    /// What a control does (jump table `0x442cd8`; the confirmation pages `0x442b7c` / `0x442bc3`), with its sound.
    pub fn press(&mut self, hit: Hit) -> Event {
        match (self.page, hit) {
            (Page::Items, Hit::Item(i)) if self.item_enabled(i) => {
                self.sounds.push(37); // ingame-select
                self.hover = None;
                match i {
                    0 => Event::Continue,
                    1 => Event::Load,
                    2 => Event::Save,
                    3 => Event::Options,
                    4 => {
                        self.page = Page::ConfirmRestart;
                        Event::None
                    }
                    _ => {
                        self.page = Page::ConfirmMain;
                        Event::None
                    }
                }
            }
            (Page::ConfirmRestart, Hit::Yes) => {
                self.sounds.push(38); // ingame-ano
                Event::Restart
            }
            (Page::ConfirmMain, Hit::Yes) => {
                self.sounds.push(38);
                Event::MainMenu
            }
            (Page::ConfirmRestart | Page::ConfirmMain, Hit::No) => {
                self.sounds.push(39); // ingame-nie
                self.page = Page::Items;
                self.hover = None;
                Event::None
            }
            _ => Event::None,
        }
    }

    pub fn draw(&self, buf: &mut [u32], w: usize, h: usize) {
        let s = Self::scale(h);
        let put = |buf: &mut [u32], frame: usize| {
            if let (Some(f), Some(r)) = (self.frames.get(frame), self.rect(frame, w, h)) {
                ui::sprite(buf, w, h, f, r.x, r.y, s);
            }
        };
        put(buf, 0);
        match self.page {
            Page::Items => {
                if let Some(Hit::Item(i)) = self.hover {
                    put(buf, 1 + i);
                }
            }
            _ => {
                put(buf, if self.hover == Some(Hit::Yes) { 10 } else { 9 });
                put(buf, if self.hover == Some(Hit::No) { 8 } else { 7 });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(tutorial: bool) -> InGameMenu {
        InGameMenu::with_frames(Vec::new(), tutorial)
    }

    #[test]
    fn items_and_confirmations() {
        let mut m = menu(false);
        assert_eq!(m.sounds, vec![35], "opening plays ingame-come");
        assert_eq!(m.press(Hit::Item(0)), Event::Continue);
        assert_eq!(m.press(Hit::Item(1)), Event::Load);
        assert_eq!(m.press(Hit::Item(2)), Event::Save);
        assert_eq!(m.press(Hit::Item(3)), Event::Options);
        assert_eq!(m.press(Hit::Item(4)), Event::None);
        assert_eq!(m.page, Page::ConfirmRestart);
        assert_eq!(m.press(Hit::Item(0)), Event::None, "the items are dead on a confirmation page");
        assert_eq!(m.press(Hit::No), Event::None);
        assert_eq!(m.page, Page::Items);
        m.press(Hit::Item(5));
        assert_eq!(m.page, Page::ConfirmMain);
        assert_eq!(m.press(Hit::Yes), Event::MainMenu);
        m.page = Page::ConfirmRestart;
        assert_eq!(m.press(Hit::Yes), Event::Restart);
        assert_eq!(m.sounds, vec![35, 37, 37, 37, 37, 37, 39, 37, 38, 38]);
    }

    #[test]
    fn tutorial_hides_load_and_save() {
        let mut m = menu(true);
        assert_eq!(m.press(Hit::Item(1)), Event::None);
        assert_eq!(m.press(Hit::Item(2)), Event::None);
        assert_eq!(m.press(Hit::Item(4)), Event::None);
        assert_eq!(m.page, Page::ConfirmRestart, "restart still works");
    }
}
