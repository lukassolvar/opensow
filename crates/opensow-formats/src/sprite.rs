//! Run-length encoded sprites: `.ps6`, `.tsp` (RGB565 colour) and `.sha` (shadow masks).
//!
//! A file is a sequence of frames terminated by a zero `u32`:
//! ```text
//! u32  size                bytes following this field
//! u16  width, height
//! i16  hot_x, hot_y        anchor point
//! u16  extra[4]            e.g. 320,240,160,120 - meaning unconfirmed
//! u32  row_offset[height]  in u16 words, relative to the end of `size`
//! rows:
//!   u16 run_count, u16 first_run_opaque
//!   run_count × u16 length, alternating transparent / opaque;
//!   colour sprites follow each opaque length with `length` RGB565 pixels,
//!   shadow sprites carry no pixel data (opaque = darken the ground).
//! ```

use crate::{Error, Reader, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `.ps6` / `.tsp`: RGB565 pixels.
    Color,
    /// `.sha`: coverage only.
    Shadow,
}

impl Kind {
    pub fn from_extension(path: &std::path::Path) -> Option<Kind> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "ps6" | "tsp" => Some(Kind::Color),
            "sha" | "msk" => Some(Kind::Shadow),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub hot_x: i16,
    pub hot_y: i16,
    pub extra: [u16; 4],
    /// Row-major RGB565 colour (0 for shadow sprites and transparent pixels).
    pub pixels: Vec<u16>,
    /// Row-major coverage: `true` where a pixel is drawn.
    pub mask: Vec<bool>,
}

impl Frame {
    /// RGBA8 image; shadow coverage becomes black at `shadow_alpha`.
    pub fn to_rgba(&self, shadow_alpha: Option<u8>) -> Vec<u8> {
        self.pixels
            .iter()
            .zip(&self.mask)
            .flat_map(|(&c, &m)| match (m, shadow_alpha) {
                (false, _) => [0, 0, 0, 0],
                (true, Some(a)) => [0, 0, 0, a],
                (true, None) => {
                    let [r, g, b] = rgb565_to_rgb8(c);
                    [r, g, b, 255]
                }
            })
            .collect()
    }
}

#[inline(always)]
pub fn rgb565_to_rgb8(c: u16) -> [u8; 3] {
    let (r, g, b) = ((c >> 11) as u8, ((c >> 5) & 63) as u8, (c & 31) as u8);
    [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2]
}

pub fn parse(data: &[u8], kind: Kind) -> Result<Vec<Frame>> {
    let mut frames = Vec::new();
    let mut r = Reader::new(data);
    loop {
        let size = r.u32("frame size")? as usize;
        if size == 0 {
            break;
        }
        let base = r.pos();
        frames.push(parse_frame(&data[..(base + size).min(data.len())], base, kind)?);
        r = Reader::at(data, base + size);
    }
    Ok(frames)
}

fn parse_frame(data: &[u8], base: usize, kind: Kind) -> Result<Frame> {
    let mut r = Reader::at(data, base);
    let width = r.u16("frame width")?;
    let height = r.u16("frame height")?;
    let hot_x = r.i16("hot x")?;
    let hot_y = r.i16("hot y")?;
    let mut extra = [0; 4];
    for e in &mut extra {
        *e = r.u16("frame extra")?;
    }
    let row_offsets = (0..height).map(|_| r.u32("row offset")).collect::<Result<Vec<_>>>()?;

    let (w, h) = (width as usize, height as usize);
    let mut pixels = vec![0u16; w * h];
    let mut mask = vec![false; w * h];
    for (y, &off) in row_offsets.iter().enumerate() {
        let mut r = Reader::at(data, base + off as usize * 2);
        let runs = r.u16("run count")?;
        let mut opaque = r.u16("first run flag")? != 0;
        let mut x = 0usize;
        for _ in 0..runs {
            let len = r.u16("run length")? as usize;
            if x + len > w {
                return Err(Error::Invalid(format!("row {y} overflows width {w}")));
            }
            let row = y * w + x;
            if opaque {
                mask[row..row + len].fill(true);
                if kind == Kind::Color {
                    for p in &mut pixels[row..row + len] {
                        *p = r.u16("pixel")?;
                    }
                }
            }
            x += len;
            opaque = !opaque;
        }
        if x != w {
            return Err(Error::Invalid(format!("row {y} covers {x} of {w} pixels")));
        }
    }
    Ok(Frame { width, height, hot_x, hot_y, extra, pixels, mask })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_bytes(rows: &[&[u16]], w: u16) -> Vec<u8> {
        let h = rows.len() as u16;
        let mut body: Vec<u16> = vec![w, h, 1, 2, 0, 0, 0, 0];
        let table_at = body.len();
        body.extend(std::iter::repeat(0).take(rows.len() * 2));
        for (i, row) in rows.iter().enumerate() {
            let off = body.len() as u32;
            body[table_at + i * 2] = off as u16;
            body[table_at + i * 2 + 1] = (off >> 16) as u16;
            body.extend(*row);
        }
        let mut out = ((body.len() * 2) as u32).to_le_bytes().to_vec();
        out.extend(body.iter().flat_map(|v| v.to_le_bytes()));
        out.extend(0u32.to_le_bytes());
        out
    }

    #[test]
    fn decodes_runs() {
        // 1 transparent, 2 opaque (white, red), 1 transparent; then 4 opaque.
        let data = frame_bytes(&[&[3, 0, 1, 2, 0xFFFF, 0xF800, 1], &[1, 1, 4, 1, 2, 3, 4]], 4);
        let f = &parse(&data, Kind::Color).unwrap()[0];
        assert_eq!((f.width, f.height, f.hot_x, f.hot_y), (4, 2, 1, 2));
        assert_eq!(f.mask, [false, true, true, false, true, true, true, true]);
        assert_eq!(&f.pixels[..4], &[0, 0xFFFF, 0xF800, 0]);
        assert_eq!(rgb565_to_rgb8(0xF800), [255, 0, 0]);
    }

    #[test]
    fn shadow_runs_have_no_pixels() {
        let data = frame_bytes(&[&[3, 0, 1, 2, 1]], 4);
        let f = &parse(&data, Kind::Shadow).unwrap()[0];
        assert_eq!(f.mask, [false, true, true, false]);
    }

    #[test]
    fn rejects_bad_row_width() {
        let data = frame_bytes(&[&[1, 0, 3]], 4);
        assert!(parse(&data, Kind::Shadow).is_err());
    }
}
