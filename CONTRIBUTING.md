# Contributing to OpenSOW

Thank you for looking into the project. This page says what you need, how to check a change, and
the few rules that are not negotiable. `docs/architecture.md` is the map of the code.

## What you need

- A recent stable Rust toolchain (`cargo`, `rustfmt`, `clippy`).
- Your own install of State of War (for example from Steam). The repository contains no game
  data. Point the tools at the install with the `OPENSOW_GAME_DIR` environment variable; on macOS
  the default Steam location is found without it. Warmonger, if you have it, is found next to the
  base game or through `OPENSOW_WARMONGER_DIR`.
- Optional, for work on the original executable: Python 3 with `unicorn`, `pefile` and
  `capstone` in a virtual environment outside the repository.

## Before you send a change

Run these four; all must come back clean:

```bash
cargo fmt --all -- --check
```

```bash
cargo clippy --workspace --all-targets
```

```bash
cargo test --workspace
```

```bash
cargo run --release -p opensow-tools -- check
```

The last one parses every file of your install and must report 0 failures. The tests of the game
crate take a few minutes because many of them play parts of real missions.

If your change touches drawing, also render the affected screen headless at several sizes, always
including 640x480 and 1920x1080, and look at each picture. Layouts that are right at one size
often break at another. For example:

```bash
cargo run --release -p opensow-game -- --screen skirmish --size 640x480 --screenshot out/lobby-small.png
```

A new drawing primitive needs a record hook so the GPU replay stays identical; check with
`--screenshot ... --gpu-check`, which must report 0 pixels that differ (see `docs/game.md`).

## Rules

- **No game data in the repository.** Never commit files from the install or anything derived
  from them: pictures, sprites, sounds, text. `.gitignore` blocks the usual ones; keep experiments
  in the ignored `out/` and `assets/` folders. The only exceptions are the launcher's art
  (`crates/opensow-launcher/art`) and the README screenshots (`docs/screenshots`).
- **Behaviour comes from the original.** When you fix or add game behaviour, find it in the
  original executable first and say where (an address or a doc section) in a comment. Do not
  guess. If something is a stand-in for behaviour that is not decoded yet, mark it `PLACEHOLDER`.
- **Parsers never index raw bytes.** Read through `Reader`, which checks bounds. Everything in
  the game's files is little-endian.
- **Game paths ignore case.** The game writes paths with `\` and mixed case; resolve them with
  `GameDir::resolve`.
- **Every parser has tests** on small synthetic inputs, and must also pass `opensow check` on a
  real install.
- **Tests never touch the user's files.** Anything that writes maps, saves or settings does so in
  a temporary folder.
- **Use names for kind numbers**: `kinds::ADVANCER`, `kinds::HEADQUARTERS` and so on, not `40`
  or `100`.

## Style

- Code is formatted by `rustfmt` with the settings in `rustfmt.toml`.
- Write for the reader. Every module starts with a `//!` comment that says what it is for, and
  public items have a `///` comment. A comment should say why the code is the way it is, or where
  in the original it comes from; it should not repeat what the code already says.
- Keep changes that only move or reformat code separate from changes that alter behaviour.
- Add a revision to `.git-blame-ignore-revs` when a commit only reformats.

## Licence

By contributing you agree that your work is licensed under the same terms as the project: MIT or
Apache-2.0, at the user's choice.
