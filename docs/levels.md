# Level objects and markers (`.edt` / `.net`)

Parser: `crates/opensow-formats/src/level.rs`. Regression checks: `crates/opensow-formats/tests/real_data.rs` (skipped without a game install).

## Object kinds (confirmed enough to build on)
| kind | meaning | evidence |
|---|---|---|
| 25-29 | Turret 1-5 | HP 900/1250/1540/2100/2600; base `sentry01..03` + a gun top unit (`buildings::turret_art`, docs/art.md); InfoBox `Tur1..5`; manual: 5 turret keys |
| 100 | Headquarters | 2 per MP map, 4 on "Cross of Fire" (two HQs per side per manual) |
| 101 / 102 / 103 | Light / Medium / Heavy factory | exe name table order; heavy first appears on map 06 ("Heavy Units introduced") |
| 104 | Air base | absent on First Spark and Island of Fate; present where manual mentions air forces |
| 105 | Gold mine | 11 on "Gold Rush" (manual: "numerous Gold Mines") |
| 106 | Research lab | M01: neutral, half-HP building left of the player's start = the lab the briefing says to seize |
| 107 | Wind trap (`kinds::WINDTRAP`) | name table + InfoBox `Wind`, `energy1..3` meters; its levels speed up production (docs/production.md) |
| 108 | Walker factory | exe treats {101,102,103,108} as unit producers; only on maps whose manual text mentions walkers (04,05,07,08) |

The exe loads building art in the order `_base _light _med _heavy _walker _air _mine _rlab _wind` (then `sentry01..03`), but the kind
enum puts the walker factory last (108); all other buildings follow load order from 100.

## Object words
13 = hit points (buildings max ~13536), 14 = owner (0 player/blue, 1 enemy/green `_gren`, 2 neutral and capturable, usually at 50 % HP = word 18 of 50),
16/17 = cell x/y, 18 = hit-point percentage. **What the loader (`0x41db8d..0x41e002`) really does with the 18 words** (record words 1..18 after the tag 123, read in order into locals): `0x439e30(x = w16, y = w17, owner = w14, kind = w1)` creates the building; `0x40d870(w3, w8, w4, w9, w5, w10, w6, w11, w7, w12)` sets the five factory slots (kind, per-slot value; docs/production.md); w2 -> `[bld+0x144]` (active slots; for a windtrap, 107, the levels added to `[0x5baaa0 + owner*4]` by `0x40d9a0`); w15 -> `[bld+0x114]` (1 = headquarters flag, set by the constructor for kind 100 anyway); w18 = percent of the maximum hit points (`hp = min(max * w18 / 100, max)`, 100 when absent, **skipped for owner 2**: neutral buildings get exactly half their maximum, `0x43a003`). **Word 13 is read and never used** (the editor's own copy of the hit points; it drifts from the percentage by up to ~3 % on turrets), as are w5-w7 / w9-w12 on buildings without slots. `Object::start_hp` is the rule; the game used word 13 before. Word 3 is the first slot's kind (e.g. 16 = Gold Influx 1 on a gold mine), not a "per-kind constant".
OpenSOW: campaign same (`World::from_level`, `sim/setup.rs`, with `Object::start_hp`).

**Differs from the exe:** skirmish (not in the exe) rewrites word 18 to 100 for owned and 50 for neutral objects (`skirmish::even_start_health`) and then gives neutral ones their word-18 percentage instead of exactly half (`World::neutral_start_hp_from_level`), so custom maps can set it.

## Markers = spawned units (decoded from the exe loader)
Word layout: `224, unit kind, player index, x px, y px`. The loader (`0x41da00`) spawns each as a unit at cell `(x>>5, y>>5)`
(`0x43a420` -> constructor `0x46bd30`). Player index 0 = first/blue player, 1 = second/green (252 vs 153 markers in the shipped levels).
- **Kind 40 = Advancer** (800 HP). Manual: "most important unit", repairs/destroys turrets, captures buildings; Cross of Fire has "two
  Headquarters and two Advancers" per side = 4 kind-40 markers. Median start distance to its own HQ is 2.1 cells over 50 levels.
  The loader skips it when a per-player flag (`[0x5179f0 + player*4]`, `0x41db60`) is 1. **Earlier notes calling it a "start position" were wrong.**
  The flag is header word 34 (player) / 35 (enemy), the same word that forbids the HQ's replacement Advancer (the header block is read to `0x517970`; `0x41d7d4` copies it to `[0x5baab8]`, docs/ai.md). Warmonger: `0x41d12c`.
  OpenSOW: every marker of a known kind becomes a unit (`World::add_unit` in `World::from_level`), except an Advancer of a side whose word is 1; test `an_advancer_marker_of_a_side_without_advancers_is_not_spawned`.
- **Kinds 1-15 = ground units**, five weapon classes x three tiers: `class = (kind-1)/3`, `tier = (kind-1)%3+1`.
  Classes (from the weapon constructor's top-sprite table, `docs/art.md`): 0 anti-air `AA`, 1 artillery `A`, 2 cannon `C`, 3 flamer `F`, 4 special `S`.
  Max HP by kind: 180,400,550 | 180,360,450 | 180,360,500 | 240,430,600 | 210,400,550. Levels only ever place tier 1 and 2 (kinds 1,2,4,5,7,8,10,11,13,14).
- **Kinds 41-45 = the five walker robots** (HP 1100, 900, 1000, 1100, 1200; tops `5W1..5W5` on `chW` legs); **kind 46** = a bare `chN` pod
  (1000 HP, hazard symbol; the Rogon bomb, `rogon.rs`, docs/ai.md; 17 placed in campaign levels). Walkers and pods appear in campaign levels only.
- Kinds 109-113 exist in the constructor but the loader never spawns them from files (building max HP 50, the constructor's default: index past the table bound `0x40a988`; their tops are the turret sprites `tur_02,2AA,tur_03,tur_01,tur_05`: they are the gun tops the exe creates for turrets 25-29 once a turret has healed in, `kinds::TURRET_TOPS`, docs/art.md).
- `Marker.kind` for 41-46 and 1-15 is validated only structurally (every shipped marker is one of these, `tests/real_data.rs`).

## Air forces
The level header also carries the starting air-force stock; see `docs/air.md`.

## Techniques that worked (reuse them)
- Count kinds per multiplayer map and compare with `Data/Manuals/manual_EN.pdf` (maps 01-08 are described in words).
- RTTI + string tables in the exe; `[obj+0x94]` is the building kind field; compare immediates 0x64..0x6c to find kind-specific code
  (e.g. 0x4167a1, 0x455daa = factory checks).
- Follow data flow from the file loader: script loader `0x41da00` -> `0x43a420` -> unit ctor `0x46bd30` (kind-indexed byte table `0x46c280` + jump table `0x46c23c` for HP).
- Beware `0x43a9b0` and its pushed values 1,3,4,5,6: that is a per-category synchronised random-number slot allocator, **not** a chassis table (I misread it once).
- Data invariants (objects never on blocked cells, 2 HQs per MP map) catch wrong mappings quickly.

In code, use the names in `formats::kinds` (`kinds::ADVANCER`, `kinds::HEADQUARTERS`, ...) instead of the bare kind numbers.
