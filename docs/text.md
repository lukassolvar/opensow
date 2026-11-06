# Text, languages and localisation

Code: `crates/opensow-formats/src/lang.rs` (`Language`, `LangFile`, `read_lang_dat`), CLI `opensow lang <code> [index]`,
test `tests/real_data.rs::language_files_parse_and_hungarian_is_the_damaged_one`.

## Selection and files
- `Data/lang.dat` = one ASCII digit, the language id (`'0'` in the install). Ids (jump table `0x466f20`, same order as the in-game menu strings
  ENGLISH, SLOVAK, CESKY, POLSKY, MADARSKY, RUSKY, CHINESSE_S, CHINESSE_T): 0 EN, 1 SK, 2 CZ, 3 PL, 4 HU, 5 RU, 6 CS (simplified), 7 CT (traditional).
- `Data/Language/lang{EN,SK,CZ,PL,HU,RU,CS,CT}.txt`: UTF-8 with BOM, CRLF, strings separated by `@` (`strtok` with `"@"`, loader around `0x466bc0`).
  364 strings in every file, same indices in every language (0 = "New Game", 1 = "Load Game", ...). Some strings contain CRLF (multi-line briefings).
- Strings are plain text with C `%s`/`%d` placeholders (info box, see the table below).

## What ships
| id | file | state |
|---|---|---|
| EN | langEN | original text |
| SK, CZ | langSK/CZ | full translations (about 310 of 364 strings differ from English), UTF-8 with accents |
| PL, RU | langPL/RU | **English text** except 3 labels (no Polish/Russian translation in the Steam package) |
| HU | langHU | translation, but **100 strings are damaged**: accented letters became U+FFFD or half-width katakana from a bad re-encoding; the original characters are not recoverable from the file |
| CS, CT | langCS/CT | full Chinese translations |
`LangFile::damaged()` finds the broken strings.

## How text reaches the screen (two paths)
1. **GDI path** for menus/dialogs: helper `0x466f40` converts with `MultiByteToWideChar(CP_UTF8 = 0xFDE9)` and calls `TextOutW` twice, a shadow at (+2,+2) in one colour and the text in another
   (wrapper `0x467060` uses white text, black shadow). Fonts come from `CreateFontA`, e.g. 12 px "Times New Roman", weight 400 (`0x4225d7`; other call sites at `0x434749`, `0x434f4b`, `0x4353f2`, `0x435642`, `0x44c66a`). This is why accents and Chinese work in the Steam build.
2. **Bitmap path** for in-game text: `docs/fonts.md`; only ASCII 32..125 has glyphs, so non-ASCII bytes would index glyph slots of the wrong font.
Which strings use which path was not mapped; any port needs a Unicode-capable font renderer (e.g. a system font stack with CJK fallback) for path 1.

## String-index table (decoded)
**Numbering.** The exe's string vector is the `strtok` of the whole file, so token 0 is the 3-byte BOM and token 365 the trailing `\r\n`: the exe asks for `GetString(file index + 1)` ("New Game" = 1). `LangFile` skips both, so everything below uses **file indices** (0-based, `LangFile::get`); add 1 to compare with the disassembly. The getter is `0x466ae0(out CString*, index)` on the global object `0x6bed04` (which also owns the text-draw helpers `0x466f40`/`0x467060`/`0x4670e0`); of its 377 call sites 375 push an immediate index directly; the main menu uses a loop and `0x4321c0`, `0x44b320` (268) and `0x443ad0` (360) reach their string through a shared tail (`push imm; jmp tail`, found by following the jumps). Code: `formats::strings` (ranges, `object_name`, `weapon_name`), exe-tested by `string_groups_match_the_exes_name_switches`.

| file index | English | used by (exe function) |
|---|---|---|
| 0-5 | New Game, Load Game, Tutorial, Multiplayer, Options, Exit Game | main menu `0x445620`, loop `GetString(1+i)`, i < 6, rows 62 px apart; white when hovered, grey `0xb4` otherwise |
| 4 | Options (again) | options title `0x44b320` |
| 6-19 | GFX, ACCESSORY, SFX, Radar, Quake, Clouds, HWMOUSE, VBLANK, FX, MUSIC, VOICE, RESOLUTION, Keys scroll, Mouse scroll | options screen `0x44b320` (docs/options.md) |
| 20, 23-30 | Language; ENGLISH SLOVAK CESKY POLSKY MADARSKY RUSKY CHINESSE_S CHINESSE_T | **not referenced** (no call with these indices; the Steam build has no language row in the options screen) |
| 21, 22 | OK, Cancel | options `0x44b320`, dialog `0x44be60`; Cancel also load/save `0x443ad0` |
| 31-52 | TERRITORY : nn DIFFICULTY : d from 10 | campaign map heading of land L = 30+L (territory numbers are not in map order), `0x43f8b0` |
| 53 | No data available | locked land |
| 54-75 | Coordinate: .. Battle ground: .. | land L = 53+L |
| 76-97 | briefing texts | land L = 75+L |
| 98-119 | Objectives: ... | land L = 97+L |
| 120-143 | Building captured ... Building repaired | speech events 1..24, `0x402ec0` (messages above) |
| 144 | speed | game-speed readout `speed %.1f` (`0x4031d9`, `[0x49ceec]`, same function) |
| 145 | SATELLITE PROTECTION | building hover/draw `0x409400` (docs/advancer-actions.md) |
| 146-171 | tutorial steps (`i ` = lesson heading, `! ` = task) | tutorial text box `0x4325b0` (docs/tutorial.md) |
| 172-227 | Radar, Light AntiAir .. Research Influx 3 | object name by kind: `0x431ad0` (switch below); kinds >= 1000 give 261 |
| 228-259 | weapon names (2x antiair rockets .. 4x AG bombs) | weapon by kind: `0x4321c0` via shared tail; 260 = "n/a" for kinds without one |
| 261 | increase tech-level for 500 | `0x431ad0` for kind >= 1000 (the tech-level button) |
| 262 | Selected level: | multiplayer level picker `0x447100` |
| 263, 264 | CREATE SESSION, SHOW ALL SESSIONS | multiplayer lobby `0x446ca0`; token 365 (`\r\n`) is also pushed there |
| 265, 266 | `2Green player has joined / left the game` | lobby event lines `0x4485e0` (the leading digit is a colour/team code, not translated) |
| 267, 268 | On, Off | options `0x44b320` (268 via a shared tail) |
| 269-272 | Empty Savegame..., No Time, No Date, savegame is damaged!!! | save slots `0x443ca0` |
| 273-276 | BLUE SIDE, GREEN SIDE, CREDITS, RESEARCH | multiplayer level picker `0x447100` |
| 277-279 | Session Lost, Green/Blue Player has left the game | in-game MP `0x434e80` |
| 280, 281 | Now increase tech-level in your factory. / Now use fighters ... island. | mission hint boxes `0x4303e0`, `0x430770` (docs/ai.md) |
| 282-301 | No Info Available; NAME/BLAST/SHELL: %s; TYPE: ...; OFFENCE/DEFENCE: %d, n/a, special, ultimate; SCOPE/SPEED: %s/%d; cargo: reinforcements | info box lines `0x43cf20` |
| 302-350 | unit/building description pages | info box page text `0x43c410` |
| 351 | Quick Save | `0x444660` |
| 352, 353 | ACCEPT, BACK | MP level picker `0x447100` |
| 354-358 | CAPTURED, LOST, PRODUCED, BUILT, SENT | statistics screen `0x44d500` |
| 359, 360, 363 | LOAD, SAVE, Save Game | load/save screen `0x443ad0` (359/360 via a shared tail) |
| 361 | P A U S E | pause banner `0x435330` |
| 362 | Remaining Time | mission clock `0x430d50` |

**Kind -> object name** (`0x431ad0`: kind >= 1000 -> 261, else `kind-1` through byte table `0x43214c` into jump table `0x432064`, other kinds have no name): 1-15 ground chassis 173..187 (light/medium/heavy AntiAir, Artillery, Panzer, Flamer, Special); 16-18 Gold Influx 1-3 (219-221), 22-24 Energy Influx (222-224), 19-21 Research Influx (225-227); 25-29 turrets Guard Cannon .. Defragmentator (195-199); 30-34 Bomber Attack 200, Meteor Storm 201 (kind 34), CarryAll 202 (31), Tripler Strafe 203 (33), Fighter Assault 204 (32); 40 Advancer 193; 41-45 walkers Kodiak .. Achilles 188-192; 46 Rogon Bomb 194; 100-108 Headquarters .. Air Navigator, Gold Mine, Research Lab, WindTrap (205-213, 108 = Walker Factory 209); 109-113 Top Sentry G10 .. G50 (214-218); 115 Radar 172. **Kind -> weapon** (`0x4321c0`, byte table `0x4323d0`, jump table `0x432348`): kinds 1-15 -> 228-242, 25-29 -> 250-254, 30 -> 255, 34 -> 256, 31 -> 257, 33 -> 258, 32 -> 259, 40 -> 248, 41-45 -> 243-247, 46 -> 249; everything else (influx levels, buildings) shows "n/a".

## GDI versus bitmap fonts (census of every call site)
The bitmap string drawer `0x461a30` (entry `0x461ac0`) has **nine** call sites in four functions only; everything else is GDI (`0x466f40` / `0x467060` / `0x4670e0`, about 110 sites):
- Bitmap: the FPS counter `0x4302e0` ("FPS: n", from the frame draw at `0x4209a4`), the editor overlay in the frame draw `0x41f690` (`[0x517ae9]`: "Editor ACTIVE" and object names by `0x431ad0`), the version string "Version %d.%d" of the main menu (`0x445620`) and the centred value texts of the statistics screen (`0x44d370`, 12 call sites in `0x44ce..0x44d1..`). So the in-game HUD text is **not** bitmap: counters/pills are sprite digits (`money`, `panel2nu`), every other HUD, hint, message, info box, menu and options text is GDI.
- GDI by function: `0x403680` messages, `0x409400` health bar / satellite label, `0x415490`, `0x41f690` (the frame draw's debug lines), `0x42fe40`, `0x4303e0` / `0x430770` mission hints, `0x430d50` countdown, `0x430fd0`, `0x434650` / `0x434e80` / `0x435330` / `0x435580` mission end texts, `0x43b1f0..0x43b3c0` info box, `0x43ebe0` campaign map, `0x441700`, `0x443500` / `0x443ad0` (load/save screens), `0x445620` main menu, `0x446a80` / `0x446ca0`, `0x447100` multiplayer menus, `0x44b320` options (26 sites), `0x44d500` statistics labels. The port's bitmap-font stand-ins for these are listed in the per-screen docs.
- Characters >= 0x80 in the bitmap fonts: the byte is sign-extended and indexes `font * 100 + c`, i.e. lands in the arrays of the earlier fonts (a UTF-8 byte of an accented letter draws a glyph of the previous font, another size); `font::glyph_of_byte` (docs/fonts.md). Not relevant for ASCII-only English strings; the non-English bitmap text of those few sites was not examined.
Placeholders are plain C `printf` (`%s`, `%d`) filled by `0x4856f8` (sprintf) in the info box; the `i `/`! ` tutorial prefixes pick the icon.

## Message events (`0x402ec0`, decoded)
`0x402ec0(id, x, y, player)` is the "speech event" function (one audio object per player, `0x6bf6b0 + player*0x20`; only the local player's speaks). Its jump table `0x4034ac` maps event ids 1..=24 to the message strings 121-144 (`formats::messages::MESSAGES`, exe-tested): 1 Building captured, 2 Turret destroyed, 3 Building lost, 4 Building upgraded, 5 No space for units, 6 New airstrike ready, 7 New Turret available, 8 New tech-level available (string 144), 9 Advancer destroyed, 10 Advancer disengaged, 11 5000 credits arrived, 12 5000 research points arrived, 13 Reinforcements have arrived, 14-17 20/10/5/2 minutes to reinforcement, 18-22 Bomber/CarryAll/Asteroid/Fighter/Tripler ready, 23 Turret repaired, 24 Building repaired. The speech file is the wav with the same English name in `Data/Sounds/Speech` (`01-renforcement.wav`, `06-building-captured.wav`, ...); `01-game-saved`, `01-mis-complet`, `01-mis-failed` and `05-airforce-destroyed` belong to the save, end-of-mission and aircraft-loss events. Messages 14-17 fire when the countdown shows minutes 20/10/5/2 and seconds 1; 7 and 8 when credits cross a turret price or research reaches 500; 4 and 23/24 at the end of an Advancer upgrade/repair; 9 when an Advancer dies.
