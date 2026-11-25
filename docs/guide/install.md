# Installation

You need Omarchy 4.5 or newer, on x86-64 or ARM64. To install, follow the steps in the [README](../../README.md#install).

## What gets installed

| Part | Where | What it does |
|---|---|---|
| The plugin | `~/.config/omarchy/plugins/com.t4t5.spaces` | Draws the sidebar and the tabs |
| The `space` command | `~/.local/bin/space` | Does the work. Downloaded as the plugin first starts: the build for the plugin's version from its GitHub release, checked against the release's checksum |
| The Hyprland part | `~/.config/hypr/omarchy-spaces.lua`, a link into the plugin | Keeps every monitor on the same space, moves you on when you close a tab's last window, and holds the [actions for your keys](keys.md) |

The plugin runs `install.sh --setup` as it starts, which puts in place what's missing of these. Nothing binds keys for you.

If the download fails, as it does offline, it tries again the next time the shell starts. To try at once:

```sh
omarchy restart shell
```

## The bar

A notification asks whether Spaces should replace Omarchy's menu and workspaces on the bar, if you put it in the left section. Until you answer, it asks again each time the shell starts. To change your mind, or to answer without it:

```sh
~/.config/omarchy/plugins/com.t4t5.spaces/install.sh --bar replace
```

To put Omarchy's back, beside Spaces or in place of it:

```sh
omarchy plugin enable omarchy.menu --section left --index 0
omarchy plugin enable omarchy.workspaces --after omarchy.menu
```

## Update

```sh
omarchy plugin update com.t4t5.spaces
```

If the plugin installed `space`, it sees the new version in `manifest.json`, runs `install.sh --update` to download the matching release binary and switches to it. If the plugin's UI changed, it notifies you to run `omarchy restart shell` (otherwise the new UI loads on your next login). Builds installed from source are left alone — update those with `cargo install` below.

## Build from source

With Rust (`sudo pacman -S rust` if `cargo --version` fails), run this once the plugin is added, and again after each update:

```sh
cargo install --locked --force --root ~/.local --target-dir ~/.cache/omarchy-spaces --path ~/.config/omarchy/plugins/com.t4t5.spaces
```

It builds outside the plugin's folder, as the shell watches that for changes.

## Uninstall

1. Bring back any windows you want to keep. Once the plugin is off, Omarchy's workspace keys can't reach windows still in a space. From each of those windows, run:

   ```sh
   space window send home
   ```

2. Turn the plugin off, and put Omarchy's menu and workspaces back on the bar:

   ```sh
   omarchy plugin disable com.t4t5.spaces
   omarchy plugin enable omarchy.menu --section left --index 0
   omarchy plugin enable omarchy.workspaces --after omarchy.menu
   ```

3. Take the `require("hypr.omarchy-spaces")` line and the Spaces keys out of `~/.config/hypr/bindings.lua`, and save.

4. Only then remove the file it loaded, so Hyprland's config never points at a missing file, along with the plugin, the command and what it remembers:

   ```sh
   rm ~/.config/hypr/omarchy-spaces.lua
   omarchy plugin remove com.t4t5.spaces --yes
   rm ~/.local/bin/space
   rm -rf ~/.local/state/omarchy-spaces ~/.config/omarchy-spaces ~/.cache/omarchy-spaces
   ```

   If you built it from source, run `cargo uninstall --root ~/.local omarchy-spaces` in place of `rm ~/.local/bin/space`.
