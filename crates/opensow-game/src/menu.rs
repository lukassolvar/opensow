//! Menu screens: main menu (the game's own button sprites), level select, options, pause and message panels.

use crate::assets::Assets;
use crate::ui::{self, Rect};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    NewGame,
    /// Main menu TUTORIAL: mission M00 straight away (no briefing movie), `0x4460a0` case 2 -> state 5 (docs/tutorial.md).
    Tutorial,
    Unavailable(&'static str),
    Options,
    /// The load page (state `0x11`).
    LoadGame,
    Multiplayer,
    /// The prototype's own settings (game speed, health bars ...), key `P`.
    Prototype,
    Exit,
    Level(String),
    Back,
    CycleSpeed,
    ToggleHp,
    ToggleRange,
    ToggleSquad,
    Resume,
    Restart,
    ToMain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Main,
    Levels,
    Options,
    Pause,
    /// Victory / defeat / not-implemented text with a single Back button.
    Message(String),
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub ticks_per_second: u32,
    pub hp_bars: bool,
    pub show_range: bool,
    /// Production is not implemented, so missions that start with few units get a free squad next to the Advancer.
    pub starter_squad: bool,
    /// Options menu "KEYS SCROLL" / "MOUSE SCROLL" (0..=10): the map moves `2 * ticks * speed` px per step (docs/options.md).
    pub scroll_keys: u32,
    pub scroll_mouse: u32,
    /// Options RADAR / QUAKE switches (`+8` / `+9`).
    pub radar_on: bool,
    pub quake_on: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { ticks_per_second: crate::sim::TICKS_PER_SECOND, hp_bars: true, show_range: false, starter_squad: true, scroll_keys: 5, scroll_mouse: 3, radar_on: true, quake_on: true }
    }
}

#[derive(Debug, Clone)]
pub struct Button {
    pub rect: Rect,
    pub action: Action,
    pub label: String,
    /// Index of the main-menu button sprite (0..6), when drawn from the game's art.
    pub sprite: Option<usize>,
}

const MAIN: [(&str, Action); 6] = [
    ("New game", Action::NewGame),
    ("Load game", Action::LoadGame),
    ("Tutorial", Action::Tutorial),
    ("Multiplayer", Action::Multiplayer),
    ("Options", Action::Options),
    ("Exit game", Action::Exit),
];

pub const SPEEDS: [u32; 4] = [20, 40, 60, 120];

/// Buttons of a screen for a window of `w` x `h` pixels.
pub fn layout(screen: &Screen, assets: &mut Assets, levels: &[String], settings: &Settings, w: usize, h: usize) -> Vec<Button> {
    let (wi, hi) = (w as i32, h as i32);
    let s = h as f32 / 1080.0;
    let mut out = Vec::new();
    match screen {
        Screen::Main if assets.buttons.len() >= 12 => {
            // The art was drawn for 640x480 with the frame hotspot = offset from the screen centre; here the column
            // sits under the logo of the 1920x1080 backdrop (x = 20.6% of the width, starting at 58% of the height).
            let cx = (w as f32 * 0.206) as i32;
            let top = (h as f32 * 0.58) as i32;
            let pitch = (65.0 * s * 1.0) as i32;
            for (i, (label, action)) in MAIN.iter().enumerate() {
                let f = &assets.buttons[6 + i];
                let (bw, bh) = ((f.width as f32 * s) as i32, (f.height as f32 * s) as i32);
                out.push(Button { rect: Rect { x: cx - bw / 2, y: top + i as i32 * pitch, w: bw, h: bh.max(pitch * 3 / 4) }, action: action.clone(), label: (*label).into(), sprite: Some(i) });
            }
        }
        Screen::Main => {
            for (i, (label, action)) in MAIN.iter().enumerate() {
                out.push(text_button(wi / 5, hi / 2 + i as i32 * 60, label, action.clone()));
            }
        }
        Screen::Levels => {
            let cols = 3;
            for (i, l) in levels.iter().enumerate() {
                let (col, row) = ((i % cols) as i32, (i / cols) as i32);
                out.push(Button { rect: Rect { x: wi / 2 - 330 + col * 220, y: hi / 5 + row * 54, w: 200, h: 44 }, action: Action::Level(l.clone()), label: format!("Mission {}", l.trim_start_matches(['M', 'm'])), sprite: None });
            }
            out.push(text_button(wi / 2 - 100, hi - 90, "Back", Action::Back));
        }
        Screen::Options => {
            out.push(text_button(wi / 2 - 230, hi / 3, &format!("Game speed: {} ticks/s", settings.ticks_per_second), Action::CycleSpeed));
            out.push(text_button(wi / 2 - 230, hi / 3 + 70, &format!("Health bars: {}", on_off(settings.hp_bars)), Action::ToggleHp));
            out.push(text_button(wi / 2 - 230, hi / 3 + 140, &format!("Weapon ranges: {}", on_off(settings.show_range)), Action::ToggleRange));
            out.push(text_button(wi / 2 - 230, hi / 3 + 210, &format!("Starter squad: {}", on_off(settings.starter_squad)), Action::ToggleSquad));
            out.push(text_button(wi / 2 - 100, hi - 90, "Back", Action::Back));
        }
        Screen::Pause => {
            for (i, (label, a)) in [("Resume", Action::Resume), ("Restart mission", Action::Restart), ("Options", Action::Options), ("Main menu", Action::ToMain)].into_iter().enumerate() {
                out.push(text_button(wi / 2 - 160, hi / 3 + i as i32 * 70, label, a));
            }
        }
        Screen::Message(_) => out.push(text_button(wi / 2 - 100, hi / 2 + 60, "Back", Action::Back)),
    }
    out
}

fn on_off(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
}

fn text_button(x: i32, y: i32, label: &str, action: Action) -> Button {
    Button { rect: Rect { x, y, w: 320, h: 54 }, action, label: label.into(), sprite: None }
}

/// Draws a screen. `over_game` leaves the frame already in `buf` visible (pause menu) instead of the backdrop.
pub fn draw(screen: &Screen, buttons: &[Button], hover: Option<usize>, assets: &mut Assets, buf: &mut [u32], w: usize, h: usize, over_game: bool) {
    if over_game {
        ui::dim(buf, w, h, Rect { x: 0, y: 0, w: w as i32, h: h as i32 }, 150);
    } else if let Some(img) = &assets.backdrop {
        ui::backdrop(buf, w, h, img);
    } else {
        buf.fill(0x00101810);
    }
    let big = assets.font("big");
    let medium = assets.font("medium");
    let s = (h as f32 / 1080.0).max(0.6);
    let white = (255, 255, 255);
    let ink = (0, 30, 0);
    let (title, panel) = match screen {
        Screen::Main => ("", false),
        Screen::Levels => ("Choose a mission", true),
        Screen::Options => ("Prototype settings", true),
        Screen::Pause => ("Paused", true),
        Screen::Message(_) => ("", true),
    };
    if panel {
        let x0 = (w as f32 * 0.5 - 380.0 * s.max(1.0)) as i32;
        ui::dim(buf, w, h, Rect { x: x0, y: h as i32 / 8, w: (760.0 * s.max(1.0)) as i32, h: h as i32 * 3 / 4 }, 150);
    }
    if !title.is_empty() {
        let sc = 1.4 * s.max(1.0);
        ui::text(buf, w, h, &big, title, w as i32 / 2 - ui::text_width(&big, title, sc) / 2, h as i32 / 8 + 12, sc, white, ink);
    }
    if let Screen::Message(msg) = screen {
        for (i, line) in msg.lines().enumerate() {
            let sc = 1.2;
            ui::text(buf, w, h, &medium, line, w as i32 / 2 - ui::text_width(&medium, line, sc) / 2, h as i32 / 3 + i as i32 * 40, sc, white, ink);
        }
    }
    for (i, b) in buttons.iter().enumerate() {
        let hot = hover == Some(i);
        if let (Some(si), true) = (b.sprite, assets.buttons.len() >= 12) {
            let (frame, lift) = if hot { (&assets.buttons[si], 0) } else { (&assets.buttons[6 + si], 175) };
            let sc = h as f32 / 1080.0;
            let (fw, fh) = ((frame.width as f32 * sc) as i32, (frame.height as f32 * sc) as i32);
            ui::sprite_lit(buf, w, h, frame, b.rect.x + (b.rect.w - fw) / 2, b.rect.y + (b.rect.h - fh) / 2, sc, lift);
        } else {
            if hot {
                ui::fill(buf, w, h, b.rect, (60, 220, 90), 70);
            }
            ui::fill(buf, w, h, Rect { x: b.rect.x, y: b.rect.y + b.rect.h - 2, w: b.rect.w, h: 2 }, (60, 200, 90), 120);
            let sc = 0.95 * s.max(1.0);
            let tw = ui::text_width(&big, &b.label, sc);
            let th = ui::text_height(&big, sc);
            let colour = if hot { (255, 255, 120) } else { white };
            ui::text(buf, w, h, &big, &b.label, b.rect.x + (b.rect.w - tw) / 2, b.rect.y + (b.rect.h - th) / 2, sc, colour, ink);
        }
    }
}

pub fn hit(buttons: &[Button], x: i32, y: i32) -> Option<usize> {
    buttons.iter().position(|b| b.rect.contains(x, y))
}
