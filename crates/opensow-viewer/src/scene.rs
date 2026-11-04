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
    /// Foreground pixels (`bg_w * bg_h`): tree tops, rocks and the like that hide units walking behind them. Built from the
    /// level's `.tmi` tile grid and `.til` masks (a tile's set pixels are copied from the background over the units).
    pub foreground: Vec<bool>,
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
        let foreground = foreground_mask(&read("tmi").ok(), &read("til").ok(), bg_w, bg_h);
        Ok(Scene { name: name.to_owned(), bg, bg_w, bg_h, map, level, placed, facing, aim, shots: Vec::new(), ranges: Vec::new(), foreground })
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
        placed.extend(level.objects.iter().enumerate().flat_map(|(i, o)| art.place(i, o)));
        placed.extend(level.markers.iter().enumerate().flat_map(|(i, m)| art.place_marker(i, m, facing, aim)));
        placed.sort_by_key(|p| (p.sort_y, p.layer));
    }
    placed
}

/// Per-pixel foreground mask from `.tmi` (tile index per cell, 0 = none) and `.til` (32x32 coverage masks).
fn foreground_mask(tmi: &Option<Vec<u8>>, til: &Option<Vec<u8>>, bg_w: usize, bg_h: usize) -> Vec<bool> {
    use opensow_formats::{map::TileMap, sprite};
    let mut fg = vec![false; bg_w * bg_h];
    let (Some(tmi), Some(til)) = (tmi, til) else { return fg };
    let (Ok(tiles), Ok(frames)) = (TileMap::parse(tmi), sprite::parse(til, sprite::Kind::Shadow)) else { return fg };
    for cy in 0..tiles.height as usize {
        for cx in 0..tiles.width as usize {
            let k = tiles.tiles[cy * tiles.width as usize + cx] as usize;
            let Some(f) = frames.get(k).filter(|_| k != 0) else { continue };
            for ty in 0..(f.height as usize).min(CELL as usize) {
                for tx in 0..(f.width as usize).min(CELL as usize) {
                    let (x, y) = (cx * CELL as usize + tx, cy * CELL as usize + ty);
                    if f.mask[ty * f.width as usize + tx] && x < bg_w && y < bg_h {
                        fg[y * bg_w + x] = true;
                    }
                }
            }
        }
    }
    fg
}

