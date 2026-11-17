# Releases (not in the exe)

The archives the README links and the launcher's self-update installs (docs/launcher.md "Updates") are built by
`.github/workflows/release.yml`; `scripts/package.sh` packs one platform and also runs locally.

## Making a release
1. Set `version` in every `crates/*/Cargo.toml` to the new `X.Y.Z` (the launcher compares the tag with its own
   `CARGO_PKG_VERSION`), commit.
2. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`. The workflow fails at once when the tag is not `v` plus the
   crates' version: a release whose launcher reports an older version would offer itself as an update forever.
3. Per platform the workflow runs `cargo test` (tests that need the game's files skip), builds `opensow-launcher` and
   `opensow-game`, and packs them with `README.md` and both licences into one folder:

   | Platform | Runner | Archive |
   |---|---|---|
   | `macos-aarch64` | macos-14 | `opensow-X.Y.Z-macos-aarch64.zip` |
   | `macos-x86_64` | macos-14, cross-built (not tested) | `opensow-X.Y.Z-macos-x86_64.zip` |
   | `windows-x86_64` | windows-2022 | `opensow-X.Y.Z-windows-x86_64.zip` |
   | `linux-x86_64` | ubuntu-22.04 (older glibc) | `opensow-X.Y.Z-linux-x86_64.tar.gz` |
   | `linux-aarch64` | ubuntu-22.04-arm | `opensow-X.Y.Z-linux-aarch64.tar.gz` |

   Each archive gets `<archive>.sha256` (`sha256sum` format). macOS builds target macOS 11. Windows release builds use the
   GUI subsystem, so no console window opens behind the launcher or the game; `cargo` builds keep the console for the
   headless switches.
4. A **draft** release with every archive and generated notes appears on GitHub. Edit the notes and publish it as a normal
   release, **not** a pre-release: the launcher and the README's "latest release" link ignore pre-releases and drafts, and
   a tag like `v0.2.0-beta` is not read as a version. Call a beta a beta in the title or notes instead.

`workflow_dispatch` (Actions tab, "Run workflow") runs the same builds as a dry run and keeps the archives as workflow
artifacts without a release.

Locally: `cargo build --release -p opensow-launcher -p opensow-game && scripts/package.sh macos-aarch64 target/release dist`.

## Not done
- The macOS programs are not signed or notarised: a browser download is quarantined, and macOS refuses to open it until
  the user allows it (README "How to run"). Updates through the launcher are not quarantined (`curl` sets no flag).
- No `.app` bundle: on macOS the launcher is a plain program.
- The Windows programs are not signed (SmartScreen warns on first start).
