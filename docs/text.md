# Text, languages and localisation

Code: `crates/opensow-formats/src/lang.rs` (`Language`, `LangFile`, `read_lang_dat`), CLI `opensow lang <code> [index]`,
test `tests/real_data.rs::language_files_parse_and_hungarian_is_the_damaged_one`.

## Selection and files
- `Data/lang.dat` = one ASCII digit, the language id (`'0'` in the install). Ids (jump table `0x466f20`, same order as the in-game menu strings
  ENGLISH, SLOVAK, CESKY, POLSKY, MADARSKY, RUSKY, CHINESSE_S, CHINESSE_T): 0 EN, 1 SK, 2 CZ, 3 PL, 4 HU, 5 RU, 6 CS (simplified), 7 CT (traditional).
- `Data/Language/lang{EN,SK,CZ,PL,HU,RU,CS,CT}.txt`: UTF-8 with BOM, CRLF, strings separated by `@` (`strtok` with `"@"`, loader around `0x466bc0`).
  364 strings in every file, same indices in every language (0 = "New Game", 1 = "Load Game", ...). Some strings contain CRLF (multi-line briefings).
- Strings are plain text, so they can contain `%`-style or other placeholders; none of that was traced.

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

## Not decoded
String-index meanings (menu items, level names, briefings: for example the `TERRITORY : nn DIFFICULTY` and `Coordinate/Battle ground` lines are shuffled relative to level order), placeholder syntax, how the BOM shifts indices in the exe, and the bitmap-font behaviour for characters >= 0x80.


## Message events (`0x402ec0`, decoded)
`0x402ec0(id, x, y, player)` is the "speech event" function (one audio object per player, `0x6bf6b0 + player*0x20`; only the local player's speaks). Its jump table `0x4034ac` maps event ids 1..=24 to the message strings 121-144 (`formats::messages::MESSAGES`, exe-tested): 1 Building captured, 2 Turret destroyed, 3 Building lost, 4 Building upgraded, 5 No space for units, 6 New airstrike ready, 7 New Turret available, 8 New tech-level available (string 144), 9 Advancer destroyed, 10 Advancer disengaged, 11 5000 credits arrived, 12 5000 research points arrived, 13 Reinforcements have arrived, 14-17 20/10/5/2 minutes to reinforcement, 18-22 Bomber/CarryAll/Asteroid/Fighter/Tripler ready, 23 Turret repaired, 24 Building repaired. The speech file is the wav with the same English name in `Data/Sounds/Speech` (`01-renforcement.wav`, `06-building-captured.wav`, ...); `01-game-saved`, `01-mis-complet`, `01-mis-failed` and `05-airforce-destroyed` belong to the save, end-of-mission and aircraft-loss events. Messages 14-17 fire when the countdown shows minutes 20/10/5/2 and seconds 1; 7 and 8 when credits cross a turret price or research reaches 500; 4 and 23/24 at the end of an Advancer upgrade/repair; 9 when an Advancer dies.
