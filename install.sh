#!/bin/bash

# Installs the `space` command that goes with this checkout of the plugin, from
# its GitHub release, into ~/.local/bin. With `--update`, runs only when this
# script installed the current binary and its version differs from the plugin's.

set -euo pipefail

fail() {
  echo "install.sh: $*" >&2
  exit 1
}

update=0
case ${1:-} in
"") ;;
--update) update=1 ;;
*) fail "usage: install.sh [--update]" ;;
esac

plugin_dir=$(dirname "${BASH_SOURCE[0]}")
cd "$plugin_dir"
releases=${SPACES_RELEASES:-https://github.com/t4t5/omarchy-spaces/releases/download}
version=$(sed -n 's/^  "version": "\([0-9][0-9.]*\)",\?$/\1/p' manifest.json)
[[ -n $version ]] || fail "no version in manifest.json"

arch=$(uname -m)
case $arch in
x86_64 | aarch64) ;;
*) fail "there's no build for $arch: build it from source (see the README)" ;;
esac

bin="$HOME/.local/bin"
state="${XDG_STATE_HOME:-$HOME/.local/state}/omarchy-spaces"
marker="$state/installed"

if (( update )); then
  [[ ! -L $plugin_dir ]] || exit 0
  [[ -f $marker && -x $bin/space ]] || exit 0
  found=$(command -v space || true)
  [[ -z $found || $found == "$bin/space" ]] || exit 0
  current=$(sha256sum "$bin/space" | awk '{print $1}')
  [[ $current == "$(<"$marker")" ]] || exit 0
  [[ $("$bin/space" --version 2>/dev/null || true) != "space $version" ]] || exit 0
fi

if [[ -e $bin/space ]] && ! grep -q omarchy-spaces "$bin/space"; then
  fail "$bin/space is another program: move it first"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
archive="space-$version-$arch-linux.tar.gz"
for file in "$archive" "$archive.sha256"; do
  curl -fsSL --proto-redir =https -o "$tmp/$file" "$releases/v$version/$file" ||
    fail "can't download $file: is version $version released yet? Nothing was changed"
done
(cd "$tmp" && sha256sum --quiet --check "$archive.sha256") ||
  fail "$archive isn't what the release says it is. Nothing was changed"
tar -xzf "$tmp/$archive" -C "$tmp" space

# In one step, so that nothing runs half of it.
install -Dm755 "$tmp/space" "$bin/.space.$$"
mv "$bin/.space.$$" "$bin/space"
mkdir -p "$state"
sha256sum "$bin/space" | awk '{print $1}' >"$marker.tmp.$$"
mv "$marker.tmp.$$" "$marker"
echo "Installed space $version into $bin"

if (( ! update )); then
  found=$(command -v space || true)
  if [[ -z $found ]]; then
    echo "install.sh: $bin isn't on your PATH, so nothing finds it there yet" >&2
  elif [[ $found != "$bin/space" ]]; then
    echo "install.sh: $found comes first on your PATH, so that one runs instead" >&2
  fi
fi
