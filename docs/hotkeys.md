# Keyboard (decoded)

The per-frame key handler of the running game is `0x415ba0` (continued up to `0x416876`); `0x415a00` refreshes the key flags first. The flags are bytes at `[pointer + 0x64 + DirectInput scan code]` (pointer object of player 0 = `0x6bed08`, so scan code `s` is at `0x6bed6c + s`): 0 up, 1 pressed this frame, 2 held. Checks of `== 1` are one-shot hotkeys, checks of `== 2` are held keys. In a network game the same scan codes travel as input records (docs/network.md). R is `0x13` (= `+0x77`), which is how the offsets below were anchored. **Keys are scan codes, so the physical key counts**: OpenSOW reads the physical digit (`Game::digit_of`, winit `PhysicalKey`) so 1-5 work unshifted on layouts like Czech, where the number row types `+ ě š č ř` (before, only Shift+digit worked there). Tests: `mission_hotkeys_follow_the_exes_key_handler` (tests.rs).

**Rebindable (not in the exe):** every mission key below (and the development shortcuts) is an action of `keybinds.rs` (`Act`, two key slots each) that the Options menu's Controls view rebinds (the CONTROLS button; docs/options.md, "Controls view"). The tables give the defaults, which are the exe's keys. `Game::hotkey` looks the key up in `GameOptions::keys` (`Game::action_of`); the window passes the physical key (winit `KeyCode`, like the exe's scan codes), tests and headless runs map the logical one (`keybinds::code_of`). The mouse buttons past left and right (middle = 3, back = 4, forward = 5, others) can be bound in a slot like a key (`keybinds::Input::Mouse`; `Game::bound_input` runs them in a mission with the keys' pause and chat rules, held scroll included, and the window checks a bound fullscreen button). Shift keeps its meaning on whatever key or button is bound: Shift + quick save loads, Shift + a bookmark key sets it, Shift + next Advancer picks an idle one. Fixed: Esc (the in-game menu, and Cancel on every screen), the left and right mouse buttons (select and command), Shift / Ctrl / Alt (held, Ctrl or Alt is the attack modifier, docs/cursor.md) / the system keys, and in development builds F12 and `` ` `` (dev panel). Menu screens and text lines (chat, save names, the lobby) keep their own keys.

**Differs from the exe:** the letter hotkeys (W A S D, H, X, R, P, the development letters) follow the physical key now, like the exe's scan codes; before the rebinding they followed the typed letter, so on AZERTY or QWERTZ layouts they sit where a US keyboard has them. The Controls pages name keys by their US-layout labels.

**Differs from the exe:** the F1 key help overlay is the original picture (`extras\shortcuts`) and still shows the original keys after a rebinding.

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
| X (0x2d) | `+0x91` | `0x4167e1` -> `0x418dc0` | scatter (below) | done (`World::scatter_selected`, `formats::scatter`) |
| R (0x13) | `+0x77` | `0x416769` | rally mode of the selected own factory, else Advancer repair mode | done (docs/rally.md) |
| Enter (0x1c) | `+0x80` | `0x41612d` | opens the chat line (`[0x5baa9a]`), in single player too; typed text and cheat phrases: docs/chat.md | done (`Play::chat_*`) |
| F1 (0x3b) | `+0x9f` | `0x415bda` | sound 0x20, toggles `[0x5baa9b]` (cleared at mission start, `0x422b4b`): the **key help overlay**, `0x420a40` darkens a box round the screen centre (`0x460a00`) and draws `extras\shortcuts` frame 0 (`[0x5e6e3c]`, `0x45edf0`) at (W/2, H/2 - 30), called from the frame draw at `0x4209d8`. The FPS counter is a different flag, `[0x5baad5]`, set only by the `draw fps` cheat (docs/chat.md) | same: sound 0x20 and the key help overlay (`Play::show_keys`, drawn in `Play::draw`; headless `--keys`); the FPS counter stays on the `draw fps` cheat (`Play::show_fps`; headless `--fps`) |
| F2 (0x3c) | `+0xa0` | `0x415c05` | abandon the mission to the campaign map (not in a network game) | done |
| F3 / F4 (0x3d / 0x3e) | `+0xa1/0xa2` | `0x415c49`, `0x415cb1` | game speed `[0x49ceec]` (starts at 1.0, kept from mission to mission) -0.5 / +0.5, clamped to 0.5..3.0 (the code clamps below 0.51 / above 2.99), then message 0x1a / 0x19 (the "speed" line, below); not in a network game. It scales the frame time in `0x42e890` | done (`Game::game_speed`, `Play::show_speed`; headless `--speed 1.5`) |
| F5 (0x3f) | `+0xa3` | `0x416811` | quick save, with shift quick load | done (docs/saveload.md) |
| F9 (0x43) | `+0xa7` | `0x415bca` | only cleared when held (no function) | - |
| F12 (0x58) | `+0xbc` | `0x41a870` | exits the program | the port uses F12 for the dev panel |

**Differs from the exe:** Esc opens the in-game menu but does not end the air-strike / rally modes or deselect (`Game::hotkey` -> `open_pause`); right click ends the modes.

**Differs from the exe:** the left/right Windows keys do nothing (the exe treats them like Esc).

**Differs from the exe:** Backspace and Space / numpad 0 jump the view at once (`Play::look_at`); the exe scrolls there smoothly (`0x4588c0`).

**Differs from the exe:** Tab skips only dead units (`Play::select_next_unit`); the exe's extra test `[unit+0x2c] == 0` is not modelled.

**Differs from the exe:** numpad `*` / `/` / `-` take the whole window as the view (`Play::select_visible`); the exe's view rectangle leaves out the HUD.

**Differs from the exe:** F12 opens the dev panel (`devtools.rs`) instead of quitting.

**Differs from the exe:** the arrow / WASD scroll is 1.5 times the exe's speed (`options::SCROLL_BOOST`, docs/options.md).

Shift (0x2a/0x36, `+0x8e/+0x9a`) is read by the F5, bookmark and click code (`0x415963` copies it into the pointer for the mouse code: shift/ctrl toggles a unit in the selection, docs/controls.md).

## Scatter (X, `0x418dc0`)

`formats::scatter`, exe-verified (`scripts/verify_against_exe.py`, 0x4378d0, 3000 random cases). Over the player's units with the selected byte `[unit+0x48+player]`, the pixel box of `[unit+0x14/0x18]` is taken; in a network game (`0x41f020`) the handles `[unit+0x3c]` go out as the UnitScatter command (`0x418ea5`) and nothing else happens. Otherwise the box middle in half cells is `((max >> 4) + (min >> 4)) >> 1` per axis (`0x418f9d`), and for every selected unit, with `n` = 6:

1. `0x4378d0(&hx, &hy, cx, cy, n)` with the unit's half cell (`px >> 4`): a Bresenham walk from the middle towards it; once the major axis equals the unit's, that step and the next `n - 1` are taken, so the end is `n` half cells past the unit (a unit right in the middle goes down-right). `n` outside 1..=10 leaves the point as it is.
2. The cell is the end halved towards zero (`cdq; sub; sar 1`). If it is on the map, the walkers' grid `0x6c7a4c` is 0 there (terrain and buildings, not units) and the unit is not a pod of missions 3/8 (`[0x49a33c]` 3 or 8, kind 0x2e), the unit gets the normal go-to `0x4706a0` type 1 to the cell's centre (`cell * 32 + 16`). Else `n -= 2` and again while `n >= 0`; the last try, `n` = 0, is the unit's own cell.

No sound is played. OpenSOW: same (`World::scatter_selected`, `formats::scatter`), test `scatter_sends_the_selection_away_from_its_middle`; no network game, so no UnitScatter command.

## The speed line (F3 / F4)

`0x402ec0(0x1a or 0x19, -1, -1, player)`: both events share case `0x4031d9`, which formats `[0x49ceec]` with `"  %.1fx"` (`0x485aa8`) and appends it to lang string 145 (1-based, `push 0x91`; "speed"), so the line reads "speed  1.5x" in the message colours. The speech switch (`0x40335d`) only covers events 1..24: no voice. Like every message it is skipped during the tutorial (`[0x70f2cc] != -1`, `0x402f02`). OpenSOW: same text and colours; `Play::notes` holds these lines; they share the message queue (`message_pose`).

## Not in the original (port extras)

Quality-of-life additions (they change input and information only, never the simulation):

- **Shift+H**: the next *idle* Advancer (no building order, no chase, not walking, nothing queued: `Unit::is_idle`), and the view centres on it. Plain H stays the original round robin over all Advancers without a camera move.
- **Double click** on one of our units (same kind twice within 500 ms, no shift): every own unit of that kind inside the view is selected, like numpad `*` narrowed to one kind. Advancers and nuclear vehicles stay single-select. The decoded mouse handler has no double click (docs/controls.md).
- **Turret range while placing**: with a turret armed (1-5), the cells its gun reaches (range rings of top kind 108 + tier from the gun's cell, where `World::find_target` searches from: the footprint's top-left cell, one row lower for the 1x2 turrets 4 and 5) are tinted under the placement tiles; the anti-air turret's in blue. On by default; Options page 2 "Turret range" (`Extra::turret_range`, `turret_range` in `opensow.cfg`) turns it off.
- **Autosave**: the windowed game saves into the quick save slot (slot 0, "Quick Save", so Shift+F5 loads it) on the first frame of a mission and every 5 minutes of game time after (`AUTOSAVE_TICKS`), without the "game saved" speech; after a load the first one comes one interval later. Headless runs and tests never autosave (`Game::autosave` is only set by the windowed start).

  **Differs from the exe:** the exe never writes the quick save slot by itself; a manual quick save there is overwritten.

Development shortcuts:

`Q` select all (the original has numpad `*`), `T` buy every affordable tech-level upgrade at once (`World::tech_up_all`: each costs its research points, the mission's level limits hold, only the player's own live factories; the same as clicking each upgrade, and what the exe's AI does in `0x42f490`, so no cheat), `B M C Y F` arm an air strike (docs/air.md: **not decoded in the exe**, the original starts strikes only from the air pills), `` ` `` / F12 dev panel, `L` level list and `P` prototype settings on the menus. They stay as development shortcuts; the mission ones (`Q`, `T`, `B M C Y F`) are rebindable like the rest, the menu ones and the dev panel keys are fixed. F11 (fullscreen, not in the exe) is rebindable too (`Act::Fullscreen`, checked by the window before the game sees the key).

**Differs from the exe:** these keys (and `L` / `P` on the menus) do nothing in the exe; here they select, buy the affordable upgrades or launch strikes without the air pills.

## Other states

Menus read their own keys: arrows and Enter (`0x432eb0..`), Enter / numpad Enter / Backspace in the text boxes (`0x444801`, `0x44480a`, `0x444aa7`), F2 on the pre-mission screens (`0x43ec5d`, `0x441332`). Mouse input: docs/controls.md.

**Differs from the exe:** the port's menus have no arrow / Enter navigation (mouse, Esc goes back; the skirmish screen's arrows step through the maps) and F2 does nothing on the pre-mission screens; the text boxes edit with Backspace, Delete, the arrows, Home and End (`textinput.rs`).

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

OpenSOW: `World::say_at` with the same positions, including the repaired quirk (`sim/advancer_actions.rs`).

**Differs from the exe:** for the repaired events OpenSOW takes the y of the side's first Advancer; the exe finds the one whose target handle matches the beam.

## Turret placement tiles (`0x4156b0`, `0x417f80`, `0x408be0`)

While a turret is armed (`[pointer+0x16c]` = 0x19 + tier - 1, by key 1-5 or the tab) and the pointer is on the map (`x < [0x517af4]`), the pointer stays the normal arrow and `0x417f80` re-runs the placement test `0x408be0` every frame for the footprint (`0x409760`, top-left = the cell under the pointer, `[+0x48/0x4c]` = map pixel >> 5) and stores one state per cell at `[pointer+0x170 + 4*i]`: **2** = free (static grid `0x5eac38` not 1, walkers' grid `0x6c7a4c` 0, no-build layer `0x6efa4c` 0) **and** an own building within two cells of the footprint; otherwise **1**, and the byte `[+0x1e0]` becomes 0. `0x4156b0` blits one `extras\sizer` tile with the **additive** blit (`0x45cde0`, so it brightens the ground; a plain copy looked flat grey; hot spot at the cell centre) per cell: state 2 -> `[0x5e68c8]` (frame 0, pale hatching), state 1 -> `[0x5e68cc]` (frame 1, red-orange hatching). The left release builds only when `[+0x1e0] == 1` (`0x4172cb`), takes 200 * tier credits, plays sound 0x48 (72, `sfx/turets/turret-place.wav`, via `0x4077e0` at the cell's top-left pixel) and ends the mode (`0x41740a`); right click ends it too (`0x416a36`). The tab icon has **no** selection frame (the port drew a yellow box; removed). OpenSOW: `World::turret_cells`, `Play::draw_placement`, headless `--arm-turret TIER --mouse-px X,Y`.
