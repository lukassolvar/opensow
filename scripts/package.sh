#!/usr/bin/env bash
# Packs one platform's release file and its .sha256 (docs/release.md; the launcher's update rules: docs/launcher.md "Updates").
#
#   scripts/package.sh PLATFORM OUT_DIR BIN_DIR [BIN_DIR_2]
#
# BIN_DIR holds release builds of opensow-launcher and opensow-game made WITHOUT the `dev` feature
# (`cargo build --profile dist --no-default-features -p opensow-launcher -p opensow-game`). One file per platform:
#
#   macos-universal  BIN_DIR = aarch64 build, BIN_DIR_2 = x86_64 build, joined with lipo
#                    -> opensow-<v>-macos-universal.dmg (OpenSOW.app + an Applications link)
#   macos-aarch64    one build (local tests) -> opensow-<v>-macos-aarch64.dmg
#   windows-x86_64   -> opensow-<v>-windows-x86_64.zip   (folder OpenSOW with both .exe)
#   linux-*          -> opensow-<v>-linux-<arch>.tar.gz  (folder OpenSOW with both programs)
#
# The version is opensow-launcher's crate version.
set -euo pipefail

platform=${1:?platform}
out=${2:?out dir}
bin=${3:?bin dir}
bin2=${4:-}
root=$(cd "$(dirname "$0")/.." && pwd)

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/crates/opensow-launcher/Cargo.toml" | head -1)
name="opensow-$version-$platform"
programs=(opensow-launcher opensow-game)
# the application icon (docs/release.md "Icons"); Windows has it inside the .exe files (build.rs)
icon="$root/crates/opensow-launcher/art/icon.png"
icon_macos="$root/crates/opensow-launcher/art/icon-macos.png"

mkdir -p "$out"
out=$(cd "$out" && pwd)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

checksum() {
    cd "$out"
    if command -v sha256sum >/dev/null; then sha256sum "$1" >"$1.sha256"; else shasum -a 256 "$1" >"$1.sha256"; fi
    cat "$1.sha256"
}

case "$platform" in
macos-*)
    app="$stage/dmg/OpenSOW.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    for p in "${programs[@]}"; do
        if [ -n "$bin2" ]; then
            lipo -create -output "$app/Contents/MacOS/$p" "$bin/$p" "$bin2/$p"
        else
            cp "$bin/$p" "$app/Contents/MacOS/$p"
        fi
    done
    lipo -info "$app/Contents/MacOS/opensow-launcher"
    cp "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$app/Contents/Resources/"
    # the icon: art/icon-macos.png (1024 px, Apple's grid) at every size macOS asks for, 16 to 512@2x
    iconset="$stage/OpenSOW.iconset"
    mkdir "$iconset"
    for s in 16 32 128 256 512; do
        sips -z $s $s "$icon_macos" --out "$iconset/icon_${s}x${s}.png" >/dev/null
        sips -z $((s * 2)) $((s * 2)) "$icon_macos" --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
    done
    iconutil -c icns "$iconset" -o "$app/Contents/Resources/OpenSOW.icns"
    cat >"$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>OpenSOW</string>
    <key>CFBundleDisplayName</key><string>OpenSOW</string>
    <key>CFBundleIdentifier</key><string>io.github.opensow</string>
    <key>CFBundleExecutable</key><string>opensow-launcher</string>
    <key>CFBundleIconFile</key><string>OpenSOW</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.strategy-games</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
    # ad-hoc signature over the whole bundle (no Apple certificate yet): Apple silicon refuses unsigned code, and updates
    # replace the whole bundle, so the seal stays intact
    codesign --force --deep --sign - "$app"
    codesign --verify --deep --strict "$app"
    ln -s /Applications "$stage/dmg/Applications"
    file="$name.dmg"
    rm -f "$out/$file"
    hdiutil create -quiet -volname "OpenSOW $version" -srcfolder "$stage/dmg" -fs HFS+ -format UDZO "$out/$file"
    ;;
windows-*)
    mkdir "$stage/OpenSOW"
    for p in "${programs[@]}"; do cp "$bin/$p.exe" "$stage/OpenSOW/"; done
    cp "$root/README.md" "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$stage/OpenSOW/"
    file="$name.zip"
    rm -f "$out/$file"
    (cd "$stage" && 7z a -tzip -bso0 -bsp0 "$out/$file" OpenSOW)
    ;;
linux-*)
    mkdir "$stage/OpenSOW"
    for p in "${programs[@]}"; do cp "$bin/$p" "$stage/OpenSOW/"; done
    cp "$root/README.md" "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$stage/OpenSOW/"
    cp "$icon" "$stage/OpenSOW/opensow.png"
    file="$name.tar.gz"
    tar czf "$out/$file" -C "$stage" OpenSOW
    ;;
*)
    echo "unknown platform $platform" >&2
    exit 1
    ;;
esac

checksum "$file"
