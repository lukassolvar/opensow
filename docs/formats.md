# File format notes

Findings from analysing the Steam release (32-bit MSVC C++/MFC, DirectDraw/DirectInput/DirectSound,
Intel JPEG Library). Class names recovered from RTTI: CMegaFile, CHuffcomp, CBuilding, CUnitChassis, ...

See the module docs in `crates/opensow-formats/src/` for the authoritative description of each decoded format. OpenSOW reads every format below with these parsers (`opensow check` parses every file of the install); where it uses a file differently from the exe it is marked.

## Level files (`Data/Levels/`)
- Per level `Mxx`: `.srf` (terrain JPEG, 32 px per cell, e.g. 2048x2048 = 64x64 cells), `.srfa` (the **low-detail background**: the level loader `0x4256c3` loads `.srf` when the detail byte `[0x49a33a]` is non-zero and `.srfa` when it is 0; the byte is 1 in the exe's data and nothing ever writes it, so the shipped game never loads a `.srfa`, nor does OpenSOW; half resolution),
  `.map`, `.tmi`, `.til`, `.edt`, `.sgv` (briefing/intro video: MPEG-1, or AVI for mA/mB; docs/video.md). `M00` is the tutorial level (no `.sgv`, docs/tutorial.md). Multiplayer maps are `NN-Name.net`.
- `.map` (VER7): header + `w*h` 17-byte cells; see `map.rs`. It is the exe's own serialization of the map object, so the same stream appears in savegames (docs/savegame.md, `save::MapState` reads and rewrites every shipped file). Per cell (memory index `x*128+y`): `u16` coordinate stamp `(x<<8)|y`, `u32` ground obstacle grid (`0xFFFFFD` = lake/mountain), `u32` air-layer terrain (the outer ring and, on M04/M10/M22, no-fly rectangles; `+0x180cc`, docs/pathfinding.md), three bytes `reserved` (always 0), **no-marks** (1 = no decals/debris/smoke; M02 only) and **no-build** (1 = buildings may not be placed: shores and rock edges), `u32` building handle (0 in files). Header `a,b` = player 0's initial scroll position (top-left pixel, e.g. M01 1341,1560). Sizes seen: 64x64, 96x80.
- `.tmi`: `u16 w,h` + `w*h` u16 tile indices (0 = none) into `.til`. Max index = frame count - 1 (M01: 1030 frames).
- `.til`: identical framing to sprites, 32x32 frames, parsed with `Kind::Shadow` (rows are 3 words each in M01: one opaque run => likely full masks;
  pixels come from the `.srf` background). Role: the foreground mask, docs/art.md; the viewer draws it by default.
- `.edt` / `.net` (decoded, `level.rs`): 376-byte header (94 i32), then 76-byte objects (tag 123), then 20-byte markers (tag 224), then footer 7656.
  All 31 files parse to the last byte. Object words 16/17 are cell x/y (2131 objects, all inside their map, none on a blocked cell). Marker x/y are pixels.
  Object and marker kinds are decoded: see `docs/levels.md`.
  Header: word 1 = level id (campaign 1..22; .net ids 15,17,19 seen), words 2-3 = start credits per player (2500/5000, 3000/3000...).
  The exe reads only these header words (a scan of every absolute reference to the header block `0x517970`, word `w` at `0x517970 + 4*(w-2)`): 2/3 credits, 5+3g / 6+3g air stock of the player / enemy (g = bomber, asteroid, carryall, tripler, fighter; word 4+3g is never read), bytes of words 23-27 (turret tiers allowed: 5 per player, the other 5 unread), 31/32 Advancer strength, 34/35 no HQ Advancer replacement, 37/38 start research, 64 weather, 65/66/67 countdown. **Every other header word is ignored by the engine** (docs/ai.md table).

## Sprites (`.ps6`, `.tsp`, `.sha`)
- See `sprite.rs` module docs. 931 files verified (rows sum exactly to width, file ends with u32 0).
- `unit_blue/_gren` = team colours, `_boom` explosion, `_fire` muzzle flash, `.sha` ground shadow, `tur_*` turrets. Frames = phases x facings: tops 32 facings (e.g. `1A`, `1C` 32 frames, `2A` 64 = 2 recoil phases), walker tops 16 (`5W1` 16 frames), chassis 3 phases x 32 (`chA` 96); docs/art.md "Facing and frame selection".
- `.po1/.po2`: weapon fire points per facing: tables of 32 `(i16,i16)` pairs; see `docs/art.md` (decoded).

## Archive
- `sprites.info` + `sprites.data` (CMegaFile): 377 entries, names shifted +10; contents are `.ps6/.sha/.po1/.po2/.msk` under `units\` and `__extras\`.
  Same files also exist loose in `Data/units`; OpenSOW reads the archive (`opensow_viewer::art`, a language pack's own archive first).

## Fonts
See `docs/fonts.md` (`.msk` = coverage masks in sprite frame format; frame k = ASCII 32+k).

## generic.file (decoded)
`Data/generic.file` (111111 bytes, same in every language pack) is the game's lockstep **random table**: little-endian u16 words, `rand(n) = table[i] % n`, `i` advancing from 0 and wrapping after 0xc346 (`crates/opensow-formats/src/rng.rs`, exe-tested). Purely visual randomness (`0x437cd0`) uses a second table filled with the C runtime's `rand()` at start-up.
OpenSOW: `GameRng` (`rng.rs`) loaded from the install drives `World::random` (a xorshift stands in only when no table is loaded, in pure tests).
**Differs from the exe:** the visual randomness is a xorshift (`World::fx_rng`), not a `rand()`-filled table, so visual details (smoke jitter, sparks, hover phases) do not repeat the exe's sequence.

## Settings and language files (decoded)
`State of War_1.cfg` and `Savegame\player.cfg`: `config.rs`, docs/config.md. `lang.dat` (one ASCII digit, selects `Language/lang*.txt`): `lang.rs`, docs/text.md.
