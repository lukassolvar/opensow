# Playable prototype (`opensow-game`)

Run: `cargo run --release -p opensow-game` (needs the game install, see README). Code: `crates/opensow-game/src`:
`main.rs` (window, input, CLI), `menu.rs` (screens, layout, actions), `play.rs` (camera, selection, drawing), `sim.rs` (pure simulation, unit-tested), `ui.rs` (software drawing, bitmap-font text), `assets.rs`.
Rendering reuses the viewer library (`opensow_viewer::{art, render, scene}`).

## Screens
- **Main menu**: backdrop `Data\PICTURES\MainMenu.srf` (the Steam 1920x1080 art), buttons from `Data\Sprites2\Menu\001_buttons.tsp` (frames 0-5 highlighted, 6-11 normal: NEW GAME, LOAD GAME, TUTORIAL, MULTIPLAYER, OPTIONS, EXIT GAME). The frames' `extra` field `[640, 480, 320, 240]` is the design resolution and its centre: `hot` is the offset from the screen centre; the Steam backdrop has the logo on the left, so the column is placed under it instead (x 20.6%, y 58% of the window).
- New game = the campaign map with 22 lands and the `mA` animation (docs/lands.md; progress in memory); `L` on the main menu lists every mission.  Options (game speed, health bars, weapon ranges, starter squad), Pause (Esc), mission complete / failed message.
- Load game, Tutorial and Multiplayer show "not implemented".

## Simulation (`sim.rs`) - what is real and what is a placeholder
Real (decoded, see the other docs): chassis walk/turn animation (`anim`), weapon top aiming and fire sequences (`aim`, `recoil`), target search over the range rings with the exe's priority unit > turret > building (`rings`), bullet paths and speeds (`projectile`), area damage radius 32 px incl. friendly fire, unit max HP (`UnitKind::max_hp`), object HP from the level script, anti-air weapons ignore ground targets.
Effects (bullets, blasts, building/unit/air explosions) are drawn with the real sprites (docs/effects.md); `--fx CX,CY,KIND,AGE` (KIND blast0..6, turret, building, air, cloud, wreck) places one for a headless screenshot.
Paths come from the exe's wave search (docs/pathfinding.md); ground units avoid each other and building corners with the exe's local search (docs/walking.md) and wait when blocked.
Decoded since: the computer opponent (docs/ai.md), economy and tech-levels, turret placement and the five turret guns, air strikes and anti-air, Rogon bombs, satellite protection, Advancer actions (docs/advancer-actions.md), win/lose rules, mission countdowns and reinforcements, convoy missions 3 and 8, HQ Advancer replacement, building footprints, the exe's random table.
Keys: left click select/target (with the Advancer: capture, upgrade, repair), right click move or attack, `R` repair/upgrade mode, `1`-`5` then click: turret, `T`: tech-levels, `B M C Y F` then click: air strike (bomber, meteor storm, carryall, tripler, fighter), `Q` select all, space pause.
Placeholders / not implemented (marked `PLACEHOLDER` in code):
- starter squad (8 units next to the Advancer when a mission starts with fewer than 4 units); enemy units are capped at 60;
- no unit collision, no fog (the original has none), menu, map, weapon and explosion sound effects and music but no speech, no HUD icon bars, aircraft are drawn as coloured boxes, no weapon sounds, no savegames/multiplayer/tutorial script;
- the turret placement tiers of the AI follow a simplified threat rule; the AI strategy picks the nearest target by Chebyshev distance instead of the exe's reachability flood fill; unit idle sounds, messages and voices are not played;
- air-strike icons recharge only through the air base's production slots; the exact destroy-animation delay of buildings is skipped.
Victory/defeat: docs/ai.md ("Win, lose and mission scripts").

## Testing without a window
`--screenshot out.png [--size WxH]` renders any screen (`--screen main|levels|options|pause|message`, `--hover N`) or a mission (`--play M01 --select-all --move CX,CY --ticks N --center CX,CY --zoom Z --range --report`). `cargo test -p opensow-game` runs the simulation tests and scripted UI flows (menu navigation, box select, move order, pause) against the install; they skip without one.
