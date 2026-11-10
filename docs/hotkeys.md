# Keyboard (decoded)

The per-frame key handler of the running game is `0x415ba0` (continued up to `0x416876`); `0x415a00` refreshes the key flags first. The flags are bytes at `[pointer + 0x64 + DirectInput scan code]` (pointer object of player 0 = `0x6bed08`, so scan code `s` is at `0x6bed6c + s`): 0 up, 1 pressed this frame, 2 held. Checks of `== 1` are one-shot hotkeys, checks of `== 2` are held keys. In a network game the same scan codes travel as input records (docs/network.md). R is `0x13` (= `+0x77`), which is how the offsets below were anchored. **Keys are scan codes, so the physical key counts**: OpenSOW reads the physical digit (`Game::digit`, winit `PhysicalKey`) so 1-5 work unshifted on layouts like Czech, where the number row types `+ ě š č ř` (before, only Shift+digit worked there). Tests: `mission_hotkeys_follow_the_exes_key_handler` (main.rs).

## In a mission (state 7)

| Key (scan) | Flag | Exe | Effect | OpenSOW |
|---|---|---|---|---|
| Esc (0x01) | `+0x65` | `0x4164e9` | end air-strike (`+0x1e8`) and rally (`+0x1e9`) modes, deselect everything, state `0x14` = the in-game menu (sound 0x14) | done (`open_pause`; mode clearing only by right click) |
| Left/Right Win (0xdb/0xdc) | `+0x13f/0x140` | `0x415d2d` | the same block as Esc | not ported |
| 1 .. 5 (0x02..0x06, top row) | `+0x66..0x6a` | `0x415d7c..0x415edd` | turret tier k needs turret flag `[0x70baf5 + player*0xc68 + k-1]` and `credits >= 200*k` (200, 400, 600, 800, 1000, `[0x517af8 + 4*player]`): then **deselect all** (`0x43ad60(0)`), air-strike mode off, rally mode off, turret mode `[+0x16c] = 0x19 + k-1`; the next map click places it | done (`Play::turret_key`; before the port neither the selection nor the rally mode were cleared) |
| P (0x19) | `+0x7d` | `0x415d05` | **pause**: `0x41f660`, state `0xf`, network pause packet. The pause screen waits for P again (`0x435330`, `0x435537` for state `0x10`) | done; the port used Space before, which was wrong (docs/pause.md had Space) |
| Arrows (0xc8/0xd0/0xcb/0xcd) and W/S/A/D (0x11/0x1f/0x1e/0x20), held | `+0x12c/0x134/0x12f/0x131`, `+0x75/0x83/0x82/0x84` | `0x415ee4..0x415f98` | scroll the map by the scroll speed `[0x552494]` (options) | done |
| Backspace (0x0e) | `+0x72` | `0x4161b9` | the view jumps (smooth scroll `0x4588c0`) to the player's headquarters (building kind 100; every HQ is visited, the last one wins); the selection stays | done (`Play::look_at_hq`, instant jump) |
| Tab (0x0f) | `+0x73` | `0x41622d` | round robin `[pointer+0x2dc]` over the player's units in list order: the next one with `[unit+0x2c] == 0` and `hp > 0` is selected alone and the view centres on the selection (`0x418150`) | done (`select_next_unit`) |
| H (0x23), numpad + (0x4e) | `+0x87`, `+0xb2` | `0x415f98` | the same round robin (`[pointer+0x2e0]`) over the units of kind 40 (Advancers); selects it alone, **no** camera move | done (`select_next_advancer`) |
| Numpad * (0x37) | `+0x9b` | `0x4162b1` | every own unit inside the view (strict, `[0x71ceb8/c]` + `[0x49a340/4]`) that is not an air unit (`[+0x4b]`, the Advancer) and not a turret gun (kinds 109..113) gets the selected flag, all others lose it | done (`select_visible`; whole viewport, the original excludes the HUD) |
| Numpad / (0xb5), numpad - (0x4a) | `+0x119`, `+0xae` | `0x416369`, `0x416427` | the same, but only anti-air units (`[+0x4c] == 1`: kinds 1, 2, 3, 42) / only the others | done |
| Numpad Enter (0x9c) | `+0x100` | `0x416167` | centre the view on the box round the selected units (`0x418150`) | done |
| Numpad 1..9 (0x4f 0x50 0x51 0x4b 0x4c 0x4d 0x47 0x48 0x49) | `+0xb3..` | `0x416564..0x416674` | jump to bookmark k (`[pointer + 0x2e4 + 8*(k-1)]`, x/y), with **no** shift held (`+0x8e`/`+0x9a`) | done (`Play::bookmark`) |
| Shift + numpad 1..9 | | `0x4166ac..0x416763` | set bookmark k to the middle of the view | done |
| Space (0x39), numpad 0 (0x52) | `+0x9d`, `+0xb6` | `0x416177` | if a message line is shown (`0x4037b0`: `[list+0x14] > 0`; a line lives 480 ticks), the view jumps (`0x4588c0`) to the **position of the last message event** (`[0x6bf6b0 + player*0x20 + 4/8]`, written by `0x402ec0(id, x, y, player)` unless x / y are -1, so it survives later position-less events) | done (`World::say_at`, `event_position`, `Play::look_at_event`) |
| X (0x2d) | `+0x91` | `0x4167e1` -> `0x418dc0` | scatter: the selected units are spread round the centre of their box (UnitScatter command `0x418ea5`) | **not ported** |
| R (0x13) | `+0x77` | `0x416769` | rally mode of the selected own factory, else Advancer repair mode | done (docs/rally.md) |
| Enter (0x1c) | `+0x80` | `0x41612d` | opens the chat line (`[0x5baa9a]`), in single player too; typed text and cheat phrases: docs/chat.md | done (`Play::chat_*`) |
| F1 (0x3b) | `+0x9f` | `0x415bda` | sound 0x20, toggles the FPS counter `[0x5baa9b]` | not ported |
| F2 (0x3c) | `+0xa0` | `0x415c05` | abandon the mission to the campaign map (not in a network game) | done |
| F3 / F4 (0x3d / 0x3e) | `+0xa1/0xa2` | `0x415c49`, `0x415cb1` | game speed `[0x49ceec]` (starts at 1.0, kept from mission to mission) -0.5 / +0.5, clamped to 0.5..3.0 (the code clamps below 0.51 / above 2.99), then message 0x1a / 0x19 (the "speed %.1f" line, string 144); not in a network game. It scales the frame time in `0x42e890` | done (`Game::game_speed`; the on-screen "speed" line is not drawn) |
| F5 (0x3f) | `+0xa3` | `0x416811` | quick save, with shift quick load | done (docs/saveload.md) |
| F9 (0x43) | `+0xa7` | `0x415bca` | only cleared when held (no function) | - |
| F12 (0x58) | `+0xbc` | `0x41a870` | exits the program | the port uses F12 for the dev panel |

Shift (0x2a/0x36, `+0x8e/+0x9a`) is read by the F5, bookmark and click code (`0x415963` copies it into the pointer for the mouse code: shift/ctrl toggles a unit in the selection, docs/controls.md).

## Not in the original (port extras)

`Q` select all (the original has numpad `*`), `T` raise tech levels, `B M C Y F` arm an air strike (docs/air.md: **not decoded in the exe**, the original starts strikes only from the air pills), `` ` `` / F12 dev panel, `L` level list and `P` prototype settings on the menus. They stay as development shortcuts.

## Other states

Menus read their own keys: arrows and Enter (`0x432eb0..`), Enter / numpad Enter / Backspace in the text boxes (`0x444801`, `0x44480a`, `0x444aa7`), F2 on the pre-mission screens (`0x43ec5d`, `0x441332`). Mouse input: docs/controls.md.

## Message event positions (`0x402ec0` callers)

Only these pass a position (map pixels); every other caller (`0x415ca4` / `0x415d00` speed lines, reinforcement countdown, credits, new turret / tech level, air force ready `0x453ef4`) passes -1, -1 and leaves the stored position alone.

| Event | Caller | Position |
|---|---|---|
| 1 Building captured | `0x43ad46` | building centre `((w*16 + cell.x*32), ...)` (`[+0x14] + 2*[+4]`, `[+0x18] + 2*[+8]`, times 16) |
| 2 Turret destroyed / 3 Building lost | `0x409270` | the same, of the destroyed building |
| 4 Building upgraded | `0x40e445` | the same |
| 5 No space for units | `0x40e323` | the same, of the factory |
| 9 Advancer destroyed | `0x42e412`, `0x46c3d7` | the unit's cell * 32 |
| 10 Advancer disengaged | `0x46f30a` | the unit's position |
| 23 Turret repaired / 24 Building repaired | `0x410150` | **quirk:** x and y are both the Advancer's y (`[unit+0x18]`) |

OpenSOW takes the first own Advancer for the repaired events (the exe finds the one whose target handle matches the beam).

## Turret placement tiles (`0x4156b0`, `0x417f80`, `0x408be0`)

While a turret is armed (`[pointer+0x16c]` = 0x19 + tier - 1, by key 1-5 or the tab) and the pointer is on the map (`x < [0x517af4]`), the pointer stays the normal arrow and `0x417f80` re-runs the placement test `0x408be0` every frame for the footprint (`0x409760`, top-left = the cell under the pointer, `[+0x48/0x4c]` = map pixel >> 5) and stores one state per cell at `[pointer+0x170 + 4*i]`: **2** = free (static grid `0x5eac38` not 1, walkers' grid `0x6c7a4c` 0, no-build layer `0x6efa4c` 0) **and** an own building within two cells of the footprint; otherwise **1**, and the byte `[+0x1e0]` becomes 0. `0x4156b0` blits one `extras\sizer` tile with the **additive** blit (`0x45cde0`, so it brightens the ground; a plain copy looked flat grey; hot spot at the cell centre) per cell: state 2 -> `[0x5e68c8]` (frame 0, pale hatching), state 1 -> `[0x5e68cc]` (frame 1, red-orange hatching). The left release builds only when `[+0x1e0] == 1` (`0x4172cb`), takes 200 * tier credits, plays sound 0x48 (72, `sfx/turets/turret-place.wav`, via `0x4077e0` at the cell's top-left pixel) and ends the mode (`0x41740a`); right click ends it too (`0x416a36`). The tab icon has **no** selection frame (the port drew a yellow box; removed). OpenSOW: `World::turret_cells`, `Play::draw_placement`, headless `--arm-turret TIER --mouse-px X,Y`.
