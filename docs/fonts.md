# Fonts

Code: `crates/opensow-formats/src/font.rs` (`Font`), CLI `opensow text`, test `tests/real_data.rs::fonts_follow_the_ascii_minus_32_layout_and_masks_nest`.

## Files (all inside `sprites.data` under `extras\fonts\`)
- `{small,medium,big,msg}_i.msk` and `_o.msk`: coverage masks in the sprite frame format (`Kind::Shadow`, same as `.sha`). `_i` = inner (fill), `_o` = outline. 94 glyphs each. `info.msk` (94 glyphs) is a fifth mask font used by the info box (not traced).
- `font02_dark.ps6`, `font02_light.ps6`, `font_msg2.ps6` (94 glyphs) and `font_briefing_big.ps6`, `font_briefing_small.ps6` (95 glyphs): pre-coloured fonts. `font02_*` and `font_msg2` have no true lowercase (lowercase shows as small capitals).

## Character mapping and layout (from the exe: width `0x45ed50`, string draw `0x461ac0`)
- Glyph table = `font*100 + char` from a base 32 slots before the first glyph, so **frame k = character 32+k**: frame 0 is space, frame 33 is `A`, frame 65 is `a`, frame 93 is `}`. Characters outside 32..=125 (including all accented letters) have no glyph.
- `' '` advances 4 px and draws nothing. Other characters advance by `glyph width + spacing`; a missing glyph advances by `spacing - 1`. `|` is drawn 2 px to the right.
- Frames for ` `, `!`, `#`, `@`, `` ` `` are all the same "missing character" shape (a bar with a base) in the mask fonts.
- Vertical: glyph top = line top `- hot_y` (hot_y is a negative bearing, so capitals and lowercase share a baseline; round capitals overshoot by 1 px).
- Outline: `_o` glyphs are larger (big `A`: inner 17x16 hot (-2,-2), outer 21x20 hot (0,0)). They nest in each other when both masks are placed at `pen - hot` in x and y with the same pen (the inner font's advances). Verified for A-Z, a-z, 0-9 in all four sizes. The exe's x arithmetic nets to `pen` (it adds hot_x then subtracts it), so how it offsets the outline pass is not traced; the nesting rule above renders correctly.
- Draw order: outline mask first in the outline colour, inner mask over it in the fill colour.

## Bytes >= 0x80 and the font ids
The exe loads ten fonts into one table, 100 slots each, glyph k of font f at slot `f*100 + 32 + k` (loader `0x425af1..0x425c3e`, `0x45dab0(name, array, 100)`): ids 0 small_i, 1 small_o, 2 medium_i, 3 medium_o, 4 big_i, 5 big_o, 6 msg_i, 7 msg_o, 8 font02_dark, 9 font02_light. Characters are read with `movsx` (signed), so a byte >= 0x80 indexes **backwards** into the earlier fonts: `glyph_of_byte(2, 0xff)` is font 1's `c`, and font 0 reaches before its table. Only 9 call sites draw bitmap text at all (docs/text.md).

## Open questions
- Localisation: resolved in `docs/text.md`. Accented and Chinese text goes through a GDI path (UTF-8 -> `TextOutW`), not these bitmap fonts; the language packs under `Data/DiffData` ship the same 94-glyph fonts.
- Colour fonts use a different hotspot convention than the masks (their glyphs clip if placed with `-hot_y`); the CLI sizes the canvas from glyph extents.
- Letter spacing is an argument of every call (the bitmap string draw is `0x461a30(x, y, text, ..., spacing at [ebp+0x14], font id at [ebp+0x20], blit mode at [ebp+0x30])`, the width `0x45ed50(text, spacing, font)`), so there is no default. Measured from the push arguments of the call sites: the main menu's version string uses font 2 (`medium_i`) with spacing **2** (`0x4458e3`), the statistics values font 10 (`font_briefing_big`) with **-2** (`0x44d4d6`), the FPS counter -3 and 1 (`0x43036b`, `0x4303ac`), the briefing wrap measure `0x45ed50(text, 1, 0xb)`, and the save page's title limit `0x45ed50(text, -4, 9)` (`font02_light`). Negative values are normal for the wide pre-coloured faces.
