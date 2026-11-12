# Ambient clouds (`CCloud`)

Pure decoration, switched by Options CLOUDS (`State of War_1.cfg` detail, docs/options.md). **Off by default** (first-run detail 0), so a fresh install shows none until CLOUDS is turned on. Port: `formats::clouds` (`Cloud`, `Sky`), exe-verified by `scripts/verify_clouds.py` (constructor, scatter and move run in an emulator, 0 mismatches); `play.rs` draws them, `--clouds D` shows them headless. OpenSOW: same counts, moves and draws (`World::sky_step` once per tick, shadow before the shots in `Play::draw_shots`, clouds after the aircraft); off by default like the exe.

**Differs from the exe:** the visual generator `0x437cd0` is an xorshift in OpenSOW (`World::sky_step`), so the clouds' positions, speeds and pictures have the exe's ranges but not its sequence; a loaded savegame's clouds get picture 0 (`sim/savestate.rs`) instead of a re-rolled one.

## Object (20 bytes, vtable `0x4878e4`, list `[0x49a3fc]` / count `[0x49a400]`)
`+4` x, `+8` y (map pixels), `+0xc` speed class 10..=39, `+0x10` picture 0..=4 (frame of `extras\bigexplo\clouds`, array `0x5e95b0`). The save record (`0x414820`) stores x, y, speed only; the picture is re-rolled by the constructor on load.

## Randomness
Everything uses the visual generator `0x437cd0` (not lockstep), so no sequence has to be reproduced.
- Constructor `0x4145a0` = re-roll `0x414770`, draws in this order: `x = rand(400) - 800`, `y = rand(mapH * 32 - 200) + 100`, `speed = rand(30) + 10`, `picture = rand(5)` (`[0x71cec4]` = map height in cells, `[0x71cec0]` = width).
- When the list grows, `0x453060` creates a cloud and then runs `0x4149d0` which overwrites the position so the sky is full at once: `x = rand((mapW + 25) * 32) - 400`, `y = rand(mapH * 32)`.

## List size (`0x453060`, called at mission start `0x41f302`, on load `0x42bc52` and when the options change)
Detail 0 destroys all; 1..=4 -> 13, 32, 45, 70 clouds (jump table `0x453298`). Growing appends scattered clouds; shrinking removes `rand(count)` one at a time.

## Move (`0x4147d0`, per rendered frame from `0x42e290`)
For each tick of the frame (`[0x552494]` of them, in-second indices `[0x517b10 + 4i]`): `x += table[speed * 60 + tick]` with the speed table `0x5524a0` built by `0x436df0` (the same rows as bullet speeds and unit walking: `projectile::steps_at`, so speed 30 = 30 px/s). Returns false when `x > (mapW + 25) * 32`; the caller then re-rolls the cloud (`0x414770`). Clouds only move right.

## Draw
- Shadow, `0x414730` (called at `0x420095`, after the aircraft shadows and just before the bullets/effects): `0x45f4d0` at `(x + 100, y)`: the darkening masked blit `max(0, dst - src/4)` (only where the screen mask is below the generation, docs/effects.md).
- Cloud, `0x414700` (`0x420435`, after the aircraft and the bullets, before the HUD): the additive blit `0x45cde0` at `(x, y - 200)`.
- `0x414a10` (`0x41f9c1`) is not a draw: it is the dirty-rectangle erase (`0x466140`) of both rectangles, which the CPU renderer does not need.
