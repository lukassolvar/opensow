//! Everything the menus load from the player's install: backdrop, button sprites and the bitmap fonts.

use crate::ui::Image;
use opensow_formats::font::Font;
use opensow_formats::sprite::{self, Frame, Kind};
use opensow_formats::{srf, GameDir};
use opensow_viewer::art::Art;
use std::collections::HashMap;
use std::rc::Rc;

pub struct Assets {
    pub game: GameDir,
    pub art: Art,
    fonts: HashMap<String, Rc<(Font, Font)>>,
    pub backdrop: Option<Image>,
    /// `001_buttons.tsp`: frames 0..6 highlighted, 6..12 normal (NEW GAME, LOAD GAME, TUTORIAL, MULTIPLAYER, OPTIONS, EXIT GAME).
    pub buttons: Vec<Frame>,
}

impl Assets {
    pub fn load(game: GameDir) -> Result<Assets, String> {
        let art = Art::open(&game).ok_or("cannot open sprites.data")?;
        let backdrop = load_backdrop(&game, "MainMenu");
        let buttons = game
            .resolve("Data\\Sprites2\\Menu\\001_buttons.tsp")
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| sprite::parse(&d, Kind::Color).ok())
            .unwrap_or_default();
        Ok(Assets { game, art, fonts: HashMap::new(), backdrop, buttons })
    }

    /// Inner and outline masks of a font (`small`, `medium`, `big`, `msg`).
    pub fn font(&mut self, name: &str) -> Rc<(Font, Font)> {
        if let Some(f) = self.fonts.get(name) {
            return f.clone();
        }
        let mut load = |suffix: &str| Font::new(self.art.load(&format!("extras\\fonts\\{name}_{suffix}.msk")).map(|f| (*f).clone()).unwrap_or_default());
        let pair = Rc::new((load("i"), load("o")));
        self.fonts.insert(name.to_owned(), pair.clone());
        pair
    }
}

pub fn load_backdrop(game: &GameDir, name: &str) -> Option<Image> {
    let data = std::fs::read(game.resolve(&format!("Data\\PICTURES\\{name}.srf"))?).ok()?;
    let jpeg = srf::to_jpeg(&data).ok()?;
    let mut dec = jpeg_decoder::Decoder::new(jpeg.as_slice());
    let rgb = dec.decode().ok()?;
    let info = dec.info()?;
    (info.pixel_format == jpeg_decoder::PixelFormat::RGB24).then(|| Image { w: info.width as usize, h: info.height as usize, rgb })
}
