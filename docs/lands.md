# The campaign map (`CMenuBriefing`)

"New game" does not list missions: it opens a world map where the player picks the next land. Class `CMenuBriefing`, state handlers
`0x433d00` (arg 0 = from New game), `0x433d90` (arg 1 = after a mission, via the statistics screen), `0x433e20` (arg 2 = show a land's
text directly); main states 24, 25 and 26 of the dispatcher `0x42d020` (`0x42d624`). Per-frame function `0x441210`, init `0x441c00(mode)`,
load of the assets `0x441d2d..0x441f9a`. Code: `crates/opensow-game/src/lands.rs`, progress rules `config::PassedLevels::{initial, pass, merge}`.

## Assets (design resolution 640x480; hotspots are offsets from the screen centre: top-left = (320 - hot_x, 240 - hot_y))
- `Data\PICTURES\Briefing.srf` (640x480 map; `Briefing2.srf` is the 1920x1080 Steam art, not used by the layout), `Data\levels\mA.sgv`, `mB.sgv`.
- `Data\Sprites2\Menu\lands_blue.tsp` / `lands_red.tsp` (22 frames each = the 22 lands; the hotspots put each land at its place on the map),
  `lands_points.tsp` (22 small yellow dots, one per land), `lands_buttons.tsp` (0 ACCEPT, 1 BACK, 2/3 highlighted, 4/5 violet; ACCEPT at the
  bottom left (5, 434), BACK at the bottom right (542, 433)), `briefing.tsp` (40 frames of a four-bracket reticle that grows and shrinks).
- Fonts `font_briefing_big/small` (colour `.ps6`); the exe draws its text through GDI (`0x467060`: white with a black shadow), so the game's
  look differs a little. Sounds `sounds\sfx\cinky\briefing-{location,type,type02,rect01,rect02,blik}.wav` (the exe plays them on hover,
  typing and the frame; the game does not yet).

## Progress (`CPassedLevels` object `0x71c38c`, bytes `+4+land`)
State per land 1..22: **0 locked** (red, "No data available"), **2 available** (blue, blinks when the 60 Hz tick modulo 60 is <= 30),
**1 passed** (blue). `0x456a6f` (no `player.cfg`): lands 1, 2 and 9 are 2. `Pass(level)` (`0x4565a0`, called by the statistics screen
`0x44d770` after a won campaign mission): level 22 goes to state 2 (the ending); otherwise a land at 2 becomes 1, and then if lands 1..20 are all
1 land 22 becomes 2, else the first land still 0 in the order **6, 4, 7, 3, 10, 11, 12, 13, 14, 15, 5, 16, 17, 18, 19, 8, 20, 21** becomes 2.
`0x456720` merges two records (1 beats 2 beats 0). Tested against the exe bytes (`campaign_progress_rules_match_the_exes_pass_function`).
Land L is mission `M<L>` (`0x42c320(L)` stores it at ACCEPT, then state 0xb starts the level).

## States (`[this+4]`, jump table `0x4416e8`)
0. `mA` plays (`0x457220` false when done), 1. the screen is filled black, `mA` released, `mB` opened looping, then to 3 (New game) or 4 (modes 1/2).
3. **Map** (`0x441700`): every land is drawn from the blue (state 1/2) or red (0) frame; the topmost land whose pixel is under the mouse
   (pixel-exact, `0x45f3d0`) is highlighted and its two heading lines are typed at the top (y 81 and 101, centred); BACK (frame 1) highlights
   on hover and returns to the main menu (state 0x1c). A left click on a land at state 1 or 2 selects it (-> 4).
4. **Selected** (`0x43ebe0`): the land's dot is drawn, a rectangle grows from the dot to the panel (x 93..535, y 152..392: centre -0xe3, -0x58 ..
   +0xd7, +0x98; drawn with three 1-pixel lines, bright green `0x03e0` and dark green `0x0200`), the reticle frames cycle, and the briefing and
   objectives type into the panel (about 20 px inside the left edge, 11 below the top). BACK (-> 3) and ACCEPT (starts the mission:
   sound 0xb, `0x42c320(land)`).

## Texts (`langXX.txt`, 0-based vector index = the exe's index - 1)
Land L (1..22): heading `30 + L` ("TERRITORY : nn   DIFFICULTY : d from 10"; the territory numbers are not in map order), `53 + L`
("Coordinate: ... Battle ground: ..."), briefing `75 + L`, objectives `97 + L`; a locked land shows string 53 ("No data available").
(The exe's table for each land pushes indices 0x20+L-1, 0x37+L-1, 0x4d+L-1, 0x63+L-1; `0x43f8b0`, jump table `0x440d84`.)

## In the game
New game plays `mA` (any key or click skips it), then the map. ACCEPT plays the mission's briefing movie and starts it; winning passes the
land (progress is kept in memory only: the game never writes into the install's `Savegame` folder) and the map returns; BACK/Escape goes to the
main menu. `L` on the main menu still opens the plain list of all missions. Headless: `--screen lands [--land N --age SECONDS] [--hover N]
[--unlock-all]`.

## Open
Where the exe returns after a lost mission, the typing/frame timings and sounds, the statistics screen between mission and map, what
modes 1 and 2 show exactly (one-click briefing view), the ending after mission 22 (state 2 = `0x423d80`; whether and where `mO` plays).
