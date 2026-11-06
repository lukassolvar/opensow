# In-game menu (`CMenuInGame`, the Esc menu)

Read from the exe: constructor `0x442870` (vtable `0x48f514`), open `0x442cf1`, draw `0x442920`, per-frame update and click handler `0x4429e0..0x442cd4` (jump table `0x442cd8`). Code: `crates/opensow-game/src/ingame.rs`. The game's state machine is `0x42c260(state)`: 7 = playing, `0xc` Load, `0xd` Save, `0xe` Options, `0x1b` restart, `0x1c` main menu, `0x13` options from the main menu.

## Art
`menu\ingame` in `sprites.data` (11 frames; frame `extra` `[480, 360, 240, 180]`, but the exe centres on the real screen, so the art keeps its native pixel size). Top-left of a frame = screen centre minus its `hot`.

| frame | what |
|---|---|
| 0 | the bronze oval (366x306) with all six captions already printed on it |
| 1..6 | the highlighted caption of item 0..5: CONTINUE, LOAD GAME, SAVE GAME, OPTIONS, RESTART, MAIN MENU (each over its place on the oval) |
| 7 / 8 | the red X ("no"), normal / highlighted, right of the centre |
| 9 / 10 | the red check mark ("yes"), normal / highlighted, left of the centre |

The oval is drawn every frame (`0x45edf0` at the centre) over the frozen game, without dimming. Only a hovered item draws its frame on top (the draw loop skips a button whose state is "normal"). In a confirmation page (below) both the check mark and the X are drawn.

## Behaviour
- Esc during a mission opens it: active flag `[+4] = 1`, page `[+8] = -1`, sound 35 `ingame-come` (`0x442f74`). Esc again (the key flag `[player+0x6bed6d]`) closes it and sets state 7; closing plays 35 again (`0x442c99`).
- Items (jump table `0x442cd8`), hover sound 36 `ingame-move` (`+0x54 = 0x24`), click sound 37 `ingame-select` (`+0x58 = 0x25`):

| item | action |
|---|---|
| 0 CONTINUE | close, state 7 |
| 1 LOAD GAME | state `0xc`, page 1 (docs/saveload.md) |
| 2 SAVE GAME | state `0xd`, page 2 (docs/saveload.md) |
| 3 OPTIONS | state `0xe`, page 3 (the options menu of docs/options.md; it returns to state 7 with the menu still open) |
| 4 RESTART | page 4 (confirmation) |
| 5 MAIN MENU | page 5 (confirmation) |

- Items 1 and 2 are hidden and dead during the tutorial (`[0x70f2cc] != -1`); in a network game (`[0x575720] == 1`) items 1, 2 and 4 are.
- Confirmation pages 4 and 5 show only the check mark (`0x5eaa84/88`, sound 38 `ingame-ano`) and the X (`0x5eaa7c/80`, sound 39 `ingame-nie`); the six items do not react. X returns to page -1. The check mark on page 5 goes to state `0x1c` (main menu); on page 4 it restarts the mission (`0x42c260(0x1b)`, `0x42c320(level)`), or for level 0 (the tutorial) reloads the settings and goes to state 5.
- In a network game opening/closing sends a packet (`0x42a350` / `0x42a2a0`, a CNet command, docs/network.md); single player only freezes.
- "P A U S E" is the separate P pause (state `0xf`, below), not part of this menu.

# The P pause (state `0xf`, `0x435330`)

Code: `Play::draw_pause`, `main.rs` (`Game::key`), `formats::endscreen::pause_rows`; `scripts/verify_against_exe.py` emulates `0x45d3a0` against `opensow scanlines pause`.

- **Enter**: the P key in a mission (`[player+0x7d] == 1`, scan code 0x19 = P, `0x415d05`; docs/hotkeys.md) calls `0x41f660` (ResetEvent on the mouse event `[0x6b3d0c]`, `[0x5baad7] = 0`), state `0xf` (`0x42c260`) and, in a network game, sends the pause packet `0x42a2a0(1)`. The dispatcher `0x42d020` calls `0x435330` for state 0xf (jump table `0x42d624`, index state-1); state `0x10` (`0x435580`) is the same screen for the other machine's pause in a network game and prints the literal "Waiting for the server".
- **First frame** (`[0x49a339]` redraw flag): `0x420b40` (SetEvent, `[0x5baad7] = 1`), the game's frame surface `[0x5dde34]` is blitted whole onto the screen surface `[0x5ddec4]`, the surface is locked (`0x466420`) and `0x45d3a0` writes black (0) into **every even pixel row** (rows 0, 2, 4, ...; odd rows are untouched), then unlocked (`0x466970`). HUD and pointer are part of the frame, so everything is dimmed to half the rows.
- **Text**: `CreateFontA(28, 0, 0, 0, 700, italic 1, 0, 0, charset 0, 0, 0, quality 3, pitch 2, "Times New Roman")`, string id `0x16a` (362, 1-based; the lang dump index 361, "P A U S E", the letters carry their own spaces) drawn by `0x467060` (white text, black shadow at +2,+2, docs/text.md) with the left edge at `W/2 - textwidth/2` (width from `GetTextExtentPoint32W`) and the **top at `H/2`**.
- **Per frame** (no redraw): `0x420b80` polls the input device, `0x415a00` updates the key flags; `0x4220c0` (network: every player has paused / agrees) or the local P flag `[player + 0x6bed85]` (= `0x7d`) set means resume: `0x42c260(7)`, `[0x6bfa34] = 1` (resets the frame timer so no time is lost), and in a network game `0x42a2a0(0)`. **Nothing else is read**: Esc, the mouse and all other keys (Space included) do nothing while paused, and the simulation, the in-game clock and everything it drives stand still (they only run in state 7).
- OpenSOW: `Play::paused` freezes the update (as before); `draw_pause` darkens the even rows and prints string 362 (the Times New Roman font is not available, the big bitmap font stands in, scaled like the HUD); while paused `Game::key` accepts only P and the mouse and wheel are ignored.
