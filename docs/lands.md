# The campaign map (`CMenuBriefing`)

"New game" does not list missions: it opens a world map where the player picks the next land. Class `CMenuBriefing`, state handlers
`0x433d00` (arg 0 = from New game), `0x433d90` (arg 1 = after a mission, via the statistics screen), `0x433e20` (arg 2 = show a land's
text directly); main states 24, 25 and 26 of the dispatcher `0x42d020` (`0x42d624`). Per-frame function `0x441210`, init `0x441c00(mode)`,
load of the assets `0x441d2d..0x441f9a`. OpenSOW: `crates/opensow-game/src/lands.rs` (states, timings, sounds and texts as below unless marked), progress rules `config::PassedLevels::{initial, pass, merge}` (exact).

## Assets (design resolution 640x480; hotspots are offsets from the screen centre: top-left = (320 - hot_x, 240 - hot_y))
- `Data\PICTURES\Briefing.srf` (640x480 map; `Briefing2.srf` is the 1920x1080 Steam art drawn behind it, see Layout), `Data\levels\mA.sgv`, `mB.sgv`.
- `Data\Sprites2\Menu\lands_blue.tsp` / `lands_red.tsp` (22 frames each = the 22 lands; the hotspots put each land at its place on the map),
  `lands_points.tsp` (22 small yellow dots, one per land), `lands_buttons.tsp` (0 ACCEPT, 1 BACK, 2/3 highlighted, 4/5 violet; ACCEPT at the
  bottom left (5, 434), BACK at the bottom right (542, 433)), `briefing.tsp` (40 frames of a four-bracket reticle that grows and shrinks).
- Fonts `font_briefing_big/small` (colour `.ps6`); the exe draws its text through GDI (`0x467060`: white with a black shadow); the bitmap faces only measure the wrap (see Text). Sounds `sounds\sfx\cinky\briefing-{location,type,type02,rect01,rect02,blik}.wav` (ids 41..46, see States).

## Progress (`CPassedLevels` object `0x71c38c`, bytes `+4+land`)
State per land 1..22: **0 locked** (red, "No data available"), **2 available** (blue, blinks when the 60 Hz tick modulo 60 is <= 30),
**1 passed** (blue). `0x456a6f` (no `player.cfg`): lands 1, 2 and 9 are 2. `Pass(level)` (`0x4565a0`, called by the statistics screen
`0x44d770` after a won campaign mission): level 22 goes to state 2 (the ending); otherwise a land at 2 becomes 1, and then if lands 1..20 are all
1 land 22 becomes 2, else the first land still 0 in the order **6, 4, 7, 3, 10, 11, 12, 13, 14, 15, 5, 16, 17, 18, 19, 8, 20, 21** becomes 2.
`0x456720` merges two records (1 beats 2 beats 0). Tested against the exe bytes (`campaign_progress_rules_match_the_exes_pass_function`).
Land L is mission `M<L>` (`0x42c320(L)` stores it at ACCEPT, then state 0xb starts the level).

## Animations (`mA`, `mB`) and music
All through the DirectShow player object `0x6b409c`; its open function `0x4573e0(name, x, y)` places the video at the rect (x, y, x+w, y+h) of the
window (left = `[ebp+0xc]`, top = `[ebp+0x10]`). **`mA`** (`0x441d2d`, mode 0 = New game) opens with (0, 0) = full screen, **`mB`**
(`0x441c7f`, `0x4413e6`, `0x44148a`, `0x441553`) with **x = 0x20, y = 1** (the earlier note's flags 0x20 were this x, the game used (32, 0) from
image matching). Before `mA` the screen's init (`0x441c00`, mode 0 at `0x441cf9`; mode 2 does the same at `0x441c4b` but goes straight to `mB` and state 4) picks the music with the visual random generator (`0x437cd0(2)`: 0 -> `brief2.wav`, else
`brief.wav`, played by the music player `0x407bb0` and kept up on the map; mode 1 starts no music and no animation). Frame loop `0x441210`: state 0 runs
until `0x457220` reports the end of `mA` (or a key/click), state 1 releases `mA`, opens `mB` and goes to state 3 (New game) or 4; while `mB` is drawn
and the flag `[0x49cdc4]` is set the player checks `0x457220` each frame and, when `mB` has ended, closes and reopens it (that is the loop).
**Differs from the exe:** the music pick uses a clock-based `music::pick(2)` instead of the visual random generator (same 50/50 choice, different sequence); the movies are decoded by OpenSOW (`video.rs`, docs/video.md) instead of DirectShow.

## Layout at any screen size (`0x441210`, `0x441700`, `0x43ebe0`)
The exe is not a 640x480 screen scaled up: with `W` x `H` the window, `Briefing2.srf` (1920x1080, the globe art) is blitted over the whole
screen (1:1 and centred: a smaller screen shows the middle crop, `0x4412ab..0x441314`), then `Briefing.srf` (640x480, opaque) **1:1** at
`(W/2-320, H/2-240)`; lands, points, buttons and boxes are positioned from the centre `(W/2, H/2)`, the two heading lines at the absolute
window y 81 and 101, centred on `W/2`. All 22 lands **and all 22 yellow points** are baked into the picture at init (`0x441f06`: land, then
its `lands_points` frame, blue when the land is open/passed, red when locked); per frame the hovered land and a blinking open one are drawn
again additively (`0x45cde0`). `mB` plays at the window position (32, 1).
**Differs from the exe:** a window larger than the art or taller than 560 rows is drawn at natural size and zoomed uniformly by `max(w / 1920, h / 560)` (`Lands::natural`, `ZOOM_ROWS`) so the text stays readable; the exe draws everything 1:1.
**Differs from the exe:** the port draws `mB` only on an unscaled screen smaller than 1920x1080; at 1920x1080 and above (and when zoomed) it is hidden because the art's own globe shows there (the Steam screenshots show no small globe there); the exe always plays it.

## Text (GDI, not the bitmap fonts)
Headings and briefing text are drawn by `0x467060` (white, black shadow +2,+2) with font `[0x5ddf30]` = `CreateFontA(13, 6, 0,0, 400, ...,
"Times New Roman")` (`0x422740`): cell height 13, average width 6 (GDI stretches the face; the screenshots show a heavy, wide serif).
**Differs from the exe:** `gdi.rs` stands in with the system's Times New Roman Bold stretched by 1.23 (the game cannot ship the font; no font = bitmap fallback), drawn after the zoom so it stays sharp. Wrapping is the same as the exe's.
**Wrapping** (English, `[0x6bf734] = 0` -> `0x440f70`) measures with the *bitmap* face `font_briefing_small` (font 11: `0x45ed50(text, 1, 0xb)`),
a word joins the line while the line is `< 400` px (`panel width 0x1c2 - [0x48f400] = 50`); other languages use `0x440de0` (GDI extents).

## States (`[this+4]`, jump table `0x4416e8`)
0. `mA` plays (`0x457220` false when done), 1. the screen is filled black, `mA` released, `mB` opened looping, then to 3 (New game) or 4 (modes 1/2).
3. **Map** (`0x441700`): the topmost land whose pixel is under the mouse (pixel-exact, `0x45f3d0`) is highlighted and its two heading lines
   **type in**: `[+0x338]` (ticks since the hovered land changed) / 2 characters of line one, then what is left for line two (each shown
   character costs 2 ticks = 30 chars/s); while either line is unfinished `briefing-type02` (43) plays every 4 ticks (`[+0x380] += frame ticks`,
   capped at 4 before adding). No hover sound. BACK highlights on hover and returns (state 0x1c). A click on an open land (state 1/2) selects it.
4. **Selected** (`0x43ebe0`), every number below is a count `c` of 60 Hz ticks since the click, consumed in order:
   - the first frame plays `briefing-blik` (46) and `briefing-location` (41), once;
   - **reticle**: `briefing.tsp` frame `min(c/2, 39)` drawn **twice, additively** at the point (top-left of the point sprite + (3, 2), the frame's
     own hot spot); it consumes `2*frame` ticks, so ACCEPT/BACK only appear (and only work) once `c - 78 > 2` (about 1.35 s);
   - **description box** (`briefing-rect01` 44 once): grows from the point by `8*n*s` px horizontally and `4*n*s` vertically (`n = c - 2`,
     `s = W / 640` integer) up to left `W/2-0xe3`, top `H/2-0x58`, right `W/2+0xd7`, bottom `H/2 - 20*lines + 0xb1` (`lines` = number of wrapped
     objectives lines; see Open); it consumes `max(|dx|>>3, |dy|>>2)` more ticks; its inside is darkened by 6 RGB565 steps
     (table `0x73ced8`, `0x460a00`) and its four edges are **additive green lines across the whole screen** (`0x465c60`: three lines per
     edge, RGB565 `0x0200`, `0x03e0`, `0x0200`);
   - **description text**: lines at `x = left + 20`, `y = top + 11 + 20*i`, `Left(c/2)` per line in turn, 2 ticks per character, **silent**;
   - a pause of 15 ticks, then the **objectives box** (`briefing-rect02` 45): bottom `H/2 + 0xe3`, top `H/2 - 20*lines + 0xcf`, same
     left/right; it opens from its vertical middle by `2n` px (`n = c - 15`), consumes `max(|mid - top|, |mid - bottom|)` ticks, only its top
     and bottom edges are lines (top `0x0200/0x03e0`, bottom brighter `0x0400/0x0620`);
   - **objectives text** at `x = left + 20`, `y = top + 11 + 20*i`, same typing; while it types `briefing-type` (42) plays every 4 ticks.
   The headings stay fully drawn. BACK (-> 3) and ACCEPT (starts the mission: `0x42c320(land)`).

## Texts (`langXX.txt`, 0-based vector index = the exe's index - 1)
Land L (1..22): heading `30 + L` ("TERRITORY : nn   DIFFICULTY : d from 10"; the territory numbers are not in map order), `53 + L`
("Coordinate: ... Battle ground: ..."), briefing `75 + L`, objectives `97 + L`; a locked land shows string 53 ("No data available").
(The exe's table for each land pushes indices 0x20+L-1, 0x37+L-1, 0x4d+L-1, 0x63+L-1; `0x43f8b0`, jump table `0x440d84`.)

## In the game
New game plays `mA` (any key or click skips it), then the map. ACCEPT plays the mission's briefing movie and starts it; winning passes the
land and the map returns; BACK/Escape goes to the main menu. Headless: `--screen lands [--land N --age SECONDS] [--hover N]
[--unlock-all]`.

**Differs from the exe:** progress is written to `player.cfg` (Warmonger `player2.cfg`) in OpenSOW's own save folder after every change (`Store::save_progress`, docs/saveload.md); the exe writes `Savegame\player.cfg` of the install, which OpenSOW only reads as a fallback.
**Differs from the exe:** the two campaign labels STATE OF WAR | WARMONGER at the top switch the campaign, and the Warmonger map is tinted red (not in the exe; docs/warmonger.md).
**Differs from the exe:** developer shortcuts (not in the exe): `L` on the main menu opens the plain list of all missions, the dev tools' UNLOCK ALL opens every land without touching the saved progress (`DevTools::unlock_all`).

## Open
**Differs from the exe:** click-to-skip while the briefing animates: the port finishes it at once (silently) on a click that is not on ACCEPT/BACK (added on request). Not found in the exe: no code of `0x43ebe0`/`0x441210`/`0x441700` reads the click for it and nothing writes `[+0x338]` besides the frame add, so the claim that the original does it is unverified.
**Differs from the exe:** `[0x420b70]() == 3` (the current mission) lowers the description box's bottom by 9 px (`0xc0` instead of `0xb1`, `0x440d1a`): not ported.
The exact GDI face (the port uses Times New Roman Bold, 1.23x wider), what
modes 1 and 2 show exactly (one-click briefing view), the ending after mission 22 (state 2 = `0x423d80`; whether and where `mO` plays).
