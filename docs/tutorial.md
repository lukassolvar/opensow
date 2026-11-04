# The tutorial script (mission M00), decoded

Code: `crates/opensow-game/src/sim/tutorial.rs` (`World::step_tutorial`), tests `the_tutorial_*` in `sim.rs`.

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
are disabled there. In OpenSOW: `Action::Tutorial` (`menu.rs`/`main.rs`) starts `M00` directly (campaign flag off), and the
script's final `Victory` outcome goes straight to `Action::ToMain`.

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
- Not ported: the advisor's face animation (`0x70f308`) and the original panel art/position (`0x456470` places the text
  widget at x -0x50 and a height that grows with the number of lines, the arrow at (0x12d, -0x2d)); OpenSOW draws a plain panel
  at the bottom left with a ">" button. The key flags are the scroll keys (arrows/WASD).

## Open
- The advisor's face animation, the original panel art and the voice timing are not ported (above); the intro of the tutorial level
  (M00 has no `.sgv`) is not applicable. Whether the exe reloads `player.cfg` settings in `0x452c30` for a tutorial in a way that alters the
  gameplay is not verified (OpenSOW keeps its in-memory settings).
