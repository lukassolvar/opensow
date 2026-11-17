## What and why

<!-- What the change does and why. For game behaviour: where it comes from in the original (address or docs section). -->

## Checks

- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets` and `cargo test --workspace` are clean
- [ ] `cargo run --release -p opensow-tools -- check` reports 0 failures on a real install
- [ ] Drawing changes: rendered headless at several sizes (always 640x480 and 1920x1080), `--gpu-check` reports 0 differing pixels
- [ ] The matching doc in `docs/` is updated; deviations from the original say `**Differs from the exe:**`
- [ ] No files from the game's install or derived from them (see CONTRIBUTING.md "Rules")
