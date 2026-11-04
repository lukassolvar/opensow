# Ground-unit walking

Code: `formats::wave` (`Wave::find`, `Wave::local_step`), `formats::walk::pixel_line`; used by `World::find_path` / `World::avoid` in `opensow-game/src/sim.rs`. The three routines are verified against the exe in the emulator (`scripts/verify_against_exe.py` sections 8-10: wave pathfinder, pixel line, local avoidance; 300 random cases each, 0 mismatches).

## The chain (chassis step `0x46f550`, per walking step)

Unit object `[unit+0x14/+0x18]` = the unit's pixel position, chassis `[+0x5c/+0x60]` = its destination pixel (set by orders; "arrived" is position == destination). A unit takes `steps_at(speed, tick % 60)` single-pixel steps per tick (docs/art.md); each step:

1. **Arrived** (position == destination): pop the next order from the order list `[chassis+0xc8]` (stride 0x1c) and set up its goal (`0x470480`).
2. **No pixel route** (`[+0xa0] == 0`): count `[+0x80]` down; below zero it is reset to 6 and the global route is searched with the wave `0x4753a0` (start = destination, target = the unit, so the route comes back unit first; docs/pathfinding.md). A failed search clears the orders (the unit stays). Whether or not a search ran, `0x473190` then plans the next cell.
3. **`0x473190(x, y)`** (single player; networked games use the two-step planner `0x472f50`, docs/pathfinding.md, "The second-step search") (the unit's position): clears the unit's old cell in its occupancy grid, runs the local search `0x476870` (below), and if that fails tries `0x4774c0` (the reachable cell nearest to the destination, from the 0x436230 ring list; docs/pathfinding.md, "The fallback `0x4774c0`") and finally sets the destination to the unit's own position; stamps the unit's handle on the new step cell `[+0x6c/+0x70]`; removes the first cell of the global route; builds the pixel line to the step cell centre with `0x46ee80`.
4. **Each step**: the unit's facing must be within 4 turn steps of the wanted one (`0x46a190`), then the position becomes the next point of the pixel line (one pixel per step) and the line shrinks; when the line is empty step 2 repeats.

Units whose type byte `[type+0x4b]` is nonzero - only the **Advancer**, kind 40 - use the air set of grids (`0x60ac38`, `0x62ac38`, searches `0x474b30` / `0x473f10`): the same code on grids with the map's border layer as terrain, so Advancers fly over lakes, mountains and buildings and only avoid the map edge, no-fly areas (a few maps) and each other (docs/pathfinding.md, "The air layer"). The game does the same: `World::find_path_on(true, ..)`, `Occupancy.air`/`air_stat`, `World::is_air_blocked`. An Advancer given a move order (type 1, UI right-click) follows a wave route like any unit; the beam approach to a building (`0x408890`, order type 2) is a straight line (`World::order_fly_straight`).

## `0x46ee80` pixel line

Integer Bresenham, start included: `d = 2*minor - major`; each step moves one pixel along the long axis, and along the short axis too when `d >= 0` (then `d += 2*(minor - major)`, else `d += 2*minor`); equal axes count as y-major. `walk::pixel_line`.

## `0x476870` local avoidance (mode 0)

Grids: unit grid `0x61ac38` (terrain `0xFFFFFD`, buildings, other units, each cell holds a handle; free is below the base `[0x49a36c]`), static grid `0x5eac38` (terrain and buildings only, base `[0x49a360]`). `route` is the global route (the unit's own cell first), `L = route[min(len-1, 6)]`.

- No route beyond the unit's cell, or `L` is the unit's cell: no step. `L` holds a building (static grid >= `0x1fffff` and not the unit's own handle): no step.
- `L` next to the unit: step there if it is free in both grids, else stop (the exe sets the destination to the unit's position); no local route is stored.
- Otherwise: the four diagonal corners of the unit's cell are closed (`0xFFFFFD`) where one handle fills both orthogonal cells on that side (unit grid value equals the static value of the other orthogonal cell and is occupied) - so a unit never cuts the corner of a building; then the same wave as `0x4753a0` runs from `L` to the unit's cell over the unit grid (the unit's cell and `L` are free); the route back from the unit's cell (same smallest-stamp walk, at most 1000 steps) gives the next cell and the local route. The corner cells and the cells around `L` are restored afterwards; the base moves past the stamps.
- A failed search gives no step (the game makes the unit wait and retry next tick).

## In the game

`find_path` uses the exe's orientation; before a unit starts a path segment `avoid` runs the local search over a grid built once per tick (terrain, structure handles, unit handles; walkers have their own layer) and replaces the first (up to six) cells of its path by the local route, or makes it wait. Units therefore queue behind each other and walk around each other and building corners as in the original. Occupancy is the unit's current cell plus the step cell it is heading to (the next path point): `0x473190` clears the old step cell, plans, and stamps the new one (`[+0x6c/+0x70]`), so two units never take the same next cell; the game stamps the new step cell at once, so units planning later in the same tick see it. A unit plans (`avoid`) whenever it has no pixel line: after each finished segment and after every new route (`order_move` drops the line; the chase re-orders every 7 ticks, and keeping the old line made units skip planning and walk into units that had stopped in weapon range). The Rogon bomb avoids like every other ground unit: the exe's movement code has no branch on kind 46 (docs/ai.md); the game's `avoid` runs for it too (it used to skip pods on a guess).

## Orders (`chassis+0xc4..0xcc`, `0x470480`, `0x4706a0`, `0x46e510`)

The chassis keeps a list of orders (`[+0xc8]` data, `[+0xcc]` count; each 0x1c bytes: vtable, valid byte `[+4]`, type `[+8]`, x `[+0xc]`, y `[+0x10]` in pixels, two more words). `0x4706a0(type, x, y, a, b)` replaces the whole list by one order and executes it (when the unit already stands on its destination it also stores `a`, `b` and the facing in `[+0x168..0x170]`); `0x46e510(type, x, y, a, b)` appends (the walker's teleport script uses it); `0x46f950` (arrival) pops the head and `0x470480` starts the next. An empty list gets a type-0 order (do nothing). `0x470480` executes the head:
- type 1: the destination `[+0x5c/+0x60]` becomes (x, y) if inside the map and the re-search cooldown `[+0x80]` is cleared (the next step plans a route);
- type 2: destination as above, the pixel route is dropped and replaced by a straight line from the unit to (x, y) (`0x46ee80`) with the step cell set to the destination cell - a move without path finding;
- type 3: destination only (when inside the map).
Callers of `0x4706a0`: the UI orders (`0x418a42`, `0x4190c8`), the mission scripts (`0x42e644`, `0x42e6db`), the walker teleport and the Advancer/air code.

## Chasing a target (`0x46eb40`, `0x477630`)

`[unit+0x7c]` is the handle of the unit's chase target (set by the AI order `0x469fe0` and by right-clicks on enemies). Whenever the unit would plan a route (idle, or at each re-search) `0x46eb40` checks the target: a dead or vanished target clears it (a building target is dropped when the unit is a kind-40 Advancer); the target cell is the unit's cell or the building's centre (`origin + size/2`). The weapon range `[chassis+0x154]` is `(min + max) / 2` of the weapon top (5 for anti-air tops, 1 without a weapon). If the cell distance (`0x437720`: truncated Euclidean) is at most `range - 1` nothing happens (the unit keeps its orders; the weapon code stops it). Otherwise `0x477630` computes where to stand: `wave::approach`, exact vs emulation (300 random grids, 235 destinations, 65 failures, same destination and same new base): a bucket wave from the unit (orthogonal step = one generation, diagonal = two) until it reaches a cell next to the target, then `range` steps back along the lowest stamps from the *target* cell. The result replaces the order list by one type-1 order for that cell's centre; if there is no way the destination is set to the unit's own position (stop). Building targets get their own cells freed in the 7 x 7 block around them (grid value equals the handle). Handle type 4 (an Advancer target) is the same search over the air grid (`0x478200`, `wave::approach_air`, with a block-clearing quirk; docs/pathfinding.md); the game uses it for targets of kind 40.

The game's `chase_step` does exactly this (a Rogon bomb uses the same chase with range 1 and explodes once the target is in the 3 x 3 block around it, docs/ai.md).

## Not decoded

The orders' two extra words and the exact occupancy stamping time. The networked variant `0x46fa40` (position from the game clock, planner `0x472f50`, the second-step search `0x475c30` / `0x4732f0`) and the fallback `0x4774c0` are decoded and verified (docs/pathfinding.md); the game uses `0x4774c0` (`World::retarget_to_free_cell`) and, having no network play, not the rest.
