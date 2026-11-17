# AGENTS.md - OpenSOW

Rust reimplementation of State of War (2001 Windows RTS). Details live in `docs/` (start with `docs/roadmap.md` for status, `docs/architecture.md`, `docs/formats.md`, `docs/executable.md`); keep this file to rules and traps that the code does not tell you.

## Rules
- Never commit game data or derived images (PNG/JPG/sprites); `.gitignore` blocks them. Assets come from the user's own install. Two exceptions: the launcher's art in `crates/opensow-launcher/art` (three background screenshots + `tank.png` icon, docs/launcher.md) and the README screenshots in `docs/screenshots/*.jpg` (rendered headless, 1280 px wide).
- Find the game via `OPENSOW_GAME_DIR` or `GameDir::locate()` (default: Steam depot path on macOS). Game paths use `\` and mixed case; always resolve case-insensitively (`GameDir::resolve`).
- Two editions share one engine: Warmonger only contributes mission data and land texts; everything else comes from the base game via `GameDir::edition` + `GameDir::resolve` (docs/warmonger.md).
- Everything is little-endian; use `Reader` (bounds-checked), never index raw slices.
- Every format parser needs unit tests with synthetic bytes plus a pass over real files via `opensow check`.
- Decoded game logic must match the exe: trace the original, then verify with `scripts/verify_against_exe.py` (unicorn emulation diffed against the Rust port; venv with `unicorn pefile`, atan is hooked). Prefer it over hand-reading; it found real bugs.
- Docs describe what OpenSOW does today next to the decoded exe behaviour; every deviation from the exe gets its own line or list item starting `**Differs from the exe:**` (greppable), port-only features say "(not in the exe)". Update the doc in the same change as the code.
- Headless screenshot verification (`--screenshot`) must run at several resolutions, always including 640x480 and 1920x1080, and view each PNG.
- Commits carry no Co-Authored-By or other AI attribution lines (`.claude/settings.json` turns them off); commit as the human who made the change.
## Commands
- `cargo test`; `cargo run --release -p opensow-tools -- check` (parse every asset, expect 0 failures)
- `cargo run --release -p opensow-viewer -- M01 --screenshot out/x.png --overlay blocked,objects` - headless map render
- `cargo run --release -p opensow-game -- --play M01 --ticks N --screenshot out/x.png --report` - headless mission; `--skirmish "03-Gold" [--spectate]` for skirmish (docs/skirmish.md); `--screen <name>` for menus/editors (docs/game.md, docs/map-editor-ingame.md)
- `cargo run --release -p opensow-launcher -- --screenshot out.png --size WxH` - headless launcher (docs/launcher.md)
- `opensow` (no args) lists the inspection tools; most decoded routines have a matching subcommand.
- Release files: `scripts/package.sh` (macOS `.dmg`), `scripts/docker-check.sh` (Linux and Windows in Docker); CI and releases in docs/release.md.

## Traps
- Headless switches, the F12 dev tools and other developer features exist only with the `dev` cargo feature (default on); release files are built with `--no-default-features`, so a new developer switch must check `DEV` (docs/release.md).
- Tick = 16.66 ms (60 ticks/s); every `ticks` value in code and docs is at that rate.
- Exe jump tables: cases jump into SHARED tail code. Never stop at the first jmp (misses fields) nor read past it blindly (merged kinds 6/7 once); follow unconditional jumps to the known common tail (`Exe::top_case` / `Exe::bullet_case` in tests).
- GPU drawing replays recorded commands bit-identically to the CPU loops: any new drawing primitive or direct pixel loop needs a `record` hook (or `record::taint()` / `record::cpu`), and any new world blit needs the `render::hires` hook. Check with `--gpu-check` (0 differ expected).
- Headless screenshots read `opensow.cfg` from the cwd (it may hold the user's switches): run from another directory or pass `--no-fx`/`--no-enhance` knowingly.
- The level header words the exe actually reads are few (docs/ai.md, docs/levels.md); the rest are ignored.
- Exe analysis needs no Ghidra/radare2: a Python venv outside the repository is enough (capstone/pefile/pypdf; capstone needs `skipdata=True`; never name a script `dis.py`). RTTI class names survive in the exe; use them to name functions.
