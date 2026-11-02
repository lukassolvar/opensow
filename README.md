# OpenSOW

An open-source, cross-platform (macOS / arm64 / Linux / Windows) reimplementation of the
2001 RTS **State of War**, written in Rust, for preserving a community favourite.

**OpenSOW contains no game data.** You need your own legitimate copy (e.g. from Steam).
Point it at the install with `OPENSOW_GAME_DIR`, or it will look at the default Steam depot path.

## Layout

- `crates/opensow-formats` - parsers for the game's file formats (archive, SRF images, sprites, ...)
- `crates/opensow-tools` - `opensow` CLI: `where`, `check`, `unpack`, `srf`, `sprite`
- `crates/opensow-viewer` - `opensow-viewer` level viewer (winit + softbuffer, CPU rendered); also a library used by the game
- `crates/opensow-game` - `opensow-game`: playable prototype (menus + skirmish), see `docs/game.md`
- `docs/formats.md` - reverse-engineering notes on each file format

## Try it

```bash
cargo run --release -p opensow-tools -- check          # parse every asset in your install
cargo run --release -p opensow-tools -- sprite "$OPENSOW_GAME_DIR/Data/units/1C_blue.ps6" unit.png
```

## Play the prototype

```bash
cargo run --release -p opensow-game                      # menus -> New game -> pick a mission
cargo run --release -p opensow-game -- --screenshot out/menu.png --hover 0          # headless render of a screen
cargo run --release -p opensow-game -- --screenshot out/m01.png --play M01 --select-all --move 38,27 --ticks 2300 --report
```
Main menu uses the game's own button art. In a mission: left click / drag select, right click move, WASD/arrows or screen edge scroll, wheel zoom, Q select all, Space pause, Esc menu. Production, capturing, air strikes and AI are not implemented yet (see `docs/game.md`).

## Level viewer

```bash
cargo run --release -p opensow-viewer -- M05            # interactive window
cargo run --release -p opensow-viewer -- M01 --screenshot out/m01.png --overlay blocked,objects,markers
```
Keys: arrows/WASD or drag = pan, wheel or +/- = zoom, B blocked cells, G grid, O objects (squares for things without art), M markers, X sprites, N/P next/previous level, K play the walk cycle, `--aim N` turns tops independently, `--shot x1,y1,x2,y2,type` draws a bullet path, V loop the fire/recoil sequences, Home fit, Esc quit.

## Status

| Format | Status |
|---|---|
| `sprites.info/.data` archive | done |
| `.srf` / `.srfa` images | done |
| `.ps6` / `.tsp` / `.sha` sprites | done |
| `.map` / `.til` / `.tmi` / `.edt` / `.net` | done (object/marker kind meanings unknown) |
| level viewer | done (terrain, blocked cells, building, turret and unit sprites with shadows; air forces are stock counts, not drawn) |
| `.msk` / `.ps6` fonts | done (ASCII only; see docs/fonts.md) |
| language files (`lang.dat`, `lang*.txt`) | done (see docs/text.md) |
| `.po1/.po2` weapon fire points | done (viewer overlay F shows muzzles) |
| projectiles, damage, weapon stats, bullet paths | done (`opensow weapons`, `opensow path`; docs/projectiles.md) |
| verification against the original exe | `scripts/verify_against_exe.py` (emulator) |
| GDI-path text rendering | todo |
| playable prototype (menus, selection, movement, combat, win/lose) | done, with placeholders (docs/game.md) |
| economy/production, Advancer actions, air strikes, AI, audio, networking | todo |
