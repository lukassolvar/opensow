# Indeo 5 (`mA.sgv`) - decoded (intra frames)

`mA.sgv` is an AVI with Intel Indeo 5 video (`IV50`, 640x480, 104 frames, YVU9 = 4:1:0). The game relies on
Windows' codec; `indeo5.rs` decodes it in Rust. **All 104 frames are intra frames**, so no motion
compensation was needed. Every frame is bit-identical to FFmpeg (used as an oracle through PyAV in a scratch
venv, never in the repo); the test `indeo5_decodes_the_whole_ma_video_like_the_reference_decoder` decodes all
frames and pins plane sums of four. Unsupported (returns an error): inter frames, slices, transparency,
scalable (wavelet) pictures, custom transforms, quantiser inheritance, other quantiser matrices.
OpenSOW plays `mA` with this decoder (`intro::Anim`, docs/video.md).
**Differs from the exe:** the exe has Windows' Indeo 5 codec decode the movie through DirectShow; OpenSOW uses `indeo5.rs`. Only the subset that `mA` needs is decoded, so any other Indeo 5 file returns an error.

## Bitstream (bits read LSB first)
Frame: `psc(5)=0x1f`, `type(3)` (0 intra), `number(8)`. GOP header: `gop_flags(8)` (bit0 size present), `size(16)` (bytes
from `gop_flags`), `luma_levels(2)`, `chroma_levels(1)`, `pic_size(4)` (0 = 640x480, 15 = explicit 13+13), two 6-bit band
infos (`mv_res`, `mb_size_id`, `blk_size_id`, `trans_flg`, `end(2)`: luma 16x16 macroblocks of four 8x8 blocks, chroma
one 4x4 block per 4x4 macroblock). Frame header: `flags(8)` (bit0 size(24), bit4 checksum(16), bit6 macroblock codebook
`sel(3)`), 3 bits, align. Three bands in the order Y, V, U (V is output plane 2): `band_flags(8)` (bit4 table corrections,
bit6 `rv_sel(3)` else map 8, bit7 block codebook `sel(3)` (7 = custom: `rows(4)` x `xbits(4)`) else descriptor 7),
`size(24)`, [`n(8)` + `2n` bytes of swapped table entries], checksum flag(+16), `glob_quant(5)`, align. Tile: `empty(1)`,
size flag(1) -> `len(8)` (255: `len(24)`), **length counts the tile header too**, align; then per macroblock `skip(1)=0`,
`cbp` (one bit per block); then the coded blocks in macroblock order.
Block: symbols from the band codebook (row `i` of the descriptor = `2^xbits[i]` codes: `i` ones, a zero unless last row,
suffix bits; read most significant bit first) through the run/value map. **Table symbols advance the scan by `run`,
escapes read `run`, `lo`, `hi` symbols: advance `run + 1`, value `tosigned(hi<<6|lo)`.** Scan position starts at -1;
8x8 zigzag, 4x4 `0 1 4 8 5 2 3 6 9 12 13 10 7 11 14 15`.

## Arithmetic (verified on thousands of random blocks at every quantiser)
`a = max(1, base[pos] * scale[q] >> 9)`; coefficient = `v*a + (a>>1) - (a&1)` (plain `v` for a = 1), sign symmetric. Position 0
(DC) accumulates dequantised deltas across all blocks of the band (empty blocks inherit) and enters the transform as
coefficient 0. Inverse slant: butterflies are plain sums/differences, `reflect(s1,s2) = (((s1+2*s2+2)>>2)+s1,
((2*s1-s2+2)>>2)-s2)`, `part4(s1,s2) = (s2+((4*s1-s2+4)>>3), s1+((-s1-4*s2+4)>>3))`; 8-point input order
`(s1,s4,s8,s5,s2,s6,s3,s7)` = scan columns 0..7; 4-point `(s1,s4,s2,s3)`; columns first, then rows with `(x+1)>>1`;
pixel = clip(128 + result). Tables (base matrices, scale rows, run/value maps, codebook descriptors) are in `indeo5.rs`.

## Method
A small encoder (bit-exact: it reproduces the real tile payload byte for byte) built synthetic one-coefficient and
random-block frames for the reference decoder; reading the pixels back identified the arithmetic.
