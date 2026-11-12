# Wave pathfinder

Code: `crates/opensow-formats/src/wave.rs` (`Wave::find`). Verified against the exe's own code in an x86 emulator: `scripts/verify_against_exe.py` section 8 runs `0x4753a0` on 300 random grids (about 70 % routes, the rest failures; densities 0-35 % blocked, base 1 / 500 / 70000, drop 0-2) and compares the success flag, the new base and every route cell with `opensow wave`: 0 mismatches.

## What the exe does (`0x4753a0(this=unit, x, y, drop, mode)`)

- `x, y` are the target in pixels (callers pass the unit's own position) and the start is `[this+0x5c]`, `[this+0x60]` (the destination, in pixels); both are divided by 32, so the route comes back from the unit toward the destination. `mode` is the byte `[unit type + 0x4b]` of the chassis, the **air flag**: nonzero sends the unit to `0x474b30` instead, the same search over the air layer (below). Callers: `0x46f601` and `0x46fd66` (the chassis move/step code); `drop` is 0 for both.
- The grid `0x5fac38` has 128 x 128 `u32`, index `x * 128 + y`. It is refreshed from the map object (`map + 0x80cc`, 64 KiB) by `0x458230` whenever the base counter `[0x49a364]` passes `0x1fffff`; the buildings stamp their handle into the source grid (`0x40e460`). Terrain is `0xFFFFFD`. A cell is free when its value is below the base, so a finished search never has to clear its stamps: it sets the base to `counter + 3`.
- **Wave**: the start cell gets the base, the counter starts at `base + 2` and the target cell is made free (0), so a blocked target (an enemy building) can still be reached. Generations alternate between two lists (8192 cells at most). For each cell: if it is the target, stop; otherwise claim the free cells west, east, north, south (in that order) with the counter; the counter goes up by one; then the diagonals (NW, NE, SE, SW in screen terms `(-1,+1) (+1,+1) (+1,-1) (-1,-1)`), each only when none of the two orthogonal cells beside it was claimed by this cell (masks 9, 0xa, 6, 5). The counter goes up by two after every generation. The search fails when a generation is empty or too big, or the counter passes `base + 0x1f40`.
- **Route**: from the target, repeatedly step to the neighbour (checked in the order `(-1,-1) (0,-1) (1,-1) (-1,0) (1,0) (-1,1) (0,1) (1,1)`) with the smallest stamp that is below the current cell's stamp and not below the base. The route is stored target first, start last (the unit walks it from the end), at most 1000 steps (a longer route is cut there but still counts as found); `drop` removes that many cells from the start end. Start equals target fails (no steps). A dead end (no lower neighbour) fails; an `(0, 0)` neighbour is also treated as a dead end.
- The exe does no bounds checks: cells next to the grid edge read the neighbouring rows, so the real maps keep their border blocked.

**Differs from the exe:** the port skips cells outside the array.

## In the game

`World::find_path_on` (`sim/movement.rs`; search from the destination to the unit, as the exe does) builds a `Wave` per search (blocked = terrain, structure footprints, outside the map) and walks the route as cell centres. Units do not block each other in the exe's grid either. Routes are therefore the exe's: they can cut corners where no neighbouring cell was claimed and are not shortest paths (the stamps only order the wave).

**Differs from the exe:** the exe frees a blocked target cell and searches into it; `find_path_on` first moves a blocked goal to the nearest free cell (range rings up to 12) and searches to that.

## The air layer (`0x474b30`, `0x473f10`, `0x478200`)

The exe keeps two sets of occupancy grids and picks one by the byte `[unit type + 0x4b]`. Only one unit is built with it set, the **Advancer** (ctor `0x46bd30`, kind 40, docs/art.md); the earlier guess "walker kinds 41-45" was wrong. Per set (all 128 x 128 `u32`, index `x * 128 + y`, free when below the set's counter; refresh functions `0x458190 ... 0x458420`, the whole set rebuilt at map load `0x45a40e..`):

| role | ground set | air set |
|---|---|---|
| wave grid + counter | `0x5fac38`, `[0x49a364]` (terrain from map `+0x80cc`, buildings stamped by `0x40e460`) | `0x60ac38`, `[0x49a368]` (terrain from map `+0x180cc`, air units stamped at their pixel cell) |
| static grid + counter (chase, local search) | `0x5eac38`, `[0x49a360]` (map `+0x80cc`, ground units' handles) | the same grid as the wave: `0x60ac38`, `[0x49a368]` |
| unit grid + counter (local avoidance) | `0x61ac38`, `[0x49a36c]` (step cells `[chassis+0x6c/0x70]`) | `0x62ac38`, `[0x49a374]` |
| second step cell grid (`0x475c30` / `0x4732f0`, networked games only, below) | `0x63ac38`, `[0x49a370]` (`[chassis+0x74/0x78]`) | `0x64ac38`, `[0x49a378]` |

The map object holds two terrain grids: `+0x80cc` and `+0x180cc` (the savegame writes one `u32` of each per cell, docs/savegame.md; the `.map` cell carries them as its two `u32`s, `blocked` and `border`, `map.rs`). So **air terrain = the `.map` border layer**: on most maps just the outer ring (252 cells on 64 x 64), on M04, M10 and M22 also rectangular no-fly areas (722 / 966 / 839 cells that are open ground). Lakes and mountains are not obstacles for an Advancer, and no building stamps the air grids.

The three routines are **the same code as the ground versions on the other grids**, verified by the emulator (`scripts/verify_against_exe.py`: sections 8, 10, 11 alternate the layers, 300 random cases each, 0 mismatches against the one Rust implementation):
- `0x474b30(x, y, drop)` = `0x4753a0` (`Wave::find`) on `0x60ac38` / `[0x49a368]`, refreshed through `0x458190` when the counter passes `0x1fffff`.
- `0x473f10(x, y)` = `0x476870` (`Wave::local_step`) with unit grid `0x62ac38` / `[0x49a374]` and static grid `0x60ac38` / `[0x49a368]`; `0x476870(x, y, mode)` dispatches to it for `mode != 0`.
- `0x478200` = `0x477630` (`wave::approach`) for a chase target whose handle type (`0x43aaa0` = bits 19-21 of the handle) is 4, i.e. an **Advancer** (kind 40; the air grid `0x60ac38` is the only one that stamps such handles): `0x477630` hands the whole call over. The chaser's own layer does not matter, the target's does. One quirk of its unrolled block code (`wave::approach_air`, `opensow approach ... air`): of the 7 x 7 block it saves and restores, the cell *three places earlier in the linear index* (`index - 3`) is the one freed when it holds the target's handle, so the strip `y-6..y` beside the block is freed and not restored, and handles at `y+1..y+3` stay blocked. Air grids hold no building handles, so the quirk hardly ever shows.

OpenSOW: same (`find_path_on(true, ..)`, `Occupancy.air` / `air_stat`, `approach_air` in `chase_step`; `sim/movement.rs`).

## The second-step search (`0x475c30`, `0x4732f0`; `Wave::local_step_second`)

Both are **the local avoidance search of `0x476870` / `0x473f10` run over the second step cell grid**: `0x475c30(x, y, mode)` (`mode` = `[unit+0x4b]`; nonzero hands over to `0x4732f0` as `0x476870` does to `0x473f10`) plans the cell *after* the step cell. Same global route `[chassis+0x88]` (count `[+0x8c]`), same look-ahead cell `L = route[min(len-1, 6)]`, same corner closing against the static grid, same wave and same trace, and the same unit state is cleared and rewritten (`[+0xb0]` local route freed, `[+0xb4/0xb8]` = 0, the route stored again). The differences, all found by comparing the two functions instruction by instruction and then confirmed by emulation:

| | `0x476870` | `0x475c30` (ground) | `0x4732f0` (air) |
|---|---|---|---|
| unit grid + counter | `0x61ac38`, `[0x49a36c]` | `0x63ac38`, `[0x49a370]` | `0x64ac38`, `[0x49a378]` |
| static grid (terrain/buildings, `L` obstacle test, corner closing) | `0x5eac38` | `0x5eac38` | `0x60ac38` |
| counter reset when above `0x1fffff` | `0x458420` | `0x458360` | (the air twin) |
| result cell | `[chassis+0x6c/0x70]` | `[chassis+0x74/0x78]` | `[chassis+0x74/0x78]` |
| `L` next to the unit's cell: free when | unit grid < base **and** static grid < `[0x49a360]`; if not: **stop** (destination = unit position), return 0 | second grid < base **and** the first step grid (`0x61ac38`, `[0x49a36c]`) < its base; the static grid is not asked; if not: return 0, **no stop** | second grid < base **and** the first step grid (`0x62ac38`, `[0x49a374]`) < its base; if not: **stop** (destination = unit position, `0x473463`), return 0 |

`at` is the cell of the arguments, which here is the **centre of the first step cell** (below), so it plans from the first to the second step cell. A search that fails leaves everything but the local route as it was. Verified by `scripts/verify_against_exe.py` section 10b (300 random grids, ground and air alternating, with units in both step grids and look-ahead cells next to the unit: 0 mismatches against `opensow local2 ... ground|air`).

**Who calls it: only multiplayer.** `0x46f140` (the chassis step dispatcher) sends a unit to the networked step `0x46fa40` when `[0x575720] >= 1` (that variable is 0 in single player and set to 1 after a multiplayer session is created or joined, `0x4488c0` / `0x4498fb`), otherwise to `0x46f550` (units with orders) or `0x46f4a0` (idle). The two step functions differ in the planner they call: `0x46f550` -> `0x473190` (one step cell: clear the old cell in the first step grid, `0x476870`, fall back to `0x4774c0`, stamp the new cell, drop the first route cell, build the pixel line), `0x46fa40` -> **`0x472f50`**, the only caller of `0x475c30`:
1. clear the old step cells (`[+0x6c/0x70]` in the first grid, `[+0x74/0x78]` in the second), then `step1 = step2` (the old second step cell becomes the step cell);
2. if the unit stands on the cell it just promoted (`position >> 5 == step1`): plan a new step1 with `0x476870` (falling back to `0x4774c0`, then to "destination = own position", exactly as `0x473190`); otherwise nothing is planned for step1;
3. if the destination cell equals step1: `step2 = step1`; else plan step2 with `0x475c30(centre of step1)` (same fallbacks);
4. stamp the unit's handle `[unit+0x14]` on both step cells (first grid and second grid), drop the first route cell, build the pixel line to the step1 centre (`0x46ee80`); a failed plan clears the pixel line and sets both step cells to the unit's own cell.
So in a networked game every unit reserves **two** cells ahead (one in each grid) and other units avoid both: `0x476870` reads the first grid, `0x475c30` the second grid. The single-player game never runs this code. The rest of `0x46fa40` is below.

**Differs from the exe:** OpenSOW has no network play, so it never runs `0x472f50`, `0x475c30` / `0x4732f0` or `0x46fa40`; units always keep the single-player one-cell reservation of `0x473190`. `Wave::local_step_second` and `netwalk.rs` are verified ports that nothing in the game calls yet.

## The fallback `0x4774c0` and the flood `0x4726f0` (`freecell.rs`)

`0x4774c0(this=chassis, x, y)` is what `0x473190` (once) and `0x472f50` (twice) try when the local search fails; `x, y` is the unit's own pixel position, the **destination** `[+0x5c/0x60]` is what it moves. Port: `freecell::free_cell_near` + `Wave::flood_region`, verified by `scripts/verify_against_exe.py` section 10c (300 random grids, ground and air alternating, destination, return value, new base and the whole grid compared: 0 mismatches against `opensow freecell`).

- **Ground** (`[unit+0x4b] == 0`): first `0x4726f0(x, y)` floods the **unit grid** `0x61ac38` (counter `[0x49a36c]`) from the unit's cell: the wave's neighbour order and diagonal rule (W, E, S, N, then the diagonals with masks 9, 0xa, 6, 5), every free cell (value below the base) gets the stamp `base + 1`, a generation of more than 8192 cells ends the flood, the counter becomes `base + 4`, the stamp is returned as the **region id** (the counter is reset through `0x458420` when above `0x1fffff`, like every search). Then the ring list `0x65bbd8` (`rings.rs`: the centre, then ring 1, 2 ... up to entry 32 of the index table, `[0x6b3a98]`) is scanned around the destination cell for the first cell whose unit-grid value is the unit's **own handle** or the region id. So the destination moves to the nearest cell to it that the unit can actually reach, however many units, buildings or walls lie between.
- **Air** (Advancer): no flood and the arguments are ignored; the scan runs over the air unit grid `0x62ac38` and takes the first cell with the own handle or a value **below `0x11170`** (not relative to the counter).
- The found cell becomes the destination `(cell * 32 + 16)` only when it lies inside the map (`[0x71cec0]` x `[0x71cec4]` cells); the return value is 1 whenever a cell was found (even when the map test kept the old destination), 0 when 32 rings hold none. The planner then keeps its old step cell; the global route is searched again by the re-search cooldown `[+0x80]` (every 7th planning round), so the unit walks to the new destination a little later.
- The exe does no bounds checks (the maps keep a blocked border).

**Differs from the exe:** the port skips cells outside the 128 x 128 array.

In the game: `World::retarget_to_free_cell` (`sim/movement.rs`, called from `avoid` when the local search finds no way); test `a_walled_off_destination_moves_to_the_nearest_reachable_cell`.

**Differs from the exe:** the exe keeps the old step cell and searches the new route only on the re-search cooldown; OpenSOW moves the destination (last path point) and searches the route at once, and the unit waits for the next tick instead of keeping its old step cell.

## The networked step `0x46fa40` (`netwalk.rs`)

`0x46f140(this, &x, &y, a, b)` (the chassis step dispatcher, called per unit and tick from `0x46e215` with `a` = seconds and `b` = tick in the second of the **game clock**: the loop counts `b` 0..59 and carries into `a`) runs the hover animation of the Advancer (`0x46f210`), then: in a multiplayer session (`[0x575720] >= 1`) a unit that is not on its destination goes to `0x46fa40`, one that is goes to the idle step `0x46f4a0`; in single player a unit with orders (`[+0xe0]`, set from the order list `0x478ea0`) goes to `0x46f550`, otherwise to `0x46f4a0`.

`0x46fa40` differs from `0x46f550` in how it moves, not in what it plans:
1. **Position from the clock.** The chassis keeps the clock of the start of its current pixel line, `[+0x168]` = seconds, `[+0x16c]` = tick (`0x4706a0` stores the order's own clock there; they are also the two arguments the chase check `0x46eb40` is given). The pixel steps owed since then are the sum of the speed row (`0x5524a0 + [+0x18] * 60`, the table of `0x436df0`) over the ticks from that clock up to `(a, b)` (clocks compare as `seconds * 64 + tick`; `netwalk::steps_owed`). Instead of consuming one line point per step (`0x46f550` shifts the line), the unit stands on `line[steps]` and the line keeps all its points. The static-grid cell (`0x5eac38`, Advancer `0x60ac38`) it leaves is cleared and the one it enters is stamped with its handle, as in `0x46f550`.
2. **End of the line** (`steps >= len - 1`, or no line): the unit snaps to the last point, the line is freed, and the clock restarts at `(a, b)` (the steps beyond the end are lost). Then: **the local player is not player 0** (`0x41f020([0x575730])` = `[0x575720] > 0 && player != 0`; `[0x575730]` is the local player): the unit does not plan; it takes the step cell reserved by the other machine's planner, `prev = step1`, `step1 = step2`, and when they differ builds the line between the two cell centres (`0x46ee80`). **Otherwise** (host, including a unit on its destination): standing on the destination, both step cells are set to the destination cell, the global route is freed (`[+0x88]`, counts `[+0x8c/0x90]` = 0), the head order is removed with `0x470810(0, 1)` (this is `CArray<Order>::RemoveAt(index, count)` on the order list at `[+0xc4]`, element size 0x1c, calling each element's destructor `vtable[1](0)` and moving the tail down; `0x46f950` does the same inline) and the next order is started (`0x470480`); else the cooldown `[+0x80]` counts down and below zero (reset to 6) runs the chase check `0x46eb40` and the global wave `0x4753a0` (failure: the order list is freed and `0x470480` runs); finally the two-cell planner `0x472f50(x, y)` (flow above), whose line the unit follows from the next call on.
3. **Heading without turning.** The wanted and the current facing are both set at once to `0x46b180` from the first line point's cell (the start of the segment; the unit's own cell when there is no line) to the step cell `[+0x6c/0x70]`: no `0x46a190` turn steps, no "within 4 steps" gate. The animation counter `[+0x4c]/[+0x44]` and the frame `[+0x40] = phase * steps + facing` (not for kind 40) follow as in `0x46f550`; the single-player-only sprite block that follows in `0x46f550` is unreachable here (it needs `[0x575720] == 0`).

Port: `NetChassis::step(now, stat, host)` with the callees (`0x41f020`, `0x46eb40`, `0x4753a0`, `0x472f50`, `0x46ee80`, `0x470810`, `0x470480`, order-list clearing) behind the `NetHost` trait; the facing (`0x46b180`) is the real port. Verified by `scripts/verify_against_exe.py` section 12: the exe's `0x46fa40` runs with those callees stubbed and recorded (the planner writes fixed step cells and a line), 300 random cases (moves along the line, line ends, arrivals, failed routes, host plans, guest lines; ground and air grids): position, clock, line, step cells, facing, frame, cooldown, animation, route length, the static grid and the exact call sequence match `opensow netstep` (0 mismatches). The game does not use it (no network play, see above); the whole of a networked unit's movement is therefore: this clock-driven positioning + `0x472f50` + `0x4774c0`.

## Not decoded

The code that consumes the route is in docs/walking.md. The wave gives up after about 8000 processed cells (a 90-cell radius in open ground), which a 64 x 64 mission map never reaches. The order object `0x470480` (start the next order) is in docs/walking.md; the callers' `0x46eb40` chase check is there too.
