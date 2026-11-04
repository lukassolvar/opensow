# AGENTS.md - OpenSOW

Rust reimplementation of State of War (2001 Windows RTS). Details live in `docs/`; keep this file short.

## Rules
- Never commit game data or derived images (PNG/JPG/sprites); `.gitignore` blocks them. Assets come from the user's own install.
- Find the game via `OPENSOW_GAME_DIR` or `GameDir::locate()` (default: Steam depot path on macOS).
- Game paths use `\` and mixed case; always resolve case-insensitively (`GameDir::resolve`).
- Everything is little-endian; use `Reader` (bounds-checked), never index raw slices.
- Every format parser needs unit tests with synthetic bytes plus a pass over real files via `opensow check`.
- Commit messages end with the Co-Authored-By line from the session's attribution reminder.
- Headless screenshot verification (`--screenshot`) must run at several resolutions, always including the lowest (640x480) and the biggest (1920x1080), and view each PNG; layouts that look right at one size often break at another.

## Commands
- `cargo test` - unit tests
- `cargo run --release -p opensow-game` - play the prototype (menus, missions); placeholders are listed in docs/game.md
- `cargo run --release -p opensow-tools -- check` - parse every asset in the install (expect 0 failures)
- `cargo run --release -p opensow-viewer -- M01 --screenshot out/x.png --overlay blocked,objects` - headless render; view the PNG to verify visually
- `opensow text big "Hello" out.png` renders text with the game fonts; `opensow frames <file>` lists a sprite file's frames
- `opensow where|unpack|srf|sprite|map|level` - inspection tools (see `--help` via no args)

## Layout
- `crates/opensow-formats` - pure parsers, no I/O policy
- `crates/opensow-tools` - `opensow` CLI
- `crates/opensow-game` - playable prototype (intro/briefing/outro movies, menus + skirmish, docs/game.md; `--no-intro` skips movies, `--mute` no sound); `sim.rs` is pure and tested, `--screenshot ... --play M01 --ticks N --report` runs it headless
- `crates/opensow-viewer` - viewer (also a lib used by the game); `render.rs` is pure CPU (testable headless), `main.rs` is the winit window
- `docs/formats.md` - format notes; `docs/executable.md` - binary analysis; `docs/roadmap.md` - plan/status

## Key facts
- **Best verification tool:** `scripts/verify_against_exe.py` emulates exe functions (unicorn) and diffs them against the Rust ports; reuse it for any new pure function (set up: venv with `unicorn pefile`; atan is hooked). Prefer it over hand-reading; it found real bugs.
- Range rings (target-search offset list, 0x436230) = `rings.rs`, exact vs emulation; ring r = cells a growing octagon newly reaches, NOT a d2 band (docs/projectiles.md); `opensow rings`, viewer `--range CX,CY,MIN,MAX`.
- Bullet paths = Bresenham line + parabolic lift per type (docs/projectiles.md); `opensow path`, viewer `--shot`.
- Text: `lang.dat` digit -> `langXX.txt` (UTF-8, `@`-separated, 364 strings); PL/RU are English copies, HU is damaged; non-ASCII text is drawn via GDI, not bitmap fonts (docs/text.md).
- Method warning: cases in the exe's jump tables jump into SHARED tail code. Never stop at the first jmp (misses fields) nor read past it blindly (merged kinds 6/7 once): follow unconditional jumps to the known common tail. Tests use `Exe::top_case` / `Exe::bullet_case` for that. Verify tables with tests that read the exe (see `Exe` in tests/real_data.rs).
- Facing: 32 steps clockwise from north (0 N, 8 E, 16 S, 24 W); `facing::facing()` ports the exe's heading->facing incl. quirks and is tested against the exe's jump tables; frame = phase*steps + facing.
- Fonts: frame k = ASCII 32+k, `_o` outline masks nest in `_i` at pen-hot; only ASCII 32..125 has glyphs (docs/fonts.md).
- Building art is in `sprites.data` under `builds\blue|green|shadow|mask` (also loose in Warmonger `Data/Sprites/Builds`); turret 4 has no sprite by design.
- Original: 32-bit MSVC C++/MFC (VS2015+), DirectDraw/DirectInput/DirectSound, Intel IJL JPEG, 2 Steam API calls.
- Two games share one engine: `State of War` and `State of War Warmonger` (own exe + Data dir).
- RTTI class names survive in the exe (CMegaFile, CHuffcomp, CBuilding, CUnitChassis, CNet_*); use them to name Ghidra functions.
- Multiplayer sends per-unit commands (CNet_UnitGoto etc.) => lockstep sim; port logic must be deterministic and match the original.
- Viewer facts (owner outline: blue player, green enemy, gray neutral; hover shows object info in the title): bg SRF size == map cells*32 (checked M01 64x64 -> 2048px); blocked overlay matches water/mountains, objects never sit on blocked cells; 2x2 clusters of one object kind look like multi-cell buildings.
- Python venv with capstone/pefile/pypdf is handy for exe work (create it outside the repository); capstone needs `skipdata=True`; do not name a script `dis.py`.
- Exe analysis needs no Ghidra/radare2; Python 3, objdump and cargo are enough.

- Tick = 16.66 ms = 60 ticks/s (frame accumulator `0x42e890`, docs/executable.md); all `ticks` in docs/code are at that rate. Advancer (decoded 0x46f210/0x46bc70): spins 1 facing step / 3 ticks, flies 64 px up with a 42-phase hover bob (`anim::HOVER_TABLE`). Unit walking speed = px/s per kind (`WalkParams.speed`, 7..30 px/s), applied as `steps_at(speed, tick%60)` 1-px Bresenham moves (docs/art.md).
- Production (docs/production.md): factory slots from level words 2-7; speed 30+12*windtrap levels steps/s (tutorial 90), targets 2000 (ground/walker) / 4000 (air); free and automatic. `[0x70f2cc]` = TUTORIAL step (-1 elsewhere); the computer opponent, economy, tech-levels and turret prices are in docs/ai.md. `opensow level f.edt` prints building words.

## Format status
- Done: viewer draws building, turret and unit (chassis+top) sprites (docs/art.md); sprites.info/.data, .srf/.srfa, .ps6/.tsp/.sha, .til (shadow-kind frames), .map, .tmi
- Done: .edt/.net structure and object kinds (turrets 25-29, buildings 100-108, unit markers = spawned units with kind 40 = Advancer; see docs/levels.md); most header words unknown
- Air forces decoded as starting stock (docs/air.md). Fonts decoded (docs/fonts.md). Weapon fire points (.po1/.po2) decoded (docs/art.md). Facing decoded (facing.rs, docs/art.md). Walk animation decoded (anim.rs, docs/art.md). Recoil/fire sequences decoded (recoil.rs, docs/art.md). Top aiming + fire cycle decoded (aim.rs, docs/art.md). Projectiles/damage/speed table decoded (projectile.rs, docs/projectiles.md; `opensow weapons`). Air-force behaviour decoded (aircraft.rs, docs/air.md; launch + carryall squads emulation-verified, `Aircraft::step` read-only; NOTE code 32 = Fighter, 33 = Tripler). Effect sprites decoded (effects.rs, docs/effects.md: loader table, bullet draw cases, AnimSprite spawns, impact sounds; exe-tested; the game draws them with `blit_blend`; blit arithmetic exact (`Blit::apply565`, `scripts/verify_blits.py`), `--fx CX,CY,KIND,AGE` shows one headless). Smoke puffs + scorch decals decoded (docs/effects.md). Savegame container decoded (save.rs, docs/savegame.md: Huffman `.data`, `.info` + thumbnail, state head and section order; object records in saveobj.rs, tables checked by `scripts/verify_save_layout.py`; `save::State` walks a whole stream (global serializers decoded; field meanings open); `opensow save <file>`). Settings files `State of War_1.cfg` / `player.cfg`: `config.rs` (docs/config.md; `scripts/verify_config.py`). Muzzle sounds: `sfx::muzzle(kind)` (docs/audio.md). Videos `.sgv` = MPEG-1 (M01-M22, mI, mO) or AVI (mA Indeo5, mB CRAM), played by DirectShow, `video.rs` walks them and `mpeg1.rs` decodes the MPEG-1 pictures, `mp2.rs` the layer II sound (`opensow sgvwav`), `msvideo1.rs` the mB AVI (bit exact; `indeo5.rs` decodes mA, bit exact, docs/indeo5.md), checked against FFmpeg (docs/video.md; `opensow sgvframe`). Campaign map (New game = world map of 22 lands with `mA`/`mB`, unlock order, texts): docs/lands.md. Multiplayer packets: `net.rs` (docs/network.md, `scripts/verify_net.py`). Random-curve bullets (types 13/14, building debris): `curves.rs`, emulation-verified (docs/projectiles.md). String-index table: `strings.rs`, docs/text.md. TODO: lang txt, `generic.file`, `.cfg`, CHuffcomp (compression, where used?)
- Advancer actions (docs/advancer-actions.md): destroy = capture (largest damage tally takes the building, turrets/HQ are removed), repair/upgrade per beam shot `2*rate`/`rate`, rate = level header word 31/32 x 4; beam period = 10 ticks, exact. `0x40f590` is the beam-event executor.
- Wave pathfinder = `wave.rs` (exact vs emulation of 0x4753a0, docs/pathfinding.md); local unit avoidance `Wave::local_step` (0x476870) and `walk::pixel_line` (0x46ee80) are emulation-verified too (docs/walking.md); `wave::approach` (0x477630 chase cell) too; `opensow wave|local|line|approach`. Decoded game logic lives in `formats`: `advancer.rs`, `production.rs` (slots, tiers, income, prices, exits), `buildings.rs` (footprints), `messages.rs`, `sfx.rs`, `rng.rs` (generic.file), `projectile.rs` (incl. Tripler ring), `rogon.rs` (Rogon bomb touch test + type-35 explosion), `aircraft.rs`; docs: ai.md (AI, economy, win/lose, missions, turrets, Rogon bomb), advancer-actions.md, audio.md, tutorial.md. Factory rally point (button + R key, third spawn order): `rally.rs`, docs/rally.md, exe-byte-tested. Pitfall: `git add -A` would commit the user's untracked `tools/`; add explicit paths.
