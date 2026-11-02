# Projectiles and damage

Code: `crates/opensow-formats/src/projectile.rs`; CLI `opensow weapons`; tests `weapon_shots_match_the_exes_top_constructor`,
`bullet_types_match_the_exes_constructor` (`tests/real_data.rs`).

## Pipeline
1. A top fires on the first tick of a fire-sequence step (`docs/art.md`, Fire and recoil). The spawner (`0x47cf40` -> `0x47cf70`) creates `shots` bullets, bullet *i* using `.po1` table `barrel + i`: start = `unit position - po1[table][facing]`, aim offset from `po2 - po1`.
2. Each bullet is built by `0x411dc0(type, strength, start, end, owner)` (object size `0x26cc`, 35 types, jump table `0x412490`). The type's cases set speed, a step interval and sprite animation fields; the weapon's strength goes to `[bullet+0x2660]`.
3. The bullet walks a precomputed path of points (`[bullet+0xd44..]`, count `[0x2648]`; built by `0x40ef60`, see Paths below). **Speed = path steps per 60 ticks**: the number of steps taken on each tick comes from a table built at start-up (`0x436df0`): row `v` marks `v` evenly spaced ticks of a 60-tick cycle by stepping a float by `60/v`; rows above 60 are `row(v-60)+1`. So speed 60 is one step per tick, 360 six, 1800 thirty (`projectile::steps_at`).
4. On impact (`0x4107f0`) the bullet subtracts its strength from the unit it was aimed at (turret HP `[obj+0x2c]`, unit current HP `[unit+0x34]`, which also starts a hit flash) and calls the area damage `0x42f2c0(x, y, strength, owner, type)`:
   - every **ground unit** (`[unit+0x4b] == 0`; flyers are immune) with `distance < 32` px loses the **full strength**, whoever owns it (no friendly-fire check);
   - kinds 109..113 (turrets under construction) redirect the damage to their building;
   - bullet type 30 spares kind-45 units of the shooter's own team (the nuke walker);
   - the building in the impact cell loses the strength from `[bld+0x74]` unless shielded (`[bld+0x94] == 100`), and the damage is tallied per player at `[bld+0x7c+owner*4]`; some bullet types (17..35 through a table at `0x42f468`) do half damage to buildings.
   - `distance` (`0x437720`) is `trunc(sqrt(dx^2 + dy^2))`, 200 when either axis differs by more than 200.

## Weapons (`opensow weapons`)
| unit kind | bullet type | speed | strength |
|---|---|---|---|
| 1AA / 2AA / 3AA | 1 | 360 | 13 / 15 / 17 |
| 1A / 2A / 3A | 3 / 4 / 4 | 180 / 120 / 120 | 32 / 69 / 85 |
| 1C / 2C / 3C | 6 | 1800 | 20 / 24 / 26 |
| 1F / 2F / 3F | 9 | 180 | 3 / 6 / 5 |
| 1S / 2S / 3S | 27 / 28 / 29 | 600 / 600 / 300 | 70 / 133 / 75 |
| walkers 5W1..5W5 | 27 / 1 / 34 / 7 / 30 | 600 / 360 / 210 / 1800 / 300 | 18 / 21 / 22 / 7 / 920 |
Unit max HP is 180-600 for ground units (`docs/levels.md`), so one 5W5 shot (920) kills anything on the ground nearby.

## Bullet type table
`projectile::BULLET_TYPES` lists, for each of the 35 types, the speed `[+0x2664]`, the interval `[+0x264c]` (default 100; for type 1 it paces smoke puffs of the trail) and the four animation fields `[+0x2670/2674/2678/267c]`.
Types 19-23 contain loops that were not decoded (marked `partial`); types 13, 14 and 32 compute their speed with the random generator; 28, 29 and 31 also run effect code. Those are not cross-checked against the exe; all other types are.
Types 2 and 13 draw a random start value (`[+0x26ac]`, `[+0x2664]`); not modelled.

## Paths (`projectile::build_path`, viewer `--shot x1,y1,x2,y2,type`)
- The constructor passes start `(x1, y1)`, end `(x2, y2)` and a delay (`[bullet+0x26ac]`, a random 0..4 for type 2) to `0x40ef60`; the bullet starts at the start point.
- Ground track: an integer **Bresenham line**, one pixel on the long axis per step, start excluded, end included, at most **800 points**; each point is stored as (x, y) at `[bullet+0xd44+8i]`.
- Arc types lift the bullet by `h(i)` pixels at step `i` of `N` (subtracted from y; kept at `[bullet+0xc4+4i]`, probably for the shadow): types 2 and 3 `h = i - i*i/N` (peak N/4: the tier-1 artillery shell is type 3), type 4 `h = (i - i*i/N) * 20 / 13` (peak ~0.385 N: tier-2/3 artillery), types 15 and 16 `h = 4i - 4i*i/N` (peak N). Paths that hit the 800-point cap are not lifted. All other types are straight.
- Types 13 and 14 pick one of 200 random precomputed curves from tables built at start-up (`0x4ec228`, `0x49de98`...), which are not in the file; they are not reproduced (`build_path` returns `None`).
- The start index is `delay` (steps skipped) unless the path is not longer than it.
- Flight time: `flight_ticks(speed, steps)`; e.g. a 400 px cannon shot (type 6, speed 1800) takes 14 ticks, a 400 px type-4 shell (speed 120) about 200.

## Range rings
- `0x436230` builds, at start-up, a list of cell offsets `(dx, dy)` (words at `0x65bbd8`) and a cumulative index table (dwords at `0x6b3a18`, value = words up to and including ring `r`; `T[0]=2` is the centre). `rings::Rings::generate()` reproduces both exactly (`opensow rings`).
- Target search (`0x47b140`, args: x, y in pixels, ..., `min` at +0x18, `max` at +0x1c) shifts the position to cells (/32) and scans offsets from `T[min]` to `T[max]`, so a range covers rings `min+1 ..= max`. `max == 1` uses the 8-neighbour list at `0x6b3c70` instead (`rings::NEIGHBOURS`). Cells outside the map are skipped.
- Shape: ring `r` holds the cells an octagon of size `r` newly reaches. It is NOT a pure distance band: the farthest cell of ring `r` is `r^2 + floor((r-1)/2)^2`, but cumulative sets are exact discs only for r <= 7 and r = 9 (e.g. (6,6), d2 72, is in ring 9 while (8,3), d2 73, is in ring 8). Point counts for r = 0..9: 1, 4, 8, 24, 20, 40, 32, 56, 44, 72.
- Generator: 16 corner counters of a closed octagon, 145 steps cycling state 2, 3, 4 (2 and 3 close a ring and record the index, 4 only grows the octagon, so its new cells join the next ring). Each step walks the 8 edges (diagonal, horizontal, diagonal, vertical, ... ) emitting unvisited cells on a 300x300 grid; the centre is never marked visited. Order inside a ring is the edge walk order, which the search relies on to pick the first hit.
- Viewer: `--range CX,CY,MIN,MAX` (repeatable) shades rings `MIN+1..=MAX` around a cell, alternating tints per ring.
- The `0x47d7fb/801/807` calls inside it only update 30 UI helper objects (`0x70f030`) and do not affect the list.

## Verification against the exe
`scripts/verify_against_exe.py` runs the exe's own functions in an x86 emulator (unicorn) and compares them with the Rust ports on random inputs: bullet paths (`0x40ef60`, 1500 cases over 22 bullet types), the speed table (`0x436df0`, every row), heading -> facing (`0x46b180`, 2500 cases incl. 32/16/8 steps and degenerate points) the turn step (`0x46a190`, 1500 cases) and the ring list builder (`0x436230`, whole index table + 14017 offsets). All match (0 mismatches). The run found three bugs in earlier hand-reading: paths at the 800-point cap are not lifted, the 8-step facing mode is live (`(2 - round(theta/45)) mod 8`, default 2), and none in the speed table or turn step.

## Not decoded
Which bullet type uses which sprite (`extras\bodky_*`, `missile`, `flamerburst_*`, `heavylaser`, `satlaser`, `plasma`, `elektron`, `blast_*`), explosion/blast effects, the half-damage table for buildings, the homing target handle `[+0x2688]`, and network serialisation (`0x4128f0`).

## Method notes
- Weapon constructor cases share tail code, so extraction must follow the unconditional jumps up to `0x47ab5f`/`0x47ab66`. An earlier cut-off version gave several kinds reload 0 (corrected in `aim.rs`: kinds 2, 3, 11, 12, 14, 15, 42, 45).
- The test decoders in `tests/real_data.rs` (`Exe::top_case`, `Exe::bullet_case`) follow jumps and fail loudly on unknown opcodes.
