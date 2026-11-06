# Production, economy and buildings (decoded from the exe + game strings)

Code: `crates/opensow-formats/src/production.rs`, sim in `crates/opensow-game/src/sim.rs` (`step_production`). Exe tests: `production_slot_values_match_the_exes_slot_initialiser`.

## Rules (strings 154-168, 324-346 in `langEN.txt`)
- Ground-unit production is **automatic and free**; a progress bar fills, then the unit appears. Upgrading a factory with the Advancer lets it produce **several units at once** (more active slots); tech-levels (medium/heavy units) cost **research points** (500) from a Research lab; the Advancer also captures and repairs buildings.
- Gold mine: the manual says 10 / 20 / 30 g, the exe pays `active + 1` credits per second (2..6); credits buy turrets (keys 1-5). Research lab: `active + 1` research points per second. Windtrap: production speed `30 + 12 * levels` steps/s, i.e. +40 % per level over the base (docs/ai.md, not the manual's 10 / 20 / 30 %). Air base (Air Navigator): its bar fills, then an air strike is available (strings 137-141 "Bomber ready" ...).
- Headquarters produce the Advancer; Light/Medium/Heavy/Walker factories produce ground units/walkers; only the HQ cannot be captured by the Advancer? (string 338, truncated: "Headquarters is the only building ...").

## Level building record (raw words, kinds 100-108)
word 2 = **active slots** (`[bld+0x144]`), words 3-7 = **slot kinds**, words 8-12 = per-slot value (`[slot+0x24]`, used by mission scripts). Examples: M01 medium factory `[7, 13]` (cannon T1, special T1), light factory `[7]`; air base `[31]` / `[32]` / `[30, 32]` (air-force codes 30 Bomber, 31 Carryall, 32 Fighter, 33 Tripler, 34 Asteroid); gold mine `[16, 17]`, research lab `[19, 20]`, wind `[22, 23]` (kinds 16-24 = the upgrade levels "Gold/Research/Energy Influx 1-3", strings 220-228). `opensow level <file.edt>` prints these words for buildings.

## Slot (stride 0x28 from `[bld+0x15c]`, five per building; set up by `0x40d870`, filled by `0x40e8f0`)
`+4` kind, `+0xc` target, `+0x10` progress, `+0x14` rate (30 default), `+0x18` ready flag, `+0x1c` unlock price (500 ground units, 200 others), `+0x24` value.
- Target: **2000** for ground kinds 1-15, upgrade kinds 16-24 and walkers 41-45; **4000** for air forces 30-34 and the pod 46; turret kinds 25-29 and 35-40 are rejected ("Unknown unit").
- Tick (`0x40e640`, once per game tick with the tick-in-second index): `progress += steps_at(row, tick % 60)` (the 60-tick table of docs/projectiles.md). `[0x70f2cc]` is the step of the TUTORIAL script (-1 in every other mission and in multiplayer): in the tutorial `row = 3 * rate` = 90 steps/s (x4 on steps 15-17, ~22 s per ground unit), otherwise `rate + 12 * windtrap levels` = **30 steps/s without windtrap: 67 s per ground unit, 133 s per air strike** (docs/ai.md).
- At the target: air forces add to the player's stock (`0x453e80`, player block `0x70ba50 + player*0xc68`) and show the "ready" message; ground units set the ready flag and the building update (`0x40de60`) spawns the unit (`0x408740`; offsets `[bld+0x124..0x138]` = spawn cell, primary exit, alternative exit; only these two exits exist, see docs/ai.md). The unit is ordered out through the exit and then, for a player factory with a rally point, on to it (docs/rally.md). If both exits are occupied it retries each tick and after 240 ticks says message 5 ("No space for units").
- Building update `0x40de60` also runs the smoke/animation counters, the HP-dependent damage animation and runs the slots. `0x40e3d0` is the Advancer's upgrade (not research): it adds the Advancer rate to `[bld+0x148]` and unlocks slot `[bld+0x144]` when the points reach that slot's target (docs/advancer-actions.md).

## Not decoded yet
See docs/ai.md (economy, AI, missions) and docs/advancer-actions.md. Enemy factories now produce with the same rule as the player's; the only unit limit is the exe's 1000 unit slots per side (`0x43a420` fails at `cmp ecx, 0x3e8`; a corpse holds its slot for 430 steps, docs/effects.md), which the game now uses for both sides instead of the old 60-unit placeholder.
