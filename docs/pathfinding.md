# Wave pathfinder

Code: `crates/opensow-formats/src/wave.rs` (`Wave::find`). Verified against the exe's own code in an x86 emulator: `scripts/verify_against_exe.py` section 8 runs `0x4753a0` on 300 random grids (about 70 % routes, the rest failures; densities 0-35 % blocked, base 1 / 500 / 70000, drop 0-2) and compares the success flag, the new base and every route cell with `opensow wave`: 0 mismatches.

## What the exe does (`0x4753a0(this=unit, x, y, drop, mode)`)

- `x, y` are the target in pixels (callers pass the unit's own position) and the start is `[this+0x5c]`, `[this+0x60]` (the destination, in pixels); both are divided by 32, so the route comes back from the unit toward the destination. `mode` is the byte `[unit type + 0x4b]` of the chassis: nonzero sends the unit to `0x474b30` instead (not decoded; probably the units that do not follow the ground grid). Callers: `0x46f601` and `0x46fd66` (the chassis move/step code); `drop` is 0 for both.
- The grid `0x5fac38` has 128 x 128 `u32`, index `x * 128 + y`. It is refreshed from the map object (`map + 0x80cc`, 64 KiB) by `0x458230` whenever the base counter `[0x49a364]` passes `0x1fffff`; the buildings stamp their handle into the source grid (`0x40e460`). Terrain is `0xFFFFFD`. A cell is free when its value is below the base, so a finished search never has to clear its stamps: it sets the base to `counter + 3`.
- **Wave**: the start cell gets the base, the counter starts at `base + 2` and the target cell is made free (0), so a blocked target (an enemy building) can still be reached. Generations alternate between two lists (8192 cells at most). For each cell: if it is the target, stop; otherwise claim the free cells west, east, north, south (in that order) with the counter; the counter goes up by one; then the diagonals (NW, NE, SE, SW in screen terms `(-1,+1) (+1,+1) (+1,-1) (-1,-1)`), each only when none of the two orthogonal cells beside it was claimed by this cell (masks 9, 0xa, 6, 5). The counter goes up by two after every generation. The search fails when a generation is empty or too big, or the counter passes `base + 0x1f40`.
- **Route**: from the target, repeatedly step to the neighbour (checked in the order `(-1,-1) (0,-1) (1,-1) (-1,0) (1,0) (-1,1) (0,1) (1,1)`) with the smallest stamp that is below the current cell's stamp and not below the base. The route is stored target first, start last (the unit walks it from the end), at most 1000 steps (a longer route is cut there but still counts as found); `drop` removes that many cells from the start end. Start equals target fails (no steps). A dead end (no lower neighbour) fails; an `(0, 0)` neighbour is also treated as a dead end.
- The exe does no bounds checks: cells next to the grid edge read the neighbouring rows, so the real maps keep their border blocked. The port skips cells outside the array.

## In the game

`World::find_path` (search from the destination to the unit, as the exe does) builds a `Wave` per search (blocked = terrain, structure footprints, outside the map) and walks the route as cell centres; a blocked goal is moved to the nearest free cell first. Units do not block each other in the exe's grid either. Routes are therefore the exe's: they can cut corners where no neighbouring cell was claimed and are not shortest paths (the stamps only order the wave).

## Not decoded

`0x474b30` (the other path routine). The code that consumes the route is in docs/walking.md. Note: the wave gives up after about 8000 processed cells (a 90-cell radius in open ground), which a 64 x 64 mission map never reaches.
