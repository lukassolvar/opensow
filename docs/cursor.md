# Mouse pointer (decoded)

Code: `crates/opensow-formats/src/cursor.rs` (types, frames, offsets, scroll arrows), `advancer::pointer_over_building` (the Advancer cases), `Play::pointer` / `draw_pointer` in the game. Exe test: `pointer_types_match_the_exes_cursor_code`.

The game hides the system cursor and draws its own sprite: `cursor.ps6` (in `sprites.data`, array `0x5de120`, loader `0x425ac0`) = **19 types x 8 frames**, `frame = 8 * type + phase`. The pointer object holds the type `[+0x50]`, the draw offset `[+0x54]/[+0x58]` and the phase `[+0x60]`. Draw `0x4152d0`: frame at `pos - offset - hotspot`. Phase `0x4158a0`: +1 every **4 ticks**, wraps at 8. The update `0x416940..0x417e84` picks the type every frame.

| type | meaning | chosen at |
|---|---|---|
| 0 | arrow (offset -12,-8) | default |
| 1 | arrow over blocked ground (walkers' grid `0x6c7a4c`; with an Advancer selected the air grid `0x6d7a4c` is used instead) | `0x417879` |
| 2 | over an own unit or own building (select) | `0x417a25`, `0x417bca` |
| 3 | go-to: ground, an enemy *ground non-anti-air unit* while an Advancer is selected (an enemy Advancer, `[unit+0x4b]`, or anti-air unit, `[unit+0x4c]`, gets the attack pointer, test at `0x417a8c`; the click loop `0x417af2` has no kind filter, so the Advancer is ordered to chase it), the own HQ under an Advancer, an own building the Advancer cannot use | `0x417a98`, `0x417e11`, `0x417bea` |
| 4 / 5 | attack (red brackets; 5 = modifier key) over enemy units/buildings with a selection | `0x417ac1`, `0x417d21` |
| 6 | **repair** (purple wrench) | `0x417c4a` |
| 7 | **destroy / capture** (red arrow) | `0x417cdd` |
| 8 | **upgrade** (green arrow) | `0x417c8d` |
| 9 | **rally point placement** (over free ground; the blocked arrow 1 over blocked ground), offset (-1, 0), docs/rally.md | `0x416fc6`, `0x417678` |
| 10 | air-strike mode (`[+0x1e8]` set, offset 0,0) over the map, the blocked arrow 1 instead over cells the air layer `0x60ac38` marks `0xfffffd` (`0x417257..0x41727b`); drawn by OpenSOW (`Pointer::AirStrike`) | `0x416f80`, `0x41727b` |
| 0x0b..0x12 | scroll arrows at the screen edge (UL, UR, DR, DL, D, L, R, U), offsets in `Pointer::offset` | `0x416a9c..0x416b68` |

## With an Advancer selected, over a building (`0x417b8c..0x417d0a`)
The same branch issues the order when the left button is released (`0x46b8b0` destroy, `0x46b9f0` repair, `0x46bb30` upgrade), so the pointer always shows what a click does:
- building not owned by the player (enemy **or neutral**): 7 destroy.
- own HQ (kind 100): 3 (nothing).
- own turret (25-29), or any own building while repair mode `[pointer+0x1f0]` is on: damaged (`[b+0x74] < [b+0x70]`) -> 6; else a locked slot (`[b+0x144] < [b+0x140]`) -> 8; else 3.
- any other own building: locked slot -> 8; else damaged -> 6; else 3.
- **Repair mode is toggled by a right click on an own building** while an Advancer is selected (`0x417964`; a right click elsewhere deselects, `0x43ad60`). The game also keeps the `R` key.

The Advancer then flies to the building's centre cell (`docs/advancer-actions.md`) and fires there.
