use crate::art::{Art, Placed};
use opensow_formats::{level::Level, map::Map, srf, GameDir};

pub const CELL: i32 = 32;

pub struct Scene {
    pub name: String,
    /// Background RGB8, `bg_w * bg_h * 3`.
    pub bg: Vec<u8>,
    pub bg_w: usize,
    pub bg_h: usize,
    pub map: Map,
    pub level: Option<Level>,
    /// Sprites for objects that have art, sorted back-to-front.
    pub placed: Vec<Placed>,
    /// Facing (0..32, clockwise from north) given to every unit; markers store none.
    pub facing: usize,
    /// Independent facing of the weapon tops (`None` = same as the chassis).
    pub aim: Option<usize>,
    /// Bullet paths to draw as an overlay (`--shot`).
    pub shots: Vec<opensow_formats::projectile::Path>,
    /// Target-search ranges to shade: (cell x, cell y, dead zone, range) (`--range`).
    pub ranges: Vec<(i32, i32, usize, usize)>,
}

impl Scene {
    pub fn load(game: &GameDir, name: &str, facing: usize, aim: Option<usize>) -> Result<Scene, String> {
        let find = |ext: &str| game.resolve(&format!("Data\\Levels\\{name}.{ext}")).filter(|p| p.exists());
        let read = |ext: &str| -> Result<Vec<u8>, String> {
            let p = find(ext).ok_or_else(|| format!("{name}.{ext} not found"))?;
            std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))
        };

        let jpeg = srf::to_jpeg(&read("srf")?).map_err(|e| e.to_string())?;
        let mut dec = jpeg_decoder::Decoder::new(jpeg.as_slice());
        let bg = dec.decode().map_err(|e| format!("{name}.srf: {e}"))?;
        let info = dec.info().ok_or("no JPEG info")?;
        if info.pixel_format != jpeg_decoder::PixelFormat::RGB24 {
            return Err(format!("unsupported JPEG pixel format {:?}", info.pixel_format));
        }

        let map = Map::parse(&read("map")?).map_err(|e| e.to_string())?;
        let level = read("edt").ok().and_then(|d| Level::parse(&d).ok());
        let (bg_w, bg_h) = (info.width as usize, info.height as usize);
        if bg_w != map.width as usize * CELL as usize || bg_h != map.height as usize * CELL as usize {
            eprintln!("warning: background {bg_w}x{bg_h} does not match map {}x{} cells", map.width, map.height);
        }
        let placed = build_placed(game, level.as_ref(), facing, aim);
        Ok(Scene { name: name.to_owned(), bg, bg_w, bg_h, map, level, placed, facing, aim, shots: Vec::new(), ranges: Vec::new() })
    }

    /// Re-places every unit with a new facing (sprites only; the terrain is kept).
    pub fn set_facing(&mut self, game: &GameDir, facing: usize) {
        self.facing = facing % 32;
        self.placed = build_placed(game, self.level.as_ref(), self.facing, self.aim);
    }

    /// Campaign level names (`M00`..) found in the install, sorted.
    pub fn list(game: &GameDir) -> Vec<String> {
        let mut v: Vec<String> = game
            .resolve("Data\\Levels")
            .and_then(|d| std::fs::read_dir(d).ok())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                let (stem, ext) = n.rsplit_once('.')?;
                (ext.eq_ignore_ascii_case("srf") && stem.len() == 3 && stem[..1].eq_ignore_ascii_case("m"))
                    .then(|| stem.to_ascii_uppercase())
            })
            .collect();
        v.sort();
        v
    }
}

fn build_placed(game: &GameDir, level: Option<&Level>, facing: usize, aim: Option<usize>) -> Vec<Placed> {
    let mut placed = Vec::new();
    if let (Some(level), Some(mut art)) = (level, Art::open(game)) {
        placed.extend(level.objects.iter().enumerate().filter_map(|(i, o)| art.place(i, o)));
        placed.extend(level.markers.iter().enumerate().flat_map(|(i, m)| art.place_marker(i, m, facing, aim)));
        placed.sort_by_key(|p| (p.ay, p.layer));
    }
    placed
}
