//! Intel Indeo 5 (IV50) intra-frame decoder, enough for `mA.sgv` (docs/indeo5.md).
//!
//! The game plays this video through Windows' own codec; here it is decoded in Rust.
//! `mA` consists of 104 intra frames, 4:1:0 (YVU9), 640x480, one 8x8 slant-transformed luma band
//! and two 4x4 chroma bands. Everything else Indeo 5 can do (inter frames, scalability, slices,
//! other transforms, quantiser matrices) is rejected with an error rather than guessed at.
//!
//! The arithmetic (slant networks, dequantisation, DC chain) was identified with a reference decoder
//! and every frame of the real file is bit-identical to its output.

use crate::{Error, Result};

/// A decoded picture: planar YVU9.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// Picture width in pixels.
    pub width: usize,
    /// Picture height in pixels.
    pub height: usize,
    /// Width of the two chroma planes.
    pub chroma_width: usize,
    /// Height of the two chroma planes.
    pub chroma_height: usize,
    /// The luma plane, row by row.
    pub y: Vec<u8>,
    /// The V chroma plane.
    pub v: Vec<u8>,
    /// The U chroma plane.
    pub u: Vec<u8>,
}

impl Frame {
    /// One pixel as `0x00RRGGBB` (BT.601 limited range, chroma replicated 4x4).
    pub fn rgb_at(&self, x: usize, y: usize) -> u32 {
        let l = (self.y[y * self.width + x] as i32 - 16) * 298;
        let ci = (y / 4).min(self.chroma_height - 1) * self.chroma_width + (x / 4).min(self.chroma_width - 1);
        let (b, r) = (self.u[ci] as i32 - 128, self.v[ci] as i32 - 128);
        let c = |v: i32| ((v + 128) >> 8).clamp(0, 255) as u32;
        c(l + 409 * r) << 16 | c(l - 100 * b - 208 * r) << 8 | c(l + 516 * b)
    }

    /// The picture as 8-bit RGB, three bytes per pixel, row by row.
    pub fn to_rgb(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.width * self.height * 3);
        for y in 0..self.height {
            for x in 0..self.width {
                let p = self.rgb_at(x, y);
                out.extend([(p >> 16) as u8, (p >> 8) as u8, p as u8]);
            }
        }
        out
    }
}

struct RvMap {
    eob: u8,
    esc: u8,
    run: [u8; 256],
    val: [i8; 256],
}

/// Run/value maps: (end-of-block symbol, escape symbol, run per symbol, value per symbol).
const RVMAPS: [RvMap; 9] = [
    RvMap {
        eob: 5,
        esc: 2,
        run: [
            1, 1, 0, 1, 1, 0, 1, 1, 2, 2, 1, 1, 1, 1, 3, 3, 1, 1, 2, 2, 1, 1, 4, 4, 1, 1, 1, 1, 2, 2, 5, 5, 1, 1, 3, 3, 1, 1, 6, 6, 1, 2, 1, 2, 7, 7, 1, 1, 8,
            8, 1, 1, 4, 2, 1, 4, 2, 1, 3, 3, 1, 1, 1, 9, 9, 1, 2, 1, 2, 1, 5, 5, 1, 1, 10, 10, 1, 1, 3, 3, 2, 2, 1, 1, 11, 11, 6, 4, 4, 1, 6, 1, 2, 1, 2, 12,
            8, 1, 12, 7, 8, 7, 1, 16, 1, 16, 1, 3, 3, 13, 1, 13, 2, 2, 1, 15, 1, 5, 14, 15, 1, 5, 14, 1, 17, 8, 17, 8, 1, 4, 4, 2, 2, 1, 25, 25, 24, 24, 1, 3,
            1, 3, 1, 8, 6, 7, 6, 1, 18, 8, 18, 1, 7, 23, 2, 2, 23, 1, 1, 21, 22, 9, 9, 22, 19, 1, 21, 5, 19, 5, 1, 33, 20, 33, 20, 8, 4, 4, 1, 32, 2, 2, 8, 3,
            32, 26, 3, 1, 7, 7, 26, 6, 1, 6, 1, 1, 16, 1, 10, 1, 10, 2, 16, 29, 28, 2, 29, 28, 1, 27, 5, 8, 5, 27, 1, 8, 3, 7, 3, 31, 41, 31, 1, 41, 6, 1, 6,
            7, 4, 4, 1, 1, 2, 1, 2, 11, 34, 30, 11, 1, 30, 15, 15, 34, 36, 40, 36, 40, 35, 35, 37, 37, 39, 39, 38, 38,
        ],
        val: [
            1, -1, 0, 2, -2, 0, 3, -3, 1, -1, 4, -4, 5, -5, 1, -1, 6, -6, 2, -2, 7, -7, 1, -1, 8, -8, 9, -9, 3, -3, 1, -1, 10, -10, 2, -2, 11, -11, 1, -1, 12,
            4, -12, -4, 1, -1, 13, -13, 1, -1, 14, -14, 2, 5, 15, -2, -5, -15, -3, 3, 16, -16, 17, 1, -1, -17, 6, 18, -6, -18, 2, -2, 19, -19, 1, -1, 20, -20,
            4, -4, 7, -7, 21, -21, 1, -1, 2, 3, -3, 22, -2, -22, 8, 23, -8, 1, 2, -23, -1, 2, -2, -2, 24, 1, -24, -1, 25, 5, -5, 1, -25, -1, 9, -9, 26, 1, -26,
            3, 1, -1, 27, -3, -1, -27, 1, 3, -1, -3, 28, -4, 4, 10, -10, -28, 1, -1, 1, -1, 29, 6, -29, -6, 30, -4, 3, 3, -3, -30, 1, 4, -1, 31, -3, 1, 11,
            -11, -1, -31, 32, -1, -1, 2, -2, 1, 1, -32, 1, 4, -1, -4, 33, -1, 1, 1, -1, 5, 5, -5, -33, -1, -12, 12, -5, -7, 1, 1, 7, 34, 4, -4, -1, 4, -34, -4,
            35, 36, -2, -35, -2, -36, 2, 13, 2, -1, 1, -13, 1, -1, 37, 1, -5, 6, 5, -1, 38, -6, -8, 5, 8, -1, 1, 1, -37, -1, 5, 39, -5, -5, 6, -6, -38, -39,
            -14, 40, 14, 2, 1, 1, -2, -40, -1, -2, 2, -1, -1, -1, 1, 1, 1, -1, 1, -1, 1, -1, 1, -1,
        ],
    },
    RvMap {
        eob: 0,
        esc: 38,
        run: [
            0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 8, 6, 8, 7, 7, 9, 9, 10, 10, 11, 11, 1, 12, 1, 12, 13, 13, 16, 14, 16, 14, 15, 15, 17, 17, 18, 0, 18, 19, 20,
            21, 19, 22, 21, 20, 22, 25, 24, 2, 25, 24, 23, 23, 2, 26, 28, 26, 28, 29, 27, 29, 27, 33, 33, 1, 32, 1, 3, 32, 30, 36, 3, 36, 30, 31, 31, 35, 34,
            37, 41, 34, 35, 37, 4, 41, 4, 49, 8, 8, 49, 40, 38, 5, 38, 40, 39, 5, 39, 42, 43, 42, 7, 57, 6, 43, 44, 6, 50, 7, 44, 57, 48, 50, 48, 45, 45, 46,
            47, 51, 46, 47, 58, 1, 51, 58, 1, 52, 59, 53, 9, 52, 55, 55, 59, 53, 56, 54, 56, 54, 9, 64, 64, 60, 63, 60, 63, 61, 62, 61, 62, 2, 10, 2, 10, 11,
            1, 11, 13, 12, 1, 12, 13, 16, 16, 8, 8, 14, 3, 3, 15, 14, 15, 4, 4, 1, 17, 17, 5, 1, 7, 7, 5, 6, 1, 2, 2, 6, 22, 1, 25, 21, 22, 8, 24, 1, 21, 25,
            24, 8, 18, 18, 23, 9, 20, 23, 33, 29, 33, 20, 1, 19, 1, 29, 36, 9, 36, 19, 41, 28, 57, 32, 3, 28, 3, 1, 27, 49, 49, 1, 32, 26, 26, 2, 4, 4, 7, 57,
            41, 2, 7, 10, 5, 37, 16, 10, 27, 8, 8, 13, 16, 37, 13, 1, 5,
        ],
        val: [
            0, 1, -1, 1, -1, 1, -1, 1, -1, 1, -1, 1, 1, -1, -1, 1, -1, 1, -1, 1, -1, 1, -1, 2, 1, -2, -1, 1, -1, 1, 1, -1, -1, 1, -1, 1, -1, 1, 0, -1, 1, 1, 1,
            -1, 1, -1, -1, -1, 1, 1, 2, -1, -1, 1, -1, -2, 1, 1, -1, -1, 1, 1, -1, -1, 1, -1, 3, 1, -3, 2, -1, 1, 1, -2, -1, -1, -1, 1, 1, 1, 1, 1, -1, -1, -1,
            2, -1, -2, 1, 2, -2, -1, 1, 1, 2, -1, -1, 1, -2, -1, 1, 1, -1, 2, 1, 2, -1, 1, -2, -1, -2, -1, -1, 1, 1, -1, 1, -1, 1, 1, 1, -1, -1, 1, 4, -1, -1,
            -4, 1, 1, 1, 2, -1, -1, 1, -1, -1, 1, -1, -1, 1, -2, 1, -1, 1, 1, -1, -1, 1, 1, -1, -1, 3, 2, -3, -2, 2, 5, -2, 2, 2, -5, -2, -2, -2, 2, -3, 3, 2,
            3, -3, 2, -2, -2, 3, -3, 6, 2, -2, 3, -6, 3, -3, -3, 3, 7, -4, 4, -3, 2, -7, 2, 2, -2, -4, 2, 8, -2, -2, -2, 4, 2, -2, 2, 3, 2, -2, -2, 2, 2, -2,
            -8, -2, 9, -2, 2, -3, -2, 2, -2, 2, 2, 2, 4, -2, -4, 10, 2, 2, -2, -9, -2, 2, -2, 5, 4, -4, 4, -2, 2, -5, -4, -3, 4, 2, -3, 3, -2, -5, 5, 3, 3, -2,
            -3, -10, -4,
        ],
    },
    RvMap {
        eob: 2,
        esc: 11,
        run: [
            1, 1, 0, 2, 2, 1, 1, 3, 3, 4, 4, 0, 1, 1, 5, 5, 2, 2, 6, 6, 7, 7, 1, 8, 1, 8, 3, 3, 9, 9, 1, 2, 2, 1, 4, 10, 4, 10, 11, 11, 1, 5, 12, 12, 1, 5, 13,
            13, 3, 3, 6, 6, 2, 2, 14, 14, 16, 16, 15, 7, 15, 8, 8, 7, 1, 1, 17, 17, 4, 4, 1, 1, 18, 18, 2, 2, 5, 5, 25, 3, 9, 3, 25, 9, 19, 24, 19, 24, 1, 21,
            20, 1, 21, 22, 20, 22, 23, 23, 8, 6, 33, 6, 8, 33, 7, 7, 26, 26, 1, 32, 1, 32, 28, 4, 28, 10, 29, 27, 27, 10, 41, 4, 29, 2, 2, 41, 36, 31, 49, 31,
            34, 30, 34, 36, 30, 35, 1, 49, 11, 5, 35, 11, 1, 3, 3, 5, 37, 37, 8, 40, 8, 40, 12, 12, 42, 42, 1, 38, 16, 57, 1, 6, 16, 39, 38, 6, 7, 7, 13, 13,
            39, 43, 2, 43, 57, 2, 50, 9, 44, 9, 50, 4, 15, 48, 44, 4, 1, 15, 48, 14, 14, 1, 45, 45, 8, 3, 5, 8, 51, 47, 3, 46, 46, 47, 5, 51, 1, 17, 17, 58, 1,
            58, 2, 52, 52, 2, 53, 7, 59, 6, 6, 56, 53, 55, 7, 55, 1, 54, 59, 56, 54, 10, 1, 10, 4, 60, 1, 60, 8, 4, 8, 64, 64, 61, 1, 63, 3, 63, 62, 61, 5, 11,
            5, 3, 11, 62,
        ],
        val: [
            1, -1, 0, 1, -1, 2, -2, 1, -1, 1, -1, 0, 3, -3, 1, -1, 2, -2, 1, -1, 1, -1, 4, 1, -4, -1, 2, -2, 1, -1, 5, 3, -3, -5, 2, 1, -2, -1, 1, -1, 6, 2, 1,
            -1, -6, -2, 1, -1, 3, -3, 2, -2, 4, -4, 1, -1, 1, -1, 1, 2, -1, 2, -2, -2, 7, -7, 1, -1, 3, -3, 8, -8, 1, -1, 5, -5, 3, -3, 1, 4, 2, -4, -1, -2, 1,
            1, -1, -1, 9, 1, 1, -9, -1, 1, -1, -1, 1, -1, 3, -3, 1, 3, -3, -1, 3, -3, 1, -1, 10, 1, -10, -1, 1, 4, -1, 2, 1, -1, 1, -2, 1, -4, -1, 6, -6, -1,
            1, 1, 1, -1, 1, 1, -1, -1, -1, 1, 11, -1, -2, 4, -1, 2, -11, 5, -5, -4, -1, 1, 4, 1, -4, -1, -2, 2, 1, -1, 12, 1, -2, 1, -12, 4, 2, 1, -1, -4, 4,
            -4, 2, -2, -1, 1, 7, -1, -1, -7, -1, -3, 1, 3, 1, 5, 2, 1, -1, -5, 13, -2, -1, 2, -2, -13, 1, -1, 5, 6, 5, -5, 1, 1, -6, 1, -1, -1, -5, -1, 14, 2,
            -2, 1, -14, -1, 8, 1, -1, -8, 1, 5, 1, 5, -5, 1, -1, 1, -5, -1, 15, 1, -1, -1, -1, 3, -15, -3, 6, 1, 16, -1, 6, -6, -6, 1, -1, 1, -16, 1, 7, -1, 1,
            -1, -6, -3, 6, -7, 3, -1,
        ],
    },
    RvMap {
        eob: 0,
        esc: 35,
        run: [
            0, 1, 1, 2, 2, 3, 3, 4, 4, 1, 1, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 2, 2, 10, 10, 1, 1, 11, 11, 12, 12, 3, 3, 13, 13, 0, 14, 14, 16, 15, 16, 15, 4, 4,
            17, 1, 17, 1, 5, 5, 18, 18, 2, 2, 6, 6, 8, 19, 7, 8, 7, 19, 20, 20, 21, 21, 22, 24, 22, 24, 23, 23, 1, 1, 25, 25, 3, 3, 26, 26, 9, 9, 27, 27, 28,
            28, 33, 29, 4, 33, 29, 1, 4, 1, 32, 32, 2, 2, 31, 10, 30, 10, 30, 31, 34, 34, 5, 5, 36, 36, 35, 41, 35, 11, 41, 11, 37, 1, 8, 8, 37, 6, 1, 6, 40,
            7, 7, 40, 12, 38, 12, 39, 39, 38, 49, 13, 49, 13, 3, 42, 3, 42, 16, 16, 43, 43, 14, 14, 1, 1, 44, 15, 44, 15, 2, 2, 57, 48, 50, 48, 57, 50, 4, 45,
            45, 4, 46, 47, 47, 46, 1, 51, 1, 17, 17, 51, 8, 9, 9, 5, 58, 8, 58, 5, 52, 52, 55, 56, 53, 56, 55, 59, 59, 53, 54, 1, 6, 54, 7, 7, 6, 1, 2, 3, 2,
            3, 64, 60, 60, 10, 10, 64, 61, 62, 61, 63, 1, 63, 62, 1, 18, 24, 18, 4, 25, 4, 8, 21, 21, 1, 24, 22, 25, 22, 8, 11, 19, 11, 23, 1, 20, 23, 19, 20,
            5, 12, 5, 1, 16, 2, 12, 13, 2, 13, 1, 16,
        ],
        val: [
            0, 1, -1, 1, -1, 1, -1, 1, -1, 2, -2, 1, -1, 1, -1, 1, -1, 1, -1, 1, -1, 2, -2, 1, -1, 3, -3, 1, -1, 1, -1, 2, -2, 1, -1, 0, 1, -1, 1, 1, -1, -1,
            2, -2, 1, 4, -1, -4, 2, -2, 1, -1, -3, 3, 2, -2, 2, 1, 2, -2, -2, -1, 1, -1, 1, -1, 1, 1, -1, -1, 1, -1, 5, -5, 1, -1, 3, -3, 1, -1, 2, -2, 1, -1,
            1, -1, 1, 1, 3, -1, -1, 6, -3, -6, -1, 1, 4, -4, 1, 2, 1, -2, -1, -1, 1, -1, 3, -3, 1, -1, 1, 1, -1, 2, -1, -2, 1, 7, -3, 3, -1, 3, -7, -3, 1, -3,
            3, -1, 2, 1, -2, 1, -1, -1, 1, 2, -1, -2, -4, -1, 4, 1, 2, -2, 1, -1, -2, 2, 8, -8, -1, 2, 1, -2, -5, 5, 1, -1, -1, 1, -1, 1, 4, -1, 1, -4, -1, -1,
            1, 1, 9, 1, -9, 2, -2, -1, -4, 3, -3, -4, -1, 4, 1, 4, 1, -1, 1, -1, 1, 1, -1, 1, -1, -1, -1, 10, 4, 1, 4, -4, -4, -10, 6, 5, -6, -5, 1, -1, 1, 3,
            -3, -1, 1, -1, -1, -1, 11, 1, 1, -11, -2, -2, 2, 5, -2, -5, -5, 2, -2, 12, 2, -2, 2, 2, 5, -3, -2, 3, -2, -12, -2, 2, 2, 2, -5, 3, 5, 13, -3, 7,
            -3, -3, -7, 3, -13, 3,
        ],
    },
    RvMap {
        eob: 0,
        esc: 34,
        run: [
            0, 1, 1, 1, 2, 2, 1, 3, 3, 1, 1, 1, 4, 4, 1, 5, 2, 1, 5, 2, 1, 1, 6, 6, 1, 1, 1, 1, 1, 7, 3, 1, 2, 3, 0, 1, 2, 7, 1, 1, 1, 8, 1, 1, 8, 1, 1, 1, 9,
            1, 9, 1, 2, 1, 1, 2, 1, 1, 10, 4, 1, 10, 1, 4, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 2, 1, 5, 1, 1, 1, 2, 5, 1, 11, 1, 11, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2,
            1, 6, 1, 6, 1, 1, 2, 1, 1, 1, 1, 1, 1, 1, 12, 3, 1, 12, 1, 1, 1, 2, 1, 1, 3, 1, 1, 1, 1, 1, 1, 4, 1, 1, 1, 2, 1, 1, 4, 1, 1, 1, 1, 1, 1, 2, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 2, 1, 1, 5, 1, 1, 1, 1, 1, 7, 1, 7, 1, 1, 2, 3, 1, 1, 1, 1, 5, 1, 1, 1, 1, 1, 1, 2, 13, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 13, 2, 1, 1, 4, 1, 1, 1, 3, 1, 6, 1, 1, 1, 14, 1, 1, 1, 1, 1, 14, 6, 1, 1, 1, 1, 15, 2, 4, 1, 2, 3, 15, 1, 1, 1, 8, 1, 1, 8, 1,
            1, 1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 1, 1,
        ],
        val: [
            0, 1, -1, 2, 1, -1, -2, 1, -1, 3, -3, 4, 1, -1, -4, 1, 2, 5, -1, -2, -5, 6, 1, -1, -6, 7, -7, 8, -8, 1, 2, 9, 3, -2, 0, -9, -3, -1, 10, -10, 11, 1,
            -11, 12, -1, -12, 13, -13, 1, 14, -1, -14, 4, 15, -15, -4, 16, -16, 1, 2, 17, -1, -17, -2, 18, -18, 19, -19, 20, 3, -20, 21, -21, -3, 5, 22, 2,
            -22, -23, 23, -5, -2, 24, 1, -24, -1, 25, -25, 26, -26, -27, 27, 28, 29, -28, -29, 6, 30, 2, -31, -2, -30, 31, -6, -32, 32, 33, -33, 34, -35, -34,
            1, 4, -36, -1, 35, 37, 36, 7, -37, 38, -4, -38, 39, 41, 40, -40, -39, 3, 42, -43, -41, -7, -42, 43, -3, 44, -44, 45, -45, 46, 47, 8, -47, -48, -46,
            50, -50, 48, 49, 51, -49, 52, -52, 5, -51, -8, -53, 53, 3, -56, 56, 55, 54, -54, 2, 60, -2, -55, 58, 9, -5, 59, 57, -57, -63, -3, -58, -60, -61,
            61, -59, -62, -9, 1, 64, 62, 69, -64, 63, 65, -67, -68, 66, -65, 68, -66, -69, 67, -70, -1, 10, 71, -71, 4, 73, 72, 70, 6, -76, -3, 74, -78, -74,
            1, 78, 80, -72, -75, 76, -1, 3, -73, 79, 75, 77, 1, 11, -4, -79, -10, -6, -1, -77, -83, -80, 2, 81, -84, -2, 83, -81, 82, -82, 84, -87, -86, 85,
            -11, -85, 86, -89, 87, -88, 88, 89,
        ],
    },
    RvMap {
        eob: 2,
        esc: 33,
        run: [
            1, 1, 0, 2, 1, 2, 1, 3, 3, 1, 1, 4, 4, 2, 2, 1, 1, 5, 5, 6, 1, 6, 1, 7, 7, 3, 3, 2, 8, 2, 8, 1, 1, 0, 9, 9, 1, 1, 10, 4, 10, 4, 11, 11, 2, 1, 2, 1,
            12, 12, 3, 3, 1, 1, 13, 5, 5, 13, 14, 1, 1, 14, 2, 2, 6, 6, 15, 1, 1, 15, 16, 4, 7, 16, 4, 7, 1, 1, 3, 3, 8, 8, 2, 2, 1, 1, 17, 17, 1, 1, 18, 18,
            5, 5, 2, 2, 1, 1, 9, 19, 9, 19, 20, 3, 3, 20, 1, 10, 21, 1, 10, 4, 4, 21, 22, 6, 6, 22, 1, 1, 23, 24, 2, 2, 23, 24, 11, 1, 1, 11, 7, 25, 7, 1, 1,
            25, 8, 8, 3, 26, 3, 1, 12, 2, 2, 26, 1, 12, 5, 5, 27, 4, 1, 4, 1, 27, 28, 1, 28, 13, 1, 13, 2, 29, 2, 1, 32, 6, 1, 30, 14, 29, 14, 6, 3, 31, 3, 1,
            30, 1, 32, 31, 33, 9, 33, 1, 1, 7, 9, 7, 2, 2, 1, 1, 4, 36, 34, 4, 5, 10, 10, 5, 34, 1, 1, 35, 8, 8, 36, 3, 35, 1, 15, 3, 2, 1, 16, 15, 16, 2, 37,
            1, 37, 1, 1, 1, 6, 6, 38, 1, 38, 11, 1, 39, 39, 40, 11, 2, 41, 4, 40, 1, 2, 4, 1, 1, 1, 41, 3, 1, 3, 1, 5, 7, 5, 7,
        ],
        val: [
            1, -1, 0, 1, 2, -1, -2, 1, -1, 3, -3, 1, -1, 2, -2, 4, -4, 1, -1, 1, 5, -1, -5, 1, -1, 2, -2, 3, 1, -3, -1, 6, -6, 0, 1, -1, 7, -7, 1, 2, -1, -2,
            1, -1, 4, 8, -4, -8, 1, -1, 3, -3, 9, -9, 1, 2, -2, -1, 1, 10, -10, -1, 5, -5, 2, -2, 1, 11, -11, -1, 1, 3, 2, -1, -3, -2, 12, -12, 4, -4, 2, -2,
            -6, 6, 13, -13, 1, -1, 14, -14, 1, -1, 3, -3, 7, -7, 15, -15, 2, 1, -2, -1, 1, 5, -5, -1, -16, 2, 1, 16, -2, 4, -4, -1, 1, 3, -3, -1, 17, -17, 1,
            1, -8, 8, -1, -1, 2, 18, -18, -2, 3, 1, -3, 19, -19, -1, 3, -3, 6, 1, -6, 20, 2, 9, -9, -1, -20, -2, 4, -4, 1, -5, 21, 5, -21, -1, 1, -22, -1, 2,
            22, -2, 10, 1, -10, 23, 1, 4, -23, 1, 2, -1, -2, -4, -7, 1, 7, -24, -1, 24, -1, -1, 1, 3, -1, -25, 25, 4, -3, -4, 11, -11, 26, -26, 6, 1, 1, -6,
            -5, -3, 3, 5, -1, -27, 27, 1, 4, -4, -1, -8, -1, 28, 2, 8, -12, -28, -2, -2, 2, 12, -1, 29, 1, -29, 30, -30, 5, -5, 1, -31, -1, 3, 31, -1, 1, 1,
            -3, -13, 1, -7, -1, -32, 13, 7, 32, 33, -33, -1, -9, -34, 9, 34, -6, 5, 6, -5,
        ],
    },
    RvMap {
        eob: 2,
        esc: 13,
        run: [
            1, 1, 0, 1, 1, 2, 2, 1, 1, 3, 3, 1, 1, 0, 2, 2, 4, 1, 4, 1, 1, 1, 5, 5, 1, 1, 6, 6, 2, 2, 1, 1, 3, 3, 7, 7, 1, 1, 8, 8, 1, 1, 2, 2, 1, 9, 1, 9, 4,
            4, 10, 1, 1, 10, 1, 1, 11, 11, 3, 3, 1, 2, 1, 2, 1, 1, 12, 12, 5, 5, 1, 1, 13, 1, 1, 13, 2, 2, 1, 1, 6, 6, 1, 1, 4, 14, 4, 14, 3, 1, 3, 1, 1, 1,
            15, 7, 15, 2, 2, 7, 1, 1, 1, 8, 1, 8, 16, 16, 1, 1, 1, 1, 2, 1, 1, 2, 1, 1, 3, 5, 5, 3, 4, 1, 1, 4, 1, 1, 17, 17, 9, 1, 1, 9, 2, 2, 1, 1, 10, 10,
            1, 6, 1, 1, 6, 18, 1, 1, 18, 1, 1, 1, 2, 2, 3, 1, 3, 1, 1, 1, 4, 1, 19, 1, 19, 7, 1, 1, 20, 1, 4, 20, 1, 7, 11, 2, 1, 11, 21, 2, 8, 5, 1, 8, 1, 5,
            21, 1, 1, 1, 22, 1, 1, 22, 1, 1, 3, 3, 1, 23, 2, 12, 24, 1, 1, 2, 1, 1, 12, 23, 1, 1, 24, 1, 1, 1, 4, 1, 1, 1, 2, 1, 6, 6, 4, 2, 1, 1, 1, 1, 1, 1,
            1, 14, 13, 3, 1, 25, 9, 25, 14, 1, 9, 3, 13, 1, 1, 1, 1, 1, 10, 1, 1, 2, 10, 2,
        ],
        val: [
            -20, -1, 0, 2, -2, 1, -1, 3, -3, 1, -1, 4, -4, 0, 2, -2, 1, 5, -1, -5, 6, -6, 1, -1, 7, -7, 1, -1, 3, -3, 8, -8, 2, -2, 1, -1, 9, -9, 1, -1, 10,
            -10, 4, -4, 11, 1, -11, -1, 2, -2, 1, 12, -12, -1, 13, -13, 1, -1, 3, -3, 14, 5, -14, -5, -15, 15, -1, 1, 2, -2, 16, -16, 1, 17, -17, -1, 6, -6,
            18, -18, 2, -2, -19, 19, -3, 1, 3, -1, 4, 20, -4, 1, -21, 21, 1, 2, -1, -7, 7, -2, 22, -22, 23, 2, -23, -2, 1, -1, -24, 24, -25, 25, -8, -26, 26,
            8, -27, 27, 5, 3, -3, -5, -4, 28, -28, 4, 29, -29, 1, -1, -2, -30, 30, 2, 9, -9, -31, 31, 2, -2, -32, 3, 32, -33, -3, 1, 33, -34, -1, 34, -35, 35,
            -10, 10, -6, 36, 6, -36, 37, -37, -5, 38, 1, -38, -1, 3, 39, -39, -1, 40, 5, 1, -40, -3, 2, -11, -41, -2, 1, 11, -3, -4, 41, 3, 42, 4, -1, -43,
            -42, 43, 1, -44, 45, -1, 44, -45, -7, 7, -46, 1, -12, 2, 1, -47, 46, 12, 47, 48, -2, -1, -48, 49, -1, -50, -49, 50, -6, -51, 51, 52, -13, 53, -4,
            4, 6, 13, -53, -52, -54, 55, 54, -55, -56, -2, 2, -8, 56, 1, -3, -1, 2, 58, 3, 8, -2, 57, -58, -60, -59, -57, -3, 60, 59, -14, 3, 14,
        ],
    },
    RvMap {
        eob: 2,
        esc: 38,
        run: [
            1, 1, 0, 2, 2, 1, 1, 3, 3, 4, 4, 5, 5, 1, 1, 6, 6, 2, 2, 7, 7, 8, 8, 1, 1, 3, 3, 9, 9, 10, 10, 1, 1, 2, 2, 4, 4, 11, 0, 11, 12, 12, 13, 13, 1, 1,
            5, 5, 14, 14, 15, 16, 15, 16, 3, 3, 1, 6, 1, 6, 2, 2, 7, 7, 8, 8, 17, 17, 1, 1, 4, 4, 18, 18, 2, 2, 1, 19, 1, 20, 19, 20, 21, 21, 3, 3, 22, 22, 5,
            5, 24, 1, 1, 23, 9, 23, 24, 9, 2, 2, 10, 1, 1, 10, 6, 6, 25, 4, 4, 25, 7, 7, 26, 8, 1, 8, 3, 1, 26, 3, 11, 11, 27, 27, 2, 28, 1, 2, 28, 1, 12, 12,
            5, 5, 29, 13, 13, 29, 32, 1, 1, 33, 31, 30, 32, 4, 30, 33, 4, 31, 3, 14, 1, 1, 3, 34, 34, 2, 2, 14, 6, 6, 35, 36, 35, 36, 1, 15, 1, 16, 16, 15, 7,
            9, 7, 9, 37, 8, 8, 37, 1, 1, 39, 2, 38, 39, 2, 40, 5, 38, 40, 5, 3, 3, 4, 4, 10, 10, 1, 1, 1, 1, 41, 2, 41, 2, 6, 6, 1, 1, 11, 42, 11, 43, 3, 42,
            3, 17, 4, 43, 1, 17, 7, 1, 8, 44, 4, 7, 44, 5, 8, 2, 5, 1, 2, 48, 45, 1, 12, 45, 12, 48, 13, 13, 1, 9, 9, 46, 1, 46, 47, 47, 49, 18, 18, 49,
        ],
        val: [
            1, -1, 0, 1, -1, 2, -2, 1, -1, 1, -1, 1, -1, 3, -3, 1, -1, -2, 2, 1, -1, 1, -1, 4, -4, -2, 2, 1, -1, 1, -1, 5, -5, -3, 3, 2, -2, 1, 0, -1, 1, -1,
            1, -1, 6, -6, 2, -2, 1, -1, 1, 1, -1, -1, -3, 3, 7, 2, -7, -2, -4, 4, 2, -2, 2, -2, 1, -1, 8, -8, 3, -3, 1, -1, -5, 5, 9, 1, -9, 1, -1, -1, 1, -1,
            -4, 4, 1, -1, 3, -3, 1, -10, 10, 1, 2, -1, -1, -2, 6, -6, 2, 11, -11, -2, 3, -3, 1, -4, 4, -1, 3, -3, 1, 3, 12, -3, -5, -12, -1, 5, 2, -2, 1, -1,
            -7, 1, 13, 7, -1, -13, 2, -2, 4, -4, 1, 2, -2, -1, 1, 14, -14, 1, 1, 1, -1, -5, -1, -1, 5, -1, -6, 2, -15, 15, 6, 1, -1, -8, 8, -2, -4, 4, 1, 1,
            -1, -1, 16, 2, -16, -2, 2, -2, 4, 3, -4, -3, -1, -4, 4, 1, -17, 17, -1, -9, 1, 1, 9, 1, -5, -1, -1, 5, -7, 7, 6, -6, 3, -3, 18, -18, 19, -19, 1,
            -10, -1, 10, -5, 5, 20, -20, -3, 1, 3, 1, 8, -1, -8, 2, 7, -1, -21, -2, 5, 21, 5, -1, -7, -5, 1, -6, -5, -11, 6, 22, 11, 1, 1, -22, -3, -1, 3, -1,
            3, -3, -23, 4, -4, 1, 23, -1, 1, -1, 1, -2, 2, -1,
        ],
    },
    RvMap {
        eob: 4,
        esc: 11,
        run: [
            1, 1, 1, 1, 0, 2, 2, 1, 1, 3, 3, 0, 1, 1, 2, 2, 4, 4, 1, 1, 5, 5, 1, 1, 2, 2, 3, 3, 6, 6, 1, 1, 7, 7, 8, 1, 8, 2, 2, 1, 4, 4, 1, 3, 1, 3, 9, 9, 2,
            2, 1, 5, 1, 5, 10, 10, 1, 1, 11, 11, 3, 6, 3, 4, 4, 6, 2, 2, 1, 12, 1, 12, 7, 13, 7, 13, 1, 1, 8, 8, 2, 2, 14, 14, 16, 15, 16, 5, 5, 1, 3, 15, 1,
            3, 4, 4, 1, 1, 17, 17, 2, 2, 6, 6, 1, 18, 1, 18, 22, 21, 22, 21, 25, 24, 25, 19, 9, 20, 9, 23, 19, 24, 20, 3, 23, 7, 3, 1, 1, 7, 28, 26, 29, 5, 28,
            26, 5, 8, 29, 4, 8, 27, 2, 2, 4, 27, 1, 1, 10, 36, 10, 33, 33, 36, 30, 1, 32, 32, 1, 30, 6, 31, 31, 35, 3, 6, 11, 11, 3, 2, 35, 2, 34, 1, 34, 1,
            37, 37, 12, 7, 12, 5, 41, 5, 4, 7, 1, 8, 13, 4, 1, 41, 13, 38, 8, 38, 9, 1, 40, 40, 9, 1, 39, 2, 2, 49, 39, 42, 3, 3, 14, 16, 49, 14, 16, 42, 43,
            43, 6, 6, 15, 1, 1, 15, 44, 44, 1, 1, 50, 48, 4, 5, 4, 7, 5, 2, 10, 10, 48, 7, 50, 45, 2, 1, 45, 8, 8, 1, 46, 46, 3, 47, 47, 3, 1, 1,
        ],
        val: [
            1, -1, 2, -2, 0, 1, -1, 3, -3, 1, -1, 0, 4, -4, 2, -2, 1, -1, 5, -5, 1, -1, 6, -6, 3, -3, 2, -2, 1, -1, 7, -7, 1, -1, 1, 8, -1, 4, -4, -8, 2, -2,
            9, 3, -9, -3, 1, -1, 5, -5, 10, 2, -10, -2, 1, -1, 11, -11, 1, -1, -4, 2, 4, 3, -3, -2, 6, -6, 12, 1, -12, -1, 2, 1, -2, -1, 13, -13, 2, -2, 7, -7,
            1, -1, 1, 1, -1, 3, -3, 14, 5, -1, -14, -5, 4, -4, 15, -15, 1, -1, 8, -8, -3, 3, 16, 1, -16, -1, 1, 1, -1, -1, 1, 1, -1, 1, 2, 1, -2, 1, -1, -1,
            -1, 6, -1, 3, -6, 17, -17, -3, 1, 1, 1, 4, -1, -1, -4, 3, -1, 5, -3, -1, -9, 9, -5, 1, 18, -18, 2, 1, -2, 1, -1, -1, 1, 19, -1, 1, -19, -1, 4, 1,
            -1, 1, 7, -4, -2, 2, -7, 10, -1, -10, 1, 20, -1, -20, 1, -1, 2, 4, -2, 5, 1, -5, 6, -4, 21, 4, 2, -6, -21, -1, -2, 1, -4, -1, -3, 22, -1, 1, 3,
            -22, -1, 11, -11, 1, 1, 1, 8, -8, 2, 2, -1, -2, -2, -1, 1, -1, -5, 5, 2, 23, -23, -2, 1, -1, 24, -24, -1, -1, 7, 6, -7, 5, -6, 12, -3, 3, 1, -5, 1,
            1, -12, 25, -1, -5, 5, -25, -1, 1, 9, 1, -1, -9, 26, -26,
        ],
    },
];

/// Block Huffman descriptors: `xbits` of each row (the row count is the length).
const BLOCK_HUFFMAN: [&[u8]; 8] = [
    &[1, 2, 3, 4, 4, 7, 5, 5, 4, 1],
    &[2, 3, 4, 4, 4, 7, 5, 4, 3, 3, 2],
    &[2, 4, 5, 5, 5, 5, 6, 4, 4, 3, 1, 1],
    &[3, 3, 4, 4, 5, 6, 6, 4, 4, 3, 2, 1, 1],
    &[3, 4, 4, 5, 5, 5, 6, 5, 4, 2, 2],
    &[3, 4, 5, 5, 5, 5, 6, 4, 3, 3, 2, 1, 1],
    &[3, 4, 5, 5, 5, 6, 5, 4, 3, 3, 2, 1, 1],
    &[3, 4, 4, 5, 5, 5, 6, 5, 5],
];

/// Picture sizes by code, in units of four pixels (width, height).
const PICTURE_SIZES: [(usize, usize); 12] =
    [(160, 120), (80, 60), (40, 30), (176, 56), (88, 60), (88, 72), (44, 36), (60, 45), (160, 60), (176, 60), (20, 15), (22, 18)];

const ZIGZAG8: [u8; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29,
    22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];
const SCAN4: [u8; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];

/// Intra base quantiser matrices and scale rows of the default quantiser set (8x8 luma, 4x4 chroma).
const BASE8: [u16; 64] = [
    26, 46, 54, 66, 70, 74, 78, 90, 46, 50, 62, 66, 70, 78, 86, 106, 54, 62, 62, 68, 74, 84, 102, 114, 66, 66, 68, 74, 82, 98, 108, 122, 70, 70, 74, 82, 94,
    102, 114, 142, 74, 78, 84, 98, 102, 110, 134, 166, 78, 86, 102, 108, 114, 134, 154, 202, 90, 106, 114, 122, 142, 166, 202, 254,
];
const SCALE8: [u16; 24] = [11, 14, 16, 18, 20, 22, 23, 24, 26, 28, 30, 32, 34, 36, 39, 40, 42, 45, 47, 49, 52, 55, 57, 60];
const BASE4: [u16; 16] = [30, 62, 74, 82, 62, 74, 82, 94, 74, 82, 94, 122, 82, 94, 122, 146];
const SCALE4: [u16; 24] = [1, 11, 11, 13, 13, 13, 14, 15, 16, 17, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32];

fn err<T>(what: &str) -> Result<T> {
    Err(Error::Invalid(format!("indeo5: {what}")))
}

/// Bits are read least significant bit first.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn read(&mut self, n: u32) -> Result<u32> {
        if self.pos + n as usize > self.data.len() * 8 {
            return err("truncated frame");
        }
        let mut v = 0u32;
        for i in 0..n {
            let p = self.pos + i as usize;
            v |= ((self.data[p >> 3] >> (p & 7)) as u32 & 1) << i;
        }
        self.pos += n as usize;
        Ok(v)
    }

    fn flag(&mut self) -> Result<bool> {
        Ok(self.read(1)? == 1)
    }

    fn align(&mut self) {
        self.pos = (self.pos + 7) & !7;
    }

    fn peek(&self, n: u32) -> u32 {
        let mut v = 0u32;
        for i in 0..n {
            let p = self.pos + i as usize;
            if p >= self.data.len() * 8 {
                break;
            }
            v |= ((self.data[p >> 3] >> (p & 7)) as u32 & 1) << i;
        }
        v
    }
}

/// Prefix-code lookup indexed by the next `bits` bits (first bit read = lowest bit).
struct Huffman {
    bits: u32,
    lut: Vec<(u8, u8)>,
}

impl Huffman {
    /// Row `i` of the descriptor has `2^xbits[i]` codes: `i` ones, a zero (except in the last row),
    /// then `xbits[i]` suffix bits; symbols are numbered in order.
    fn from_descriptor(xbits: &[u8]) -> Result<Huffman> {
        let mut codes: Vec<(u32, u32)> = Vec::new(); // (length, codeword read most significant bit first)
        'rows: for (i, &x) in xbits.iter().enumerate() {
            let not_last = (i != xbits.len() - 1) as u32;
            let prefix = ((1u32 << i) - 1) << (x as u32 + not_last);
            for j in 0..1u32 << x {
                if codes.len() >= 256 {
                    break 'rows;
                }
                let len = (i as u32 + x as u32 + not_last).max(1);
                if len > 16 {
                    return err("huffman code too long");
                }
                codes.push((len, prefix | j));
            }
        }
        let bits = codes.iter().map(|c| c.0).max().unwrap_or(1);
        let mut lut = vec![(0u8, 0u8); 1 << bits];
        for (sym, &(len, cw)) in codes.iter().enumerate() {
            let rev = (0..len).fold(0u32, |a, b| a | ((cw >> (len - 1 - b)) & 1) << b);
            for hi in 0..1u32 << (bits - len) {
                lut[(rev | hi << len) as usize] = (len as u8, sym as u8);
            }
        }
        Ok(Huffman { bits, lut })
    }

    fn read(&self, r: &mut Bits) -> Result<usize> {
        let (len, sym) = self.lut[r.peek(self.bits) as usize];
        if len == 0 {
            return err("invalid huffman code");
        }
        r.pos += len as usize;
        if r.pos > r.data.len() * 8 {
            return err("truncated frame");
        }
        Ok(sym as usize)
    }
}

struct Band {
    run: [u8; 256],
    val: [i8; 256],
    eob: usize,
    esc: usize,
    huffman: Huffman,
    quant: usize,
}

fn read_huffman_descriptor(r: &mut Bits) -> Result<Vec<u8>> {
    let sel = r.read(3)?;
    if sel == 7 {
        let rows = r.read(4)?;
        (0..rows).map(|_| r.read(4).map(|v| v as u8)).collect()
    } else {
        Ok(BLOCK_HUFFMAN[sel as usize].to_vec())
    }
}

fn read_band_header(r: &mut Bits, frame_flags: u32) -> Result<Band> {
    let flags = r.read(8)?;
    if frame_flags & 0x80 != 0 {
        r.read(24)?; // band data size: the tile size gives the same position
    }
    if flags & 1 != 0 {
        return err("empty band");
    }
    if flags & 0xc != 0 {
        return err("quantiser or macroblock inheritance in an intra band");
    }
    let mut corr = Vec::new();
    if flags & 0x10 != 0 {
        let n = r.read(8)?;
        for _ in 0..n * 2 {
            corr.push(r.read(8)? as usize);
        }
    }
    let rv = if flags & 0x40 != 0 { r.read(3)? as usize } else { 8 };
    let xbits = if flags & 0x80 != 0 { read_huffman_descriptor(r)? } else { BLOCK_HUFFMAN[7].to_vec() };
    if r.flag()? {
        r.read(16)?;
    }
    let quant = r.read(5)? as usize;
    r.align();
    if flags & 0x20 != 0 {
        while r.flag()? {
            r.read(8)?;
        }
        r.align();
    }
    let map = RVMAPS.get(rv).ok_or_else(|| Error::Invalid("indeo5: bad run/value table".into()))?;
    let (mut run, mut val) = (map.run, map.val);
    // corrections swap pairs of table entries
    for pair in corr.chunks_exact(2) {
        if pair[0] > 255 || pair[1] > 255 {
            return err("bad table correction");
        }
        run.swap(pair[0], pair[1]);
        val.swap(pair[0], pair[1]);
    }
    if quant >= 24 {
        return err("quantiser out of range");
    }
    Ok(Band { run, val, eob: map.eob as usize, esc: map.esc as usize, huffman: Huffman::from_descriptor(&xbits)?, quant })
}

fn reflect(s1: i32, s2: i32) -> (i32, i32) {
    (((s1 + s2 * 2 + 2) >> 2) + s1, ((s1 * 2 - s2 + 2) >> 2) - s2)
}

fn inv_slant8(s: [i32; 8]) -> [i32; 8] {
    let [s1, s4, s8, s5, s2, s6, s3, s7] = s;
    let t4 = s5 + ((s4 * 4 - s5 + 4) >> 3);
    let t5 = s4 + ((-s4 - s5 * 4 + 4) >> 3);
    let (t1, t5) = (s1 + t5, s1 - t5);
    let (t2, t6) = (s2 + s6, s2 - s6);
    let (t7, t3) = (s7 + s3, s7 - s3);
    let (t4, t8) = (t4 + s8, t4 - s8);
    let (t1, t2) = (t1 + t2, t1 - t2);
    let (t4, t3) = reflect(t4, t3);
    let (t5, t6) = (t5 + t6, t5 - t6);
    let (t8, t7) = reflect(t8, t7);
    let (t1, t4) = (t1 + t4, t1 - t4);
    let (t2, t3) = (t2 + t3, t2 - t3);
    let (t5, t8) = (t5 + t8, t5 - t8);
    let (t6, t7) = (t6 + t7, t6 - t7);
    [t1, t2, t3, t4, t5, t6, t7, t8]
}

fn inv_slant4(s: [i32; 4]) -> [i32; 4] {
    let [s1, s4, s2, s3] = s;
    let (t1, t2) = (s1 + s2, s1 - s2);
    let (t4, t3) = reflect(s4, s3);
    [t1 + t4, t2 + t3, t2 - t3, t1 - t4]
}

/// 2-D inverse slant: columns first, then rows with the final rounding shift.
fn slant_2d<const N: usize>(c: &[i32], inv: fn([i32; N]) -> [i32; N]) -> Vec<i32>
where
    [i32; N]: Default,
{
    let mut tmp = vec![0i32; N * N];
    for x in 0..N {
        let mut col = <[i32; N]>::default();
        for (y, v) in col.iter_mut().enumerate() {
            *v = c[y * N + x];
        }
        for (y, v) in inv(col).into_iter().enumerate() {
            tmp[y * N + x] = v;
        }
    }
    let mut out = vec![0i32; N * N];
    for y in 0..N {
        let mut row = <[i32; N]>::default();
        row.copy_from_slice(&tmp[y * N..(y + 1) * N]);
        for (x, v) in inv(row).into_iter().enumerate() {
            out[y * N + x] = (v + 1) >> 1;
        }
    }
    out
}

/// Dequantisation: with `a = base * scale >> 9` the coefficient is `v*a + a/2 - (a & 1)` (and plain `v` for a <= 1).
fn dequant(v: i32, a: i32) -> i32 {
    if v == 0 {
        return 0;
    }
    let m = v.abs();
    let c = if a <= 1 { m } else { m * a + (a >> 1) - (a & 1) };
    if v > 0 {
        c
    } else {
        -c
    }
}

fn decode_plane(r: &mut Bits, band: &Band, width: usize, height: usize, mb: usize, blk: usize, out: &mut Vec<u8>) -> Result<()> {
    if r.flag()? {
        return err("empty tile in an intra frame");
    }
    if r.flag()? {
        let n = r.read(8)?;
        if n == 255 {
            r.read(24)?;
        }
    }
    r.align();
    let (mbx, mby) = (width.div_ceil(mb), height.div_ceil(mb));
    let per_side = mb / blk;
    let blocks = per_side * per_side;
    let mut cbp = Vec::with_capacity(mbx * mby);
    for _ in 0..mbx * mby {
        if r.flag()? {
            return err("skipped macroblock in an intra frame");
        }
        cbp.push(r.read(blocks as u32)?);
    }
    let (pw, ph) = (mbx * mb, mby * mb);
    out.clear();
    out.resize(pw * ph, 128);
    let (base, scale, scan): (&[u16], &[u16], &[u8]) = if blk == 8 { (&BASE8, &SCALE8, &ZIGZAG8) } else { (&BASE4, &SCALE4, &SCAN4) };
    let coeffs = blk * blk;
    let a_of = |pos: usize| (((base[pos] as u32 * scale[band.quant] as u32) >> 9) as i32).max(1);
    let a_dc = a_of(0);
    let mut dc = 0i32;
    let mut c = vec![0i32; coeffs];
    for (k, &mask) in cbp.iter().enumerate() {
        for i in 0..blocks {
            c.iter_mut().for_each(|v| *v = 0);
            if mask >> i & 1 == 1 {
                let mut scan_pos: i32 = -1;
                loop {
                    let sym = band.huffman.read(r)?;
                    if sym == band.eob {
                        break;
                    }
                    let (run, val) = if sym == band.esc {
                        let run = band.huffman.read(r)? as i32 + 1;
                        let lo = band.huffman.read(r)? as i32;
                        let hi = band.huffman.read(r)? as i32;
                        let v = hi << 6 | lo;
                        (run, -((v >> 1) ^ -(v & 1)))
                    } else {
                        if band.val[sym] == 0 {
                            return err("unused run/value symbol");
                        }
                        (band.run[sym] as i32, band.val[sym] as i32)
                    };
                    scan_pos += run;
                    if scan_pos < 0 || scan_pos as usize >= coeffs {
                        return err("coefficient position past the block");
                    }
                    let pos = scan[scan_pos as usize] as usize;
                    if pos == 0 {
                        dc += dequant(val, a_dc);
                    } else {
                        c[pos] = dequant(val, a_of(pos));
                    }
                }
            }
            c[0] = dc;
            let px = if blk == 8 { slant_2d::<8>(&c, inv_slant8) } else { slant_2d::<4>(&c, inv_slant4) };
            let (bx, by) = ((k % mbx) * mb + (i % per_side) * blk, (k / mbx) * mb + (i / per_side) * blk);
            for y in 0..blk {
                for x in 0..blk {
                    out[(by + y) * pw + bx + x] = (128 + px[y * blk + x]).clamp(0, 255) as u8;
                }
            }
        }
    }
    // crop the macroblock padding
    if pw != width || ph != height {
        let mut cropped = Vec::with_capacity(width * height);
        for y in 0..height {
            cropped.extend_from_slice(&out[y * pw..y * pw + width]);
        }
        *out = cropped;
    }
    Ok(())
}

/// Decode one intra frame (a chunk of the AVI video stream).
pub fn decode(data: &[u8]) -> Result<Frame> {
    let mut r = Bits { data, pos: 0 };
    if r.read(5)? != 0x1f {
        return err("bad picture start code");
    }
    let frame_type = r.read(3)?;
    if frame_type != 0 {
        return err("only intra frames are supported");
    }
    r.read(8)?; // frame number
    let gop_start = r.pos;
    let gop_flags = r.read(8)?;
    let header_size = if gop_flags & 1 != 0 { Some(r.read(16)? as usize) } else { None };
    if gop_flags & 0x20 != 0 {
        r.read(32)?; // lock word
    }
    if gop_flags & 0x40 != 0 || gop_flags & 8 != 0 {
        return err("slices and transparency are not supported");
    }
    if r.read(2)? != 0 || r.read(1)? != 0 {
        return err("wavelet-decomposed pictures are not supported");
    }
    let size = r.read(4)? as usize;
    let (width, height) = if size == 15 {
        let h = r.read(13)? as usize;
        let w = r.read(13)? as usize;
        (w, h)
    } else {
        let (w, h) = *PICTURE_SIZES.get(size).ok_or_else(|| Error::Invalid("indeo5: bad picture size".into()))?;
        (w * 4, h * 4)
    };
    if width == 0 || height == 0 {
        return err("empty picture");
    }
    let mut sizes = [(0usize, 0usize); 2]; // (block size, macroblock size) of luma and chroma
    for s in &mut sizes {
        r.flag()?; // half-pel motion vectors (unused here)
        let mb_single = r.flag()?;
        let blk = if r.flag()? { 4 } else { 8 };
        if r.flag()? {
            return err("custom transforms are not supported");
        }
        r.read(2)?;
        *s = (blk, if mb_single { blk } else { blk * 2 });
    }
    if sizes[0].0 != 8 || sizes[1].0 != 4 {
        return err("unsupported block sizes");
    }
    if let Some(n) = header_size {
        r.pos = gop_start + n * 8;
    } else {
        r.align();
        r.read(23)?;
        if r.flag()? {
            return err("gop extension");
        }
        r.align();
    }
    let frame_flags = r.read(8)?;
    if frame_flags & 1 != 0 {
        r.read(24)?;
    }
    if frame_flags & 0x10 != 0 {
        r.read(16)?;
    }
    if frame_flags & 0x20 != 0 {
        return err("frame header extension");
    }
    if frame_flags & 0x40 != 0 {
        read_huffman_descriptor(&mut r)?; // macroblock codebook: unused without motion vectors
    }
    r.read(3)?;
    r.align();
    let (cw, ch) = (width.div_ceil(4), height.div_ceil(4));
    let mut frame = Frame { width, height, chroma_width: cw, chroma_height: ch, ..Default::default() };
    let layout = [(width, height, sizes[0]), (cw, ch, sizes[1]), (cw, ch, sizes[1])];
    for (p, (w, h, (blk, mb))) in layout.into_iter().enumerate() {
        let band = read_band_header(&mut r, frame_flags)?;
        let tile_start = r.pos / 8;
        let tile_size_pos = r.pos;
        let mut tile_len = 0usize;
        {
            // the tile size (header included) locates the next band
            let mut peek = Bits { data, pos: tile_size_pos };
            if !peek.flag()? && peek.flag()? {
                tile_len = peek.read(8)? as usize;
                if tile_len == 255 {
                    tile_len = peek.read(24)? as usize;
                }
            }
        }
        let target = match p {
            0 => &mut frame.y,
            1 => &mut frame.v,
            _ => &mut frame.u,
        };
        decode_plane(&mut r, &band, w, h, mb, blk, target)?;
        if tile_len != 0 {
            r.pos = (tile_start + tile_len) * 8;
        }
    }
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slant_networks_match_the_reference_decoder() {
        // a lone coefficient 3 at the fourth position of the first row: column pass spreads it, row pass
        // makes the +-2 square wave of the reference decoder
        let mut c = [0i32; 64];
        c[4] = 3;
        let out = slant_2d::<8>(&c, inv_slant8);
        assert_eq!(&out[..8], &[2, -1, -1, 2, 2, -1, -1, 2]);
        // DC only: every pixel is (dc + 1) >> 1
        let mut d = [0i32; 64];
        d[0] = -224;
        assert!(slant_2d::<8>(&d, inv_slant8).iter().all(|&v| v == -112));
        let mut e = [0i32; 16];
        e[0] = 7;
        assert!(slant_2d::<4>(&e, inv_slant4).iter().all(|&v| v == 4));
    }

    #[test]
    fn dequantisation_follows_the_measured_rule() {
        // (a, value) -> coefficient, from probing the reference decoder
        assert_eq!(dequant(5, 1), 5);
        assert_eq!(dequant(3, 2), 7); // 2v + 1
        assert_eq!(dequant(4, 3), 12); // 3v
        assert_eq!(dequant(-2, 4), -10); // 4v + 2
        assert_eq!(dequant(1, 5), 6);
        assert_eq!(dequant(0, 9), 0);
    }

    #[test]
    fn huffman_codes_read_lsb_first() {
        for desc in BLOCK_HUFFMAN {
            assert!(Huffman::from_descriptor(desc).is_ok());
        }
        // descriptor 7 = 3,4,4,5,5,5,6,5,5: row 0 has 8 codes of 4 bits (0 + 3 suffix bits)
        let h = Huffman::from_descriptor(BLOCK_HUFFMAN[7]).unwrap();
        // symbol 0 is the codeword 0000, symbol 7 is 0111 (read first bit first, i.e. bits 0,1,1,1)
        let mut r = Bits { data: &[0], pos: 0 };
        assert_eq!(h.read(&mut r).unwrap(), 0);
        let mut r = Bits { data: &[0b1110], pos: 0 };
        assert_eq!(h.read(&mut r).unwrap(), 7);
    }

    #[test]
    fn rejects_other_frames() {
        assert!(decode(&[]).is_err());
        assert!(decode(&[0x1f, 0, 0]).is_err());
        assert!(decode(&[0x3f, 0, 0, 0, 0]).is_err()); // type 1 = inter
    }
}
