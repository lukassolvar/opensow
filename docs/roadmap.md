# Roadmap

1. [done] Asset parsers: archive, SRF, sprites, shadows, map, tile index.
2. [partly done] `.edt`/`.net` structure decoded; next: top mount offsets and air forces,  `.po1/.po2`, `generic.file`, lang files.
3. [done, v2] Map viewer: background, overlays, building/turret sprites + shadows. Next: draw real unit/building sprites at object cells (buildings, turrets and all ground/walker units done; bullet sprites/effects and the unit state machine next (range ring list done: rings.rs) (facing, walk, recoil, aiming and damage rules done); air forces decoded (stock + flight/attack behaviour, docs/air.md), launch/recharge next), `.til`/`.srfa` layers, minimap.
4. Audio (WAV 22 kHz PCM via cpal/rodio) and MPEG-1 intros (`.sgv`, e.g. `mpeg1` decoder crate).
5. Ghidra pass on the exe: naming, vtables, game loop, unit/AI/combat logic.
6. Port simulation deterministically; validate against the original running under Wine/CrossOver.
7. Networking compatible with the original lockstep protocol (optional).

- [done, prototype] `opensow-game`: menus (original button art), mission select, selection/move/combat/win-lose on the decoded rules. Production of ground units is in (docs/production.md). Advancer actions (capture/upgrade/repair) are in (docs/advancer-actions.md). Next: credits/research/turrets, air strikes, enemy AI, turret weapons, audio.

## Status after the second decoding pass
Decoded and in the game: production, tech-levels, economy, turret prices/placement/guns, Advancer actions, satellite protection, air strikes and anti-air, Rogon bomb, computer opponent, win/lose rules, mission countdowns, reinforcements, convoy missions, HQ Advancer replacement, building footprints, message events, the exe's random table. Decoded but not wired: sound-effect ids and speech files (docs/audio.md), the tutorial script's structure (docs/tutorial.md).
Still open: the exe's wave pathfinder (`0x4753a0`, the game uses A*), bullet/explosion sprite mapping and draw code, weapon firing sounds, savegame and network message formats (`0x41d0b0`, `0x4296a0`, the `CNet_*` bit packing), the tutorial script's spawn lists, `State of War.cfg`, MPEG-1 intros (`.sgv`), per-state mission scripts beyond missions 1, 3, 7, 8, 12 and 17.
