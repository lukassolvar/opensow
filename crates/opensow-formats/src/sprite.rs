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

/// What a sprite file's rows hold, by file type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `.ps6` / `.tsp`: RGB565 pixels.
    Color,
    /// `.sha`: coverage only.
    Shadow,
}

impl Kind {
    /// The kind of a sprite file by its extension, `None` for other files.
    pub fn from_extension(path: &std::path::Path) -> Option<Kind> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "ps6" | "tsp" => Some(Kind::Color),
            "sha" | "msk" => Some(Kind::Shadow),
            _ => None,
        }
    }
}

/// One decoded frame of a sprite file.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// X of the anchor point (the hotspot).
    pub hot_x: i16,
    /// Y of the anchor point (the hotspot).
    pub hot_y: i16,
    /// Four header words whose meaning is not confirmed (e.g. 320, 240, 160, 120); unit sheets store sizes here.
    pub extra: [u16; 4],
    /// Row-major RGB565 colour (0 for shadow sprites and transparent pixels).
    pub pixels: Vec<u16>,
    /// Row-major coverage: `true` where a pixel is drawn.
    pub mask: Vec<bool>,
    /// Lazily made 2x enhanced copy of this frame (`opensow_viewer::upscale`); never part of the file format.
    pub enhanced: std::sync::OnceLock<std::sync::Arc<Frame>>,
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

/// Expands an RGB565 colour to 8 bits per channel.
#[inline(always)]
pub fn rgb565_to_rgb8(c: u16) -> [u8; 3] {
    let (r, g, b) = ((c >> 11) as u8, ((c >> 5) & 63) as u8, (c & 31) as u8);
    [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2]
}

/// Decodes every frame of a sprite file of the given kind.
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
    Ok(Frame { width, height, hot_x, hot_y, extra, pixels, mask, enhanced: Default::default() })
}

/// Inverse of [`parse`]: encodes frames as a sprite file (frames, then a zero `u32`).
/// Rows are written as alternating transparent / opaque runs (the first flag says which comes first);
/// `Kind::Shadow` frames carry coverage only. Output of `parse` re-encodes to the same frames.
pub fn encode(frames: &[Frame], kind: Kind) -> Vec<u8> {
    let mut out = Vec::new();
    for f in frames {
        let (w, h) = (f.width as usize, f.height as usize);
        let mut body: Vec<u16> = vec![f.width, f.height, f.hot_x as u16, f.hot_y as u16];
        body.extend(f.extra);
        let table_at = body.len();
        body.extend(std::iter::repeat_n(0, h * 2));
        for y in 0..h {
            let off = body.len() as u32;
            body[table_at + y * 2] = off as u16;
            body[table_at + y * 2 + 1] = (off >> 16) as u16;
            let row = y * w..(y + 1) * w;
            let mask = &f.mask[row.clone()];
            let px = &f.pixels[row];
            let mut runs: Vec<u16> = Vec::new();
            let mut x = 0;
            let first_opaque = mask.first().copied().unwrap_or(false);
            let mut count = 0u16;
            while x < w {
                let cur = mask[x];
                let start = x;
                while x < w && mask[x] == cur {
                    x += 1;
                }
                runs.push((x - start) as u16);
                if cur && kind == Kind::Color {
                    runs.extend(&px[start..x]);
                }
                count += 1;
            }
            body.push(count);
            body.push(first_opaque as u16);
            body.extend(runs);
        }
        out.extend(((body.len() * 2) as u32).to_le_bytes());
        out.extend(body.iter().flat_map(|v| v.to_le_bytes()));
    }
    out.extend(0u32.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_bytes(rows: &[&[u16]], w: u16) -> Vec<u8> {
        let h = rows.len() as u16;
        let mut body: Vec<u16> = vec![w, h, 1, 2, 0, 0, 0, 0];
        let table_at = body.len();
        body.extend(std::iter::repeat_n(0, rows.len() * 2));
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

    #[test]
    fn encode_round_trips() {
        let data = frame_bytes(&[&[3, 0, 1, 2, 0xFFFF, 0xF800, 1], &[1, 1, 4, 1, 2, 3, 4]], 4);
        let frames = parse(&data, Kind::Color).unwrap();
        assert_eq!(encode(&frames, Kind::Color), data);
        let sh = frame_bytes(&[&[3, 0, 1, 2, 1]], 4);
        assert_eq!(encode(&parse(&sh, Kind::Shadow).unwrap(), Kind::Shadow), sh);
    }
}
