# Roadmap

1. [done] Asset parsers: archive, SRF, sprites, shadows, map, tile index.
2. [partly done] `.edt`/`.net` structure decoded; next: top mount offsets and air forces,  `.po1/.po2`, `generic.file`, lang files.
3. [done, v2] Map viewer: background, overlays, building/turret sprites + shadows. Next: draw real unit/building sprites at object cells (buildings, turrets and all ground/walker units done; bullet sprites/effects done (docs/effects.md) and the unit state machine next (range ring list done: rings.rs) (facing, walk, recoil, aiming and damage rules done); air forces decoded (stock + flight/attack behaviour, docs/air.md), launch/recharge next), `.til`/`.srfa` layers, minimap.
4. Audio (WAV 22 kHz PCM via cpal/rodio) and playing the `.sgv` videos (MPEG-1 video and MP2 audio decoded, intro, briefings and outro play in the game, docs/video.md).
5. Ghidra pass on the exe: naming, vtables, game loop, unit/AI/combat logic.
6. Port simulation deterministically; validate against the original running under Wine/CrossOver.
7. Networking compatible with the original lockstep protocol (optional).

- [done, prototype] `opensow-game`: menus (original button art), mission select, selection/move/combat/win-lose on the decoded rules. Production of ground units is in (docs/production.md). Advancer actions (capture/upgrade/repair) are in (docs/advancer-actions.md). Next: credits/research/turrets, air strikes, enemy AI, turret weapons, audio.

## Status after the second decoding pass
Decoded and in the game: production, tech-levels, economy, turret prices/placement/guns, Advancer actions, satellite protection, air strikes and anti-air, Rogon bomb, computer opponent, wave pathfinder, bullet and explosion sprites (docs/effects.md), win/lose rules, mission countdowns, reinforcements, convoy missions, HQ Advancer replacement, building footprints, message events, the exe's random table. Decoded but not wired: sound-effect ids and speech files (docs/audio.md), the tutorial script's structure (docs/tutorial.md).
Wave pathfinder, pixel line and local unit avoidance, the order list and the chase-cell search decoded and used by the game (docs/pathfinding.md, docs/walking.md, emulation-verified). Still open: the other path routines (`0x474b30`, `0x473f10`), the chase for handle type 4 (`0x478200`), savegame field meanings (the whole stream layout is decoded, docs/savegame.md) and the multiplayer state-sync writers (input packages and headers are done, docs/network.md), the tutorial script's spawn lists, showing the mA/mB animations in the briefing screen (both decode) of the `.sgv` videos, and playing them (MPEG-1 video decoder done, docs/video.md), per-state mission scripts beyond missions 1, 3, 7, 8, 12 and 17.
