# Composed maps (not in the exe)

A custom skirmish map (docs/map-editor-ingame.md) can have a **composed ground**: a new layout put together only from the
player's own mission art, following a written plan. Nothing is drawn or invented: the open ground is quilted from one
mission's picture, and every cliff, rock group, forest, lake and painted structure is a whole piece of a mission, with its
own cells. The map file keeps only the result (record 7 LAYOUT, `formats::custommap::Layout`); every load rebuilds the same
ground from the install (`custom_maps::base_ground`, `opensow-viewer/src/layout.rs`). The pictures keep the missions' own
colours; another look (autumn, volcanic) would be made outside the game.

Maps are designed by people, not by the game: a plan (`docs/map-plans/*.txt`) says which piece goes where, from the ideas in
docs/map-design.md, and the headless command builds it. The game and its editor have no generator. A composed map is terrain
only: the players place the headquarters and buildings in the editor; it opens, plays, saves and loads like any custom map.

## Plans
```
name Amber Ridges
base M18                      ; the ground's mission: the map's size and its open ground
seed 23                       ; fixes the open ground's patches
piece M14 42,49 at 40,18      ; the piece of M14 that holds cell 42,49; its box's top-left goes to cell 40,18
piece M18 1,55 at 0,46 whole  ; `whole`: the whole box (a plateau with the ground its cliffs ring)
structure temple at 38,55     ; a whole structure, its rectangle's top-left at 38,55
start 3,62 18x14              ; a start area: it must stay open; the first two are measured against each other
trail 12,68 30,56 40,42       ; a worn trail through these cells (each snapped to an open cell within 8)
fill 60% gap 0 spots 45 lane 4 M10 M12 M18  ; then fill the open fields with pieces of these missions up to 60 %
                                            ; blocked, keeping 45 building spots and corridors 4 cells wide
```

**Fill** (`layout::fill`), after the placed pieces and structures: while the share of blocked cells is below the target, the
most open spot (the open cell farthest from anything blocked, two cells or more, outside the start areas and two cells
around them) gets the piece of the listed missions that covers the most cells there: a piece of the map's own kind of terrain
of 4-600 cells (one running into an edge goes to the same edge), at most three times each, its cells `gap` or more cells from
anything blocked (`gap 0`: it may join a wall into a larger mass; default 3). A placement is refused when it seals open
ground off from the first start area, creates open cells that no open `lane` x `lane` square covers (a passage narrower than
`lane` cells, default 2), narrows the front between the first two start areas below `lane` + 1 cells, or leaves fewer than
`spots` building spots. A spot no piece fits
is left.

**Building spots**: separate open 6 x 6 squares outside the start areas, counted greedily in reading order: a 4 x 4 building
with a cell around it to drive past, or room for a carryall to land a squad. The shipped 96 x 80 maps have 47-73 (5-11 per
1000 cells, start areas included); the command prints the count. The pieces are stored like placed
ones, so a load rebuilds the same ground.
`opensow-viewer MXX --pieces` lists what a mission offers: per piece a seed cell, its box, its size, the map edges it runs
into and the colour of the open ground around it. A piece that runs into an edge of its mission keeps that edge (a ridge or
lake that runs off a shipped map runs off the new one), and pieces may not overlap; the command refuses a plan that breaks
either, or that blocks a start area.

**Only the map's own kind of terrain.** A forest map takes only pieces and structures from forest ground, an orange map
only from orange ground, a brown map only from brown ground: the command refuses a piece whose surrounding open ground
differs from the base mission's (`layout::SAME_TERRAIN`: the colour shares of the two soils, summed, differ by more than
0.10). Measured soils (R, G, B): forest about 72, 74, 31 (M08's grass, M10, M12, M14's grass, M18, M21), brown about
74, 44, 33 (M01's soil, M03, M05, M08's soil, M17), M00's reddish soil 81, 30, 16, orange about 155, 80, 28 (M02, M04, M07,
M11, M15, M19, M22). Mixed missions (M01, M08, M14) offer some pieces to forest maps and others to brown ones, depending on
the ground each piece stands on.

**Pieces** (`layout::catalogue`): a mission's blocked cells split into 4-connected components after taking out the blocked
outer ring (so ridges that run into the edge come apart). Lakes are the water cells (blocked, blue) of components of at least
20, with the blocked shore within 2 cells. Nothing that touches a never-cut region is offered: M10's dome by the river, M17's
structure in its lake, M05's wreck by its lake, M22's Overmind base, M03's dam and bridge, M02's walker wrecks, and the
structures below.

**Structures** (`layout::STRUCTURES`), pasted whole: `temple` (M11's ruined temple), `tower` and `tower2` (M04's domed
towers). Each rectangle holds the whole picture with open ground around its edge; checked by eye on a plain ground. All
three stand on orange ground, so only orange maps can use them; the forest and brown missions' own structures (M10's dome,
M17's, M05's wreck) are tangled with water and cannot be pasted whole.

## Flows
The maps now shipped are flow maps: a plan draws the open ground, and everything else is walled.
```
start 3,64 14x10                          ; the two start areas, kept open
flow 6 16,69 28,70 36,62 ...              ; the primary flow: one route, 6 cells wide, through these cells
branch 5 28,70 44,74 54,73                ; a side flow off it, 5 cells wide
room 52,68 12x10                          ; a pocket at the branch's end, kept open
wall 97% M08 M10 M12 M14 M18 M21          ; wall 97 % of everything else with pieces of these missions
```
- **Ways** (`way_mask`): each flow and branch is a band of its width around its line; the edge only ever wanders outward (up to
  a cell and a half, 160-pixel noise), so the width is a minimum. Rooms and start areas are added; the outer ring never is.
  The flow also gets a worn trail along it (forest grounds only).
- **Wall** (`wall`): until the share is reached, the uncovered cell deepest in the wall (farthest from the ways and a one-cell
  fringe around them) gets the candidate piece and placement that blocks the most uncovered cells there: every piece of the
  listed missions of the map's own kind of terrain, every own blocked cell as the anchor (a third of them for pieces over 24
  cells), with no own blocked cell on a way or on a placed piece. Wall pieces may overlap each other, so the mass closes.
  Copies of the same piece stay 14 cells apart and big masses (40 cells and up) are used at most three times, small ones ten,
  so rocks do not line up in rows; only a spot no other piece fits takes a repeat.
- **Snow** (blue above red in the source's ground): snow cliffs are plateaus whose blocked cells are only the face, so the
  pasted piece keeps three cells of its own surroundings whole (the snowy tops) before it fades into the ground.
- **One entrance per pocket** (`sealed_rooms`): for each room, a disk across the middle of the branch that ends at it is
  blocked; the room must then be cut off from the first start area. `MapEditor::generate` tries the next seeds (up to 40)
  until every room passes; the command prints the seed it kept.
- **Leaks**: open cells left outside the ways (gaps no piece fits) are counted and printed.
- Structure-bearing missions are left out of a ground's wall list by hand (M04's towers on orange).

## Trails
The forest missions have worn tracks: reddish-brown soil through the grass (`trail_blocks`: 16-pixel blocks of open ground
whose green-to-red ratio is at least 0.18 below the ground's median). The open ground's patches avoid them (a window with
trail texture is not taken while 20 or more clean ones exist), so tracks do not repeat all over the map; trails appear only
where the plan lays them. A trail is routed between its cells over open ground, keeping to the middle of passes (each step
costs more near a wall), smoothed, and dabbed with texture from the base mission's own tracks (areas of 7 x 7 trail blocks,
three cells or more from anything blocked). Like the originals' tracks it is worn, not drawn: it comes and goes (gaps where
300-pixel noise is low), wanders up to a cell and a half to the side, changes width (26-66 px), thins out over its first and
last fifth, and shows unevenly. There are no plazas: trails do not hint where bases go. The brown and orange missions have no
such tracks, so their maps have none (the command refuses a trail there). Stored as trail stamps in the LAYOUT record.

## Build order
1. **Open ground** (`layout::ground`), then the trails (`layout::paint_trails`). The base mission's picture is cut into 128 px patches that overlap by 40 px (image
   quilting). Each patch is joined to those above and to its left along the cheapest seam through the overlap: a
   dynamic-programming cut over the colour difference. The file lists one source position per patch, row by row; each is the
   best-joining of up to 150 windows over open cells of the base mission (not blocked, not no-build, no foreground). The
   cells all start open, except the outer ring, which is blocked and on the air border like the shipped maps'. The map keeps
   the base mission's size.
2. **Pieces and structures** (`layout::paste`), in file order:
   - a piece's own cells' pixels whole; ground-like pixels (soil and grass, compared with the source's ground 40-63 px
     around the piece) near their edge fade along a ragged line, so no square halo shows; the overhang (cliff faces,
     shadows, tree tops) fades out within about a cell; the source's ground colour shifts to the map's;
   - a structure's picture over the map's own ground: the source's soil (by its hue, averaged over 9 x 9 pixels, and no
     brighter than it) gives way to the map's ground, darkened where it lay in shadow (averaged too, fading out towards the
     rectangle's edge); foreground pixels (the tall parts units walk behind) always stay;
   - the foreground where the paste is at least half opaque;
   - the cells: the piece's own take the source's blocked, no-build and no-marks flags; the cells around take no-build and
     no-marks (shores). Stored cell coordinates and the air layer stay the map's.

A missing source (Warmonger not installed) skips its pieces, so a map always loads. The build is deterministic (fixed
noise, fixed seam search), which savegames need: they reload the map and its grids (`World::restore`).

**Differs from the exe:** the original has no composed maps; its skirmish maps are the shipped `.net` files on mission
grounds.

## The maps made so far
Plans in `docs/map-plans/`, saved to the player's maps folder, terrain only:

| Map | Ground | Layout | Blocked | Walk / straight | Pockets | Building spots |
|---|---|---|---|---|---|---|
| Amber Ridges | forest (M18) | south-west to north-east, a river-like flow | 55 % | 171 / 137 | 8 | 55 |
| Lakeland | forest with water (M21) | two M21 lakes split the map; the flow runs between and around them | 63 % | 209 / 81 | 3 | 41 |
| Sunburnt Pass | orange (M07) | west to east: the flow dips south, climbs the middle, comes back down | 60 % | 142 / 80 | 7 | 44 |
| Frost Loop | snow (M13) | north-west to south-east, one long loop through the west and south | 60 % | 180 / 140 | 5 | 40 |
| Ember Loop | brown (M17) | the Frost Loop layout on brown ground | 60 % | 180 / 140 | 5 | 45 |

All have a primary flow 6 cells wide and branches 5 cells wide. Frost Loop holds 434 features (the format keeps 512).

## Headless
```
opensow-game --screen mapeditor --plan docs/map-plans/amber-ridges.txt [--save-map] [--areas] [--spaces] --screenshot out.png --size WxH
```
It prints the pieces, the trail dabs, the blocked share, the front with the cells of its narrowest cut, the path, and the
widest open space: the most cells from an open cell to the nearest blocked one, outside the start areas (the shipped maps keep
it around 6; narrow lanes over open fields). `--spaces` lists every open space of 6 or more instead, to fill them one by one.
A plan's problems (overlaps, edges, terrain, blocked start areas) are all reported at once. Set `OPENSOW_MAPS_DIR`
to keep trial maps out of the real folder; `--save-map` never overwrites a map of the same name.

## Tests
`layout.rs`: the seam follows the cheapest column; a ground of patches copied onto itself is the picture, with the outer
ring blocked; a piece brings its cells, its pixels and keeps the map's ground far away; a plan places its pieces where it
says, measures front and path, and refuses overlaps, blocked start areas and bad lines; fill packs pieces and keeps the front
and the start areas open. `custommap.rs`:
`a_layout_round_trips_with_its_cell_bits`. Real data: `a_generated_map_is_terrain_only_and_the_game_builds_the_editors_ground`
(saved, reopened, the game's picture and cells equal to the editor's) and `a_skirmish_save_on_a_generated_map_loads_back`.

`OPENSOW_WAYS_DEBUG=1` prints the ways (`.`), walls (`#`) and leaks (`o`) as text.

## Not done yet
- Pocket checks and the seed retry cover rooms at branch ends only; a lake that reaches the map edge cannot be cut from a
  mission with an edge of its own unless it keeps that edge.
- No composing in the game or the editor (by choice: maps are designed in plans and shipped as files).
- Pieces cannot be mirrored or turned (the light comes from the upper left on every shipped map).
