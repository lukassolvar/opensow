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

## Open questions
- Localisation: resolved in `docs/text.md`. Accented and Chinese text goes through a GDI path (UTF-8 -> `TextOutW`), not these bitmap fonts; the language packs under `Data/DiffData` ship the same 94-glyph fonts.
- Colour fonts use a different hotspot convention than the masks (their glyphs clip if placed with `-hot_y`); the CLI sizes the canvas from glyph extents.
- Default letter spacing used by the game's menus is unknown (the CLI uses 1).
