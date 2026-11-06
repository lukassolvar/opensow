//! Level terrain: `Mxx.map` (per-cell flags) and `Mxx.tmi` (tile index grid).
//!
//! A `.map` file is the exe's own `Serialize` stream of the map object (`0x459960`, global `0x6bf980`, docs/savegame.md) in
//! its "level" form, so `save::MapState` parses it too (`tests/real_data.rs`). All little-endian:
//! ```text
//! u8   tag_len = 4, "VER7"      the version string; the loader compares it with the CString at 0x71cec8
//!                               ("Wrong map version!!!")
//! u32  a, b                     scroll position of player 0's view in pixels ([map+0x20], [map+0x2c]; clamped
//!                               to the map minus the screen)
//! u32  width, height            in cells (one cell = 32x32 px of the background SRF)
//! height * width * 17-byte cells, row-major (y outer, x inner; in memory index x * 128 + y):
//!   u16 coord    (x << 8) | y     [+0xcc], never read by the exe
//!   u32 ground   0xFFFFFD = impassable terrain, else 0                  [+0x80cc]   walkers' obstacle grid
//!   u32 air      the same for the air layer: the outer ring and, on M04/M10/M22, no-fly areas  [+0x180cc]
//!   u8  reserved always 0 (the only writer, 0x436130, ANDs it)          [+0x280cc]
//!   u8  no_marks 1 = no scorch decals / impact debris / smoke here (M02 only: 497 cells)  [+0x2c0cc]
//!   u8  no_build 1 = buildings and turrets may not be placed here        [+0x300cc]
//!   u32 building handle of the building covering the cell, 0 in files  [+0x340cc]
//! u8   0                         "has player records" (0 in files; 1 in savegames, then `u32 3` and 3 camera records)
//! u32  0                         number of map items (sprites added at run time; none in files)
//! ```
//! File size is always `22 + width*height*17 + 4`: the cell stream starts at byte 21 and the five tail bytes are the
//! two fields above. (Older notes read the first byte of each 17-byte window as a pad; the layout below is shifted by that
//! one byte, which is why `Cell::x` is byte 0 of the window: the high byte of `coord`.)
//!
//! `.tmi`: `u16 width, u16 height`, then `width*height` u16 indices into the
//! level's `.til` frame list (0 = no tile).

use crate::{Error, Reader, Result};

const TAG: &[u8] = b"VER7";
const CELL_SIZE: usize = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub x: u8,
    pub y: u8,
    pub blocked: bool,
    pub border: bool,
    /// `no_build` layer (`+0x300cc`, window byte 11): buildings and turrets cannot be placed here (shore and rock edges). The
    /// placement check (`0x408cfc`) refuses it; the footprint stamp (`0x40ad69`) sets it and a destroyed building clears it (`0x40b0e5`).
    pub no_build: bool,
    /// `no_marks` layer (`+0x2c0cc`, window byte 10): no scorch decal, flying piece or smoke puff lands here (docs/effects.md).
    /// Only M02 sets it (497 cells in its lower part).
    pub no_marks: bool,
}

#[derive(Debug, Clone)]
pub struct Map {
    pub a: u32,
    pub b: u32,
    pub width: u32,
    pub height: u32,
    pub cells: Vec<Cell>,
}

impl Map {
    pub fn parse(data: &[u8]) -> Result<Map> {
        let mut r = Reader::new(data);
        let tag_len = r.u8("tag length")? as usize;
        if r.bytes(tag_len, "tag")? != TAG {
            return Err(Error::Invalid("map tag is not VER7".into()));
        }
        let (a, b) = (r.u32("map a")?, r.u32("map b")?);
        let (width, height) = (r.u32("map width")?, r.u32("map height")?);
        let n = (width as usize)
            .checked_mul(height as usize)
            .filter(|&n| n <= 1 << 22)
            .ok_or_else(|| Error::Invalid(format!("absurd map size {width}x{height}")))?;
        r.u8("map header pad")?;
        let mut cells = Vec::with_capacity(n);
        for _ in 0..n {
            let c = r.bytes(CELL_SIZE, "map cell")?;
            cells.push(Cell { x: c[0], y: c[16], blocked: c[1] != 0, border: c[5] != 0, no_build: c[11] != 0, no_marks: c[10] != 0 });
        }
        if r.remaining() != 4 {
            return Err(Error::Invalid(format!("{} trailing bytes after cells, expected 4", r.remaining())));
        }
        Ok(Map { a, b, width, height, cells })
    }

    pub fn cell(&self, x: u32, y: u32) -> Option<&Cell> {
        (x < self.width && y < self.height).then(|| &self.cells[(y * self.width + x) as usize])
    }
}

#[derive(Debug, Clone)]
pub struct TileMap {
    pub width: u16,
    pub height: u16,
    pub tiles: Vec<u16>,
}

impl TileMap {
    pub fn parse(data: &[u8]) -> Result<TileMap> {
        let mut r = Reader::new(data);
        let (width, height) = (r.u16("tmi width")?, r.u16("tmi height")?);
        let tiles = (0..width as usize * height as usize).map(|_| r.u16("tmi tile")).collect::<Result<_>>()?;
        if r.remaining() != 0 {
            return Err(Error::Invalid("trailing bytes in tmi".into()));
        }
        Ok(TileMap { width, height, tiles })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_map() -> Vec<u8> {
        let mut d = vec![4];
        d.extend(TAG);
        for v in [10u32, 20, 2, 1] {
            d.extend(v.to_le_bytes());
        }
        d.push(0);
        for x in 0..2u8 {
            let mut c = [0u8; CELL_SIZE];
            c[0] = x;
            if x == 1 {
                c[1..4].copy_from_slice(&[0xFD, 0xFF, 0xFF]);
            }
            d.extend(c);
        }
        d.extend([0; 4]);
        d
    }

    #[test]
    fn no_build_and_no_marks_are_window_bytes_11_and_10() {
        let mut d = tiny_map();
        let first = 22; // the first 17-byte window (after the one-byte shift)
        d[first + 10] = 1;
        d[first + CELL_SIZE + 11] = 1;
        let m = Map::parse(&d).unwrap();
        assert!(m.cells[0].no_marks && !m.cells[0].no_build);
        assert!(m.cells[1].no_build && !m.cells[1].no_marks);
    }

    #[test]
    fn parses_cells() {
        let m = Map::parse(&tiny_map()).unwrap();
        assert_eq!((m.width, m.height, m.a, m.b), (2, 1, 10, 20));
        assert!(!m.cell(0, 0).unwrap().blocked && m.cell(1, 0).unwrap().blocked);
        assert!(m.cell(2, 0).is_none());
    }

    #[test]
    fn rejects_truncated() {
        let mut d = tiny_map();
        d.truncate(d.len() - 10);
        assert!(Map::parse(&d).is_err());
    }

    #[test]
    fn parses_tmi() {
        let t = TileMap::parse(&[2, 0, 1, 0, 1, 0, 5, 0]).unwrap();
        assert_eq!(t.tiles, [1, 5]);
    }
}
