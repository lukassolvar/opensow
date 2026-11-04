# Factory rally point (decoded from the exe)

Code: `crates/opensow-formats/src/rally.rs` (constants, `snap`, `usable`, `marker_frame`), `World::set_rally` / the spawn in `step_production` (`sim.rs`), `Play::arm_rally`, `hud::Layout::rally`. Exe test: `rally_point_constants_match_the_exes_code` (byte-checks every address below).

The original has this feature; it was simply never written up. Every building object has a rally point `[bld+0x2c0]` (x) / `[bld+0x2c4]` (y), **map pixels, -1 = none** (constructor `0x40a40d`; serialised by the save writer `0x40ba1c`, so it is part of the savegame; the network handler `0x4219e5` sets it from `7-bit cell x, 7-bit cell y, 32-bit building index`, always as `cell * 32 + 16`). Only the player's light/medium/heavy/walker factories (kinds 101, 102, 103, 108) can be given one; the computer opponent never sets one.

## The button
- HUD widget `[hud+0x840]` (id `0x7b`, 48x21 `ikonyrpoints` frame 1, frame 3 while hovered; plain copy draw), constructed at `0x4554c6`: hit box `0x456490(-0x19, -0xb, 0x19, 0xc)` relative to its position, tested **exclusively** by `0x456350` (so x in -24..24, y in -10..11).
- Placed by `0x455dcf` at the **top slot icon's position + (4, -41)**, i.e. directly above the topmost production icon, only when the selected building (selected flag, not merely hovered) is one of the four factory kinds, belongs to the local player and has at least one slot; otherwise it is parked at y = -1000 (`0x455ab0`).
- Click case `0x416cf9` (jump table `0x417eb4` entry 12, widget id `0x7b` in the byte table `0x417eec`): on the left release it scans the player's buildings for the selected factory, stores its index in `[pointer+0x1ec]`, sets `[pointer+0x1e9] = 1` (the rally mode; `[+0x1e8]`, the air-strike mode, becomes 0) and clears the turret mode `[+0x16c]`.
- **Hotkey R** (scan code 0x13 = `[pointer+0x77]`, `0x416769`): the same scan and the same mode. (R also toggles the Advancer's repair mode, `[+0x1f0]`; with a factory selected only the rally mode starts.)

## The mode (`[pointer+0x1e9]`)
- Pointer (`0x41763b`): over a free ground cell (walkers' blocked grid `0x6c7a4c`) type 9 (`cursor.ps6` frames 72..79) at draw offset (-1, 0), else type 1, the blocked arrow. Text hint in OpenSOW only.
- Left release (`0x417414`), pointer inside the play field (`x < [0x517af4]`, not on the HUD) and the clicked cell **not blocked**: the point becomes the clicked cell's centre, `((v & -0x20) + 0x10)`; the mode ends (`[+0x1e9] = 0`). A blocked cell or the HUD does nothing (the mode stays on). In a network game (`0x41f020`) the command is also appended to the command list `0x49a434` for the other machines.
- The local player gets a one-shot marker at the point: an AnimSprite (`0x4054a0`) of `extras\cross_rp` (array `0x5e6028`) frames 0..20, 3 ticks each, additive (draw mode 6), and sound **68** (`airforce-dest`) non-positionally (`0x417636`).
- Cancel: right click without drag (`0x416a40`), Esc (`0x416504`, which also deselects and opens the menu), a turret key 1-5 or an air-strike key, which each clear it as they set their own mode (`0x415d54..0x415edd`).

## The marker on the map
`0x409400` (building draw pass; `0x40db10` is the matching dirty-rectangle erase): while the building is **selected by the local player** and the point is set, `extras\rallypoint` (array `0x5eab00`, 16 frames, hot spots centred) frame `[bld+0x60]` is drawn additively at the point. The frame advances every 3 ticks (`0x40de60`: `[bld+0x64]` reloaded with 3, `[bld+0x60]` wraps at 0x10), so it is `tick / 3 % 16`.

## What produced units do (`0x408740`)
After the unit is created at the spawn cell it gets an order **list** of three entries: the type-2 straight walk to the exit cell's centre (`0x4706a0`, which first clears the list), a type-1 go-to two cells beyond the exit (primary exit: y + 0x50 px; alternative exit: y - 0x30 px, both appended by `0x46e510`), and then, **if the point is inside the map** (`0 <= x <= mapW*32`, `0 <= y <= mapH*32`, signed compare, so -1 fails; `0x40896a..0x4089a0`, `0x408b71..0x408b9b`) a third type-1 go-to the point pixel. The same code runs for both exits, and for the walker factory's teleport variant. The unit therefore leaves the factory exactly as before and then walks on; its route is planned like any go-to (blocked target cells use the usual nearest-free search), and units arriving at the same point queue up as they do for the player's group orders. Any go-to the player gives replaces the list (`0x4706a0`); in OpenSOW an attack order drops it too (the exe keeps the order list there; the chase replans the route in both cases).

## OpenSOW
- `Structure::rally: Option<(i32, i32)>`; a captured building loses it (the exe rebuilds the building through its constructor, `0x43ab40`). `Unit::queued` is the third order.
- Not decoded / not done: savegames of missions are not written by OpenSOW yet (the field is where `0x40ba1c` writes it); the pointer type 10 (air-strike mode) is still not drawn.
- Headless: `--play M01 --select-building 43,55 --rally 50,57 [--arm-rally --mouse-cell CX,CY]`.
