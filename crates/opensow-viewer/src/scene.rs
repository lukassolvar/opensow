//! A level loaded for drawing: [`Scene`] holds the background picture, the blocked-cell map, the level's objects with
//! their sprites, and the overlays the viewer can switch on.

use crate::art::{Art, Placed};
use crate::render::View;
use crate::upscale::{self, HiGround};
use opensow_formats::{level::Level, map::Map, srf, GameDir};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

/// Pixels per map cell.
pub const CELL: i32 = 32;

/// A level loaded for drawing (see the module docs).
pub struct Scene {
    /// The level's name, e.g. `M01`.
    pub name: String,
    /// Background RGB8, `bg_w * bg_h * 3`.
    pub bg: Vec<u8>,
    /// Width of the background in pixels (map cells * 32).
    pub bg_w: usize,
    /// Height of the background in pixels.
    pub bg_h: usize,
    /// The per-cell terrain flags (`.map`).
    pub map: Map,
    /// The level's objects and unit markers, when the level file was found.
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
    /// The 2x enhanced ground, made on a background thread the first time the camera zooms in (`upscale`).
    pub hi: HiSlot,
}

/// A ground picture and its foreground mask as the renderer samples them; `k` = its size relative to the map in pixels (1 or 2).
pub struct GroundRef<'a> {
    /// The ground picture, RGB8.
    pub bg: &'a [u8],
    /// The foreground mask, one flag per pixel.
    pub fg: &'a [bool],
    /// Width of both in pixels.
    pub w: usize,
    /// Height of both in pixels.
    pub h: usize,
    /// Size relative to the map in pixels: 1 for the original ground, 2 for the enhanced one.
    pub k: usize,
}

/// A ground on its own: the picture (RGB8), the foreground flags and the cell grid (a custom map's generated layout is built on
/// one, docs/generated-maps.md).
#[derive(Debug, Clone)]
pub struct Ground {
    /// RGB8, `w * h * 3`.
    pub bg: Vec<u8>,
    /// One flag per pixel.
    pub fg: Vec<bool>,
    /// The cells (`.map`).
    pub map: Map,
    /// Width in pixels.
    pub w: usize,
    /// Height in pixels.
    pub h: usize,
}

/// Holds a scene's 2x enhanced ground once the background thread has made it.
#[derive(Default)]
pub struct HiSlot {
    started: AtomicBool,
    data: Arc<OnceLock<HiGround>>,
}

/// Maps with more pixels than this (2048 x 2048 is 4.2M) stay unenhanced: the 2x ground would take hundreds of MB.
const MAX_ENHANCED_GROUND: usize = 4_300_000;

impl Scene {
    /// Loads level `name` from the install; every unit is given `facing`, weapon tops `aim` (`None` = the same).
    pub fn load(game: &GameDir, name: &str, facing: usize, aim: Option<usize>) -> Result<Scene, String> {
        Scene::load_with(game, name, facing, aim, None)
    }

    /// Like [`Scene::load`] with the level script replaced: the terrain files come from `name` (`M07`), the objects and units from
    /// `script` (a `.net` skirmish map, whose terrain is a mission's, `GameDir::net_terrain`).
    pub fn load_with(game: &GameDir, name: &str, facing: usize, aim: Option<usize>, script: Option<Level>) -> Result<Scene, String> {
        let ground = load_ground(game, name)?;
        let level = script.or_else(|| read_level_file(game, name, "edt").ok().and_then(|d| Level::parse(&d).ok()));
        let placed = build_placed(game, level.as_ref(), facing, aim);
        let Ground { bg, fg: foreground, map, w: bg_w, h: bg_h } = ground;
        Ok(Scene {
            name: name.to_owned(),
            bg,
            bg_w,
            bg_h,
            map,
            level,
            placed,
            facing,
            aim,
            shots: Vec::new(),
            ranges: Vec::new(),
            foreground,
            hi: HiSlot::default(),
        })
    }

    /// The ground to draw for `view` and the view to draw it with: the enhanced 2x ground (view doubled, zoom halved, so the screen
    /// result is placed exactly as before) once it is ready and the camera is above zoom 1; the exe's picture otherwise.
    pub fn ground(&self, view: &View) -> (GroundRef<'_>, View) {
        let plain = (GroundRef { bg: &self.bg, fg: &self.foreground, w: self.bg_w, h: self.bg_h, k: 1 }, *view);
        if !upscale::wanted(view.zoom) || self.bg_w * self.bg_h > MAX_ENHANCED_GROUND {
            return plain;
        }
        if !self.hi.started.swap(true, Ordering::Relaxed) {
            let (bg, fg, w, h, slot) = (self.bg.clone(), self.foreground.clone(), self.bg_w, self.bg_h, self.hi.data.clone());
            std::thread::spawn(move || {
                let _ = slot.set(upscale::enhance_ground(&bg, &fg, w, h));
            });
        }
        match self.hi.data.get() {
            Some(g) => (GroundRef { bg: &g.bg, fg: &g.fg, w: g.w, h: g.h, k: 2 }, View { x: view.x * 2.0, y: view.y * 2.0, zoom: view.zoom / 2.0 }),
            None => plain,
        }
    }

    /// Makes the enhanced ground now instead of on a background thread (headless screenshots must not race it).
    pub fn warm_ground(&self) {
        if upscale::enabled() && self.bg_w * self.bg_h <= MAX_ENHANCED_GROUND && !self.hi.started.swap(true, Ordering::Relaxed) {
            let _ = self.hi.data.set(upscale::enhance_ground(&self.bg, &self.foreground, self.bg_w, self.bg_h));
        }
    }

    /// A copy of the ground (picture, foreground, cells).
    pub fn ground_copy(&self) -> Ground {
        Ground { bg: self.bg.clone(), fg: self.foreground.clone(), map: self.map.clone(), w: self.bg_w, h: self.bg_h }
    }

    /// Replaces the ground (same size). New allocations and a fresh enhanced ground, so the GPU cache and the 2x ground follow.
    pub fn set_ground(&mut self, g: Ground) {
        self.bg = g.bg;
        self.foreground = g.fg;
        self.map = g.map;
        self.bg_w = g.w;
        self.bg_h = g.h;
        self.hi = HiSlot::default();
    }

    /// Replaces the level script (objects and units; the terrain stays) and re-places the sprites (the map editor after an edit).
    pub fn set_level(&mut self, game: &GameDir, level: Level) {
        self.level = Some(level);
        self.placed = build_placed(game, self.level.as_ref(), self.facing, self.aim);
    }

    /// Re-places every unit with a new facing (sprites only; the terrain is kept).
    pub fn set_facing(&mut self, game: &GameDir, facing: usize) {
        self.facing = facing % 32;
        self.placed = build_placed(game, self.level.as_ref(), self.facing, self.aim);
    }

    /// Campaign level names (`M00`..) found in the install, sorted.
    pub fn list(game: &GameDir) -> Vec<String> {
        let mut v: Vec<String> = game
            .level_files()
            .into_iter()
            .filter_map(|n| {
                let (stem, ext) = n.rsplit_once('.')?;
                (ext.eq_ignore_ascii_case("srf") && stem.len() == 3 && stem[..1].eq_ignore_ascii_case("m")).then(|| stem.to_ascii_uppercase())
            })
            .collect();
        v.sort();
        v
    }
}

fn read_level_file(game: &GameDir, name: &str, ext: &str) -> Result<Vec<u8>, String> {
    let p = game.resolve(&format!("Data\\Levels\\{name}.{ext}")).filter(|p| p.exists()).ok_or_else(|| format!("{name}.{ext} not found"))?;
    std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))
}

/// Loads mission `name`'s ground only: the picture (`.srf`), the cells (`.map`) and the foreground (`.tmi` + `.til`).
pub fn load_ground(game: &GameDir, name: &str) -> Result<Ground, String> {
    let read = |ext: &str| read_level_file(game, name, ext);
    let jpeg = srf::to_jpeg(&read("srf")?).map_err(|e| e.to_string())?;
    let mut dec = jpeg_decoder::Decoder::new(jpeg.as_slice());
    let bg = dec.decode().map_err(|e| format!("{name}.srf: {e}"))?;
    let info = dec.info().ok_or("no JPEG info")?;
    if info.pixel_format != jpeg_decoder::PixelFormat::RGB24 {
        return Err(format!("unsupported JPEG pixel format {:?}", info.pixel_format));
    }
    let map = Map::parse(&read("map")?).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    if w != map.width as usize * CELL as usize || h != map.height as usize * CELL as usize {
        eprintln!("warning: background {w}x{h} does not match map {}x{} cells", map.width, map.height);
    }
    let fg = foreground_mask(&read("tmi").ok(), &read("til").ok(), w, h);
    Ok(Ground { bg, fg, map, w, h })
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

/// A reduced picture of a map's ground (`.srf`), for menus.
#[derive(Debug, Clone)]
pub struct Thumbnail {
    /// `w * h` packed `0x00RRGGBB` pixels.
    pub pixels: Vec<u32>,
    /// Width of the thumbnail in pixels.
    pub w: usize,
    /// Height of the thumbnail in pixels.
    pub h: usize,
    /// The full picture's size in pixels (cells * 32), to place cells on the thumbnail.
    pub full_w: usize,
    /// Height of the full picture in pixels.
    pub full_h: usize,
}

/// Decodes `name.srf` and shrinks it (box filter) to fit `max_w` x `max_h`, keeping the aspect ratio.
pub fn thumbnail(game: &GameDir, name: &str, max_w: usize, max_h: usize) -> Option<Thumbnail> {
    let path = game.resolve(&format!("Data\\Levels\\{name}.srf")).filter(|p| p.exists())?;
    let jpeg = srf::to_jpeg(&std::fs::read(path).ok()?).ok()?;
    let mut dec = jpeg_decoder::Decoder::new(jpeg.as_slice());
    let rgb = dec.decode().ok()?;
    let info = dec.info()?;
    if info.pixel_format != jpeg_decoder::PixelFormat::RGB24 {
        return None;
    }
    Some(shrink(&rgb, info.width as usize, info.height as usize, max_w, max_h))
}

/// Shrinks an RGB8 picture (box filter) to fit `max_w` x `max_h`, keeping the aspect ratio.
pub fn shrink(rgb: &[u8], fw: usize, fh: usize, max_w: usize, max_h: usize) -> Thumbnail {
    let k = (max_w as f32 / fw as f32).min(max_h as f32 / fh as f32);
    let (w, h) = (((fw as f32 * k) as usize).max(1), ((fh as f32 * k) as usize).max(1));
    let mut pixels = vec![0u32; w * h];
    for y in 0..h {
        let (y0, y1) = (y * fh / h, ((y + 1) * fh / h).max(y * fh / h + 1).min(fh));
        for x in 0..w {
            let (x0, x1) = (x * fw / w, ((x + 1) * fw / w).max(x * fw / w + 1).min(fw));
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = (sy * fw + sx) * 3;
                    r += rgb[i] as u32;
                    g += rgb[i + 1] as u32;
                    b += rgb[i + 2] as u32;
                    n += 1;
                }
            }
            pixels[y * w + x] = (r / n) << 16 | (g / n) << 8 | (b / n);
        }
    }
    Thumbnail { pixels, w, h, full_w: fw, full_h: fh }
}
