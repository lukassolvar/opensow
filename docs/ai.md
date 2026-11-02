# Computer opponent, economy and tutorial state (decoded from the exe)

Code: `crates/opensow-game/src/sim.rs` (`step_ai`, `step_economy`, `tech_up`, `place_turret`), `crates/opensow-formats/src/production.rs`, `advancer.rs`.

## Who runs the AI
Per tick and per player (loop `0x42da61`): the local player reads input; every other player (not the neutral index 2) runs `0x42f490(player)` when there are no remote players (`[0x575720] == 0`), `[0x517ae9] == 0` and **`[0x70f2cc] == -1`**. `[0x70f2cc]` is the **tutorial step** (0..0x1b, written ~60 times by the tutorial script `0x432d6e..0x433b6f`), -1 everywhere else; so the AI plays every mission and skirmish except the M00 tutorial, and the tutorial has its own script. (`[0x49a33c]` is the mission/game number; `0x42e580` holds small scripts for missions 1, 3 and 8, `0x42ebd0` the reinforcement timers of missions 7, 12 and 17.)

## `0x42f490(player)` in order
1. **Tech-levels**: for every building and every active slot with `level < max level` the AI buys the next tier when research >= the slot's price (`0x40e730`).
2. Finds an enemy HQ `[ebp-0x10]` and an enemy Advancer, counts enemy turrets (`esi`).
3. **Strategy** `[player+0x70ba88]` (1..4) with timer `[+0x70ba8c]`: re-rolled when the timer ends (rand(4)+1, next in 900+rand(600) ticks) or immediately when (no enemy turrets and strategy 2), (more than 4 enemy turrets and strategy != 2) or strategy 0. Afterwards: no enemy turrets and strategy 2 -> 4; more than 4 turrets -> 2.
4. **Idle units** (no target `[unit+0x7c]`, top alive, not the Advancer, `[unit+0x4c]` = 0) are ordered to attack (`0x469fe0`, the unit then chases with a path replanned every 7 ticks, `0x46eb40`): target classes come from a bounded flood fill over the walkable map (`0x472a90`, 8000 cells, "Unit find way unreachable!!!"): nearest enemy unit, nearest turret, nearest other building. Strategy 1 = HQ; 2 = turret, unit, building; 3 = unit, turret, building; 4 = building, turret, unit; fallback the HQ; missions 3 and 8 always unit then HQ. Units with `[+0x4c] != 0` (anti-air) go for the enemy Advancer.
5. **Advancer** (`0x42fae0`, only when idle): every building gets `rand(3)`; HQs -10000; own turrets and own buildings without free upgrade slot -10000; foreign satellite-protected buildings (`[b+0x114]`) -10000; foreign turrets +2 (anti-air turret 26 another +2); foreign buildings + their active slots; minus the distance; -2 for each already chosen target within 5 cells; the best building is destroyed (foreign), upgraded (own) or repaired (own turret, never chosen in practice).
6. **Air strikes**: every 600 ticks `[0x70f2c8]` it picks Bomber/Fighter/Asteroid at random, and if it has stock and `0x435ad0` finds a target position it launches (`0x467ee0`, `0x455520` decrements the stock).
7. **Turrets** (`0x431120`, once credits >= 200, up to twice per call): for each own non-turret building it sums the units within its rectangle (+4/-3 cells around): own units -1, foreign +1, walkers and kinds 109-113 +2; `[b+0x264]` = threat, `[b+0x268]` = 6 minus 2 per neighbouring cell that already holds a turret-type object. The best building gets the best tier the credits allow (thresholds 200/400/600/800/1000/1500, tier flags `[player+0x70baf5..f9]` = unlocked by "New Turret available"), at one of eight positions around it (`[b+0x224+i*4]`, `[b+0x244+i*4]`) that passes the placement test `0x408be0`.

## Economy (`0x42d6b0` tick loop)
- Credits `[0x517af8 + p*4]`, research `[0x517b04 + p*4]` start from level header words **2/3** (credits) and **37/38** (research) of player/enemy (`0x41d9c5`); M01: 400 research, the "400 research points" of the tutorial.
- **Once per second** (the block of `0x42d6b0` only runs on the tick whose in-second index `[0x517b10 + i*4]` is 0) every finished building (`[b+0x68] == 0`: not healing in, not destroyed) adds: gold mine (105) `active + 1` credits (cap 99999), research lab (106) `active + 1` research points (cap 9999) for active = 1..5 (2/3/4... per second; the tutorial text 'two research points in a time unit' matches); when credits cross 200/400/600/800/1000 the sound "new turret" plays, at 500 research "new tech-level".
- Turret prices 200 / 400 / 600 / 800 / 1000 (`0x4172ea`); placement writes a new building (`0x439e30`) that heals in at 150 hp per tick.
- Tech-level (`0x454c19` + `0x40e730`): costs the slot's `+0x1c` price in research (500 ground, 200 others), needs `level < max level` (`slot+0x24`, level words 8-12), turns the slot kind into the next tier (`production::next_tier`: 1-2-3, 4-5-6, ..., 16-17-18 gold, 19-20-21 research, 22-23-24 energy), keeps progress.
- Windtrap (107): `[0x5baaa0 + p*4]` = sum of the active slots of the player's windtraps; normal production speed is `30 + 12 * windtrap levels` steps/s (`0x40e640`): 67 s per ground unit without windtrap. In the tutorial the speed is 90 (360 on tutorial steps 15-17).
- Factory exits (constructor `0x40a330`): light factory spawns at (+1,+1) and walks to (+3,+3); medium, heavy and walker factories spawn at (+2,+1) and walk to (+4,+3) (`production::factory_exit`, exe-tested).

## Level header words (block `0x517970` = header word 2 onward)
| header word | meaning |
|---|---|
| 2 / 3 | start credits of player / enemy |
| 4-18 | air-force start stock (docs/air.md) |
| 31 / 32 | Advancer strength of player / enemy (x4 = `rate`, usually 10) |
| 37 / 38 | start research points of player / enemy |
| 64 | weather (`[0x49a354]`, 2 draws the snow/rain effect object) |
| 65 / 66 / 67 | countdown enabled / minutes / end mode (0 victory, 1 defeat, 2 reinforcements only) |

## Win, lose and mission scripts (`0x4212f0`, `0x42ebd0`, `0x42e580`)
- **Defeat** (state 9): the player has no headquarters (kind 100). **Victory** (state 0xa): the enemy has no headquarters; in missions 4, 8, 13, 16, 21 and 22 instead: the enemy has no units and no buildings. Mission 3: defeat without a pod (kind 46) of the player; victory when a pod stands at x cells 4-6, y cells 0x48-0x4a. A countdown that ends: mode 0 victory, 1 defeat, 2 reinforcements.
- Countdown (`[0x5baac4]` minutes, `[0x5baac8]` seconds, once per second) with messages at 20, 10, 5 and 2 minutes in missions 7, 12 and 17; at 0: mission 7 +5000 credits and 2 carryalls at (0x8c0,0x600) and (0x9a0,0x480); mission 12 +10000 credits, +2500 research, 3 asteroids, 3 bombers and 3 carryalls at (0x600,0x4c0), (0x640,0x420), (0x600,0x2e0); mission 17 +5000 credits, +5000 research and 2 carryalls at (0x9a0,0x560), (0xb40,0x660). Timer missions with end mode 1: M15 30 min, M16 40, M21 and M22 50.
- Mission 1: when slots 0 and 1 of the player's medium factory reach their level limit the player gets 8 fighters and 3 carryalls.
- Header word 67 of M07/M12/M17 is 2, of M15/M16/M21/M22 is 1.

## Not decoded
The placement test `0x408be0`, the air-force target pick `0x435ad0`, mission scripts of missions 1/3/7/8/12/17, the tutorial script, the BFS flood fill's exact walkability layers, `[unit+0x4c]`.

## Footprints and turret placement (`0x409760`, `0x486b20`, `0x408be0`)
`crates/opensow-formats/src/buildings.rs` (exe-tested). Size (w x h) in cells: turrets 25-27 1x1, 28-29 1x2, HQ 4x4, light factory 3x3, medium and heavy 5x4, air base 3x4, gold mine 3x3, research lab 3x3, windtrap 3x4, walker factory 4x3. The layout mask (25 entries, row stride = width) marks the blocked cells; the top row is always open (the HQ blocks rows 1-3, the light factory 5 of its 9 cells, a 1x1 turret its cell). A turret needs every footprint cell free of units, buildings and blocked terrain **and** a building of the same player within two cells of the footprint (`can_place`).

## More decoded details
- `[unit+0x4c]` (ctor `0x46bd30`, tables `0x46c13c/0x46c1b4`) = anti-air flag, set for kinds 1, 2, 3, 42 (and 110): these units are the ones the AI sends after enemy Advancers.
- AI air-strike target (`0x435ad0`): an idle enemy pod (kind 46) first; otherwise a 24x20 grid of 128-px blocks: every non-Advancer unit adds to its block (+3), left/right (+2), above/below (+1) for enemies (x2 for an idle walker 41-45) and subtracts 3/2/1 for the AI's own units; the best block is the target if its sum is at least 9 (centre of the block).

## Turrets and their guns (`0x40e004`, jump table `0x40e3b4`; tops decoded from `0x47a000..`)
A turret building gets its gun top only when it has finished healing in (`[bld+0x68]` turns 0): turret kinds 25..29 create the tops **109..113 in order**. Top parameters (`aim::aim_params`, `projectile::weapon_shot`, `recoil::fire_sequence`; exe-tested):
| turret | top | range (cells) | reload (+jitter) | bullet type / strength | notes |
|---|---|---|---|---|---|
| 1 Guard Cannon | 109 | 0-6 | 72 | 6 / 39 | cannon shell |
| 2 A.A. Defence | 110 | 0-9 | 120 (+24) | 1 / 15 | anti-air flag `[+0x4c]`, four rocket tables; shoots aircraft only |
| 3 Plasma Mob | 111 | 0-7 | 39 (+12) | 11 / 40 | |
| 4 LedStorm | 112 | 0-8 | 1 | 7 / 16 | rapid fire (3 steps of 6 ticks), tur_01 flash set |
| 5 | 113 | 2-10 | 70 | 2 / 6, 6 projectiles per shot (`[top+0x30]` = 6) | artillery-like |
Targeted bullets (event types 1, 10, 31: anti-air and flamer shots) do not splash: the event takes its strength off the one target id it carries (an aircraft's `[+0x2c]` hit points, or a unit's `[+0x34]`), so anti-air units reduce aircraft hit points directly.

## Rogon bomb (kind 46) and event type 35 (`0x46df40`, `0x46a4f0`, `0x411220`)
- The bomb chases its target (`[unit+0x7c]`); `0x46df40` kills it (`[unit+0x34] = 0`) when the target id appears on its cell in any of the nine occupancy layers (`0x5eaa34..0x5eae3c`): it explodes by touching. Its death handler `0x46a4f0` always creates the type-35 event `0x411dc0(0x23, 0, x, y, x+20, y+20, owner)`.
- The event (`0x411220`): a counter starts at 20 and gains the elapsed ticks each call; while it is above 1 it takes one step: `k++`, blast centre `(x + 16 + 32*w[k], y + 16 + 32*w[k+1])` with `w` the ring offset list of `0x65bbd8` read as overlapping word pairs (a quirk of the exe), area damage `0x42f2c0(.., 370 - k, owner, 35)`, a random explosion sprite (30 frames) and every 17th step a sound. It ends when `k` reaches `[0x6b3a34]` = twice the offset count up to ring 7. So ~19 blasts at once, then one per tick, damage falling from 370 to about 30 over ~5 seconds. HQs are immune to this bullet type.
- Game: right-click on an enemy attacks it (the units chase, `Unit::chase`); pods detonate on touching the target (`World::nukes`).

## Headquarters and the Advancer (`0x42e38d`, `0x41e030`)
Each headquarters links an Advancer (`[bld+0xfc]` = its id): at level load every HQ takes the first free Advancer of its side. Every frame a living HQ whose Advancer is gone creates a new one at HQ cell + (1,1) (`0x43a420(.., 0x28, ..)`, sound 9) unless the level header forbids it for that side: header word 34 (player) / 35 (enemy) nonzero = no replacement (M01 enemy, M03/M04/M08 both). `World::step_hq`.

## Convoy missions 3 and 8 (`0x42e580`, `0x4212f0`)
The player's Rogon pods are driven by script: mission 3 sends every pod to (176, 2352) px each frame, mission 8 sends pods west of x 2880 and south of y 352 to (2960, 272). Both lose as soon as the player has no pod; mission 3 has no other rule and is won when a pod stands on cells x 4..6, y 72..74 (the game does exactly this, `World::step_convoy`, `check_outcome`).
