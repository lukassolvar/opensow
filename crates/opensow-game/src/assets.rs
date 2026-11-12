//! Everything the menus load from the player's install: backdrop, button sprites and the bitmap fonts.

use crate::ui::Image;
use opensow_formats::font::Font;
use opensow_formats::sprite::{self, Frame, Kind};
use opensow_formats::{srf, GameDir};
use opensow_viewer::art::Art;
use std::collections::HashMap;
use std::rc::Rc;

/// Everything loaded from the install that the screens share: the sprite archive, fonts and backdrops.
pub struct Assets {
    /// The install.
    pub game: GameDir,
    /// The sprite archive.
    pub art: Art,
    fonts: HashMap<String, Rc<(Font, Font)>>,
    /// The menu backdrop picture, when it could be loaded.
    pub backdrop: Option<Image>,
    infobox_pictures: HashMap<String, Option<Rc<Vec<Frame>>>>,
    /// `001_buttons.tsp`: frames 0..6 highlighted, 6..12 normal (NEW GAME, LOAD GAME, TUTORIAL, MULTIPLAYER, OPTIONS, EXIT GAME).
    pub buttons: Vec<Frame>,
    /// The six main-menu strings (lang file 0..=5) when the language is not English, whose labels the sprites do not carry; read once.
    menu_labels: Option<Option<Vec<String>>>,
    /// Warmonger's radar pictures (`Data\Sprites2\extras\radar2.tsp`), read once.
    radar2: Option<Option<Rc<Vec<Frame>>>>,
}

impl Assets {
    /// Opens the install's sprite archive and loads the shared art.
    pub fn load(game: GameDir) -> Result<Assets, String> {
        let art = Art::open(&game).ok_or("cannot open sprites.data")?;
        let backdrop = load_backdrop(&game, "MainMenu");
        let buttons = game
            .resolve("Data\\Sprites2\\Menu\\001_buttons.tsp")
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| sprite::parse(&d, Kind::Color).ok())
            .unwrap_or_default();
        Ok(Assets { game, art, fonts: HashMap::new(), backdrop, infobox_pictures: HashMap::new(), buttons, menu_labels: None, radar2: None })
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
                .map(|f| f.with_supplement(l))
                .and_then(|f| (0..6).map(|i| f.get(i).map(|t| t.trim().to_string())).collect::<Option<Vec<_>>>());
            self.menu_labels = Some(labels);
        }
        self.menu_labels.as_ref().and_then(|l| l.as_deref())
    }

    /// Warmonger's radar pictures: the loose `Data\Sprites2\extras\radar2.tsp` of its install (loaded by its exe at `0x424779`
    /// into `0x53e510`), one frame per Warmonger mission (`hud::radar2_frame`).
    pub fn warmonger_radar(&mut self) -> Option<Rc<Vec<Frame>>> {
        if self.radar2.is_none() {
            let set = self
                .game
                .warmonger_file("Data\\Sprites2\\extras\\radar2.tsp")
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|d| sprite::parse(&d, Kind::Color).ok())
                .filter(|f| !f.is_empty())
                .map(Rc::new);
            self.radar2 = Some(set);
        }
        self.radar2.clone().flatten()
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

    /// The named bitmap font pair (inner and outline masks), loaded once and kept.
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

/// Loads the picture `Data\PICTURES\<name>.srf`.
pub fn load_backdrop(game: &GameDir, name: &str) -> Option<Image> {
    let data = std::fs::read(game.resolve(&format!("Data\\PICTURES\\{name}.srf"))?).ok()?;
    let jpeg = srf::to_jpeg(&data).ok()?;
    let mut dec = jpeg_decoder::Decoder::new(jpeg.as_slice());
    let rgb = dec.decode().ok()?;
    let info = dec.info()?;
    (info.pixel_format == jpeg_decoder::PixelFormat::RGB24).then_some(Image { w: info.width as usize, h: info.height as usize, rgb })
}
