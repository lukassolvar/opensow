# Audio (decoded tables)

- Files: `Data/Sounds/{Music,SFX,Speech,Tutorial}` (459 PCM wavs). Music: `Menu`, `Brief`, `Brief2`, `Statistic`, `uvod` (intro) and `Level01..13`. When a mission starts (flag `[0x49a339] == 1` in `0x42d6b0`) the game plays one of `Level01..Level04` chosen with the visual random generator `0x437cd0(4)` (jump table `0x42e548`); the other level tracks are used elsewhere. Tutorial voice: `Tutorial/NN.wav` per tutorial step (docs/tutorial.md).
- **Speech**: the 24 message events (docs/text.md, `formats::messages`) plus `01-game-saved`, `01-mis-complet`, `01-mis-failed`, `05-airforce-destroyed`.
- **Sound effects**: `0x407470(path, id)` loads 72 effects at start-up (loader `0x406dc0..0x407453`), `0x4075b0(id, x, y, ...)` / `0x4077e0` play them (up to eight voices at a time, panned by map position). The id table is `formats::sfx::SFX` (exe-tested; ids 31 `mainmenu-fly` and 49 `airicon-come` are loaded by the exe but missing from the shipped data, 71 is unused): 0 anti-air, 1-3 artillery, 4-6 cannon, 7-9 special, 10 flamer, 11-15 turret guns 1-5, 16-19 walker 1/3/5/nuke, 20-22 `strelba` (fire) of anti-air/artillery/cannon, 23-28 explosions (air unit, building, building2, sentry, unit, unit-turret), 29-48 menu / in-game / briefing / statistic interface sounds, 49-61 icon, info-box, turret-tool (`tt-capture` 53, `tt-click` 54, `tt-dest` 55, `tt-fire` 56, `tt-repair` 57, `tt-Upgrade` 58) and unit interface sounds, 62-70 air forces (carryall landing, bomber `hercules*`, fighter `stihac*`, `airforce-dest`, `tripler-fire`, `meteor`), 72 `turret-place`.
- The Advancer's order plays 55 (`0x46e5c0/620/680`), its beam start 53 / 57 / 58 for destroy / repair / upgrade (`0x4111b0`).
- Not decoded: which weapon top plays which firing sound, the voice-limit and pan formulas of `0x4075b0`. 

## Music (`wav.rs`, `opensow-game/src/music.rs`)
Nothing to decode: every track is a plain RIFF PCM wav (`wav::parse`; 22050 Hz, mono or stereo, about 80 minutes in all, `every_music_track_is_plain_pcm`).
The exe's music player is `0x407bb0(name)` (stop: `0x408550`). Calls found: `menu.wav` on the main menu (`0x433cb4`), `brief.wav` / `brief2.wav` on the
campaign map (`0x441c72/0x441d20`, chosen by `0x437cd0(2)`), `Level01..04` at mission start (random, `0x42d6e0`; Level05..13 are named in the player's
track list `0x406a00..0x407d50` but no other caller was traced), `statistic.wav` on the results screen (`0x44d8ac`), `uvod.wav` on the credits (`0x4421dd..`).
The game loops the current track (looping is assumed) and plays these for the menu, the map and missions; movies stop it; `--mute` / `--no-intro` silence it.

## Effects in the game (`opensow-game/src/sfx.rs`)
Eight voices at once, effect ids from `formats::sfx::SFX`. Wired: main menu hover 30 `mainmenu-move` and click 29 `mainmenu-ok` (call sites `0x445d1d`, `0x446061`;
options 33/34), campaign map 41 on hovering a land, 46+41+44 on selecting one, 43 per four typed characters, 29 on ACCEPT (the pushes in `0x43ebe0`),
in-game muzzle sounds (`sfx::muzzle`, with the +-10 % pitch jitter), explosions (units: one of 23/24/26/27, buildings 24, turrets 28, aircraft 23), turret placed 72,
unit select 60, attack order 59 and move order 55 (the exe's call sites in the unit-order code push 55; 59/60 at `0x417b82`/`0x417783`). Only sounds on screen are played.
Not wired: speech (`Data/Sounds/Speech`, the message events), Advancer beam sounds, aircraft sounds, icon/info-box sounds, panning and the distance rules of `0x4075b0`.

## Muzzle sounds
`sfx::muzzle(kind)` (exe-tested for kinds 1..113). The volley spawner `0x47cf40` (the function that creates a volley's bullets, docs/projectiles.md) plays one effect at the fire point after the bullets: it switches on the weapon kind (`[esi+8] - 1`, byte table `0x47d28c`, jump table `0x47d244`) and calls `0x4075b0(id, x, y, pitch_jitter, loop)`. Kinds 1-3 and 42 (anti-air; Walker 2 reuses it) -> 0; 4/5/6 artillery -> 1/2/3; 7/8/9 cannon -> 4/5/6; 10-12 flamer -> 10; 13 special tier 1 -> 7; 41/43/45 walkers 1/3/5 -> 16/17/18; turrets 109-113 -> 11-15. Silent: special tiers 2-3 (kinds 14, 15; effects 8 and 9 are never played by this function), Walker 4 (44) and all other kinds. `pitch_jitter` (arg 4) shifts the playback frequency by a random +-10 % (`rand(2)` sign, `rand(freq/10)`; off for walkers 1 and 3); arg 5 is the DirectSound looping flag (always 0 here). `0x4077e0(id, ...)` is the same player with id -1 meaning a random unit explosion (23, 24, 26, 27 via `rand(4)` -> `0x17,0x18,0x1a,0x1b`).
