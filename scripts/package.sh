#!/bin/bash

# Packs a built `space` into dist/ as install.sh downloads it:
#   scripts/package.sh target/release/space x86_64

set -euo pipefail

binary=$(realpath "$1")
arch=$2
cd "$(dirname "${BASH_SOURCE[0]}")/.."
version=$(sed -n 's/^  "version": "\([0-9][0-9.]*\)",$/\1/p' manifest.json)
out=${3:-dist}
mkdir -p "$out"
out=$(realpath "$out")

# With the licenses: of its code, and of the sounds built into it.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
install -m755 "$binary" "$tmp/space"
cp LICENSE "$tmp/LICENSE"
cp assets/sounds/LICENSE "$tmp/LICENSE-sounds"

archive="space-$version-$arch-linux.tar.gz"
tar -czf "$out/$archive" -C "$tmp" space LICENSE LICENSE-sounds
cd "$out"
sha256sum "$archive" >"$archive.sha256"
