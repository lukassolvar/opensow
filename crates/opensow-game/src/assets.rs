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
    infobox_pictures: HashMap<String, Option<Rc<Vec<Frame>>>>,
    /// `001_buttons.tsp`: frames 0..6 highlighted, 6..12 normal (NEW GAME, LOAD GAME, TUTORIAL, MULTIPLAYER, OPTIONS, EXIT GAME).
    pub buttons: Vec<Frame>,
    /// The six main-menu strings (lang file 0..=5) when the language is not English, whose labels the sprites do not carry; read once.
    menu_labels: Option<Option<Vec<String>>>,
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
        Ok(Assets { game, art, fonts: HashMap::new(), backdrop, infobox_pictures: HashMap::new(), buttons, menu_labels: None })
    }

    /// Main-menu labels in the chosen language (English too: every language draws the same GDI text, not the English art), `None` when
    /// the file is missing.
    pub fn main_labels(&mut self) -> Option<&[String]> {
        if self.menu_labels.is_none() {
            let l = self.game.language();
            let labels = self
                .game
                .resolve(&format!("Data\\Language\\{}", l.file_name()))
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|d| opensow_formats::lang::LangFile::parse(&d).ok())
                .and_then(|f| (0..6).map(|i| f.get(i).map(|t| t.trim().to_string())).collect::<Option<Vec<_>>>());
            self.menu_labels = Some(labels);
        }
        self.menu_labels.as_ref().and_then(|l| l.as_deref())
    }

    /// An info box picture (`Data\\Sprites2\\InfoBox\\<path>.tsp`, case-insensitive), cached.
    pub fn infobox_picture(&mut self, path: &str) -> Option<Rc<Vec<Frame>>> {
        if let Some(p) = self.infobox_pictures.get(path) {
            return p.clone();
        }
        let loaded = self
            .game
            .resolve(&format!("Data\\Sprites2\\InfoBox\\{path}.tsp"))
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| sprite::parse(&d, Kind::Color).ok())
            .map(Rc::new);
        self.infobox_pictures.insert(path.to_owned(), loaded.clone());
        loaded
    }

    /// Inner and outline masks of a font (`small`, `medium`, `big`, `msg`).
    /// A pre-coloured font (`extras\\fonts\\{name}.ps6`, e.g. `font02_light`), or `None` when the install lacks it.
    pub fn color_font(&mut self, name: &str) -> Option<Font> {
        self.art.load(&format!("extras\\fonts\\{name}.ps6")).map(|f| Font::new((*f).clone()))
    }

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
