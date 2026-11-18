# Skirmish map editor (internal tool, not in the original)

An internal tool for adjusting the shipped skirmish maps (`*.net`) for balance. It is not part of the game players get: it lives in the viewer, and its result is a small text file in the repo that the game compiles in. Players make their own maps with the in-game editor (docs/map-editor-ingame.md), which shares this tool's editing state.

```bash
cargo run --release -p opensow-viewer -- --edit "08-Island of Fate"
```

`--edit` takes a map name or its start (any case); without one it opens the first map, and a name that matches nothing lists them. The window shows the map on the left and the editor's sidebar on the right; everything is a button, keys are optional.

## Using it
The sidebar is built from the game's own pictures (building and unit sprites, the HUD's production-line icons `extras\ikony`) and a few drawn icons; every button explains itself in a tip when the mouse rests on it. A row of buttons shows labels only when all of them fit, else pictures only.
- **Top**: the save state ("Unsaved", "Saved, n edits", "As shipped"); `<` `>` switch to the previous / next `.net` map (refused while there are unsaved changes); Save (writes `data/skirmish-maps.txt`, the other maps' sections and the file's leading comments stay), Revert (back to the shipped map, undoable), Undo, Redo. Closing the window with unsaved changes asks once (close again to discard). Pan with a drag on empty ground or the arrow keys, zoom with the wheel; the HQs are ringed in their side's colour.
- **Balance card**: each side's Advancer reach over the neutral buildings, worth / (1 + flight / 16) summed (`editor::reach`; the worth table and weighting are the port's own, not in the exe), with a bar split blue / green and a tick in the middle. Close numbers mean neither side's Advancer has more within reach.
- **Selection card**: click a building, turret or unit on the map: its picture, name, cell and flight distance from each HQ (blue / green chips), a delete button (top right) and owner buttons (units and HQs are never neutral, a side keeps one HQ). Drag it to move it: the ghost frame is green where it fits and red where it does not; a refused drop says why under the panel.
- **Place**: the owner of new things, a tab (buildings, turrets, units: pictures) and the palette; every click on the map then places one, centred on the mouse, with the same green / red ghost. Click the picture again, the cross, or right click to stop. Picking a unit or HQ while Neutral is chosen switches the owner to Blue.
- **Production** (opens when a factory, air base, mine, lab or windtrap is selected): the building's production lines (docs/production.md). Lines: **Active** (`-` / `+`: how many produce from the start; the Advancer unlocks the others) and one row per line with its HUD icon (bright when active from the start, dim when the Advancer unlocks it), its name, a tier button (`T1-3`: the tier it starts at and the highest research can raise it to; click to cycle T1, T1-2, T1-3, T2, T2-3, T3), `<` `>` to change the unit class (it keeps its tiers), air force or walker, and a cross to remove it (one stays); **Add line** (five at most). On small windows the rows shrink to one line each (the name moves into the icon's tip). Mines, labs and windtraps: their three level slots (bright: unlocked at the start, dim: the Advancer unlocks it, dashed `+`: add a level; click a level to make it the last unlocked one, again to lock it), **Unlocked** `-` / `+` and **Remove last level**. The HQ, turrets and units have none.
- **Placement rules** (`Editor::can_place`): a building or turret needs its whole footprint (`buildings::footprint`) inside the map, on passable cells without the map's no-build flag, and off every other object's footprint; a unit needs a passable cell outside every building's footprint (their open top row aside).
- Optional keys: Del (delete), Esc (stop placing, else deselect), Ctrl+S, Ctrl+Z, Ctrl+Y / Ctrl+Shift+Z.

The game picks the file up on its next build (`cargo run` rebuilds); `skirmish::rebalance` applies it when a skirmish loads the map.

**Differs from the exe:** the original plays the shipped `.net` files unchanged; only OpenSOW skirmishes see these edits (the network lobby and the files on disk stay as shipped).

## The file `data/skirmish-maps.txt`
Edits of the shipped scripts, one section per map, readable and editable by hand:

```text
[06-Meteor Swarm]
move mine 27,62 -> 75,47
owner lab 47,3 blue
remove wind 2,29
add heavy 40,30 neutral
move unit advancer 18,4 -> 20,6
add unit cannon1 20,20 green
slots medium 36,72 active 2 cannon1+1 special1+1 artillery1
add air 50,40 neutral active 1 fighter bomber
stock fighter 0
```

- `slots` sets a building's production lines: `active N` (object word 2) and up to five lines (words 3-7 the kind, 8-12 the tech-levels it may still rise): `cannon1+1` = a cannon line at tier 1 that research may raise once; `gold1 gold2 gold3` the levels of a mine (`research1..3` lab, `energy1..3` windtrap); `bomber carryall fighter tripler asteroid` for an air base; `walker1..5 pod` for a walker factory. An `add` of a building may carry its lines after the owner; without them it gets those of the first building of its kind in the map.
- `stock <bomber|carryall|fighter|tripler|asteroid> N` sets the air force both sides start with (header words `5+3g` / `6+3g`; the editor keeps it, `maptweak::diff` writes it back when both sides differ from the shipped file alike).
- Kinds: `hq light medium heavy air mine lab wind walker turret1..turret5` (or the number); units: `unit antiair1..3 artillery1..3 cannon1..3 flamer1..3 special1..3 advancer walker1..5 pod` (or the number). Owners `neutral`, `blue`, `green` (units only blue or green).
- An edit names its object by kind and its cell **in the shipped file** (`X,Y#2` = the second of that kind on that cell). All edits of a map are resolved against the original first and applied together, so they never chain; one that names nothing is skipped with a message (`maptweak::apply`). The editor writes removals first, then moves and owner changes, then additions (`maptweak::diff`, the inverse of `apply`).
- An added building copies the words of the first building of its kind in the map (factory slots and levels), else `maptweak::template` (one active slot and the slots the shipped maps give that kind; turrets none). A moved unit keeps its offset inside the cell; an added one stands in the middle of its cell.
- No game data is stored: only kinds, cells and owners.

## Code and tests
- `opensow_formats::maptweak`: the file (parse, write, apply with origins, diff); tests with synthetic levels.
- `opensow_viewer::editor`: the editing state (pick, can_place, move, add, remove, owner, production lines `change_lines`, undo/redo, revert, reach) and the marks on the map; `opensow_viewer::editor_ui`: the sidebar as one widget list that is both drawn and hit-tested (test: at 640x480 .. 2560x1440 every widget stays inside the panel and no two buttons overlap, with the palette and with a five-line factory's production; every palette item is reachable); `main.rs`: the window.
- `skirmish::rebalance` / `load_map` in the game; real-data test that every tweak names an object and lands on free buildable ground.
- Headless: `opensow-viewer --edit MAP --screenshot out.png [--size WxH] [--select CX,CY] [--tool NAME]` (`--select` selects what stands on that cell and shows its production lines if it has any; with `--tool mine` / `--tool "unit cannon2"` it arms that palette item and shows its ghost at that cell).
