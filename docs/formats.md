# File format notes

Findings from analysing the Steam release (32-bit MSVC C++/MFC, DirectDraw/DirectInput/DirectSound,
Intel JPEG Library). Class names recovered from RTTI: CMegaFile, CHuffcomp, CBuilding, CUnitChassis, ...

See the module docs in `crates/opensow-formats/src/` for the authoritative description of each decoded format.

## Level files (`Data/Levels/`)
- Per level `Mxx`: `.srf` (terrain JPEG, 32 px per cell, e.g. 2048x2048 = 64x64 cells), `.srfa` (second JPEG layer, half resolution; purpose unconfirmed),
  `.map`, `.tmi`, `.til`, `.edt`, `.sgv` (briefing/intro video: MPEG-1, or AVI for mA/mB; docs/video.md). `M00` is a test/menu level without `.sgv`. Multiplayer maps are `NN-Name.net`.
- `.map` (VER7): header + `w*h` 17-byte cells; see `map.rs`. Cell byte 1 = blocked (lake/mountain outline verified on M01), byte 5 = border ring,
  byte 11 = sparse flag (unknown). Header `a,b` look like pixel coordinates (M01: 1341,1560 => cell ~42,48), maybe player start. Sizes seen: 64x64, 96x80.
- `.tmi`: `u16 w,h` + `w*h` u16 tile indices (0 = none) into `.til`. Max index = frame count - 1 (M01: 1030 frames).
- `.til`: identical framing to sprites, 32x32 frames, parsed with `Kind::Shadow` (rows are 3 words each in M01: one opaque run => likely full masks;
  pixels probably come from `.srf`/`.srfa`). Role in rendering unverified.
- `.edt` / `.net` (decoded, `level.rs`): 376-byte header (94 i32), then 76-byte objects (tag 123), then 20-byte markers (tag 224), then footer 7656.
  All 31 files parse to the last byte. Object words 16/17 are cell x/y (2131 objects, all inside their map, none on a blocked cell). Marker x/y are pixels.
  Object and marker kinds are decoded: see `docs/levels.md`.
  Header: word 1 = level id (campaign 1..22; .net ids 15,17,19 seen), words 2-3 = probable start resources per player (2500/5000, 3000/3000...),
  bytes 92..107 look like per-player flag bytes (0x01010101 = 4 players on), words 31/32 = 15/15, 12/8, 10/10 (unknown), word 37-38 = 400/1000 on some levels.

## Sprites (`.ps6`, `.tsp`, `.sha`)
- See `sprite.rs` module docs. 931 files verified (rows sum exactly to width, file ends with u32 0).
- `unit_blue/_gren` = team colours, `_boom` explosion, `_fire` muzzle flash, `.sha` ground shadow, `tur_*` turrets; 32 frames = 2 rows x 16 rotations.
- `.po1/.po2`: weapon fire points per facing: tables of 32 `(i16,i16)` pairs; see `docs/art.md` (decoded).

## Archive
- `sprites.info` + `sprites.data` (CMegaFile): 377 entries, names shifted +10; contents are `.ps6/.sha/.po1/.po2/.msk` under `units\` and `__extras\`.
  Same files also exist loose in `Data/units`, check which the game prefers when porting.

## Fonts
See `docs/fonts.md` (`.msk` = coverage masks in sprite frame format; frame k = ASCII 32+k).

## generic.file (decoded)
`Data/generic.file` (111111 bytes, same in every language pack) is the game's lockstep **random table**: little-endian u16 words, `rand(n) = table[i] % n`, `i` advancing from 0 and wrapping after 0xc346 (`crates/opensow-formats/src/rng.rs`, exe-tested). Purely visual randomness (`0x437cd0`) uses a second table filled with the C runtime's `rand()` at start-up.

## Not yet decoded
 `State of War.cfg`, `lang.dat` (1 byte `0`; selects `Language/lang*.txt`).
