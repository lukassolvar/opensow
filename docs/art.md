# Art lookup (what the viewer draws)

All names are inside `sprites.data` (see `MegaFile`); the `Data/units` folder only has a loose copy of `units\*`.

## Buildings: `builds\{blue|green}\{stem}.ps6`, shadow `builds\shadow\{stem}.sha`, footprint-ish mask `builds\mask\{stem}.msk`
Stems: `_base _light _med _heavy _air _mine _rlab _wind _walker` (kinds 100..108, see `docs/levels.md`).
- Frame layout (blue set, 18 frames for most): 0 scaffold, 1 construction, 2-14 idle animation (13 frames; e.g. the lab's eye lights up),
  15 damaged, 16 destroyed, 17 grey = neutral/uncaptured. `_air` has 28 frames (last = neutral, 25 damaged, 26 destroyed), `_walker` 14
  (13 neutral, 12 destroyed), `_base` 7 (0-4 upgrade stages, 5 destroyed, 6 = preview picture on a solid background; never neutral).
- Green (enemy) sets are the same art recoloured with one trailing frame fewer. Neutral uses the blue set's last frame.
- Shadows: same index as the sprite; use frame `min(idx, len-1)`; draw before sprites at ~40 % black. `.sha` hot spots are relative to the same anchor.
- Anchor: the sprite hot spot goes at the centre of the object's cell (`cell*32+16`). Verified on M01 (HQ, factories, lab sit correctly on terrain).
- `.msk` files use the same frame layout (Kind::Shadow); likely click/placement masks. Not used yet.
- Visuals: `_air` looks like a radar dish tower, `_walker` a flat round pad, `_wind` a tall rotor.

## Turrets: `units\tur_0N_{blue|gren}.ps6` (+ `_fire`), N = kind - 24
- The exe loads only N = 1, 2, 3, 5. Turret 4 (kind 28, the most common after 26) has no sprite: it is the satellite-laser turret (`extras\satlaser`).
- tur_01 has 96 frames, the others 32 (16 facings x 2 rows); frame 0 is used as the default facing. Facing and fire animation are not decoded yet.
- `units\turrets_shadow.ps6` has only 3 big frames; not mapped to turrets yet.

## Units = chassis + top (decoded from the exe; implemented in `opensow-formats/src/units.rs`)
- Files: `units\{name}_{blue|gren}.ps6` and `.sha` (shadow); loose copies also in `Data/units`.
- Chassis index per kind (`0x46ff50`, draw `0x46e8f0`) into `chA chB chC chI1 chW chN`: ground tier 1/2/3 -> `chA/chB/chC` for every class (an earlier note that kind 6 used `chA` was a mis-read: the extraction ran past the end of its case),
  Advancer (40) -> `chI1`, walkers (41-45) -> `chW`, kind 46 -> `chN`.
- Top index per kind (weapon ctor byte table `0x47ac70`, draw `0x479a80`) into `1AA 1A 1C 1F 1S 2AA..2S 3AA..3S 1I tur_01 tur_02 tur_03 tur_05 5W1..5W5`:
  `index = (tier-1)*5 + class` for ground units, 40 -> `1I` (a 16x2 empty placeholder), 41-45 -> `5W1..5W5`, 46 -> none.
- Frames: chassis `chA/B/C` have 6 animation rows x 16 facings, `chW` 10 rows x 16, `chI1` 2 x 16; tops 2 x 16 (walker tops 16). 16 facings run clockwise from north.
  The engine indexes `chassis[type*200 + frame]` and `top[idx*150 + facing*[obj+0x50] + [obj+0x6c]]`. Markers have no facing; the viewer defaults to 0 (north), see the Facing section.
- Weapon offsets `.po1/.po2` are decoded (`fire.rs`, below). They are muzzle/fire points, not mount offsets: the top is drawn at the same anchor as the chassis
  and that looks right for tanks and walkers (M01, M03, M06, M15). Fields `[obj+0x5c/0x60]` of the top object (a small vertical offset for a few tops) are not decoded.
- Verified visually: cannon tanks (M01, M15), walkers with different tops (M03), hazard pods (M03), Advancer next to the HQ (M01).
- Test: `tests/real_data.rs` checks every unit kind placed in the campaign has both team sprite files.

## Not done
Unit sprites (`units\1A..3S`, `5W1..5`, `ch*`) need the marker unit kinds (1..14) and a facing; flying units are under `specials\`.

## Weapon fire points: `units\{top}.po1` / `.po2` (`opensow-formats/src/fire.rs`, viewer overlay `F` / `--overlay muzzles`)
- A file = tables of 32 `(i16 dx, i16 dy)` points, one per top facing (frames 0..31 = 2 rows of 16, clockwise from north). `.po1`: one table per barrel/simultaneous shot
  (`size/128`: 1 for most, 2 for twin AA `1AA`, 6 for `3AA`, 2-4 for walkers, up to 3 for `tur_05`); `.po2`: exactly one table (the weapon reference point; identical to `.po1` for single-barrel tanks).
- The spawner `0x47cf70` (loaders `0x437f10`, arrays `0x70efb8`/`0x70efa4` indexed by top) starts a shot at `unit position - po1[variant*32 + facing]` and derives its direction from `po2 - po1`.
  So `muzzle = anchor - po` in screen pixels, y down. Points trace a squashed circle around a pivot ~16 px above the anchor (1C: `po1[8] = (-20,-1)` -> muzzle 20 px right).
- Verified visually: red points land on barrel tips for cannons (M15), artillery and twin AA (M06: two points per AA unit) and on both walker arms (M03).
- Unused barrel slots in walker files are all zeros. `1I` (Advancer, no weapon) is a constant (0, 2). `tur_04.po1/po2` exist even though turret 4 has no sprite (it shares `2AA`/`tur_04`-style tables with the satellite laser).
- Not traced: how the facing is derived from the unit's heading, and what `po2 - po1` does for single-barrel units (zero vector; probably the target direction is used instead).

## Facing and frame selection (`opensow-formats/src/facing.rs`; viewer `--facing N`, keys `[` `]`)
- Facings are numbered **clockwise from north**: with 32 steps 0 = north, 8 = east, 16 = south, 24 = west. Verified in the viewer: at facing 0/16/24 cannon barrels point up/down/left and the red muzzle points sit on the tips.
- Heading -> facing is `0x46b180(ax, ay, bx, by, steps)`: atan of `|dy|/|dx|` in degrees with `dx = bx - ax`, `dy = ay - by`, quadrant-corrected to a counter-clockwise angle from east, then `(8 - round(theta/11.25)) mod 32` (16 steps: `(4 - round(theta/22.5)) mod 16`).
  Ported faithfully, including two original quirks: for 16 steps the wrap-around sector (heading just below east) returns 5 instead of 4, and a == b (NaN) returns the table default (8 for 32 steps, 0 for 16). The 8-step mode is live: `(2 - round(theta/45)) mod 8` (0 N, 2 E, 4 S, 6 W), default 2; verified by emulating the exe.
  A test reads the exe's own jump tables and compares every sector (`facing_port_matches_the_exes_jump_tables`); `scripts/verify_against_exe.py` additionally emulates the whole function.
- Turning (`0x46a190`): one facing step toward the target along the shorter arc per call; a tie goes backwards. A chassis keeps its current facing at `[obj+0x30]`, the target at `[obj+0x34]`, a turn-delay countdown at `[obj+0x38]` (reload `[obj+0x3c]`).
- Frames: chassis `frame = phase * steps + facing` (`0x46fe80`; `[obj+0x44]` = animation phase, `[obj+8]` = steps, default 32). `chA/B/C` = 3 phases of 32 (96 frames), `chW` = 5 phases (160), `chI1` = 1 (32).
  Tops: `top[idx*150 + stride*phase + facing]` (draw `0x479a80`), stride 32 (16 for the 16-facing walker tops 5W1-5W3, 5W5); `2A/2C/3A/3C/tur_01/5W4` have 2-3 phases of recoil frames (64/96/48 frames, see Recoil below).
- Fire points for 16-facing tops are indexed by the top's own frame (their `.po` tables fill only the first 16 entries); 32-facing tops use the 32-step facing.
- A unit's initial facing: none of the constructors (`0x46bd30`, `0x46e250`, `0x46ff50`) write `[obj+0x30]`/`[obj+0x34]`, so new units start with whatever the allocator provides (probably zeroed = north). Unverified; the viewer defaults to `--facing 0`.
- Not traced: how the top's facing follows its target independently of the chassis (the top object keeps its own facing), recoil animation timing, and the death/explosion frame counters (`[obj+0x40]` up to 31 at `0x46f22e`).

## Walk animation (`opensow-formats/src/anim.rs`; viewer `--walk TICKS`, key `K`)
- Chassis tick (`0x46f8b9`, `0x46fe00`): turn first (counter `[0x38]`, reload `[0x3c]` = 4, 0 for the Advancer; one `turn_step` every `reload+1` ticks), and only once the facing equals the target does the animation counter
  (`[0x4c]`, reload `[0x50]` = period) run; each expiry advances the phase `[0x44]` **if the unit moved** and wraps it at `[0x48]` phases. `frame = phase*steps + facing`; the Advancer's frame is not updated.
- Per-kind values (all verified against the exe by `tests/real_data.rs`): ground kinds 1-15 have 3 phases and periods 8,8,11,7,8,**1000**,5,6,10,4,8,11,7,8,12 (kind 6, tier-3 artillery, never sets one and effectively does not animate);
  walkers 41-45 have 10 phases, 16 facings and periods 8,6,6,6,6; the pod (46) has 3 phases, 16 facings and period 3 (13 when the game-mode global `[0x49a33c]` is 3 or 8, which also drops its speed field to 7); the Advancer (40) has none.
  `steps = frames / phases`: `chA/B/C` 96/3 = 32, `chW` 160/10 = 16, `chN` 48/3 = 16, `chI1` 32.
- Field `[obj+0x18]` = **walking speed in pixels per second** (decoded): kinds 1-15 = 18,14,10 / 15,12,8 / 22,18,14 / 20,17,12 / 16,15,10 (tiers 1-3 of five classes), Advancer 30, walkers 41-45 = 20,15,20,15,20, pod 46 = 30 (7 in game modes 3/8). The unit step (`0x46f140` -> `0x46f550`) runs once per tick with the tick-in-second index (0..59) and pops `steps_at(speed, idx)` points from its path (`[obj+0x9c]`, count `[obj+0xa0]`); the path is a list of **single pixels**: `0x46ee80` fills it with a Bresenham line between waypoints (cell centres `cell*32+16`), so diagonals cost the same as straight moves (Chebyshev, not Euclidean). It moves only when the chassis is within 4 turn steps of the wanted facing; the route is re-planned every 7 ticks (`[obj+0x80]` counter, `0x46eb40`/`0x4753a0`). Tier-1 cannon (kind 7) = 22 px/s = 0.69 cells/s. Test: `walk_parameters_match_the_exes_chassis_initialiser`.
- All counters are in game ticks; the tick length in milliseconds was not found (the viewer preview assumes 20 ticks/s, which is a guess).
- Verified visually: walker legs alternate across the five phases; tank track detail changes subtly.
- Not decoded: how the unit decides it has moved (compares its position with `[0x5c/0x60]`, copied from the unit at `0x46ee3d`).

## Fire and recoil (`opensow-formats/src/recoil.rs`; viewer `--fire TICK`, key `V`)
- A weapon top (object built by the constructor at `0x47a000`, state machine `0x479c60`) has a state at `[obj+0x48]`: 0 looks for a target every 30 ticks, 3 aims (waits until its facing equals the target facing), 4 plays the fire sequence, 6 is the finished state.
- The sequence is a list of **steps**: `(phase, flash_group, barrel, shots)`, added by `0x479760` in each kind's constructor case after an implicit rest step 0. Each step lasts `reload + 1` ticks (`[obj+0x210]`, default 10); the first step starts on the first tick of state 4; shots fire on a step's first tick; after the last step the override ends.
  `phase` = which phase block of the top sprite (`frame = phase * steps + facing`): the barrel slides back for phase 1 (2A, 2C, 3C) and 3A uses phases 1, 0, 2. `flash_group` = group of 32 facings in `units\{top}_fire.ps6` drawn at the top's anchor on top of it (the `_fire` sets are `1A 1C 2A 2C 3A 3C tur_01 tur_02 5W4`, `[obj+0x58]` = index).
  `barrel` + `shots` select `.po1` tables: a step fires tables `barrel .. barrel+shots-1` at once (3C fires 3 shots from its 3 tables; the 6-barrel 3AA fires barrels 0..5 one per step).
- Examples (reload): 1AA/2AA/3AA alternate barrels with no flash (15); 1A flash groups 0,1 (5); 2A phase 1 with groups 0,1 (10); 3A two volleys of phase 1 then 2 with groups 0..3 (10); 1C two flash groups (10); 5W4 phases 0,1,2 with groups 0,0,1 (reload 2); flamers and `S` tops have no flash or phase change.
- Verified: every table row is rebuilt from the exe's bytes by `fire_sequences_match_the_exes_top_constructor`, and cross-checked against sprite frame counts (phases, `_fire` groups) and `.po1` table counts. Visually: 1C shows a large then a small flash at the muzzle point; 2A's barrel is retracted while firing and extends afterwards.
- Idle: no top has an idle animation (the idle phase array `[obj+0x94]` holds a single 0).
- Not covered: kind 40 (Advancer, `1I_fire` is an empty 32-frame placeholder) and the static turrets' own sequences; damage and projectile motion. State 6 and the volley delay are decoded in Aiming below.

## Top aiming and the fire cycle (`opensow-formats/src/aim.rs`; viewer `--aim N`)
- The weapon top keeps its **own facing** (`[obj+0x6c]`, target `[obj+0x70]`), independent of the chassis; with `--aim` the viewer shows a turret facing another way than its hull (verified on M15 cannons).
- Per tick: (1) cooldown base class (`0x47c6b0`): a counter above `2*reload` resets to `reload`; in state 6 it counts down and returns the state to 0 at 0. (2) aim (`0x4797c0`): when the state is not 0 and a target exists the target facing is `facing(top pos + [0x5c/0x60], aim point, steps)`; the aim point is the building centre, or the unit position (flying units: plus half the height from a table at `0x89cf58`). A counter (`[0x74]`, initial 4, reload `[0x78]` = 4) then turns the top one `turn_step` per 4 ticks.
  (3) state machine (`0x479c60`): 0 polls for a target every 30 ticks (counter `[0x80]` reloaded with 30; starts at 0 so the first tick searches); a hit sets the cooldown counter to `reload - jitter/2 + random(jitter)` (`0x47c5f0`) and enters 3. 3 -> 0 if the target is gone; 3 -> 4 when `facing == target facing`. 4 plays the fire sequence, then 6 (cooldown), which counts down to 0 and returns to 0.
  Example (1C cannon, facing 0, target at facing 8, tick 1 = acquire): turns on ticks 4, 8, ..., 32 (8 steps); fires from tick 33; the 22-tick sequence ends on tick 55; cooldown 72 ticks puts it back to idle on tick 127, which re-acquires immediately.
- Per-kind values (`[0x18]` min range, `[0x1c]` max range in cells, `[0x28]` reload, `[0x2c]` jitter; jitter is 0 where a case never sets it, assuming zeroed allocation) are in `aim_params` and verified against the exe (the cases are decoded by following their jumps):
  anti-air 0-9 cells (reload 120 +-12, all three tiers); artillery 2-8, 3-9, 4-10 (72); cannons 0-6, 0-7, 0-8 (72); flamers 0-4, 0-5, 0-6 (reload 2); special 0-5, 0-6, 0-7 (180 +-18); walkers 5W1 2-8 (18 +-1), 5W2 2-9 (180 +-18), 5W3 2-6 (0), 5W4 2-11 (2 +-0.5), 5W5 0-1 (180 +-18). Walker tops have 16 facings. Projectiles: `docs/projectiles.md`.
- Target choice (`0x46c990` -> `0x47b140`): cells from the minimum to the maximum range are scanned in rings (the offset list built at `0x436230`, reproduced in `rings.rs`, see docs/projectiles.md "Range rings"); the current target is kept if still found, else the nearest unit, then a turret-like building (types 3/6), then any building (the manual's "units, turrets, buildings"). Anti-air tops (`[unit+0x4c]`) and flying units use other search functions (`0x47b3c0`, `0x47aed0`), not read.
- Not decoded: the exact range shape, random generator (`0x437b10`), the unit-level flag that suppresses searching in state 0, fields `[0x20]` (10/20/5/1/24) and `[0x34]`/`[0x38]` (projectile type and damage?), and projectile flight/impact.

## Advancer (kind 40) behaviour (decoded)
- Own per-tick update `0x46f210` (chassis kind `[obj+0xc]` == 0x28): frame counter `[obj+0x54]` decrements, on underflow the frame `[obj+0x40]` +1 (wraps after 31) and the counter reloads to 2, i.e. **one facing step every 3 ticks (50 ms), always, moving or not**, counting up (clockwise). The generic chassis tick skips the Advancer's frame (`0x46fe7a`). Measured on a recording: same rate and direction. It also counts idle ticks at `[obj+0x150]` and every 420 ticks calls `0x402ec0` with 0xa (probably an idle voice/sound; not traced).
- The Advancer is the only unit built with the air flag `[obj+0x4b]` (ctor `0x46bd30`, case sets `[obj+0x30]`=0x320 = 800 HP, `[obj+0x64]`=-0x17). Its sprite is drawn **64 px above** its ground point (`[obj+0x88]` = -64 for all units, only added when the flag is set) plus the hover bob `HOVER_TABLE[phase]` (-6..6, 42 phases, one step per 4 ticks = 2.8 s period; start phase random < 20); the shadow is drawn at the ground point (`0x46de10`, `0x46a310`). Table built by `0x46bc70`, verified by emulation (`opensow hover`). The air flag also routes the unit into the air occupancy map (`0x60ac38`/`0x62ac38` instead of `0x5eac38`/`0x61ac38`), so it ignores ground obstacles.
- Chassis draw offsets `[obj+0x2c]` (extra y, set by the chassis ctor `0x470334` from table `0x470438`): kinds 1,4,7,10,13 = -10; other ground kinds and the pod = -4; walkers 41-45 = -16; Advancer 0. NOT applied by the viewer/game yet (tops have their own offsets, see above).
