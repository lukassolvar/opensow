#!/usr/bin/env bash
# Packs one platform's release archive and its .sha256 (docs/launcher.md "What a release must look like").
#
#   scripts/package.sh PLATFORM BIN_DIR OUT_DIR
#
# PLATFORM is `<os>-<arch>` as the launcher names it (`update::platform()`: macos-aarch64, macos-x86_64, windows-x86_64,
# linux-x86_64, linux-aarch64); BIN_DIR holds the release builds of opensow-launcher and opensow-game. Writes
# OUT_DIR/opensow-<version>-<platform>.zip (.tar.gz on Linux, which keeps the programs executable for unzip-less systems)
# and OUT_DIR/<archive>.sha256. The version is opensow-launcher's crate version.
set -euo pipefail

platform=${1:?platform}
bin=${2:?bin dir}
out=${3:?out dir}
root=$(cd "$(dirname "$0")/.." && pwd)

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/crates/opensow-launcher/Cargo.toml" | head -1)
name="opensow-$version-$platform"
exe=""
case "$platform" in windows-*) exe=".exe" ;; esac

mkdir -p "$out"
out=$(cd "$out" && pwd)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/$name"
for p in opensow-launcher opensow-game; do
    cp "$bin/$p$exe" "$stage/$name/"
done
cp "$root/README.md" "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$stage/$name/"

cd "$stage"
case "$platform" in
    windows-*) archive="$name.zip"; 7z a -tzip -bso0 "$out/$archive" "$name" ;;
    macos-*) archive="$name.zip"; ditto -c -k --keepParent "$name" "$out/$archive" ;;
    *) archive="$name.tar.gz"; tar czf "$out/$archive" "$name" ;;
esac

cd "$out"
if command -v sha256sum >/dev/null; then
    sha256sum "$archive" >"$archive.sha256"
else
    shasum -a 256 "$archive" >"$archive.sha256"
fi
cat "$archive.sha256"
