//! Level terrain: `Mxx.map` (per-cell flags) and `Mxx.tmi` (tile index grid).
//!
//! `.map` layout (all little-endian):
//! ```text
//! u8   tag_len = 4, "VER7"
//! u32  a, b            probably start/camera position in pixels (unconfirmed)
//! u32  width, height   in cells (one cell = 32x32 px of the background SRF)
//! u8   unknown         always 0 so far
//! width*height * 17-byte cells, row-major:
//!   u8 x; [u8;3]+u8 blocked (0xFFFFFD when impassable, else 0);
//!   [u8;4] border (same encoding; set on the outer ring of the map and, on M04/M10/M22, on no-fly areas: the air layer's terrain, docs/pathfinding.md);
//!   [u8;3] zero; u8 flag (sparse, meaning unknown); [u8;4] zero; u8 y
//! 4 zero bytes
//! ```
//! File size is always `22 + width*height*17 + 4` (checked on all shipped maps).
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
    /// Sparse flag at cell byte 11; meaning not yet identified.
    pub flag: bool,
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
            cells.push(Cell { x: c[0], y: c[16], blocked: c[1] != 0, border: c[5] != 0, flag: c[11] != 0 });
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
