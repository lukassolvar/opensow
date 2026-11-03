//! Microsoft Video 1 (`CRAM`, 8-bit palettised) decoder, used by the `mB.sgv` AVI.
//!
//! The picture is cut into 4x4 blocks, stored bottom row first like a DIB.
//! Each block is a skip, one colour, two colours with a 16-bit mask, or eight
//! colours (two per 2x2 quadrant) with a mask.

use crate::{Error, Result};

/// Decoder state: the palettised picture, kept between frames because skipped blocks keep their pixels.
pub struct Decoder {
    pub width: usize,
    pub height: usize,
    pub palette: Vec<[u8; 3]>,
    pub pixels: Vec<u8>,
}

impl Decoder {
    pub fn new(width: usize, height: usize, palette: Vec<[u8; 3]>) -> Decoder {
        Decoder { width, height, palette, pixels: vec![0; width * height] }
    }

    /// Palette from the BITMAPINFO of an AVI `strf` chunk (BGRX entries after the 40-byte header).
    pub fn palette_from_strf(strf: &[u8]) -> Vec<[u8; 3]> {
        strf.get(40..).unwrap_or(&[]).chunks_exact(4).take(256).map(|c| [c[2], c[1], c[0]]).collect()
    }

    /// Apply one frame; an empty chunk repeats the previous picture.
    pub fn decode(&mut self, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        let (bw, bh) = (self.width / 4, self.height / 4);
        let mut pos = 0;
        let mut next = |what: &str| -> Result<u8> {
            let v = *data.get(pos).ok_or_else(|| Error::Invalid(format!("truncated {what} at {pos:#x}")))?;
            pos += 1;
            Ok(v)
        };
        let mut skip = 0usize;
        for by in (0..bh).rev() {
            for bx in 0..bw {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let (a, b) = (next("block")?, next("block")?);
                if b & 0xfc == 0x84 {
                    // run of skipped blocks, this one included
                    skip = ((b as usize - 0x84) << 8) + a as usize - 1;
                    continue;
                }
                let mut block = [0u8; 16];
                if b < 0x80 {
                    let flags = (b as u16) << 8 | a as u16;
                    let colors = [next("colour")?, next("colour")?];
                    for (i, p) in block.iter_mut().enumerate() {
                        *p = colors[((flags >> i) & 1) as usize ^ 1];
                    }
                } else if b >= 0x90 {
                    let flags = (b as u16) << 8 | a as u16;
                    let mut colors = [0u8; 8];
                    for c in &mut colors {
                        *c = next("colour")?;
                    }
                    for (i, p) in block.iter_mut().enumerate() {
                        let (x, y) = (i % 4, i / 4);
                        *p = colors[((y & 2) << 1) + (x & 2) + (((flags >> i) & 1) as usize ^ 1)];
                    }
                } else {
                    block = [a; 16];
                }
                for (i, &p) in block.iter().enumerate() {
                    // block rows are sent bottom first, and so are the rows inside a block
                    let (x, y) = (bx * 4 + i % 4, by * 4 + 3 - i / 4);
                    self.pixels[y * self.width + x] = p;
                }
            }
        }
        Ok(())
    }

    pub fn to_rgb(&self) -> Vec<u8> {
        self.pixels.iter().flat_map(|&p| self.palette.get(p as usize).copied().unwrap_or([0; 3])).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Vec<[u8; 3]> {
        (0..=255u8).map(|i| [i, i, i]).collect()
    }

    #[test]
    fn one_two_and_eight_colour_blocks_and_skips() {
        // 8x4 picture = two blocks; the bottom row of the block is stored first
        let mut d = Decoder::new(8, 4, palette());
        // block 0: one colour (7); block 1: two colours, mask picks colour 1 where the bit is clear
        d.decode(&[7, 0x80, 0b0101, 0x00, 10, 20]).unwrap();
        assert!(d.pixels[..4].iter().all(|&p| p == 7));
        // block 1 stream row 0 (= bottom picture row 3) has mask 0101: bits set -> colour 10 at x 0 and 2
        assert_eq!(&d.pixels[3 * 8 + 4..3 * 8 + 8], &[10, 20, 10, 20]);
        assert_eq!(&d.pixels[4..8], &[20, 20, 20, 20]);
        // skip both blocks: nothing changes; an empty chunk also keeps the picture
        let before = d.pixels.clone();
        d.decode(&[2, 0x84]).unwrap();
        d.decode(&[]).unwrap();
        assert_eq!(d.pixels, before);
        // eight colours: quadrant colour pairs
        let mut e = Decoder::new(4, 4, palette());
        e.decode(&[0, 0x90, 1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        // mask 0x9000 sets the bits of stream row 3 (the top picture row) at x 0 and 3: those take the first colour of the pair
        assert_eq!(&e.pixels[12..16], &[2, 2, 4, 4]);
        assert_eq!(&e.pixels[..4], &[5, 6, 8, 7]);
        assert!(e.decode(&[0]).is_err());
    }
}
