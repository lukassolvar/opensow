# Architecture

A map of the code for someone opening the repository for the first time. The details of each subject
(file formats, the original executable, each game system) live in the other files of `docs/`.

## What the project is

OpenSOW is a Rust reimplementation of the 2001 real-time strategy game State of War. It reads the
data files of the player's own install and contains no game data itself. The game rules are not
invented: they are decoded from the original `State of War.exe` and, wherever possible, checked
against it by running the original functions in an emulator.

## The crates

```
opensow-formats   parsers and decoded game rules; no window, no drawing
      ^
      |
opensow-viewer    loads a level and draws it (library + the `opensow-viewer` binary)
      ^
      |
opensow-game      the game: menus, missions, skirmish, map editor

opensow-tools     the `opensow` command-line tool (uses formats)
opensow-launcher  window to locate the installs, pick the language, edit the game's settings,
                  start the game and update itself (uses formats; docs/launcher.md)
```

Dependencies only point upwards in this picture: `formats` knows nothing about the others.

### `opensow-formats`

Two kinds of modules live here:

- **File formats**: one module per format (`megafile` for the sprite archive, `sprite`, `srf`,
  `map`, `level`, `save` / `saveobj` for savegames, `video` / `mpeg1` / `mp2` / `indeo5` /
  `msvideo1` for the movies, `lang`, `config`, `custommap`, ...). Each is a pure function from
  bytes to a struct. All reading goes through `Reader`, which checks bounds, so a damaged file
  gives an error and never a panic.
- **Decoded game rules**: tables and functions ported from the exe (`aim`, `anim`, `facing`,
  `projectile`, `rings`, `wave` for path finding, `aircraft`, `production`, `advancer`,
  `effects`, `turretai`, ...). They take plain numbers and return plain numbers, which is what
  makes them testable against the original.

`GameDir` (in `lib.rs`) finds the install and resolves the game's own paths, which use `\` and
mixed case, without regard to case. `kinds` names the kind numbers of units and buildings.

### `opensow-viewer`

`scene` loads a level into a `Scene` (ground picture, blocked cells, objects with their sprites).
`render` draws a scene into a frame buffer of `0x00RRGGBB` pixels with plain CPU loops, so it runs
without a window and in tests. `record` notes every drawing call as a command so that the game can
replay the same frame on the GPU. `upscale` and `fx` are the optional run-time enhancements.
`editor` / `editor_ui` are the internal skirmish map editor.

### `opensow-game`

- `main.rs` holds the two central types: `Game` (everything the program knows) and `App` (the
  window around it). Their methods are split by subject into `game_flow`, `game_screens`,
  `game_input` and `window`; `headless` holds the `--screenshot` runs.
- `sim.rs` and `sim/` are the simulation: the `World` and everything that happens in it each
  tick. It is pure (no drawing, no files, no clock) and has its own table of submodules at the top
  of `sim.rs`.
- `play.rs` is the mission screen: camera, selection, drawing the world and the HUD (`hud.rs`,
  `infobox.rs`).
- One module per screen: `menu`, `lands` (campaign map), `options`, `saveui`, `skirmish_ui`,
  `mapeditor_ui`, `multiplayer`, `results`, `ingame` (pause menu), `intro` / `boot` (movies).
- `gpu.rs` and `replay.rs` present the frame and replay recorded drawing commands in a compute
  shader; `music`, `sfx` and `hifi` are sound.

## How a frame happens

1. The window (`window.rs`) receives input and passes it to `Game` (`game_input.rs`).
2. `Game::update` advances the open screen. In a mission it steps the simulation: one
   `World::step` per game tick, 60 ticks per second.
3. `Game::draw` draws the open screen into the frame buffer. Every drawing primitive also records
   itself (`opensow_viewer::record`).
4. The frame is presented: either the recorded commands are replayed on the GPU, or the CPU frame
   buffer is uploaded as it is. Both give the same pixels; `--gpu-check` proves it for a frame.

## How correctness is kept

- **Unit tests** next to the code: every parser has tests on synthetic bytes, the simulation has
  scenario tests on small made-up maps.
- **Real data**: `opensow check` parses every file of the install; `tests/real_data.rs` reads
  tables straight from the exe and compares them with the Rust ones.
- **Emulation**: the scripts in `scripts/` run functions of the original exe in an x86 emulator
  and compare them with the Rust ports on many random inputs (`verify_against_exe.py` and the
  other `verify_*.py`).
- **Headless screenshots**: the game and the viewer can render any screen to a PNG without a
  window (`--screenshot`), which is how layouts are checked at several resolutions.

Comments in the code often carry addresses such as `0x46f210` or offsets such as `[obj+0x1c]`.
These point into the original executable and are the evidence for the line they stand next to;
`docs/executable.md` explains how to follow them.
Wherever OpenSOW deliberately or knowingly behaves differently from the exe, the subject's doc says so on a line starting with `**Differs from the exe:**` (grep for it).

## Where to look for a subject

| Subject | Start here |
|---|---|
| A file format | `docs/formats.md`, then the module in `opensow-formats` |
| The original executable | `docs/executable.md` |
| What the game screen can do and what is a placeholder | `docs/game.md` |
| Units walking, path finding | `docs/walking.md`, `docs/pathfinding.md` |
| Weapons and bullets | `docs/projectiles.md` |
| Production, economy, the computer opponent | `docs/production.md`, `docs/ai.md` |
| Skirmish and custom maps | `docs/skirmish.md`, `docs/map-editor-ingame.md` |
| Savegames | `docs/savegame.md` |
| Plan and status | `docs/roadmap.md` |
