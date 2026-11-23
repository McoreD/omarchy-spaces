# renSpace

Spaces for Hyprland on Omarchy: switch projects, not workspaces.

A **space** is a project: a name and a folder. Each space has its own **tabs**, and each tab is a Hyprland workspace (`ren:1`, `ren:2`, …) holding whatever windows you tile there. The plain numbered workspaces are the `home` space.

The active space's name and tabs sit on Omarchy's bar, in place of its workspaces. Click the name, or press `super+p`, for the spaces sidebar: it sits on the base under the workspaces, and opening it slides the windows aside to reveal it. It stays open until you close it, keeping up as you switch spaces and tabs. It takes the bar's look, which double-clicking the bar cycles between opaque, glassy and clear.

## Install

```sh
just install   # build the space CLI into ~/.cargo/bin
just setup     # add the tabs to the bar, the sidebar and the keys
```

## Keys

| Keys | Does |
|---|---|
| `super+p` | Open the spaces sidebar: type to filter, Enter to switch. Press it again to close it, or, after you've gone back to your windows, to type in it again |
| `super+t` | New tab |
| `super+1..0` | Go to tab N |
| `super+tab` / `super+shift+tab` | Next / previous tab |
| `super+shift+1..0` | Move the window to tab N |

In `home`, the tab keys behave exactly as Omarchy's workspace keys.

## CLI

```sh
space new [name] [dir]   # make a folder a space (writes .renspace.toml) and go to it
space go [name]          # go to a space where you left it
space ls [--json]        # list spaces with window counts
space ws <n|new|next|prev>
space move <n> [--silent]
```

## How it fits together

- `src/` is the `space` CLI, in Rust. It keeps no state about windows: it asks Hyprland over its socket, then dispatches.
- `omarchy/` is the Omarchy shell plugin (QML): the tabs as a bar widget (`TabsWidget.qml`), and the sidebar, which `Base.qml` draws under the windows. Their logic is in `Tabs.qml`, `Sidebar.qml` and `Spaces.mjs`.
- `hypr/renspace.lua` holds the keybindings, and the blur behind a glassy sidebar and bar.
- A folder is a space when it has a `.renspace.toml`. `~/.config/renspace/config.toml` can set `roots`, the folders scanned for spaces (default `~/Projects` and `~/Spaces`). `~/.local/state/renspace/` is a disposable cache.

## Development

```sh
cargo test
node --test omarchy/tests/*.test.mjs
omarchy-restart-shell   # after changing the plugin; the shell doesn't reload it by itself
```
