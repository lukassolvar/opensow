# Ground-unit walking

Code: `formats::wave` (`Wave::find`, `Wave::local_step`), `formats::walk::pixel_line`; used by `World::find_path` / `World::avoid` in `opensow-game/src/sim.rs`. The three routines are verified against the exe in the emulator (`scripts/verify_against_exe.py` sections 8-10: wave pathfinder, pixel line, local avoidance; 300 random cases each, 0 mismatches).

## The chain (chassis step `0x46f550`, per walking step)

Unit object `[unit+0x14/+0x18]` = the unit's pixel position, chassis `[+0x5c/+0x60]` = its destination pixel (set by orders; "arrived" is position == destination). A unit takes `steps_at(speed, tick % 60)` single-pixel steps per tick (docs/art.md); each step:

1. **Arrived** (position == destination): pop the next order from the order list `[chassis+0xc8]` (stride 0x1c) and set up its goal (`0x470480`).
2. **No pixel route** (`[+0xa0] == 0`): count `[+0x80]` down; below zero it is reset to 6 and the global route is searched with the wave `0x4753a0` (start = destination, target = the unit, so the route comes back unit first; docs/pathfinding.md). A failed search clears the orders (the unit stays). Whether or not a search ran, `0x473190` then plans the next cell.
3. **`0x473190(x, y)`** (the unit's position): clears the unit's old cell in its occupancy grid, runs the local search `0x476870` (below), and if that fails tries `0x4774c0` (a free cell near the destination from the 0x436230 ring list) and finally sets the destination to the unit's own position; stamps the unit's handle on the new step cell `[+0x6c/+0x70]`; removes the first cell of the global route; builds the pixel line to the step cell centre with `0x46ee80`.
4. **Each step**: the unit's facing must be within 4 turn steps of the wanted one (`0x46a190`), then the position becomes the next point of the pixel line (one pixel per step) and the line shrinks; when the line is empty step 2 repeats.

Units whose type byte `[type+0x4b]` is nonzero use a second set of grids (`0x62ac38`, `0x60ac38`, and `0x474b30`/`0x473f10` for the search); the game treats the walker kinds 41-45 that way (their own occupancy layer, otherwise the same search - the second search routines are not decoded).

## `0x46ee80` pixel line

Integer Bresenham, start included: `d = 2*minor - major`; each step moves one pixel along the long axis, and along the short axis too when `d >= 0` (then `d += 2*(minor - major)`, else `d += 2*minor`); equal axes count as y-major. `walk::pixel_line`.

## `0x476870` local avoidance (mode 0)

Grids: unit grid `0x61ac38` (terrain `0xFFFFFD`, buildings, other units, each cell holds a handle; free is below the base `[0x49a36c]`), static grid `0x5eac38` (terrain and buildings only, base `[0x49a360]`). `route` is the global route (the unit's own cell first), `L = route[min(len-1, 6)]`.

- No route beyond the unit's cell, or `L` is the unit's cell: no step. `L` holds a building (static grid >= `0x1fffff` and not the unit's own handle): no step.
- `L` next to the unit: step there if it is free in both grids, else stop (the exe sets the destination to the unit's position); no local route is stored.
- Otherwise: the four diagonal corners of the unit's cell are closed (`0xFFFFFD`) where one handle fills both orthogonal cells on that side (unit grid value equals the static value of the other orthogonal cell and is occupied) - so a unit never cuts the corner of a building; then the same wave as `0x4753a0` runs from `L` to the unit's cell over the unit grid (the unit's cell and `L` are free); the route back from the unit's cell (same smallest-stamp walk, at most 1000 steps) gives the next cell and the local route. The corner cells and the cells around `L` are restored afterwards; the base moves past the stamps.
- A failed search gives no step (the game makes the unit wait and retry next tick).

## In the game

`find_path` uses the exe's orientation; before a unit starts a path segment `avoid` runs the local search over a grid built once per tick (terrain, structure handles, unit handles; walkers have their own layer) and replaces the first (up to six) cells of its path by the local route, or makes it wait. Units therefore queue behind each other and walk around each other and building corners as in the original. Occupancy is approximated by the unit's current cell (the exe stamps the step cell it is heading to).

## Orders (`chassis+0xc4..0xcc`, `0x470480`, `0x4706a0`, `0x46e510`)

The chassis keeps a list of orders (`[+0xc8]` data, `[+0xcc]` count; each 0x1c bytes: vtable, valid byte `[+4]`, type `[+8]`, x `[+0xc]`, y `[+0x10]` in pixels, two more words). `0x4706a0(type, x, y, a, b)` replaces the whole list by one order and executes it (when the unit already stands on its destination it also stores `a`, `b` and the facing in `[+0x168..0x170]`); `0x46e510(type, x, y, a, b)` appends (the walker's teleport script uses it); `0x46f950` (arrival) pops the head and `0x470480` starts the next. An empty list gets a type-0 order (do nothing). `0x470480` executes the head:
- type 1: the destination `[+0x5c/+0x60]` becomes (x, y) if inside the map and the re-search cooldown `[+0x80]` is cleared (the next step plans a route);
- type 2: destination as above, the pixel route is dropped and replaced by a straight line from the unit to (x, y) (`0x46ee80`) with the step cell set to the destination cell - a move without path finding;
- type 3: destination only (when inside the map).
Callers of `0x4706a0`: the UI orders (`0x418a42`, `0x4190c8`), the mission scripts (`0x42e644`, `0x42e6db`), the walker teleport and the Advancer/air code.

## Chasing a target (`0x46eb40`, `0x477630`)

`[unit+0x7c]` is the handle of the unit's chase target (set by the AI order `0x469fe0` and by right-clicks on enemies). Whenever the unit would plan a route (idle, or at each re-search) `0x46eb40` checks the target: a dead or vanished target clears it (a building target is dropped when the unit is a kind-40 Advancer); the target cell is the unit's cell or the building's centre (`origin + size/2`). The weapon range `[chassis+0x154]` is `(min + max) / 2` of the weapon top (5 for anti-air tops, 1 without a weapon). If the cell distance (`0x437720`: truncated Euclidean) is at most `range - 1` nothing happens (the unit keeps its orders; the weapon code stops it). Otherwise `0x477630` computes where to stand: `wave::approach`, exact vs emulation (300 random grids, 235 destinations, 65 failures, same destination and same new base): a bucket wave from the unit (orthogonal step = one generation, diagonal = two) until it reaches a cell next to the target, then `range` steps back along the lowest stamps from the *target* cell. The result replaces the order list by one type-1 order for that cell's centre; if there is no way the destination is set to the unit's own position (stop). Building targets get their own cells freed in the 7 x 7 block around them (grid value equals the handle). Handle type 4 goes to another routine (`0x478200`, not decoded).

The game's `chase_step` does exactly this (a Rogon bomb still walks onto the target's cell: the exe's touch test `0x46df40` reads the occupancy layers, which are not modelled).

## Not decoded

The orders' two extra words, `0x4774c0`'s fallback in the game, `0x478200` (chase for handle type 4), the second search family for `[type+0x4b] != 0` units (`0x474b30`, `0x473f10`, `0x475c30`), the networked variant `0x46fa40`, and the exact occupancy stamping time.
