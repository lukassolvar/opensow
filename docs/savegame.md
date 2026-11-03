# Savegames

Code: `crates/opensow-formats/src/save.rs` (`opensow save <file>`). Read from the exe: save function `0x428bb0`, state serializer `0x42a400` (store and load in one function, MFC `Serialize` style), info writer/reader `0x42be20`, compressor `0x439240`, decompressor `0x439860` (class `CHuffcomp`, the only use found). The install's `Savegame` folder ships empty, so there are no sample files: the codec is tested by round trip plus a hand-built stream, the constants by `savegame_constants_match_the_exes_save_code`.

## Files
`Savegame\savegame_<N>.data` and `.info` (N from the save slot). Saving first writes the state to the temp file `savegame\t`, compresses it into `.data`, deletes the temp file, then writes `.info`; afterwards it plays `01-game-saved.wav`. It needs 2 MB of free space ("Error: you need more that 2MB free space"). Everything is little-endian MFC `CArchive` data (no alignment).

## `.info` (the load menu's entry)
`u16 2, u16 0x1e5`, `u32` time stamp (`[this+0x94]`), four MFC strings (title `[+0x84]`, `%X` time, `%b %d, %Y` date, `%a %H:%M`), `u32 [0x517aec]`, then a 160x120 RGB565 screenshot (the u16 range `0x71065c..0x719c5c` of the exe's memory). MFC string = `u8 len` (or `0xff, u16`, or `0xff, 0xffff, u32`) + ANSI bytes.

## `.data` (compressed state)
Huffman with the exe's own layout: `u32` uncompressed length, `u16` root index, nodes from the root down to 0 (`u8` symbol, `u16` left, `u16` right; a leaf has both `0xffff`), then the codes as `u32` words read from bit 31 down (0 = left, 1 = right). Decoding stops after `length` bytes. `save::huff_decode`; `huff_encode` produces valid but not byte-identical streams (the exe's tie-breaking is not reproduced).

## Decompressed state stream (`0x42a400`)
`u16 1, u16 0x5a10`, then in order (load reads the same order and checks the tags):
1. scalars: `u32 [0x49a33c]` (level: the loader calls `0x4267d0` with it first, so the map and objects come from the level, then the saved state overwrites them), `u32 [0x71c0f4]` (lockstep RNG index, `rng.rs`), `u32 [0x5baafc]`, `u8 [0x49a339]`, `u32` x4 `[0x5baad8] [0x5baadc] [0x5baac4] [0x5baac8]`, `u8 [0x5baa99]`, `u32 [0x89cf50]` (`save::StateHead`).
2. three players (index i): `u32 [0x517af8+4i]`, `u32 [0x517b04+4i]`, `u32 [0x5baaa0+4i]`, then byte arrays of 1000 (`0x5bb910`), 300 (`0x5bc4c8`), 500 (`0x5bc850`) and 30 (`0x5bce30`) bytes per player.
3. three records of two counted lists of `u32` (`0x6bf90c/0x6bf910` and `0x6bf948/0x6bf94c`, stride 0x14).
4. object serializers (virtual `Serialize`, not decoded): `0x459960` on `0x6bf980`; `0x455700` x3 (stride `0xc68` from `0x70ba50`, the players' state); `0x419100` x3 (`0x338`, `0x6bed08`); `0x465220` x3 (`0x80`, `0x6bf788`); `0x456ac0` on `0x71c38c`. Then tag `"P2v3"` (MFC string).
5. `0x403b20` on the speech objects `0x6bf6b0 + 0x20*i` (i up to `[0x575720]` inclusive), tag `"aS2n"`.
6. the decal flag grid `0x513878` as 128x128 bytes (x*128+y, 0/1), then the units: for each of 3 players a `u32` count (`0x575734+4p`) and 1000 slots (`0x5b62c0 + 0xfa0*p`), each slot a byte 0/1 followed, when 1, by the object's own `Serialize` (vtable +8). Tag `"Ua63"` (the exe's error text calls this "unit loading").
7. buildings the same way: counts `0x575740+4p`, 300 slots at `0x5bab00 + 0x4b0*p`; tag `"B17u"` ("building loading").
8. 500-slot objects (counts `0x57574c+4p`, `0x5b91a0 + 0x7d0*p`, probably bullets) and 30-slot objects (counts `0x575758+4p`, `0x5ba910 + 0x78*p`), same format, no tag.
9. two counted object lists: `[0x49a3d8]` items from `[0x49a3d4]`, `[0x49a400]` items from `[0x49a3fc]`, each item `Serialize`d; tag `"Checksum"` (checked on load; nothing follows it), then `0x437d10` runs.

A mismatching tag shows "The savegame is corrupted" (with "(panel2 loading)" for `P2v3`, "(asituation loading)" for `aS2n`, "(unit loading)", "(building loading)").

## Object records (`saveobj.rs`)
Every record is `u16 version, u16 magic` plus fields in the order of `Class::scalars()` (offset in the in-memory object, `u32`, or a one-byte bool), then class-specific tails. Tables are checked against the exe by `scripts/verify_save_layout.py` (all 11 classes: header magic and the scalar order). Versions: chassis 2, bullet 3, the rest 1 (read from the code; not part of the script check). `opensow savelayout` prints the tables.

| class | Serialize | magic | tail after the scalars |
|---|---|---|---|
| Unit | `0x46caf0` | `0x76be` | bools `+0x48..0x4a`; then for `+4` (Top) and `+8` (Chassis): a presence byte and the record |
| Top | `0x47b5e0` | `0x393d` | 10 `u32` from `+0x94`; 20 x 4 `u32` from `+0xcc` (stride 0x10); the Weapon record of `+8` |
| Chassis | `0x4709a0` | `0x3462` | Order record of `+0xd8` (the current order); `u32 [+0xcc]` queued Orders; `u32 [+0x8c]` pairs (`[+0x88]`, the route); `u32 [+0xa0]` pairs (`[+0x9c]`, the local route); for kind `[+0xc] == 0x28` (Advancer) a Weapon record of `+0x104` |
| Order | `0x478f80` | `0x1685` | none: `+8..+0x18` then bool `+4` (type, x, y, two extra words, valid) |
| Weapon | `0x47c700` | `0xb377` | none (`+8..+0x38`, `+0x40`, `+0x44`, bool `+4`) |
| Building | `0x40b160` | `0xebb9` | 3 x (bool `+0x88+k`, `u32 +0x7c+4k`); 25 `u32` from `+0x98`; 5 BuildingPart records (`+0x15c`, stride 0x28); 8 pairs (`+0x224+4i`, `+0x244+4i`); 10 pairs (`+0x26c+4i`, `+0x294+4i`); `u32 [+0x394]` n, then n x 5 `u32` from `+0x2cc` (stride 0x14) |
| BuildingPart | `0x40e9f0` | `0x11b7` | none (9 `u32`) |
| Bullet | `0x412a70` | `0x5d2a` | 15 pairs (`+0xc+4i`, `+0x48+4i`); `[+0x2648]/2` `u32` from `+0xc4`; the `u32` path tail `[+0x2644]..[+0x2648]` from `+0xd44`; 9 `u32` `+0x2698..+0x26b4,+0x26bc`; bools `+0x26b8`, `+0x26c0` |
| Aircraft | `0x468c30` | `0xa2e2` | 5 bools from `+0x8c` |
| Decal | `0x41a1f0` | `0x57a9` | none (x, y, frame, ...) |
| Cloud | `0x414820` | `0x500c` | none (3 `u32`, the `[0x49a3fc]` list) |

Slots: units are Unit records (0xa0 bytes), 300-slot objects Buildings, 500-slot Bullets, 30-slot Aircraft (`0xb4` bytes, `0x4671a0`), list `[0x49a3d4]` Decals, `[0x49a400]`'s Clouds.

## Global serializers (step 4 and 5, `save::State`)
`save::State::parse(data, speech_objects)` walks a whole decompressed stream (synthetic-tested; there is no real save to try). `speech_objects` is `[0x575720] + 1`, a run-time global that is not saved (the exe sets it to 0 or 1).
- **Map object** (`0x459960`, `MapState`, no header words): MFC string (the level path, `0x71cec8`); `u32 [+0x20]`, `[+0x2c]`, width `[+0x40]`, height `[+0x44]`; for each of height x width cells (outer loop over the height counter, cell index `j*128+i`): `u16` tile (`+0xcc`), `u32` (`+0x170cc`), `u32` (`+0x180cc`), 3 bytes (`+0x280cc`, `+0x2c0cc`, `+0x300cc`), `u32` (`+0x340cc`); then the constants `u8 1`, `u32 3`; three player records (`bool +4+k` and six `u32`: `+8+4k, +0x14+4k, +0x54+4k, +0x48+4k, +0x60+4k, +0x6c+4k`); `u32` n and n MapItem records (`+0xbc` array, count `+0xc0`). MapItem (`0x43e480`, magic `0xab30`): `u32 +0xc` kind; kind 0: `u32 +4,+8,+0x10,+0x14,+0x18` and bools `+0x1c,+0x1d`; kind 1: `u32 +4,+8,+0x10`; other kinds nothing. The cell layers' meaning (tile, overlay, owner, ...) is unknown; the map-object tables were read by hand (the layout script covers the MapItem prefix only).
- **PlayerState** (`0x455700`, `0x70ba50`, stride `0xc68`, magic `0x41c2` v2): `u32 +0x38,+0x3c,+0x40`, then 10 `u32` from `+0x10`.
- **Stock** (`0x419100`, `0x6bed08`, stride `0x338`, `0x7b33`): `u32 +0x2dc,+0x32c,+0x330,+0x2e0`, 9 pairs (`+0x2e4+8i`, `+0x2e8+8i`), then ten lists (stride `0x14` from `+0x218`; pointer `+0`, count `+4`): count and items.
- **Params** (`0x465220`, `0x6bf788`, stride `0x80`, `0x18c7`): 30 `u32` from `+4`, `u32 +0x7c`.
- **Signature = campaign progress** (`0x456ac0`, the `CPassedLevels` object `0x71c38c`, magic `0x1d6c`): written after the three Params as 313 obfuscated bytes carrying the 50 level states; decoded by `config::PassedLevels` (docs/config.md).
- **Speech queue** (`0x403b20`, `0x6bf6b0 + 0x20*i`, `0xf8a8`): `u32 +4,+8`, count `+0x14`, then SpeechItem records (`0xd8db`: MFC string, `u32 +8,+0xc,+0x10,+0x14`).
- Remaining sections as listed above; the decal flags are 128x128 bytes, the slot groups `u32` count + presence byte and record per slot (1000 units, 300 buildings, 500 bullets, 30 squads per player), then decal and cloud lists.

## Not decoded
What most scalars mean (field names exist only for the chassis kind `+0xc`, orders, bullet path fields), the map cell layers and whether `[0x517aec]` is a tick counter. The post-load fixups (`0x458230..0x453060`) are unread. Network messages (`CNet_*`) are separate.
