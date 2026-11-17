# Releases and CI (not in the exe)

Three GitHub Actions workflows in `.github/workflows`; the packing itself is `scripts/package.sh`, which also runs locally.

## What a release brings
One file per system, plus its `.sha256` (`sha256sum` format) for the launcher's self-update (docs/launcher.md "Updates"):

| System | File | Inside |
|---|---|---|
| macOS 11+ (Apple silicon and Intel) | `opensow-X.Y.Z-macos-universal.dmg` | `OpenSOW.app` (universal: `lipo` of both builds) and an `Applications` link; drag the app over, double-click it, the launcher starts |
| Windows 10+ x86_64 | `opensow-X.Y.Z-windows-x86_64.zip` | folder `OpenSOW`: `opensow-launcher.exe`, `opensow-game.exe`, README, licences |
| Linux x86_64 / aarch64 (glibc 2.35+, ALSA `libasound.so.2`) | `opensow-X.Y.Z-linux-<arch>.tar.gz` | folder `OpenSOW`: `opensow-launcher`, `opensow-game`, README, licences |

`OpenSOW.app`: `Contents/MacOS` holds both programs (the launcher is the bundle's executable and starts the game beside it),
`Contents/Resources` the icon (`OpenSOW.icns`, see "Icons") and the licences; identifier `io.github.opensow`, `LSMinimumSystemVersion` 11.0. The bundle carries an ad-hoc signature over the whole
bundle (no Apple certificate): Apple silicon runs no unsigned code, and the launcher's update replaces the whole bundle
(docs/launcher.md), so the seal stays valid.

**Release builds have no developer features.** The `dev` cargo feature of `opensow-game` and `opensow-launcher` is on by
default (cargo builds, tests, the headless switches in AGENTS.md); release files are built with `--no-default-features`:
- game: no F12 / backquote dev tools panel, no headless `--screenshot` runs (and with them `--play`, `--screen`,
  `--gpu-check`, `--plan`, ...), no `--skirmish` straight start, no `OPENSOW_START`, `OPENSOW_PERF`, `OPENSOW_AUDIO_DUMP`;
  those switches are ignored and the game starts normally. The player's switches stay (`--windowed`, `--no-intro`, `--mute`,
  `--lang`, `--no-fx`, `--no-enhance`, `--no-sound-enhance`, the folder and troubleshooting variables such as
  `OPENSOW_CPU_DRAW` and `OPENSOW_AUDIO_DEBUG`). The original's cheat phrases are part of the game and stay.
- launcher: no `--screenshot` / `--icon-png`; `--check-update` stays.
- Windows: without `dev` both programs use the GUI subsystem, so no console window opens behind them.

## Icons
One picture in two framings, `crates/opensow-launcher/art/icon.png` (Windows, Linux: the full square) and `icon-macos.png`
(macOS), both 1024 px RGBA and committed launcher art. It is a 64x64 pixel image (40 colours, rounded corners on the
pixel grid) blown up without smoothing, in the game's sprite style: a factory on a patch of desert in a brass frame, drawn
new for OpenSOW, not one of the game's sprites. Each system gets it its own way:
- macOS: `package.sh` makes `OpenSOW.icns` (16 to 512@2x, `sips` / `iconutil`) from `icon-macos.png`, which follows
  Apple's grid (the 64 pixels at 13 px each, 832 px, centred with a transparent margin); Finder, the Dock and the `.dmg` show it, and the game
  shows it too because it runs from the same bundle. macOS ignores window icons.
- Windows: the build scripts of both programs (`build.rs`, shared code in `crates/opensow-launcher/build_icon.rs`) make
  `OpenSOW.ico` (PNG entries 16 to 256) from `icon.png` and link it into the `.exe` as resource 1 (`embed-resource`: `rc.exe`
  on Windows, `llvm-rc` when cross-compiling), so Explorer, the taskbar and shortcuts show it; both windows also set it as
  their window icon.
- Linux: both windows set it as their window icon (X11) and the app id / X11 class `opensow`. Wayland desktops take an
  application's icon and name only from an installed `.desktop` file whose name matches the app id, so the launcher of a
  release file (not a `dev` build, which would point it at the target folder) installs one at every start (not in the exe;
  `desktop.rs`): `$XDG_DATA_HOME/applications/opensow.desktop` (default `~/.local/share`), whose `Exec` is the launcher's
  own path, and the icon at 64 and 512 px in `icons/hicolor/*/apps/opensow.png`. A file is written only when it differs,
  so moving the folder updates the entry on the next start; deleting the folder leaves the entry behind. The game's window
  has the same app id, so it shows under the same entry and icon. The archive also carries the icon as `opensow.png`.

The `dist` profile (`Cargo.toml`) is `release` with thin LTO and stripped symbols.

## Making a release
1. Set `version` in every `crates/*/Cargo.toml` to the new `X.Y.Z` (the launcher compares the tag with its own
   `CARGO_PKG_VERSION`), commit, wait for CI to pass.
2. `git tag vX.Y.Z && git push origin vX.Y.Z`. `release.yml` stops at once when the tag is not `v` plus the crates' version: a
   release whose launcher reports an older version would offer itself as an update forever.
3. `build.yml` builds and packs every system; `release.yml` then opens a **draft** release with all files. Its notes are
   `docs/releases/vX.Y.Z.md` when that file exists (write it before tagging), else notes GitHub generates from the
   commits.
4. Edit the notes and publish it as a normal release, **not** a pre-release: the launcher and the README's "latest release"
   link ignore pre-releases and drafts, and a tag like `v0.2.0-beta` is not read as a version. Call a beta a beta in the
   title or notes.

## CI
`ci.yml` runs on every push to `main` (changes to docs and Markdown only are skipped) and on every pull request, docs-only
ones included, because the `main` ruleset requires its `lint` and `test` checks before a merge; a newer push cancels the
older run:
- `lint` (Ubuntu): `cargo fmt --check`, clippy on all targets, clippy without the `dev` feature;
- `test` on Ubuntu 22.04, macOS 15 and Windows 2022: `cargo test --release` (tests that need the game's files skip: no
  game data on the runners);
- `build`: `build.yml`, the release files of every system, kept 14 days as the run's artifacts (test builds). Only when
  the workflow is started by hand (Actions tab, "Run workflow", or `gh workflow run ci.yml`) and on release tags
  (`release.yml`), not on every push.

macOS runs on `macos-15` (Apple silicon; the release file's Intel half is cross-compiled there); `macos-14` is retired
("The configuration 'macos-14-arm64-us-xl' is not supported" cancels a job before it starts).

Speed: `Swatinem/rust-cache` keeps the dependencies' builds per job and system; only `main` (and tags) save caches, pull
requests read them, so they never evict `main`'s. Warnings fail the lint and test jobs (`RUSTFLAGS=-D warnings`).

Cost: a public repository runs all of it on the standard runners for free. A private one on the Free plan has 2000
minutes a month; Windows minutes count twice, macOS ten times, and each job is rounded up to whole minutes. Measured with
warm caches: lint 2, Ubuntu tests 3, macOS tests 2 x 10, Windows tests 5-6 x 2, so a push costs about 35-40 minutes; a run
started by hand adds the release files, about 47 more (macOS universal 3 x 10, Windows 5 x 2, Linux 3 + 4), about 85 in
all.

## Local checks
- macOS: `cargo build --profile dist --no-default-features -p opensow-launcher -p opensow-game` then
  `scripts/package.sh macos-aarch64 dist target/dist` (one processor: a toolchain without the Intel target, such as Homebrew's Rust, builds only its own; with rustup add
  `x86_64-apple-darwin` and pass both folders with `macos-universal` for the universal image).
- Linux and Windows in Docker: `scripts/docker-check.sh [linux-aarch64] [linux-x86_64] [windows-x86_64]`. Linux runs the CI's
  Ubuntu 22.04: tests, then the release build and its archive. Windows cross-compiles for MSVC with `cargo-xwin` (builds and
  links; nothing runs). Toolchains, registries and target folders live in Docker volumes (`opensow-*`), so later runs are
  incremental; the files land in `dist/` (ignored by git). On Apple silicon `linux-x86_64` is emulated and slow.

## Not done
- No Apple Developer ID signature or notarisation: a downloaded `.dmg` is quarantined, so macOS asks once (README "How to
  run"). Updates through the launcher are not quarantined (`curl` sets no flag).
- No Windows code signature (SmartScreen warns on first start).
- No installer on Windows and no AppImage / Flatpak on Linux: the launcher updates itself in place, which needs a writable
  folder.
