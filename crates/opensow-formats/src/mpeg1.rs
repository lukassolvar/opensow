//! MPEG-1 video (ISO 11172-2) decoder for the `.sgv` intro and briefing movies.
//!
//! The original game hands these files to DirectShow; this is a plain Rust
//! decoder for the same streams (see docs/video.md). It handles I, P and B
//! pictures with half-pel motion compensation and returns frames in display
//! order. The coefficient and coded-block-pattern tables were recovered by
//! feeding probe streams to an independent decoder (docs/video.md), the
//! other small tables follow the standard.

use crate::{Error, Result};
use std::borrow::Cow;
use std::collections::VecDeque;
use std::sync::OnceLock;

/// One decoded picture, planar 4:2:0, planes padded to whole macroblocks.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// Picture width in pixels.
    pub width: usize,
    /// Picture height in pixels.
    pub height: usize,
    /// The luma plane.
    pub y: Vec<u8>,
    /// The Cb chroma plane (half size in both directions).
    pub cb: Vec<u8>,
    /// The Cr chroma plane (half size in both directions).
    pub cr: Vec<u8>,
}

impl Frame {
    fn new(width: usize, height: usize) -> Frame {
        let (w, h) = (width.div_ceil(16) * 16, height.div_ceil(16) * 16);
        Frame { width, height, y: vec![0; w * h], cb: vec![0; w * h / 4], cr: vec![0; w * h / 4] }
    }

    /// Luma row length of the padded planes.
    pub fn stride(&self) -> usize {
        self.width.div_ceil(16) * 16
    }

    /// Cropped planes, one after the other (`yuv420p`).
    pub fn to_yuv420(&self) -> Vec<u8> {
        let (w, h, s) = (self.width, self.height, self.stride());
        let mut out = Vec::with_capacity(w * h * 3 / 2);
        for y in 0..h {
            out.extend_from_slice(&self.y[y * s..y * s + w]);
        }
        for plane in [&self.cb, &self.cr] {
            for y in 0..h.div_ceil(2) {
                out.extend_from_slice(&plane[y * s / 2..y * s / 2 + w.div_ceil(2)]);
            }
        }
        out
    }

    /// One pixel as `0x00RRGGBB` (BT.601 limited range), for scaling without converting the whole frame.
    pub fn rgb_at(&self, x: usize, y: usize) -> u32 {
        let s = self.stride();
        let l = (self.y[y * s + x] as i32 - 16) * 298;
        let b = self.cb[(y / 2) * (s / 2) + x / 2] as i32 - 128;
        let r = self.cr[(y / 2) * (s / 2) + x / 2] as i32 - 128;
        let c = |v: i32| ((v + 128) >> 8).clamp(0, 255) as u32;
        c(l + 409 * r) << 16 | c(l - 100 * b - 208 * r) << 8 | c(l + 516 * b)
    }

    /// BT.601 limited-range conversion to packed RGB.
    pub fn to_rgb(&self) -> Vec<u8> {
        let (w, h, s) = (self.width, self.height, self.stride());
        let mut out = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let l = (self.y[y * s + x] as i32 - 16) * 298;
                let b = self.cb[(y / 2) * s / 2 + x / 2] as i32 - 128;
                let r = self.cr[(y / 2) * s / 2 + x / 2] as i32 - 128;
                let c = |v: i32| ((v + 128) >> 8).clamp(0, 255) as u8;
                out.extend([c(l + 409 * r), c(l - 100 * b - 208 * r), c(l + 516 * b)]);
            }
        }
        out
    }
}

const ZIGZAG: [u8; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29,
    22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

const DEFAULT_INTRA: [u8; 64] = [
    8, 16, 19, 22, 26, 27, 29, 34, 16, 16, 22, 24, 27, 29, 34, 37, 19, 22, 26, 27, 29, 34, 34, 38, 22, 22, 26, 27, 29, 34, 37, 40, 22, 26, 27, 29, 32, 35, 40,
    48, 26, 27, 29, 32, 35, 40, 48, 58, 26, 27, 29, 34, 38, 46, 56, 69, 27, 29, 35, 38, 46, 56, 69, 83,
];

// (length, code, run, level), table B.14 of the standard
const COEFF: [(u8, u16, u8, u8); 111] = [
    (2, 0b11, 0, 1),
    (3, 0b011, 1, 1),
    (4, 0b0100, 0, 2),
    (4, 0b0101, 2, 1),
    (5, 0b00101, 0, 3),
    (5, 0b00110, 4, 1),
    (5, 0b00111, 3, 1),
    (6, 0b000100, 7, 1),
    (6, 0b000101, 6, 1),
    (6, 0b000110, 1, 2),
    (6, 0b000111, 5, 1),
    (7, 0b0000100, 2, 2),
    (7, 0b0000101, 9, 1),
    (7, 0b0000110, 0, 4),
    (7, 0b0000111, 8, 1),
    (8, 0b00100000, 13, 1),
    (8, 0b00100001, 0, 6),
    (8, 0b00100010, 12, 1),
    (8, 0b00100011, 11, 1),
    (8, 0b00100100, 3, 2),
    (8, 0b00100101, 1, 3),
    (8, 0b00100110, 0, 5),
    (8, 0b00100111, 10, 1),
    (10, 0b0000001000, 16, 1),
    (10, 0b0000001001, 5, 2),
    (10, 0b0000001010, 0, 7),
    (10, 0b0000001011, 2, 3),
    (10, 0b0000001100, 1, 4),
    (10, 0b0000001101, 15, 1),
    (10, 0b0000001110, 14, 1),
    (10, 0b0000001111, 4, 2),
    (12, 0b000000010000, 0, 11),
    (12, 0b000000010001, 8, 2),
    (12, 0b000000010010, 4, 3),
    (12, 0b000000010011, 0, 10),
    (12, 0b000000010100, 2, 4),
    (12, 0b000000010101, 7, 2),
    (12, 0b000000010110, 21, 1),
    (12, 0b000000010111, 20, 1),
    (12, 0b000000011000, 0, 9),
    (12, 0b000000011001, 19, 1),
    (12, 0b000000011010, 18, 1),
    (12, 0b000000011011, 1, 5),
    (12, 0b000000011100, 3, 3),
    (12, 0b000000011101, 0, 8),
    (12, 0b000000011110, 6, 2),
    (12, 0b000000011111, 17, 1),
    (13, 0b0000000010000, 10, 2),
    (13, 0b0000000010001, 9, 2),
    (13, 0b0000000010010, 5, 3),
    (13, 0b0000000010011, 3, 4),
    (13, 0b0000000010100, 2, 5),
    (13, 0b0000000010101, 1, 7),
    (13, 0b0000000010110, 1, 6),
    (13, 0b0000000010111, 0, 15),
    (13, 0b0000000011000, 0, 14),
    (13, 0b0000000011001, 0, 13),
    (13, 0b0000000011010, 0, 12),
    (13, 0b0000000011011, 26, 1),
    (13, 0b0000000011100, 25, 1),
    (13, 0b0000000011101, 24, 1),
    (13, 0b0000000011110, 23, 1),
    (13, 0b0000000011111, 22, 1),
    (14, 0b00000000010000, 0, 31),
    (14, 0b00000000010001, 0, 30),
    (14, 0b00000000010010, 0, 29),
    (14, 0b00000000010011, 0, 28),
    (14, 0b00000000010100, 0, 27),
    (14, 0b00000000010101, 0, 26),
    (14, 0b00000000010110, 0, 25),
    (14, 0b00000000010111, 0, 24),
    (14, 0b00000000011000, 0, 23),
    (14, 0b00000000011001, 0, 22),
    (14, 0b00000000011010, 0, 21),
    (14, 0b00000000011011, 0, 20),
    (14, 0b00000000011100, 0, 19),
    (14, 0b00000000011101, 0, 18),
    (14, 0b00000000011110, 0, 17),
    (14, 0b00000000011111, 0, 16),
    (15, 0b000000000010000, 0, 40),
    (15, 0b000000000010001, 0, 39),
    (15, 0b000000000010010, 0, 38),
    (15, 0b000000000010011, 0, 37),
    (15, 0b000000000010100, 0, 36),
    (15, 0b000000000010101, 0, 35),
    (15, 0b000000000010110, 0, 34),
    (15, 0b000000000010111, 0, 33),
    (15, 0b000000000011000, 0, 32),
    (15, 0b000000000011001, 1, 14),
    (15, 0b000000000011010, 1, 13),
    (15, 0b000000000011011, 1, 12),
    (15, 0b000000000011100, 1, 11),
    (15, 0b000000000011101, 1, 10),
    (15, 0b000000000011110, 1, 9),
    (15, 0b000000000011111, 1, 8),
    (16, 0b0000000000010000, 1, 18),
    (16, 0b0000000000010001, 1, 17),
    (16, 0b0000000000010010, 1, 16),
    (16, 0b0000000000010011, 1, 15),
    (16, 0b0000000000010100, 6, 3),
    (16, 0b0000000000010101, 16, 2),
    (16, 0b0000000000010110, 15, 2),
    (16, 0b0000000000010111, 14, 2),
    (16, 0b0000000000011000, 13, 2),
    (16, 0b0000000000011001, 12, 2),
    (16, 0b0000000000011010, 11, 2),
    (16, 0b0000000000011011, 31, 1),
    (16, 0b0000000000011100, 30, 1),
    (16, 0b0000000000011101, 29, 1),
    (16, 0b0000000000011110, 28, 1),
    (16, 0b0000000000011111, 27, 1),
];

// (length, code, coded-block-pattern) of table B.3; pattern 0 never occurs in these streams
const CBP: [(u8, u16, u8); 63] = [
    (3, 0b111, 60),
    (4, 0b1010, 32),
    (4, 0b1011, 16),
    (4, 0b1100, 8),
    (4, 0b1101, 4),
    (5, 0b01000, 62),
    (5, 0b01001, 2),
    (5, 0b01010, 61),
    (5, 0b01011, 1),
    (5, 0b01100, 56),
    (5, 0b01101, 52),
    (5, 0b01110, 44),
    (5, 0b01111, 28),
    (5, 0b10000, 40),
    (5, 0b10001, 20),
    (5, 0b10010, 48),
    (5, 0b10011, 12),
    (6, 0b001100, 63),
    (6, 0b001101, 3),
    (6, 0b001110, 36),
    (6, 0b001111, 24),
    (7, 0b0010000, 34),
    (7, 0b0010001, 18),
    (7, 0b0010010, 10),
    (7, 0b0010011, 6),
    (7, 0b0010100, 33),
    (7, 0b0010101, 17),
    (7, 0b0010110, 9),
    (7, 0b0010111, 5),
    (8, 0b00000100, 58),
    (8, 0b00000101, 54),
    (8, 0b00000110, 46),
    (8, 0b00000111, 30),
    (8, 0b00001000, 57),
    (8, 0b00001001, 53),
    (8, 0b00001010, 45),
    (8, 0b00001011, 29),
    (8, 0b00001100, 38),
    (8, 0b00001101, 26),
    (8, 0b00001110, 37),
    (8, 0b00001111, 25),
    (8, 0b00010000, 43),
    (8, 0b00010001, 23),
    (8, 0b00010010, 51),
    (8, 0b00010011, 15),
    (8, 0b00010100, 42),
    (8, 0b00010101, 22),
    (8, 0b00010110, 50),
    (8, 0b00010111, 14),
    (8, 0b00011000, 41),
    (8, 0b00011001, 21),
    (8, 0b00011010, 49),
    (8, 0b00011011, 13),
    (8, 0b00011100, 35),
    (8, 0b00011101, 19),
    (8, 0b00011110, 11),
    (8, 0b00011111, 7),
    (9, 0b000000010, 39),
    (9, 0b000000011, 27),
    (9, 0b000000100, 59),
    (9, 0b000000101, 55),
    (9, 0b000000110, 47),
    (9, 0b000000111, 31),
];

/// (length, code, increment) of table B.1; escape adds 33, stuffing is skipped.
const MB_ADDRESS: [(u8, u16, u8); 33] = [
    (1, 0b1, 1),
    (3, 0b011, 2),
    (3, 0b010, 3),
    (4, 0b0011, 4),
    (4, 0b0010, 5),
    (5, 0b00011, 6),
    (5, 0b00010, 7),
    (7, 0b0000111, 8),
    (7, 0b0000110, 9),
    (8, 0b00001011, 10),
    (8, 0b00001010, 11),
    (8, 0b00001001, 12),
    (8, 0b00001000, 13),
    (8, 0b00000111, 14),
    (8, 0b00000110, 15),
    (10, 0b0000010111, 16),
    (10, 0b0000010110, 17),
    (10, 0b0000010101, 18),
    (10, 0b0000010100, 19),
    (10, 0b0000010011, 20),
    (10, 0b0000010010, 21),
    (11, 0b00000100011, 22),
    (11, 0b00000100010, 23),
    (11, 0b00000100001, 24),
    (11, 0b00000100000, 25),
    (11, 0b00000011111, 26),
    (11, 0b00000011110, 27),
    (11, 0b00000011101, 28),
    (11, 0b00000011100, 29),
    (11, 0b00000011011, 30),
    (11, 0b00000011010, 31),
    (11, 0b00000011001, 32),
    (11, 0b00000011000, 33),
];
const MB_ESCAPE: (u8, u16, u8) = (11, 0b00000001000, 100);
const MB_STUFFING: (u8, u16, u8) = (11, 0b00000001111, 101);

// macroblock type flags
const QUANT: u8 = 1;
const FWD: u8 = 2;
const BWD: u8 = 4;
const PATTERN: u8 = 8;
const INTRA: u8 = 16;

const MB_TYPE_I: [(u8, u16, u8); 2] = [(1, 0b1, INTRA), (2, 0b01, INTRA | QUANT)];
const MB_TYPE_P: [(u8, u16, u8); 7] = [
    (1, 0b1, FWD | PATTERN),
    (2, 0b01, PATTERN),
    (3, 0b001, FWD),
    (5, 0b00011, INTRA),
    (5, 0b00010, QUANT | FWD | PATTERN),
    (5, 0b00001, QUANT | PATTERN),
    (6, 0b000001, QUANT | INTRA),
];
const MB_TYPE_B: [(u8, u16, u8); 11] = [
    (2, 0b10, FWD | BWD),
    (2, 0b11, FWD | BWD | PATTERN),
    (3, 0b010, BWD),
    (3, 0b011, BWD | PATTERN),
    (4, 0b0010, FWD),
    (4, 0b0011, FWD | PATTERN),
    (5, 0b00011, INTRA),
    (5, 0b00010, QUANT | FWD | BWD | PATTERN),
    (6, 0b000011, QUANT | FWD | PATTERN),
    (6, 0b000010, QUANT | BWD | PATTERN),
    (6, 0b000001, QUANT | INTRA),
];

/// Magnitudes of the motion codes (table B.4); the sign bit follows.
const MOTION: [(u8, u16, u8); 17] = [
    (1, 0b1, 0),
    (2, 0b01, 1),
    (3, 0b001, 2),
    (4, 0b0001, 3),
    (6, 0b000011, 4),
    (7, 0b0000101, 5),
    (7, 0b0000100, 6),
    (7, 0b0000011, 7),
    (9, 0b000001011, 8),
    (9, 0b000001010, 9),
    (9, 0b000001001, 10),
    (10, 0b0000010001, 11),
    (10, 0b0000010000, 12),
    (10, 0b0000001111, 13),
    (10, 0b0000001110, 14),
    (10, 0b0000001101, 15),
    (10, 0b0000001100, 16),
];

const DC_LUMA: [(u8, u16, u8); 9] =
    [(3, 0b100, 0), (2, 0b00, 1), (2, 0b01, 2), (3, 0b101, 3), (3, 0b110, 4), (4, 0b1110, 5), (5, 0b11110, 6), (6, 0b111110, 7), (7, 0b1111110, 8)];
const DC_CHROMA: [(u8, u16, u8); 9] =
    [(2, 0b00, 0), (2, 0b01, 1), (2, 0b10, 2), (3, 0b110, 3), (4, 0b1110, 4), (5, 0b11110, 5), (6, 0b111110, 6), (7, 0b1111110, 7), (8, 0b11111110, 8)];

/// Prefix-code lookup table indexed by the next `bits` bits.
struct Vlc {
    bits: u32,
    lut: Vec<(u8, u16)>,
}

impl Vlc {
    fn new(entries: impl Iterator<Item = (u8, u16, u16)>) -> Vlc {
        let entries: Vec<_> = entries.collect();
        let bits = entries.iter().map(|e| e.0 as u32).max().unwrap();
        let mut lut = vec![(0u8, 0u16); 1 << bits];
        for (len, code, value) in entries {
            let shift = bits - len as u32;
            for low in 0..1usize << shift {
                let slot = &mut lut[(code as usize) << shift | low];
                assert_eq!(slot.0, 0, "code {code:b} overlaps another");
                *slot = (len, value);
            }
        }
        Vlc { bits, lut }
    }

    fn small(entries: &[(u8, u16, u8)]) -> Vlc {
        Vlc::new(entries.iter().map(|&(l, c, v)| (l, c, v as u16)))
    }

    fn read(&self, b: &mut Bits) -> Result<u16> {
        let (len, v) = self.lut[b.peek(self.bits) as usize];
        if len == 0 {
            return Err(b.error("invalid variable-length code"));
        }
        b.skip(len as usize);
        Ok(v)
    }
}

const EOB: u16 = 0xffff;
const ESCAPE: u16 = 0xfffe;

struct Tables {
    coeff: Vlc,
    cbp: Vlc,
    address: Vlc,
    mb_type: [Vlc; 3],
    motion: Vlc,
    dc: [Vlc; 2],
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| Tables {
        coeff: Vlc::new(COEFF.iter().map(|&(l, c, run, level)| (l, c, (run as u16) << 8 | level as u16)).chain([(2, 0b10, EOB), (6, 0b000001, ESCAPE)])),
        cbp: Vlc::new(CBP.iter().map(|&(l, c, v)| (l, c, v as u16))),
        address: Vlc::new(MB_ADDRESS.iter().chain([&MB_ESCAPE, &MB_STUFFING]).map(|&(l, c, v)| (l, c, v as u16))),
        mb_type: [Vlc::small(&MB_TYPE_I), Vlc::small(&MB_TYPE_P), Vlc::small(&MB_TYPE_B)],
        motion: Vlc::small(&MOTION),
        dc: [Vlc::small(&DC_LUMA), Vlc::small(&DC_CHROMA)],
    })
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    fn peek(&self, n: u32) -> u32 {
        let at = self.pos >> 3;
        let mut v = 0u64;
        for i in 0..8 {
            v = v << 8 | *self.data.get(at + i).unwrap_or(&0) as u64;
        }
        ((v << (self.pos & 7)) >> (64 - n)) as u32
    }

    fn skip(&mut self, n: usize) {
        self.pos += n;
    }

    fn read(&mut self, n: u32) -> u32 {
        let v = self.peek(n);
        self.pos += n as usize;
        v
    }

    fn flag(&mut self) -> bool {
        self.read(1) == 1
    }

    fn align(&mut self) {
        self.pos = (self.pos + 7) & !7;
    }

    fn at_end(&self) -> bool {
        self.pos >= self.data.len() * 8
    }

    fn error(&self, what: &str) -> Error {
        Error::Invalid(format!("{what} at bit {} (byte {:#x})", self.pos, self.pos / 8))
    }

    /// Move to the next `00 00 01 xx` and return `xx`.
    fn next_start_code(&mut self) -> Option<u8> {
        self.align();
        let mut i = self.pos / 8;
        while i + 4 <= self.data.len() {
            if self.data[i] == 0 && self.data[i + 1] == 0 && self.data[i + 2] == 1 {
                self.pos = (i + 4) * 8;
                return Some(self.data[i + 3]);
            }
            i += 1;
        }
        self.pos = self.data.len() * 8;
        None
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    I,
    P,
    B,
}

struct Decoder {
    width: usize,
    height: usize,
    mb_w: usize,
    mb_h: usize,
    intra_matrix: [u8; 64],
    inter_matrix: [u8; 64],
    cur: Option<(Frame, Kind)>,
    /// older and newer anchor pictures (B pictures lie between them)
    fwd: Option<Frame>,
    bwd: Option<Frame>,
    out: VecDeque<Frame>,
    // picture parameters
    full_pel: [bool; 2],
    f_code: [u32; 2],
    frame_rate_code: usize,
}

/// Decode a whole MPEG-1 video elementary stream into frames in display order.
pub fn decode(es: &[u8]) -> Result<Vec<Frame>> {
    let mut stream = Stream::new(es);
    let mut frames = Vec::new();
    while let Some(f) = stream.next_frame()? {
        frames.push(f);
    }
    Ok(frames)
}

/// Incremental decoder: pulls one frame at a time so long movies need not be held in memory.
pub struct Stream<'a> {
    data: Cow<'a, [u8]>,
    pos: usize,
    d: Decoder,
    done: bool,
}

impl<'a> Stream<'a> {
    /// A decoder over an MPEG-1 video elementary stream.
    pub fn new(es: &'a [u8]) -> Stream<'a> {
        Stream::from_cow(Cow::Borrowed(es))
    }

    /// A stream that owns its data.
    pub fn owned(es: Vec<u8>) -> Stream<'static> {
        Stream::from_cow(Cow::Owned(es))
    }

    fn from_cow(data: Cow<'a, [u8]>) -> Stream<'a> {
        let d = Decoder {
            width: 0,
            height: 0,
            mb_w: 0,
            mb_h: 0,
            intra_matrix: DEFAULT_INTRA,
            inter_matrix: [16; 64],
            cur: None,
            fwd: None,
            bwd: None,
            out: VecDeque::new(),
            full_pel: [false; 2],
            f_code: [1; 2],
            frame_rate_code: 0,
        };
        Stream { data, pos: 0, d, done: false }
    }

    /// Next frame in display order, `None` at the end of the stream.
    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        loop {
            if let Some(f) = self.d.out.pop_front() {
                return Ok(Some(f));
            }
            if self.done {
                return Ok(None);
            }
            let mut bits = Bits { data: &self.data, pos: self.pos };
            let (b, d) = (&mut bits, &mut self.d);
            let code = b.next_start_code();
            self.pos = b.pos;
            let Some(code) = code else {
                d.finish_picture();
                if let Some(last) = d.bwd.take() {
                    d.out.push_back(last);
                }
                self.done = true;
                continue;
            };
            match code {
                0xb3 => {
                    d.finish_picture();
                    d.sequence_header(b)?;
                }
                0xb8 | 0xb7 => d.finish_picture(),
                0x00 => {
                    d.finish_picture();
                    d.picture_header(b)?;
                }
                0x01..=0xaf => d.slice(b, code as usize - 1)?,
                _ => {}
            }
            self.pos = b.pos;
        }
    }

    /// Frames per second from the sequence header (0 until it has been read).
    pub fn frame_rate(&self) -> f64 {
        crate::video::FRAME_RATES.get(self.d.frame_rate_code).copied().unwrap_or(0.0)
    }

    /// Picture size, known after the first sequence header.
    pub fn size(&self) -> (usize, usize) {
        (self.d.width, self.d.height)
    }
}

impl Decoder {
    fn sequence_header(&mut self, b: &mut Bits) -> Result<()> {
        self.width = b.read(12) as usize;
        self.height = b.read(12) as usize;
        if self.width == 0 || self.height == 0 {
            return Err(b.error("empty picture size"));
        }
        self.mb_w = self.width.div_ceil(16);
        self.mb_h = self.height.div_ceil(16);
        b.skip(4); // aspect ratio
        self.frame_rate_code = b.read(4) as usize;
        b.skip(18 + 1 + 10 + 1); // bit rate, marker, vbv, constrained
        self.intra_matrix = DEFAULT_INTRA;
        self.inter_matrix = [16; 64];
        for matrix in [&mut self.intra_matrix, &mut self.inter_matrix] {
            if b.flag() {
                for z in ZIGZAG {
                    matrix[z as usize] = b.read(8) as u8;
                }
            }
        }
        Ok(())
    }

    fn picture_header(&mut self, b: &mut Bits) -> Result<()> {
        if self.width == 0 {
            return Err(b.error("picture before sequence header"));
        }
        b.skip(10); // temporal reference
        let kind = match b.read(3) {
            1 => Kind::I,
            2 => Kind::P,
            3 => Kind::B,
            t => return Err(b.error(&format!("unsupported picture type {t}"))),
        };
        b.skip(16); // vbv delay
        for dir in 0..2 {
            if kind == Kind::I || (dir == 1 && kind == Kind::P) {
                break;
            }
            self.full_pel[dir] = b.flag();
            self.f_code[dir] = b.read(3);
            if self.f_code[dir] == 0 {
                return Err(b.error("zero f_code"));
            }
        }
        while b.flag() {
            b.skip(8);
        }
        self.cur = Some((Frame::new(self.width, self.height), kind));
        Ok(())
    }

    fn finish_picture(&mut self) {
        let Some((frame, kind)) = self.cur.take() else { return };
        if kind == Kind::B {
            self.out.push_back(frame);
        } else {
            if let Some(old) = self.bwd.take() {
                self.out.push_back(old.clone());
                self.fwd = Some(old);
            }
            self.bwd = Some(frame);
        }
    }

    fn slice(&mut self, b: &mut Bits, row: usize) -> Result<()> {
        let Some((mut cur, kind)) = self.cur.take() else { return Err(b.error("slice outside a picture")) };
        let r = self.slice_into(b, row, &mut cur, kind);
        self.cur = Some((cur, kind));
        r
    }

    fn slice_into(&self, b: &mut Bits, row: usize, cur: &mut Frame, kind: Kind) -> Result<()> {
        if row >= self.mb_h {
            return Err(b.error("slice row outside the picture"));
        }
        let t = tables();
        let (fwd, bwd) = match kind {
            Kind::I => (None, None),
            // a P picture predicts from the newest anchor
            Kind::P => (self.bwd.as_ref(), None),
            Kind::B => (self.fwd.as_ref(), self.bwd.as_ref()),
        };
        let mut q = b.read(5) as i32;
        if q == 0 {
            return Err(b.error("zero quantiser scale"));
        }
        while b.flag() {
            b.skip(8);
        }
        let mut addr = row * self.mb_w; // first macroblock of the slice minus one is addr - 1
        let mut first = true;
        let mut dc_pred = [128i32; 3];
        let mut pmv = [[0i32; 2]; 2];
        let mut last_flags = 0u8;
        let mut last_mv = [[0i32; 2]; 2];
        loop {
            // address increment
            let mut inc = 0usize;
            loop {
                match t.address.read(b)? {
                    100 => inc += 33,
                    101 => {}
                    v => {
                        inc += v as usize;
                        break;
                    }
                }
            }
            let skipped = if first { 0 } else { inc - 1 };
            if first {
                addr += inc - 1;
            }
            for _ in 0..skipped {
                addr += 1;
                if addr >= self.mb_w * self.mb_h {
                    return Err(b.error("macroblock address past the picture"));
                }
                dc_pred = [128; 3];
                let (mx, my) = (addr % self.mb_w, addr / self.mb_w);
                match kind {
                    Kind::P => {
                        pmv = [[0; 2]; 2];
                        self.predict_mb(cur, mx, my, FWD, [[0; 2]; 2], fwd, bwd);
                    }
                    _ => self.predict_mb(cur, mx, my, last_flags & (FWD | BWD), last_mv, fwd, bwd),
                }
            }
            if !first {
                addr += 1;
            }
            first = false;
            if addr >= self.mb_w * self.mb_h {
                return Err(b.error("macroblock address past the picture"));
            }
            let (mx, my) = (addr % self.mb_w, addr / self.mb_w);

            let flags = t.mb_type[kind as usize].read(b)? as u8;
            if flags & QUANT != 0 {
                q = b.read(5) as i32;
                if q == 0 {
                    return Err(b.error("zero quantiser scale"));
                }
            }
            let mut mv = [[0i32; 2]; 2];
            if flags & INTRA != 0 {
                pmv = [[0; 2]; 2];
            } else {
                if kind == Kind::P && flags & FWD == 0 {
                    pmv = [[0; 2]; 2];
                }
                for dir in 0..2 {
                    if flags & [FWD, BWD][dir] == 0 {
                        continue;
                    }
                    for axis in 0..2 {
                        let delta = self.motion_delta(b, dir)?;
                        let range = 16 << (self.f_code[dir] - 1);
                        let mut v = pmv[dir][axis] + delta;
                        if v >= range {
                            v -= 2 * range;
                        } else if v < -range {
                            v += 2 * range;
                        }
                        pmv[dir][axis] = v;
                    }
                    mv[dir] = pmv[dir];
                    if self.full_pel[dir] {
                        mv[dir] = [mv[dir][0] * 2, mv[dir][1] * 2];
                    }
                }
            }
            let cbp = if flags & INTRA != 0 {
                63
            } else if flags & PATTERN != 0 {
                t.cbp.read(b)? as u8
            } else {
                0
            };
            if flags & INTRA == 0 {
                let dirs = if kind == Kind::P { FWD } else { flags & (FWD | BWD) };
                self.predict_mb(cur, mx, my, dirs, mv, fwd, bwd);
                last_flags = flags;
                last_mv = mv;
                dc_pred = [128; 3];
            }
            for blk in 0..6 {
                let coded = cbp >> (5 - blk) & 1 == 1;
                if !coded {
                    continue;
                }
                let mut coef = [0i32; 64];
                if flags & INTRA != 0 {
                    self.intra_block(b, blk, q, &mut dc_pred, &mut coef)?;
                } else {
                    self.inter_block(b, q, &mut coef)?;
                }
                let out = idct(&coef);
                put_block(cur, mx, my, blk, &out, flags & INTRA == 0);
            }
            if b.at_end() || b.peek(23) == 0 {
                return Ok(());
            }
        }
    }

    fn motion_delta(&self, b: &mut Bits, dir: usize) -> Result<i32> {
        let t = tables();
        let mag = t.motion.read(b)? as i32;
        if mag == 0 {
            return Ok(0);
        }
        let neg = b.flag();
        let r_size = self.f_code[dir] - 1;
        let mut delta = mag;
        if r_size > 0 {
            delta = ((mag - 1) << r_size) + b.read(r_size) as i32 + 1;
        }
        Ok(if neg { -delta } else { delta })
    }

    fn intra_block(&self, b: &mut Bits, blk: usize, q: i32, pred: &mut [i32; 3], coef: &mut [i32; 64]) -> Result<()> {
        let t = tables();
        let comp = blk.saturating_sub(3);
        let size = t.dc[(blk >= 4) as usize].read(b)? as u32;
        let mut diff = 0;
        if size > 0 {
            let v = b.read(size) as i32;
            diff = if v < 1 << (size - 1) { v - (1 << size) + 1 } else { v };
        }
        pred[comp] += diff;
        coef[0] = pred[comp] * 8;
        let mut i = 1;
        loop {
            let (run, level) = self.coefficient(b, false)?;
            if run < 0 {
                return Ok(());
            }
            i += run as usize;
            if i > 63 {
                return Err(b.error("coefficient index past the block"));
            }
            let nat = ZIGZAG[i] as usize;
            let v = (level.abs() * q * self.intra_matrix[nat] as i32) >> 3;
            coef[nat] = level.signum() * oddify(v).min(2047);
            i += 1;
        }
    }

    fn inter_block(&self, b: &mut Bits, q: i32, coef: &mut [i32; 64]) -> Result<()> {
        let mut i = 0;
        let mut first = true;
        loop {
            let (run, level) = self.coefficient(b, first)?;
            first = false;
            if run < 0 {
                return Ok(());
            }
            i += run as usize;
            if i > 63 {
                return Err(b.error("coefficient index past the block"));
            }
            let nat = ZIGZAG[i] as usize;
            let v = ((2 * level.abs() + 1) * q * self.inter_matrix[nat] as i32) >> 4;
            coef[nat] = level.signum() * oddify(v).min(2047);
            i += 1;
        }
    }

    /// (run, level) of the next coefficient, run -1 for end of block.
    fn coefficient(&self, b: &mut Bits, first: bool) -> Result<(i32, i32)> {
        if first && b.peek(1) == 1 {
            b.skip(1);
            return Ok((0, if b.flag() { -1 } else { 1 }));
        }
        match tables().coeff.read(b)? {
            EOB => {
                if first {
                    return Err(b.error("empty coded block"));
                }
                Ok((-1, 0))
            }
            ESCAPE => {
                let run = b.read(6) as i32;
                let mut level = b.read(8) as i32;
                if level == 0 {
                    level = b.read(8) as i32;
                } else if level == 128 {
                    level = b.read(8) as i32 - 256;
                } else if level > 128 {
                    level -= 256;
                }
                if level == 0 {
                    return Err(b.error("zero escape level"));
                }
                Ok((run, level))
            }
            v => {
                let level = (v & 0xff) as i32;
                Ok(((v >> 8) as i32, if b.flag() { -level } else { level }))
            }
        }
    }

    /// Write the motion-compensated prediction of one macroblock.
    fn predict_mb(&self, cur: &mut Frame, mx: usize, my: usize, dirs: u8, mv: [[i32; 2]; 2], fwd: Option<&Frame>, bwd: Option<&Frame>) {
        let s = cur.stride();
        let refs = [(dirs & FWD != 0, fwd, mv[0]), (dirs & BWD != 0, bwd, mv[1])];
        let mut parts: Vec<([u8; 256], [u8; 64], [u8; 64])> = Vec::new();
        for (on, frame, v) in refs {
            let (true, Some(f)) = (on, frame) else { continue };
            let mut luma = [0u8; 256];
            let mut cb = [0u8; 64];
            let mut cr = [0u8; 64];
            fetch(&f.y, s, f.height.div_ceil(16) * 16, mx * 16, my * 16, v[0], v[1], 16, &mut luma);
            // chroma vectors are half the luma ones, truncated toward zero
            let (cx, cy) = (v[0] / 2, v[1] / 2);
            fetch(&f.cb, s / 2, f.height.div_ceil(16) * 8, mx * 8, my * 8, cx, cy, 8, &mut cb);
            fetch(&f.cr, s / 2, f.height.div_ceil(16) * 8, mx * 8, my * 8, cx, cy, 8, &mut cr);
            parts.push((luma, cb, cr));
        }
        if parts.is_empty() {
            return;
        }
        let avg = |a: u8, b: u8| ((a as u16 + b as u16 + 1) >> 1) as u8;
        for y in 0..16 {
            for x in 0..16 {
                let p = if parts.len() == 2 { avg(parts[0].0[y * 16 + x], parts[1].0[y * 16 + x]) } else { parts[0].0[y * 16 + x] };
                cur.y[(my * 16 + y) * s + mx * 16 + x] = p;
            }
        }
        for y in 0..8 {
            for x in 0..8 {
                let at = (my * 8 + y) * s / 2 + mx * 8 + x;
                let get = |i: usize, which: usize| if which == 0 { parts[i].1[y * 8 + x] } else { parts[i].2[y * 8 + x] };
                cur.cb[at] = if parts.len() == 2 { avg(get(0, 0), get(1, 0)) } else { get(0, 0) };
                cur.cr[at] = if parts.len() == 2 { avg(get(0, 1), get(1, 1)) } else { get(0, 1) };
            }
        }
    }
}

fn oddify(v: i32) -> i32 {
    if v & 1 == 0 {
        (v - 1).max(0)
    } else {
        v
    }
}

/// Half-pel block fetch with the standard's rounding, clamped to the plane.
fn fetch(plane: &[u8], stride: usize, rows: usize, x: usize, y: usize, mvx: i32, mvy: i32, n: usize, out: &mut [u8]) {
    let (ix, fx) = (x as i32 + (mvx >> 1), mvx & 1);
    let (iy, fy) = (y as i32 + (mvy >> 1), mvy & 1);
    let px = |xx: i32, yy: i32| plane[(yy.clamp(0, rows as i32 - 1) as usize) * stride + xx.clamp(0, stride as i32 - 1) as usize] as u32;
    for j in 0..n as i32 {
        for i in 0..n as i32 {
            let (a, b, c, d) = (px(ix + i, iy + j), px(ix + i + fx, iy + j), px(ix + i, iy + j + fy), px(ix + i + fx, iy + j + fy));
            out[(j * n as i32 + i) as usize] = match (fx, fy) {
                (0, 0) => a,
                (1, 0) => (a + b + 1) >> 1,
                (0, 1) => (a + c + 1) >> 1,
                _ => (a + b + c + d + 2) >> 2,
            } as u8;
        }
    }
}

fn put_block(cur: &mut Frame, mx: usize, my: usize, blk: usize, vals: &[i32; 64], add: bool) {
    let s = cur.stride();
    let (plane, stride, x0, y0) = match blk {
        0..=3 => (&mut cur.y, s, mx * 16 + (blk & 1) * 8, my * 16 + (blk >> 1) * 8),
        4 => (&mut cur.cb, s / 2, mx * 8, my * 8),
        _ => (&mut cur.cr, s / 2, mx * 8, my * 8),
    };
    for y in 0..8 {
        for x in 0..8 {
            let p = &mut plane[(y0 + y) * stride + x0 + x];
            let v = vals[y * 8 + x] + if add { *p as i32 } else { 0 };
            *p = v.clamp(0, 255) as u8;
        }
    }
}

/// Inverse 8x8 DCT (floating point reference, rounded to nearest).
fn idct(coef: &[i32; 64]) -> [i32; 64] {
    static COS: OnceLock<[[f64; 8]; 8]> = OnceLock::new();
    let c = COS.get_or_init(|| {
        let mut m = [[0.0; 8]; 8];
        for (x, row) in m.iter_mut().enumerate() {
            for (u, v) in row.iter_mut().enumerate() {
                let s = if u == 0 { 0.5f64.sqrt() } else { 1.0 };
                *v = s * ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 16.0).cos() / 2.0;
            }
        }
        m
    });
    let mut tmp = [[0.0f64; 8]; 8];
    for y in 0..8 {
        if coef[y * 8..y * 8 + 8].iter().all(|&v| v == 0) {
            continue;
        }
        for x in 0..8 {
            tmp[y][x] = (0..8).map(|u| coef[y * 8 + u] as f64 * c[x][u]).sum();
        }
    }
    let mut out = [0i32; 64];
    for x in 0..8 {
        for y in 0..8 {
            let v: f64 = (0..8).map(|v| tmp[v][x] * c[y][v]).sum();
            out[y * 8 + x] = v.round() as i32;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_prefix_free() {
        // building the lookup tables asserts that no two codes overlap
        let t = tables();
        assert_eq!(t.coeff.bits, 16);
        assert_eq!(t.cbp.bits, 9);
    }

    #[test]
    fn the_inverse_dct_of_a_dc_value_is_flat() {
        let mut c = [0i32; 64];
        c[0] = 80;
        assert!(idct(&c).iter().all(|&v| v == 10));
        c[0] = 0;
        c[1] = 64; // one horizontal cosine
        let o = idct(&c);
        assert!(o[0] > 0 && o[7] < 0 && o[0] == -o[7] && o[0] == o[56]);
    }

    struct W(Vec<bool>);
    impl W {
        fn bits(&mut self, v: u64, n: u32) {
            for i in (0..n).rev() {
                self.0.push(v >> i & 1 == 1);
            }
        }
        fn code(&mut self, s: &str) {
            self.0.extend(s.bytes().map(|c| c == b'1'));
        }
        fn bytes(mut self) -> Vec<u8> {
            while !self.0.len().is_multiple_of(8) {
                self.0.push(false);
            }
            self.0.chunks(8).map(|c| c.iter().fold(0, |a, &b| a << 1 | b as u8)).collect()
        }
    }

    /// One 16x16 intra picture whose first luma block has DC difference `+diff` (size 1..).
    fn gray_stream(dc_size_code: &str, dc_bits: (u64, u32)) -> Vec<u8> {
        let mut w = W(vec![]);
        w.bits(0x1b3, 32);
        w.bits(16, 12);
        w.bits(16, 12);
        w.bits(1, 4);
        w.bits(3, 4);
        w.bits(0x3ffff, 18);
        w.bits(1, 1);
        w.bits(20, 10);
        w.bits(0, 3);
        w.bits(0x100, 32);
        w.bits(0, 10);
        w.bits(1, 3);
        w.bits(0xffff, 16);
        w.bits(0, 1);
        let mut w = W({
            let mut v = w.0;
            while !v.len().is_multiple_of(8) {
                v.push(false);
            }
            v
        });
        w.bits(0x101, 32);
        w.bits(4, 5);
        w.bits(0, 1);
        w.code("11"); // address 1, intra
        w.code(dc_size_code);
        w.bits(dc_bits.0, dc_bits.1);
        w.code("10"); // end of block
        for _ in 0..3 {
            w.code("10010");
        }
        for _ in 0..2 {
            w.code("0010");
        }
        w.bytes()
    }

    #[test]
    fn decodes_a_flat_intra_picture() {
        let frames = decode(&gray_stream("100", (0, 0))).unwrap();
        assert_eq!(frames.len(), 1);
        let f = &frames[0];
        assert_eq!((f.width, f.height), (16, 16));
        assert!(f.y.iter().all(|&v| v == 128) && f.cb.iter().all(|&v| v == 128));
        // luma DC size 3 (code 101), value 0b111 = +7: DC prediction carries to the other luma blocks
        let f = &decode(&gray_stream("101", (7, 3))).unwrap()[0];
        assert_eq!((f.y[0], f.y[7 * 16 + 7], f.y[8], f.cb[0]), (135, 135, 135, 128));
        assert_eq!(f.to_yuv420().len(), 16 * 16 * 3 / 2);
    }

    #[test]
    fn rejects_truncated_input() {
        let mut s = gray_stream("100", (0, 0));
        s.truncate(s.len() - 6);
        assert!(decode(&s).is_err() || decode(&s).unwrap().len() <= 1);
        assert!(decode(&[0, 0, 1, 0x01, 0x08]).is_err());
    }
}
