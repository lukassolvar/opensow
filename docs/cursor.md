# Mouse pointer (decoded)

Code: `crates/opensow-formats/src/cursor.rs` (types, frames, offsets, scroll arrows), `advancer::pointer_over_building` (the Advancer cases), `Play::pointer` / `draw_pointer` in the game. Exe test: `pointer_types_match_the_exes_cursor_code`.

The game hides the system cursor and draws its own sprite: `cursor.ps6` (in `sprites.data`, array `0x5de120`, loader `0x425ac0`) = **19 types x 8 frames**, `frame = 8 * type + phase`. The pointer object holds the type `[+0x50]`, the draw offset `[+0x54]/[+0x58]` and the phase `[+0x60]`. Draw `0x4152d0`: frame at `pos - offset - hotspot`. Phase `0x4158a0`: +1 every **4 ticks**, wraps at 8. The update `0x416940..0x417e84` picks the type every frame.

| type | meaning | chosen at |
|---|---|---|
| 0 | arrow (offset -12,-8) | default |
| 1 | arrow over blocked ground (walkers' grid `0x6c7a4c`; with an Advancer selected the air grid `0x6d7a4c` is used instead) | `0x417879` |
| 2 | over an own unit or own building (select) | `0x417a25`, `0x417bca` |
| 3 | go-to: ground, an enemy *ground non-anti-air unit* while an Advancer is selected (an enemy Advancer, `[unit+0x4b]`, or anti-air unit, `[unit+0x4c]`, gets the attack pointer, test at `0x417a8c`; the click loop `0x417af2` has no kind filter, so the Advancer is ordered to chase it), the own HQ under an Advancer, an own building the Advancer cannot use | `0x417a98`, `0x417e11`, `0x417bea` |
| 4 / 5 | attack (red brackets) over enemy units/buildings with a selection; 5 (the brackets tilting side to side) = `4 + flag` with **Ctrl or Alt** held, see below | `0x417ac1`, `0x417d21` |
| 6 | **repair** (purple wrench) | `0x417c4a` |
| 7 | **destroy / capture** (red arrow) | `0x417cdd` |
| 8 | **upgrade** (green arrow) | `0x417c8d` |
| 9 | **rally point placement** (over free ground; the blocked arrow 1 over blocked ground), offset (-1, 0), docs/rally.md | `0x416fc6`, `0x417678` |
| 10 | air-strike mode (`[+0x1e8]` set, offset 0,0) over the map, the blocked arrow 1 instead over cells the air layer `0x60ac38` marks `0xfffffd` (`0x417257..0x41727b`); drawn by OpenSOW (`Pointer::AirStrike`) | `0x416f80`, `0x41727b` |
| 0x0b..0x12 | scroll arrows at the screen edge (UL, UR, DR, DL, D, L, R, U), offsets in `Pointer::offset` | `0x416a9c..0x416b68` |

OpenSOW: `Play::pointer` picks the type the same way and `Pointer::frame` / `cursor::phase_at` give the frame; the picture becomes the system cursor (`Play::pointer_cursor`, docs/game.md), `draw_pointer` draws it into the frame for headless screenshots and with `OPENSOW_SOFT_POINTER=1`.

**Differs from the exe:** the exe draws the pointer itself (or, with HW CURSOR, from its mouse thread, docs/options.md); OpenSOW hands the same frame to the system cursor so it moves at the mouse's rate.


**Differs from the exe:** with an Advancer selected OpenSOW never shows the blocked arrow 1 over open ground; the exe tests the air grid `0x6d7a4c` there.

**Differs from the exe:** the scroll arrows appear within 6 px of the window edge (not over the HUD at the right edge); the exe's band is 4 px (x < 4, x > w - 5, docs/options.md).

## The modifier key (type 5)
`0x4179bf..0x4179e5`: the flag `[ebp-0xcc]` is 1 while any of the input object's key bytes `+0x81`, `+0x101`, `+0x9c`, `+0x11c` is set. The key array starts at `+0x64` (DirectInput codes), so these are **left / right Ctrl (0x1d / 0x9d) and left / right Alt (0x38 / 0xb8)**; Shift and the Windows keys do not count (emulated: the type over an enemy building is 5 for exactly these four keys). The flag gives:
- over an enemy unit or an enemy / neutral building with a selection: type `4 + flag` (`0x417abb`, `0x417d17`);
- the click's order `0x469fe0(target, flag)`: on a unit the flag is always 1 (`push 1`, `0x417b2e`), on a building it is the modifier flag (`0x417d81`). With flag 1 the target stays in `[unit+0x7c]` (a lasting chase); with flag 0 the unit gets one route and the target is cleared again (`0x46a141..0x46a14d`, a pod keeps it), as for the computer's orders (docs/ai.md). Both flags flash the target (`0x46a170` / `0x4086a0`) and play sound 59.
- over an own unit or own building the flag sends the click down the attack path as well (`0x417a1d`, `0x417ba6`): with a selection type 5 and `0x469fe0(target, 1)`, a lasting attack on the own object (a selected unit ordered onto itself is skipped, `0x417b29`); with nothing selected the ground pointer (0 or 1) and **no selection** (the select calls `0x43ad60` at `0x417a50` / `0x417bd7` are skipped). With an Advancer selected an own building shows destroy (7) and the click is the destroy order `0x46b8b0`; an own ground unit shows go-to (3) as an enemy one does (`0x417a8c`).
- the weapon scan treats the order target as an enemy whatever its side (`0x47b232`, docs/ai.md), bullet and blast damage have no owner test (`0x42f2c0`), and the destroy beam's re-check (`0x46f3a6`) and landing (`0x4102b6`) test only the building's id, so own units and buildings really are shot and beamed down; a building that falls goes to the capture winner as always (docs/advancer-actions.md). The orders end when the building is rebuilt for its new owner: the new object has a new handle.

OpenSOW: the same. `Play::own_unit_at` / `Play::attack_target`, `World::order_action_with` (the pointer's Advancer action), `World::find_target` and the chase take an own order target (`World::target_alive` instead of the side test), and `finish_destruction` drops every chase, Advancer order and beam that held the captured building (the exe's new handle). Tests `ctrl_or_alt_attack_own_units_and_buildings`, `an_own_unit_as_order_target_is_shot_like_an_enemy`, `a_lasting_chase_on_an_own_building_holds_until_the_building_changes_hands`, `an_advancer_ordered_to_destroy_an_own_building_damages_it_until_it_changes_hands`.

OpenSOW: `Play::attack_key` (the window's Control or Alt state, `Game::set_attack_key`; headless `--attack-key`), `Play::attack_pointer`, `World::order_attack_selected(target, lasting)`. Ctrl and Alt are fixed keys that cannot be bound (docs/hotkeys.md), so nothing else uses them in a mission. Exe test: `pointer_types_match_the_exes_cursor_code` (the four key offsets, `4 + flag` at both sites, the two flag pushes); tests `ctrl_or_alt_turns_the_building_attack_into_a_lasting_chase`, `a_building_attack_is_one_route_unless_ctrl_or_alt_makes_it_a_lasting_chase`.

## With an Advancer selected, over a building (`0x417b8c..0x417d0a`)
The same branch issues the order when the left button is released (`0x46b8b0` destroy, `0x46b9f0` repair, `0x46bb30` upgrade), so the pointer always shows what a click does:
- building not owned by the player (enemy **or neutral**): 7 destroy.
- own HQ (kind 100): 3 (nothing).
- own turret (25-29), or any own building while repair mode `[pointer+0x1f0]` is on: damaged (`[b+0x74] < [b+0x70]`) -> 6; else a locked slot (`[b+0x144] < [b+0x140]`) -> 8; else 3.
- any other own building: locked slot -> 8; else damaged -> 6; else 3.
- **Repair mode is toggled by a right click on an own building** while an Advancer is selected (`0x417964`; a right click elsewhere deselects, `0x43ad60`). The game also keeps the `R` key.

OpenSOW: same (`advancer::pointer_over_building`, `World::advancer_pointer`; `R` toggles repair mode like `0x416769`).

The Advancer then flies to the building's centre cell (`docs/advancer-actions.md`) and fires there.
