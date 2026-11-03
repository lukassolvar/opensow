# Settings files

Code: `crates/opensow-formats/src/config.rs` (`opensow cfg <file>`); checks: `scripts/verify_save_layout.py` (the `Options` field order and header), `scripts/verify_config.py` (the exe's own `player.cfg` writer, 20 random level-state sets, decoded by `PassedLevels`). Both files are MFC `CArchive` streams (little-endian, no compression) that start with `u16 1, u16 0x1d6c`. The install ships neither: the game writes them on first run.

## `State of War_1.cfg` (`COptions`)
Written next to the exe (current directory); the file name is the string the constructor `0x452a90` stores at `+4`. One object per player lives at `0x6bf710 + 0x28 * player`; the file holds the one of the local player (`0x452c30` loads it after the video modes are known, `0x453310` saves it when the options menu closes). Stream after the header: `u32` detail (`+0xc`), scroll speed (`+0x14`), scroll speed (`+0x18`), width (`+0x1c`), height (`+0x20`), four bytes (bools `+8 +9 +0x10 +0x11`), `u32` language (`+0x24`).
- detail `0..=4` (the options menu's arrow buttons `0x452bc0/0x452c00`; the ring toggle at `0x44c16f` flips 0 <-> 2): after loading, `0x453060` trims or grows the cloud list `[0x49a3fc]` to 0, 13, 32, 45 or 70 objects.
- the two scroll speeds go `0..=10` through the `+/-` buttons (`0x452be0/0x452c20`, `0x452bd0/0x452c10`) and multiply the map scroll step (`0x457f80` on the map object, `0x416b8f` / `0x415ef4`).
- width/height: constructor defaults 1920 x 1080 (`0x780`, `0x438`); the options menu offers 640x480, 800x600, 1024x768, 1600x900 and 1920x1080.
- the four bools are the options menu's switches (`0x44c131`, `0x44c150`, `0x44c1cb`, `0x44c1e7`); the sound switches (music, effects, speech) and volumes (`0x6b3d88/8c/90`, +-5 per click) belong to the audio object and are not in this file.
- language id 0..=7 as in docs/text.md; it selects `data\diffdata\<code>\` for the localised level files.

## `Savegame\player.cfg` (`CPassedLevels`)
`0x456ac0` writes the campaign progress as a 313-byte block after the header, so it cannot be read at a glance (the savegame stream contains the same record, docs/savegame.md "Signature"): 79 random bytes; 20 random bytes followed by a balancing byte so that those 21 bytes add up to `0xac` (mod 256); then for each of 50 levels a state byte and a random byte; 113 random bytes. The state byte is `base + 0x86 * index` (mod 256) with `base` 0x17, 0x2d, 0x4c for the object's byte values 0, 1, 2 (the 50 bytes at `+4..+0x36`). The random bytes come from the visual generator `0x437cd0`. The loader (`0x456530`, `0x456720`, `0x456e50` on the load path) rejects a block whose checksum or state bytes do not fit. The game creates the `Savegame` folder if it is missing.
