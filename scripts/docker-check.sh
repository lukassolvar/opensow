#!/usr/bin/env bash
# Builds and packs the Linux and Windows releases in Docker, as CI does (docs/release.md "Local checks").
#
#   scripts/docker-check.sh [linux-aarch64] [linux-x86_64] [windows-x86_64]   (default: all three)
#
# Linux: the CI's Ubuntu 22.04 with its libraries; runs the tests (without the game's files they skip), then builds without
# the `dev` feature and packs. linux-x86_64 on an Apple silicon Mac runs emulated (slow). Windows: cross-compiled for MSVC
# with cargo-xwin (builds and links, nothing runs). Cargo's registry and each platform's target folder live in Docker volumes,
# so later runs are incremental. Archives land in dist/.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
platforms=("$@")
[ ${#platforms[@]} -eq 0 ] && platforms=(linux-aarch64 linux-x86_64 windows-x86_64)
toolchain=stable

for platform in "${platforms[@]}"; do
    echo "=== $platform"
    vol=(-v "$root:/src" -v "opensow-target-$platform:/target" -e CARGO_TARGET_DIR=/target -w /src)
    case "$platform" in
    linux-*)
        arch=${platform#linux-}
        docker_platform=linux/arm64
        [ "$arch" = x86_64 ] && docker_platform=linux/amd64
        # the toolchain and cargo's registry stay in per-architecture volumes
        docker run --rm --platform "$docker_platform" "${vol[@]}" -v "opensow-cargo-$arch:/usr/local/cargo" -v "opensow-rustup-$arch:/usr/local/rustup" \
            ubuntu:22.04 bash -euc "set -o pipefail
            export DEBIAN_FRONTEND=noninteractive PATH=/usr/local/cargo/bin:\$PATH CARGO_HOME=/usr/local/cargo RUSTUP_HOME=/usr/local/rustup
            apt-get update -qq && apt-get install -y -qq curl ca-certificates build-essential pkg-config libasound2-dev libpulse-dev >/dev/null
            command -v cargo >/dev/null || curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain $toolchain >/dev/null
            cargo test --release --locked -q 2>&1 | grep -E '^test result|FAILED|panicked|^error'
            cargo build --profile dist --locked --no-default-features -p opensow-launcher -p opensow-game
            scripts/package.sh $platform dist /target/dist
        "
        ;;
    windows-x86_64)
        # the image's Rust can be older than the dependencies need: current stable, kept in a volume
        docker run --rm --platform linux/amd64 "${vol[@]}" -v opensow-cargo-xwin-registry:/usr/local/cargo/registry -v opensow-xwin-cache:/root/.cache/cargo-xwin \
            -v opensow-rustup-xwin:/usr/local/rustup messense/cargo-xwin:latest bash -euc "
            rustup toolchain install $toolchain --profile minimal --target x86_64-pc-windows-msvc >/dev/null 2>&1 && rustup default $toolchain >/dev/null
            cargo xwin build --profile dist --locked --no-default-features --target x86_64-pc-windows-msvc -p opensow-launcher -p opensow-game
            apt-get update -qq && apt-get install -y -qq p7zip-full >/dev/null
            ln -sf \$(command -v 7za || command -v 7z) /usr/local/bin/7z
            scripts/package.sh windows-x86_64 dist /target/x86_64-pc-windows-msvc/dist
        "
        ;;
    *)
        echo "unknown platform $platform" >&2
        exit 1
        ;;
    esac
done
ls -l "$root/dist"
