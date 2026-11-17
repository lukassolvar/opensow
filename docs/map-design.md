# How the shipped maps are designed (a study for new maps)

Notes from a study of the 22 missions and the 8 skirmish maps (`.net`), read together with the briefings (lang strings 76-97)
and objectives (98-119). They guide the maps made with plans (docs/generated-maps.md). Measured with a script over the
`.map` grids and the level scripts: "front" is the narrowest cross-section between the two headquarters (the number of
cell-disjoint paths), "path" the walking distance against the straight (Manhattan) one.

## What the numbers say
| Map | Size | Blocked | Front | Path / straight |
|---|---|---|---|---|
| 01-First Spark (M01) | 64x64 | 49 % | 2 | 107 / 59 |
| 02-Ice Blizzard (M09) | 64x64 | 36 % | 2 | 112 / 64 |
| 03-Gold Rush (M12) | 64x64 | 35 % | 4 | 97 / 97 |
| 04-Great Crush (M05) | 96x80 | 42 % | 4 | 128 / 88 |
| 05-Nuclear Devastation (M13) | 96x80 | 45 % | 3 | 181 / 147 |
| 06-Meteor Swarm (M15) | 96x80 | 45 % | 4 | 256 / 144 |
| 07-Cross of Fire (M17) | 96x80 | 51 % | 5 | 129 / 89 |
| 08-Island of Fate (M19) | 96x80 | 37 % | 6 | 153 / 139 |

The missions agree: 29-61 % blocked, front 2-6, path 1.0-2.6 times the straight distance. Every map, however open it
looks, joins the two sides through a few narrow passes.

## The ideas behind them
- **One dominant barrier** decides where armies meet: a river or lake (01, 05, 08; M10, M21), a long cliff wall (06 is
  three parallel walls that make a switchback), water arms meeting in a hub (07: four inlets form a cross, all fights in the
  middle), or a painted structure (04: the wreck lies across the centre, M10's dome).
- **Bases sit in pockets**: a corner or an edge plateau with one or two narrow exits (M21's two enemy plateaus each behind a
  ridge, 05's red base on a strip the lake cuts off, M18's blue corner). The exit is often the map's narrowest point.
- **Lanes of different worth**: a short dangerous route and a long safe one (01: across the middle or around the lake;
  06: every switchback alternates sides; 08: curved ridges make three routes).
- **Side pockets hold the prizes**: the neutral buildings stand in dead ends and on flat ground off the main routes (03's
  gold mines along the edges, 08's island of mines and a lab reached along the shore, M01's lab "left of your initial
  position", M16's gold behind the enemy).
- **The briefings tell the plan**: "seize the research lab left of your start", "occupy buildings to the east and hold",
  "the enemy headquarters on the island", "destroy the turrets that guard the base". Missions are built around a first
  prize near the start, a contested middle and a protected enemy core.
- **Narrow lanes, not fields**: the widest open space away from the bases is about 6 cells; the space between features is
  filled with ridges, rocks and forest so units move in channels. Room still matters: the shipped 96 x 80 maps
  keep 47-73 open 6 x 6 spots for buildings and carryall landings; ours keep 45 outside the start areas, at about 50 % blocked.
- **Worn tracks** (forest maps): reddish trails with ends and vague directions, never lines from base to base.
- **Asymmetry is normal**: only 07 is close to symmetric. Bases differ in exits and nearby prizes.

## Rules for our maps
From these ideas, and from rounds of review:
- **One primary flow**: a single route between the bases that winds like a river; it need not run corner to corner.
- **Side flows** branch off it, some long, each ending in a pocket.
- **Each segment has one entrance**: every pocket is reached only by its branch.
- **Narrow corridors** but room for several units: the flow 6 cells wide, branches 5.
- **Room for buildings and landings** in the pockets and the start areas; the rest walled (about 60 %).
- **Only the map's own kind of terrain**, and no repeated rocks lined up in rows.
- **Terrain only**: the players place headquarters and buildings in the editor.

The maps are listed in docs/generated-maps.md ("The maps made so far").
