# Air forces

Code: `crates/opensow-formats/src/air.rs` (`AirForce`, `AirStock`, `Level::air_stock`). Test: `tests/real_data.rs::air_force_stock_*`.

## The five forces
| code | force | sprites (`specials\`) | notes |
|---|---|---|---|
| 30 | Bomber | `bomber_l/r` (3 frames each), bombs `bomba_l/r` (9), `bomba_ex` | |
| 31 | Carryall | `carryAll` (5 rotor frames), `carryAll_down`, `carryAll_rear` | long transport; the "reinforcement" aircraft (unconfirmed) |
| 32 | Fighter | `fighter` (1 frame, a jet), `fighter_smoke`, `fighter_bomb`, `fighter_ex` | manual: immune to anti-air units and turrets (8000 HP); "Fighter Assault" |
| 33 | Tripler | `tripler` (10 frames, three-pod craft), `tripler_ph` (20) | "Tripler Strafe" |
| 34 | Asteroid | `meteor_medium/small` + `_aura` + `_smoke` (16 frames each) | no aircraft: a meteor strike |

## How it was decoded
- Aircraft draw routine `0x467240` indexes `[obj+0x28] - 30` (0..3) into bomber / carryall / **fighter (32, one frame, `0x5ea3a4`)** / **tripler (33, ten frames, `0x5ea328`)** sprite arrays (an earlier version of these notes had 32/33 swapped); code 34 only appears with the meteor sprites (`0x410xxx` cluster).
- Speech events 17-21 (`0x403340` switch) = bomber, carryall, asteroid, fighter, tripler "ready" (strings 136-140; a different order than the codes). Icon tooltips are strings 200-204: Bomber Attack, Meteor Storm, CarryAll, Tripler Strafe, Fighter Assault (= header order 30, 34, 31, 33, 32).
- Icon/panel order is 30, 34, 31, 33, 32 (switch at `0x416d00`) and the level header uses the same order.

## Level header: starting stock
Header words `4+3g, 5+3g, 6+3g` for group `g` in (Bomber, Asteroid, Carryall, Tripler, Fighter) = codes 30, 34, 31, 33, 32 (the loader queues exactly this order) = counts for side 0, 1, 2.
- Campaign (`.edt`): loader `0x41d88e..0x41d9bd` queues the type code that many times for side 0 (the human) and side 1 (the enemy); every shipped campaign level has
  side 0 = 0 (the player earns forces) and side 2 = 0. Examples: M10 enemy = 4 bombers, 7 carryalls, 4 triplers, 6 fighters; M05 = 10 fighters; M04/M06 = 6 fighters.
- Multiplayer (`.net`): the same count appears in slots 1 and 2 (e.g. Island of Fate: 12 fighters each, Ice Blizzard: 3 carryalls + 2 fighters each). The MP path that reads
  these was not traced, so which of slots 1/2 belongs to which player is unverified.
- Air forces are never level objects or markers; they are launched from the player's stock, and the Air base (object kind 104) presumably rebuilds it ("X ready" events).

## Behaviour (`aircraft.rs`)
Aircraft are objects in per-side arrays (`0x5ba910`, 30 slots per side, counts `0x575758`) drawn sorted by y (`0x420280`). Launch `0x467ee0` (called from the air-strike click handlers, lockstep RNG slot via `0x43a9b0`) -> `Aircraft::launch`, verified field by field against the exe (`opensow launch`, 300 random cases); the per-tick update is `0x467b50` (per-type cases at jump table `0x467ec4`), death `0x467850`, spawns `0x468240`, carryall unload `0x468960`. Ported from reading, not emulation-verified: `Aircraft::step`.
- Common: speed 120 (steps per 60 ticks, `projectile::steps_at`), altitude 200, death timer 180. Enter from the map edge opposite the target: target x > 1024 -> from the right (x = map width + 400, flying left), else from the left (x = -400). Removed past x = -400 / width + 400, or after the death timer once HP <= 0.
- HP / frames: bomber 480 (3 frames, 3 ticks each), carryall 360, tripler 270 (10 frames, 5 ticks each), fighter 8000 (altitude 120, always from the left, 12 px per tick = 720 px/s), asteroid 120 (no aircraft: x 1000, y -8000).
- **Bomber (30)**: slows near the target: speed 120 minus 0/15/30/40/50/60 when |dx| to the target is >= 190 / < 190 / < 180 / < 170 / < 160 / < 150. While `|x + reach - target_x| < 150` (reach = -45 flying left, +45 flying right) it drops a bomb every `rand(2)+4` ticks: bullet type 18, strength 110, from (x+dx, y-altitude+dy) to (x+reach+dx, y+dy) with dx in -13..12, dy in -20..19 (carpet bombing, ~60 bombs per pass).
- **Fighter (32)**: two salvos per pass, each two type-26 missiles (strength 150, speed 150) at x >= target_x - 226 and x >= target_x - 182: from (x-4, y-alt+22) to (x+204, y-8) and from (x, y-alt+42) to (x+204, y+12).
- **Tripler (33)**: moves in 5-tick steps of 10 px (|dx| >= 170) down to 2 px, hovers when |dx| < 7 and fires six pods one after another, each on a fixed animation frame (frame 0 for the 1st/2nd/3rd, frame 6 for the 4th/5th/6th; cooldown 12 ticks, 52 after the third), as type-17 bullets (speed 360) dropped straight down 20+35 px with per-pod offsets (y,x): (-55,-80) (83,-23) (-30,96) (9,-109) (66,69) (-72,41); then leaves. Their launch strength is 0, but the impact event (type 17, `0x40f590` -> `0x40fd30`) runs 34 sub-blasts: for the first 34 pairs of the ellipse table built at `0x412520` (`projectile::TRIPLER_RING`, radii about 64 x 41 px) with a random -5..4 px jitter it calls `0x42f2c0(x, y, 100, owner, 17)`, i.e. **34 x 100 area damage per pod, 204 blasts per strafe**; smoke particles appear on the blasts that land on free cells.
- **Carryall (31)**: always flies in from the right; speed = d+1 for distance d < 120 (so it nearly stops over the target), altitude = d for d in 41..=199 (descends and climbs again), unloads once when d < 5: `[+0x88]` squad (0..9, global counter `[0x89cf50]`, incremented per launch and wrapped to 0 at 10; the new value is used) lands on the 4x2 cells starting at (cell_x-1, cell_y) where free (`aircraft::squad`: e.g. squad 0 = four kind 7 and four kind 10, squad 5 = eight kind 1, squad 17 = two walkers 41); verified with `opensow squad`. With game mode `[0x49a33c] == 1` the squad is fixed (7,7,7,7,10,10).
- **Asteroid (34)**: 61 ticks after launch, then every 25 ticks, 20 times: one meteor (type 24 or 25, 50/50, strength 500, speed 240) falling from (x2+300, y2-400) to a random point (x2, y2) within 150 px of the target.
- Manual / strings: the Air Navigator (building 104) fills an icon bar before a strike is available; fighters are used to clear Anti-Air turrets and destroy Rogon nuke-bombs.

## Not decoded
Recharge times and the strike click handler (who calls `0x467ee0`: `0x417129`, `0x41f387`, `0x42ed6e`, ... include the AI), air-force damage on impact (bullet type 17), death effects, the third header word's role in campaign files, and the exact tick scheduling of `0x467b50` (it runs a variable number of ticks per call from `0x70ef8c..98`). The aircraft are not drawn by the viewer.
The viewer shows the stock in the window title.

## Header words (status)
Words 2-3 are the start **credits** of player and enemy, words 37-38 the start research points (docs/ai.md). Air-base slots (kinds 30-34, target 4000) add one force to the stock when full; the strike click calls `0x467ee0` and `0x455520` removes it from the stock; the AI launches a bomber, fighter or tripler every 600 ticks if it has one.
