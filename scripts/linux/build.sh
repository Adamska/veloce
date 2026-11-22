#!/usr/bin/env bash
# Builds a portable x86_64 Linux package of Veloce in Docker and writes
# veloce-<version>-linux-x86_64.tar.gz to OUTPUT_DIR (default: target/linux/dist).
# Works from macOS on Apple silicon through Docker's x86_64 emulation; the first run compiles
# OpenSSL, dav1d and FFmpeg into the image and takes a while, later runs reuse it.
set -euo pipefail

cd "$(dirname "$0")/../.."
output_dir="${1:-target/linux/dist}"
image=veloce-linux-builder
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
name="veloce-$version-linux-x86_64"

docker build --platform linux/amd64 -t "$image" scripts/linux
docker run --rm --platform linux/amd64 \
    -v "$PWD":/work -w /work \
    -v veloce-cargo-registry:/root/.cargo/registry \
    "$image" \
    cargo build --release --locked --features ffmpeg-next/static --target-dir target/linux

binary=target/linux/release/veloce
stage="target/linux/$name"
rm -rf "$stage"
mkdir -p "$stage"
cp "$binary" "$stage/veloce"
cp assets/logo/veloce-256.png "$stage/veloce.png"
cp LICENSE "$stage/LICENSE"
cp scripts/linux/install.sh "$stage/install.sh"
cp scripts/linux/README.txt "$stage/README.txt"
chmod +x "$stage/veloce" "$stage/install.sh"

mkdir -p "$output_dir"
tar -C target/linux -czf "$output_dir/$name.tar.gz" "$name"
ls -lh "$output_dir/$name.tar.gz"
