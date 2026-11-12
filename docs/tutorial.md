# The tutorial script (mission M00), decoded

OpenSOW: `crates/opensow-game/src/sim/tutorial.rs` (`World::step_tutorial`) runs the script below step for step (scenes, conditions, the `index + base` quirk, arrow and voice), the panel is `tutorial_panel.rs`; tests `the_tutorial_*` in `sim/tests.rs` and `tests.rs`. Differences are marked.

The script is the function `0x4325b0`, called once per frame while `[0x70f2cc]` (the step, 0..0x1b) is not -1 (`0x42da2b`). In the
tutorial the HQ win/lose rules (`0x421230`, guarded by `[0x70f2cc] == -1` at `0x42e306`) and the computer opponent
(`0x42dc0a`) are switched off, and production uses the fast rate (docs/production.md). Step 0 is the init block
(`0x432bf1`: instruction panel widgets), then the dispatcher `jmp [step*4 + 0x433be0]`.

## Entry and exit (main menu)
The main menu's click handler `0x445f40` dispatches the pressed button through the jump table `0x4461b0` (button -> case):
NEW GAME -> `0x42c260(0x19)`, LOAD -> `0x42c260(0x11)`, **TUTORIAL (button 2, case `0x446090`)**, MULTIPLAYER -> `0x42c260(0x15)`,
OPTIONS -> `0x42c260(0x13)`, EXIT -> `0x41a870`. The tutorial case runs `0x403580(2)` (an empty stub, `ret`), `0x44f030` (frees the
menu's button list), `0x422b00` (new-game reset: `[0x70f2cc] = -1`, network off, credits/research 0, `[0x49a339] = 1`, clears the
object tables), the level loader `0x41f140(0)` (**level 0 = M00**; there is no briefing movie, unlike `0x42c320(level)` + state 0xb for
campaign lands), the settings reload `0x452c30` and then the state machine call `0x42c260(5)`. State 5 (`0x42c2f6`) sets
`[0x70f2cc] = 0` (step 0, the script starts) and the game state to 7 (playing).
Exit: step 27 calls `0x42c260(0x1c)` = main menu (music "menu" restarts); no statistics/result screen, no win sound, nothing is
passed in the campaign progress. Pause menu Restart in the tutorial loads level 0 again and enters state 5 (docs/pause.md); Load and Save
are disabled there. In OpenSOW: `Action::Tutorial` (`menu.rs`/`game_flow.rs`) starts `M00` directly (campaign flag off), and the
script's final `Victory` outcome goes straight to `Action::ToMain`; the pause menu hides Load and Save (`ingame.rs`), F5 is not blocked, as in the exe.
**Differs from the exe:** there is no settings reload (`0x452c30` re-reads `State of War_1.cfg`); OpenSOW keeps its in-memory settings.

## Frame
1. **Arrow button** `[0x70baf4]`: while it waits for a click (pointer 0x7a + left click `[0x6bed13]`, sound 0x456410(5)) the
   whole function returns; the click hides it.
2. On the first frame of a step (`[0x70f2e2]`) the panel text `lang[146 + step]` (a leading `i` = information, `!` = task; step 2
   is string 148 "Use the right mouse button to scroll on the map") is word-wrapped to 300 px (`0x431900`) and the voice file
   `Data\Sounds\Tutorial\NN.wav` (`NN` = step, `%02d`) is played (a new voice cuts the old one).
3. The step's case runs with `al` = first-frame flag. **Entering a new step** (`0x433b5a`) clears the key flags `[0x70f2d8..e1]`,
   sets the first-frame flag and, except for steps **1 and 17**, raises the arrow button and plays sound 29. So every step
   but those two starts with an arrow click, tasks included; step 1 (the "Welcome" text, string 147) hands over to step 2 within
   the same frame, so it is never really read.
4. Cases that scan a list set `step = base + index` of the matching entry (index in the player's building / unit array);
   with the scenes below the index is 0 (buildings) and the Advancer's position (units).

## Steps
Scene helpers: *wipe* = every building (`0x43a640`, silently) and unit (`0x43a7e0`) of all three players is removed;
`unit(x, y, player, kind)` = `0x43a420`, `building(x, y, owner, kind)` = `0x439e30(x, y, owner, kind, 1, 0)` followed by the
slot list `0x40d870(kind0, max0, kind1, max1, ...)`; `camera(px, py)` = `0x4588c0`. Cells are map cells, kinds are the object ids
(40 Advancer, 7 light cannon, 100 HQ, 101 light factory, 102 medium factory, 104 air base, 105 gold mine, 106 research lab).

| step | text | case |
|---|---|---|
| 0 | init | -> 1 |
| 1 | 147 welcome | -> 2 (no arrow) |
| 2 | scroll with RMB | waits `[0x6bed20] == 1` (right-button scrolling) -> 3 |
| 3 | arrow keys | the four key flags `[0x6bee34, 0x6bee3c, 0x6bee37, 0x6bee39]` all seen -> 4 |
| 4 | Lesson A | -> 5 |
| 5 | select the Advancer | entry: `unit(9, 46, 0, 40)`, camera (0x120, 0x5c0). Then: the Advancer is selected, pointer 3 (go-to) and a left click, `[0x6bed34] == 0` -> `6 + index` |
| 6 | info | -> 7 |
| 7 | capture the light factory | entry: wipe, `building(8, 8, neutral, 101)` slots (7, 0), (4, 0); `unit(5, 9, 0, 40)`; camera (0x100, 0x100). Then: the player owns a building that is not an HQ -> `8 + index` |
| 8 | the factory produces units | a unit of the player that is not the Advancer exists -> `9 + index` of the last such unit |
| 9 | upgrade | a non-HQ building of the player with `[b+0x144] >= 2` active slots -> `10 + index` |
| 10 | repair | entry: wipe, `unit(44, 39, 0, 40)`, `building(46, 39, player, 101)` slots (7,0),(4,0) with hit points `max/2 - 100`, camera (0x500, 0x5c0). Then: a light factory at full hit points -> 11 |
| 11 | Lesson B | -> 12 |
| 12 | gold mine | entry: wipe, `unit(40, 6, 0, 40)`, enemy cannons `unit(39, 8, 1, 7)` facing 6 and `unit(40, 9, 1, 7)` facing 8, `building(44, 5, player, 105)` slots 16, 17, 18, camera (0x5c0, 0xa0), credits 120. Then: credits >= 200 -> 13 |
| 13 | build a turret | all units not of the player are dead (their `[+0x2c]` flag) -> 14 |
| 14 | Lesson C | -> 15 |
| 15 | air navigator | entry: wipe, credits of both players 0, `building(22, 51, player, 104)` slot (32, 0), `unit(27, 53, 0, 40)`, enemy cannons at (31,55), (32,55), (31,57), (32,57), camera (0x340, 0x6a0). Then: the air base is there and a fighter (`0x454a60(0x20)`) is in stock -> 16 |
| 16 | click the airstrike icon | `[0x6beef0] == 1` -> 17 |
| 17 | strike the enemy units (no arrow) | all foreign units dead -> 18 and the air force stock is cleared (`0x455670`) |
| 18 | Lesson D | -> 19 |
| 19 | medium factory | entry: wipe, research 400, `building(12, 13, neutral, 106)` slots 19, 20; `building(6, 6, player, 102)` slot (7, max level 1); `unit(5, 9, 0, 40)`; camera (0x100, 0x100). Next frame -> 20 |
| 20 | capture the research lab | the player owns a research lab -> `21 + index` |
| 21 | research | research >= 500 -> 22 |
| 22 | tech-level | the medium factory's slot 0 has level 1 -> `23 + index` |
| 23 | Lesson E | -> 24 |
| 24 | the HQ | entry: wipe, research 0, `unit(41, 17, 0, 40)`, `building(41, 17, player, 100)` (its Advancer set), `building(56, 18, enemy, 100)` at half hit points + 10, neutral light factories at (49, 20) and (56, 12), camera (0x640, 0x240). Then: the HQ built a replacement Advancer (`[0x70f2f9]`, set by `0x42e41d`) -> 25; no enemy HQ -> 26 |
| 25 | satellite protection | no enemy HQ -> 26 |
| 26 | "Excellent!" | -> 27 |
| 27 | end | `0x42c260(0x1c)`: the mission is won |

Notes
- Step 8 as coded jumps to `9 + index`: the Advancer is unit 0, so the first produced unit (index 1) sends the script to step
  10 and the upgrade lesson (step 9) is skipped; the same holds for the other `index + base` cases whenever the index is not 0.
  OpenSOW follows the code (indexes = position in the player's list).
- The panel (ported as `tutorial_panel.rs`, drawn by `Play::draw_tutorial_panel`; there is no advisor face, the `0x70f308` counters drive two spinning icons):
  - **Wrap** (`0x431900`, string `[0x70f358 + 4*step]`): the first character is the marker `i` (information) or `!` (task), the second a separator. Words (up to and including a space) are measured with bitmap font 2 `medium_i`, spacing 0 (`0x45ed50`), and a word that brings the line to 300 px or more starts the next line (at most nine lines). `[0x70f2e8]` = line breaks, `[0x70f2e4]` = widest line measured with spacing 2, plus 50.
  - **Draw** (`0x430fd0`, called every frame while the step is not -1, before the hint box): a box darkened through the `0x71ced0` table (`0x460a00`, the same table as the hint boxes) from x 31 to widest + 58 and from `bottom - 37 - 20 * breaks` to `bottom - 5`; the lines with the GDI font `[0x5ddf30]` (the campaign map's Times New Roman, white with the black +2 shadow, `0x467060`; **Differs from the exe:** OpenSOW uses the `gdi.rs` Times New Roman Bold stand-in, docs/lands.md) at x 48, 20 px apart, the last line's top 25 px above the bottom; the coin (`extras\tutorialgfx` frames 0-15 "i" or 32-47 "!", frame `[0x70f308]` = one every 6 ticks, 16 frames) with its hotspot at x 15, `bottom - 21 - 10 * breaks`.
  - **Arrow button** (widget `0x70ba94 + player * 0xc68`; frames 16-31, a spinning arrow): on a step's first frame it is put at x -80 (below the left edge), y = `bottom - 20 - 10 * breaks`, with the target x = widest + 80 (`[0x70f2e4] + 0x1e`) and speed 6; while it is up (`[0x70baf4]`) `0x456300` moves it 6 px per tick (Bresenham `0x4377e0`). Hit box x +-21, y +-19 (`0x456490`). Its frame is 0 unless the pointer (0x7a) is over it, then it spins one frame per 4 ticks. A click (`0x456410(5)`) plays sound 33 (`[+0x58] = 0x21`) and hides it.
  - **0x7a** is not a pointer image: it is the hit id the HUD hit test `0x454cf0` stores in `[hud+4]` when the arrow is up (`[hud+0xa4]`) and the pointer is inside its widget (`0x456350`); it is tested last, so the arrow wins over any HUD element under it, and the click handler (`0x432d16`) needs it. `Play::left_down` checks the arrow first for the same effect; the pointer stays the ordinary arrow.
  - **Which frame** (`0x4564b0`, the widget's dirty-rectangle flush through `0x466140` = BltFast of the sprite's rectangle): the animated frame `[+0x2c]` while hovered, the base frame `[+0x28]` (frame 16) otherwise, `[+0x30]` (frame 16) during the 5 click ticks. The call that draws the sprite into the back buffer was not found among the `0x45edf0` call sites.
  - **Differs from the exe:** (possibly) OpenSOW draws the arrow plain with its hotspot like the coin, since the exe's draw call is unknown.
  - **Differs from the exe:** the widget's hover-in / hover-out sounds `[+0x54]` / `[+0x50]` are never set for this global widget (0), which would play effect 0 (anti-air) when the pointer enters or leaves; not ported because nothing shows the original does audibly.
  - **Bytes**: the exe wraps byte by byte (`CString::GetAt`) and measures with `0x45ed50`, where a byte >= 0x80 indexes the glyph table backwards into fonts 0 and 1 (docs/fonts.md); `tutorial_panel::wrap` / `byte_width` do the same, so non-ASCII languages wrap like the original. Breaks only follow spaces, so UTF-8 sequences stay whole.
  - **Differs from the exe:** the panel is scaled by the HUD scale like the rest of OpenSOW's HUD (the exe draws it 1:1). Checked at 640x480 and 1920x1080.
## Open
- Voice: OpenSOW plays `Tutorial\NN.wav` at the same moment as the exe (the step's first frame, i.e. after the arrow click; `TutEvent::Step` -> `Sfx::say_in`), and a new voice cuts the old one; nothing about its timing is left open. The tutorial level has no intro movie (M00 has no `.sgv`).
- Whether the settings reload `0x452c30` for a tutorial alters the gameplay is not verified (see Entry and exit).
