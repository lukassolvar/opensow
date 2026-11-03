# Effect sprites (bullets, explosions, smoke)

Code: `crates/opensow-formats/src/effects.rs`. Exe checks: `tests/real_data.rs` (`extras_table_matches_the_exes_loader`, `bullet_draw_table_matches_the_exes_draw_function`, `impact_sounds_match_the_exes_impact_function`, `animation_spawn_sites_match_the_exes_calls`). Viewer/game use: `blit_blend` in `opensow-viewer/src/render.rs`, `Play::draw_bullets` in `opensow-game/src/play.rs`.

## The sprite table

`0x424700` (the start-up sprite loader) reads every effect sprite file with the same call shape: `push cap; push dest; push ecx; mov ecx, esp; push name; call 0x45dab0`. `dest` is a static array of frame pointers at a fixed address, `cap` the number of frames the loader will keep (the file's real frame count is smaller or equal; neighbouring arrays may overlap beyond it, e.g. `sizer` / `plasma`). `effects::EXTRAS` lists 82 entries (`extras\*`, `__extras\*`, `specials\*`; the file is `<name>.ps6` inside `sprites.data`). Code refers to a frame as `[array + 4*index]`, so an address inside an array names a frame (the shell sprites are `gula2` frames 1 and 2 at `0x5e6694` / `0x5e6698`; `effects::extra_at` resolves that).

Unit explosion pieces are not in this table: `Units\<name>_blueboom` / `_grenboom` (80 frames per unit, arrays `0x5870b0` / `0x58a2b0`, stride `0x50` per unit name in the `units::TOPS` order plus the 1AA..3S names) are loaded by the unit loader `0x4246xx`.

`extras\bigexplo\e_air2` and `e_bubg` have `_low` variants loaded into the same arrays when the low-detail flag `[0x49a33a]` is set.

## Bullets

Bullets are objects of size `0x26cc` (docs/projectiles.md). Drawing is `0x410a30` (jump table `0x410e7c`, one case per type); `0x414020` is the matching erase pass for the exe's dirty-rectangle renderer (same sprites, it also erases the previous position `[+0xc0]`, which is why the arcing types look like they are drawn twice there; the picture shows only the bullet at its lifted position, there is no separate shadow sprite).

`effects::bullet_draw(type)` gives the layers (sprite array, frame rule, blit routine), exe-tested for all 34 types:

| type | draws |
|---|---|
| 1 | `missile` frame = facing `[+0x2654]` (plain), then `MissileFlare` frame 0 (additive) |
| 2, 3 | `gula2` frame 1 (plain); 4: `gula2` frame 2; 5: `gula2` frame 0 |
| 6-10, 12, 14, 20, 27, 31 | nothing (10 is the satellite laser, drawn by the beam code) |
| 11 | `plasma` frame = counter (additive) |
| 13 | `wreck_bu1`/`wreck_bu2` frame `[+0x268c]*20 + counter` |
| 15, 16 | unit boom sprite `blueboom`/`grenboom` frame `[+0x2680]*80 + counter` (a destroyed unit's flying pieces) |
| 17 | `tripler_ph` frame = progress (additive) |
| 18 | `bomba_l`/`bomba_r` frame `[+0x268c]*20 + progress` |
| 19, 21 | `bodky_b` / `bodky_r` fragments: `[+0x268c]` of them, each with its own offset and frame (additive) |
| 22 | `fighter_smoke` (the smoke blend) |
| 23 | `dim1..dim4` frame `[+0x2680]*21 + counter` |
| 24, 25 | `meteor_medium` / `meteor_small` (plain) plus their `_aura` (half blend) |
| 26 | `fighter_bomb` frame = counter |
| 28 | `elektron` frame counter + 1 (additive) |
| 29, 30, 33 | `heavylaser` frame 0 (additive) |
| 32 | `menu\001_lights` frame 0 (additive; a loose `Sprites2` file) |
| 34 | `walker_blast` frame 0 (additive) |

**Frame advance** (`0x411c8x`, once per logic step; `[0x552494]` steps per tick): types 17 and 18 take the frame from the flight progress `p = [+0x2644]` (twice the path index): `min((p-2)/20, 19)` and `min((p-2)/50, 8)`. All other types count `[+0x2678]` down; at zero it is reloaded from `[+0x267c]` and the counter `[+0x2670]` goes up by one (type 22, the fighter's smoke, adds one more when the counter is a multiple of 3); at `[+0x2674]` frames it wraps to 0, and types 22 and 23 end instead (the bullet is removed). The four values come from the constructor (`projectile::BULLET_TYPES.anim`). `effects::BulletAnim` implements this.

## Blit routines
Every draw call picks one of the exe's software blitters (RGB565, RLE sprites, clipped, colour-keyed). All kernels are now exact, ported as `effects::Blit::apply565` and checked pixel for pixel against the exe's own routines in the emulator by `scripts/verify_blits.py` (random RLE sprites, random destinations). Per channel (5/6/5-bit fields; "src/2" is the source field shifted right):
- `0x45edf0` plain copy; `0x461740` is *not* plain (see lens).
- `0x45cde0` (also `0x45e560` with a flag, and `0x45b5e0` which additionally needs the screen mask byte below `[0x49a33b]`, ignored in the game): `min(dst + src, max)`.
- `0x45c220` (draw mode 9): `min(dst + src/2, max)` - the source halved first (an additive glow at half strength).
- `0x4611f0` (modes 10 and 11): `(dst + src) / 2`, a 50 % blend rounded down.
- `0x45f4d0` (mode 14, the fighter's smoke): `max(0, dst - src/4)`; like `0x45b5e0` it only draws where the screen mask byte is below `[0x49a33b]`.
- `0x45df40` (mode 15, decals): `max(0, dst - src/2)`.
- `0x460f60` (shadows; bullet draw cases): the sprite is only a stencil (flag 1: its pixel words are skipped, flag 0: the file has none); each covered destination pixel is looked up in the 64K table `0x73ced8`, which `0x45d590` builds as `max(0, channel - 6/12/6)`. The same builder makes `0x71ced0` (-4/8/4), `0x75cee0` (the 6/12/6 table applied twice), `0x77cee8` (`min(channel + 1/2/1, max)`) and `0x79cef0` (a grey ramp, `5 bit = (r*299 + g*587 + b*114) / 1000 + 4`, clamped to 31, replicated into all three fields); the 555 variants of every routine exist for display format `[0x517aec] != 2`.
- `0x461740` (mode 13, "lens"): stencil sprite; each covered pixel becomes the picture pixel 2 columns right and 3 rows down (the loop stops at row `0x1de`), brightened through `0x77cee8`. `Blit::Lens`; `blit_blend` reads the offset pixel.
- `0x466140` is not a sprite draw: it is a DirectDraw blit that restores the background under a rectangle (dirty-rectangle erase), used by the `0x414020` pass.

## Animation sprites (`AnimSprite`)

Explosions, smoke and markers are `AnimSprite` objects (class name survives in the vtable at `0x485e9c`; constructor `0x404cf0`, initialiser `0x4054a0`). `0x4054a0(x, y, period, array, from, to, repeat, flag)`: frames `from..to` of `array` repeated `repeat` times form a list (`[+0x24]`, length `[+0x3c]`); `[+0x10]` = `period` is how many logic steps each frame stays; `[+0x34]` is the draw mode set afterwards (6 for all explosions: the additive blit `0x45cde0`; the draw method `0x404e30` has a jump table `0x405080` over modes 1..15: 1-5 and anything else plain `0x45edf0`, 6 `0x45cde0`, 7 `0x45e560`, 8 `0x45b5e0`, 9 `0x45c220`, 10-11 `0x4611f0`, 12 a stack of plain-sprite draws, 13 `0x461740`, 14 `0x45f4d0`, 15 `0x45df40`); `[+0x1c]` flag. The update `0x4050d0` counts `[+0xc]` down, advances `[+0x38]` when it reaches 0 and removes the object when the index reaches the list length; mode 10 additionally moves the sprite up (rising smoke). `AnimSpec::frame_at(age)` is that rule.

`effects::SPAWNS` lists the 30 call sites with constant arguments and their trigger; all addresses are exe-tested. Highlights:
- building destroyed: turrets `e_turret` (45 frames, period 2), others `e_bubg` (83 frames) - `structure_destroyed` in the sim;
- air force unit destroyed: `e_air2` (52 frames); ground unit destroyed: see "Unit destruction" below;
- order markers `cross_as`, `cross_rp`, `click`; the walker teleport; the missile and meteor smoke trails; the fighter's smoke trail.

**Blasts** (`0x405240(x, y, size)`): `blast_verysmall, small, medium, big, blast2_defrag, blast2_artligh, blast2_artheav` = size 0..6; frames 22 (23 for size 4, 30 for sizes 5 and 6), period 4, mode 6. Which bullet type makes which blast (`effects::impact_blast`, from the impact function `0x40f590`): type 1, 6, 17 size 1; 2, 7 size 0; 3 size 5; 4 size 6; 11 size 4; 15, 16, 24, 25 size 2. The same function creates the type's own effect (`effects::impact_anims`): flame bursts (8, 9), `bomba_ex` (18), `fighter_ex` (26), `specialfall_*` (27-29), `bodky` fragments (19, 21).

## Impact sounds

The impact function plays one sound at the end point (table `0x410a14` -> `0x4109f0`, sound ids = `formats::sfx`): type 1 -> 20 (anti-air `strelba`), 3, 4 -> 21 (artillery `strelba`), 6, 7 -> 22 (cannon `strelba`), 15, 16 -> 28 (`unit-turet` explosion), 17 -> 26 (`sentry` explosion), 18 -> 65 (`hercules-boom`, only half of the time), 24, 25 -> 70 (`meteor`), 26 -> 67 (`stihac-boom`); other types are silent. So the three ground-unit fire sounds (20-22) play when the shell lands, not at the muzzle; the muzzle sounds are played when the volley is spawned (docs/audio.md, `sfx::muzzle`). `effects::impact_sound`.

## Smoke puffs and scorch decals

Both come from the generic tail of the impact function (`0x410405`, reached by every bullet type except the targeted hits 1, 10, 31, the Tripler pod 17 - which has its own code - the bodky types 19-21 and 22). `effects::impact_leaves_decal`, `impact_smoke`, `smoke_puff`, `DECAL_ARRAY`; the game's `World::impact_ground_effects`.
- **Decal** (class vtable `0x487b04`, init `0x41a1d0(x, y, frame)`, draw `0x41a160`): when the landing cell is inside the map, free (no building, no terrain block) and has no decal yet (flag grid `0x513878`), a decal with frame `random(9) / 2` (visual generator `0x437cd0`) is added to the list `[0x49a3d0]`. It is drawn from sprite array `0x5e6960` (`__extras\pustkrat`, 25 frames, variant 0-4) with the darkening blit `0x45df40`, and removed (its flag cleared) once a building stands on the cell.
- **Puffs** (`0x405340(x, y)`, list `[0x49a3a8]`): an `AnimSprite` with period 5, mode 8 (blit `0x45b5e0`), flag 0; the sprite is `fire_sm1` or `fire_sm2` (`random(2)`), frames 0-8 once and then `4 + random(13)` more passes (45 to 153 frames, 225-765 ticks). Where they appear, at offsets -10..9 px: type 17 and 18 impacts make two (inside their free-cell test, visual generator); after the decal the exe rolls `random(3)` (lockstep `0x437b10`): on 1 types 15 and 16 make three puffs, otherwise types 13 to 16 make one; a destroyed ground unit makes three (`0x46af25..0x46b047`, lockstep generator, offset by the chassis hover height).

**Blit routines settled**: `0x45df40` = `max(0, dst - src/2)` per channel (decals), `0x45b5e0` = the additive blit but only where a screen mask byte `[0x6b3cd4 + ...]` is below `[0x49a33b]` (the mask is ignored in the game), `0x45cde0` additive, `0x45edf0` plain. All blit kernels are exact now (see above).

## Unit destruction (`0x46a560..0x46b0f0`)
`effects::UNIT_DEATH_CLOUDS`, `debris`, `DEBRIS_DELAY`, `UNIT_DEATH_SOUND`; exe-tested by `unit_death_matches_the_exes_destruction_code`; `World::step` uses them. In order, for a unit of kind `k` at `(x, y)` (turret kinds 109-113 take a separate path that just frees the handle):
1. **Four clouds** (`e_unit2`, period 3, additive, flag 1) at `(x - 15 + rand(30), y - 15 + hover + rand(30))`; two lockstep draws each, the first for y, the second for x. The Advancer (kind 40) makes the same four; every other kind has the `newvrak` wreck inserted after the second cloud at `(x, y + 7)` (period 5, default blit).
2. Sound 27 (`explodes\unit`) at the unit through `0x4077e0`.
3. **Flying pieces** for kinds 1-15 and 41-45 that have a top part: a bullet of type 15 (blue unit) or 16 (green), from `(x, y + top height)` to `(x + sx, y + sy)`, `s = (2 * rand(2) - 1) * (rand(spread) + spread)` for each axis, path delay 40; `spread` is 30, 40 or 60 by weapon tier (`(k - 1) mod 3` for kinds 1-15, 60 for the walkers); two more class values go to the bullet (`+0x265c` = 2, 2, 3) and the unit (`+0x94` = 2, 3, 4). Skipped when `0x41f020(player)` says the local side does not own the unit (network clients).
4. Except for the Advancer: the burning wreck object and three smoke puffs (`UNIT_DEATH_PUFFS`).

## Not decoded

- Draw mode 12 of `AnimSprite` (a vertical stack of plain draws, `0x404e51`) and the screen mask contents of `0x45b5e0` / `0x45f4d0`.
- Unit destruction: the hover offset (`[unit+0x88]` for hovering units), the wreck's `[unit+0x94]` use, the burning-wreck object `0x45afc0` (list `[0x49a3e4]`, `0x45b210(x, y + [chassis+0x2c], 0, 0, 20, 5)`), the debris bullet's strength and top-part variant `[top+0x54]` (the sim throws variant 0 with strength 0).
- The fragment offsets of bullet types 19 and 21 (loop over `[+0x48 + 4i]`), the `[+0x268c]` / `[+0x2680]` variants (set by the constructor; the game draws variant 0), and bullet type 10 (satellite laser column: `satlaser` frame `[+0x2670]` repeated 13 times upward from the impact).
- Missile facing: `missile` has 40 frames; the game uses the 32-step flight heading, which may not be the exe's index.
