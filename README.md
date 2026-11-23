# renSpace

Spaces for Hyprland on Omarchy: switch projects, not workspaces.

A **space** is a project: a name and a folder. Each space has its own **tabs**, and each tab is a Hyprland workspace (`ren:1`, `ren:2`, …) holding whatever windows you tile there. The plain numbered workspaces are the `home` space.

The active space's name and tabs sit on Omarchy's bar, in place of its workspaces. Click the name, or press `super+b`, to show or hide the spaces sidebar: it sits on the base under the workspaces, and showing it slides the windows aside to reveal it. It keeps up as you switch spaces and tabs, and leaves the keyboard with your windows. Press `super+p` to search it; type a name no space has to make a new one, with a folder of that name in your first root (or an existing folder there). Right-click a space to rename it, close its windows, or remove it. It takes the bar's look, which double-clicking the bar cycles between opaque, glassy and clear.

## Install

```sh
just install   # build the space CLI into ~/.cargo/bin
just setup     # add the tabs to the bar, the sidebar and the keys
```

## Keys

| Keys | Does |
|---|---|
| `super+b` | Show or hide the spaces sidebar |
| `super+p` | Find a space: type to filter, Enter to switch, Escape to stop. Shows the sidebar for the search if it's hidden |
| `super+t` | New tab |
| `super+1..0` | Go to tab N |
| `super+tab` / `super+shift+tab` | Next / previous tab |
| `super+shift+1..0` | Move the window to tab N |

In `home`, the tab keys behave exactly as Omarchy's workspace keys.

## CLI

```sh
space new [name] [dir]       # make a folder a space (writes .renspace.toml) and go to it
space new <name> --in-roots  # the same, in a folder of that name in your roots
space go [name]              # go to a space where you left it
space ls [--json]            # list spaces with window counts
space rename [old] <new>     # rename a space, the active one by default; its windows stay
space close [name]           # close a space's windows, the active one by default
space rm <name>              # stop a folder being a space; the folder stays
space ws <n|new|next|prev>
space move <n> [--silent]
```

`space close` asks each app to close, as `super+w` does, so an app can still stop to save. `space rm` only removes the `.renspace.toml`, and refuses while the space has windows open.

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
