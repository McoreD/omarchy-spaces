# renSpace

Spaces for Hyprland on Omarchy: a set of tabs for each thing you work on.

A **space** holds one piece of work. Each space has its own **tabs**, and each tab is a Hyprland workspace (`s1:1`, `s1:2`, …) holding whatever windows you tile there. The plain numbered workspaces are the `home` space.

A space is called after the folder its first terminal is in: the repository's folder when it's in one, `~` for home. It follows that terminal as it changes folder. Name a space when you want a name that stays. A space lasts as long as its windows: close the last one, however it goes (`super+w`, `exit`, the app quitting), and the space ends, taking you back to the space you were in before. Closing the last window on one of a space's tabs takes you to its next tab, or the one before.

The spaces have their own column down the left of the screen, and the active space's tabs sit beside it on Omarchy's bar, in place of its workspaces. Collapsed, the column is a rail of badges, one per space: two letters for its name, as T3 Code marks projects, in a colour from the theme that stays with the name, with the space you're in outlined. Press `super+b`, or click the toggle at its top, to expand it into the spaces sidebar, with each space's name: your windows and the bar's tabs slide aside to reveal it, and the keyboard stays with your windows. Press `super+p` to search it.

Click `+` at the end of the list to open a new space with a terminal in the folder you are in, or home if you aren't in one: you go there once the terminal is up, so you never see the space empty. Right-click a space to rename or close it. The sidebar takes the bar's look, which double-clicking the bar cycles between opaque, glassy and clear.

## Install

```sh
just install   # build the space CLI into ~/.cargo/bin
just setup     # add the tabs to the bar, the sidebar and the keys
```

## Keys

| Keys | Does |
|---|---|
| `super+b` | Expand or collapse the spaces sidebar |
| `super+p` | Find a space: type to filter, Enter to switch, Escape to stop. Expands the sidebar for the search if it's collapsed |
| `super+enter` | Terminal in the folder you are in; on an empty tab, the space's folder |
| `super+t` | New tab |
| `super+1..0` | Go to tab N |
| `super+tab` / `super+shift+tab` | Next / previous tab |
| `super+shift+1..0` | Move the window to tab N |
| `super+shift+alt+1..0` | Move the window to tab N, staying where you are |

In `home`, the tab keys behave exactly as Omarchy's workspace keys.

## CLI

```sh
space new [dir]              # open a space with a terminal, in the folder you are in or dir
space go [space]             # go to a space where you left it
space ls [--json]            # list spaces with window counts
space rename [space] <name>  # name a space, the active one by default; "" for its folder's name
space close [space]          # close a space's windows, the active one by default
space terminal               # a terminal in the folder you are in, or the space's
space watch                  # print the spaces as ls --json does, a line each time they change
space ws <n|new|next|prev>
space move <n> [--silent]
```

A space can be given by what it's called or by its id (`s1`, as `space ls` shows). `space close` asks each app to close, as `super+w` does, so an app can still stop to save.

## How it fits together

- `src/` is the `space` CLI, in Rust. It keeps no state about windows: it asks Hyprland over its socket, then dispatches. It finds a terminal's folder from its shell, in `/proc`, as Omarchy's `omarchy-cmd-terminal-cwd` does.
- `omarchy/` is the Omarchy shell plugin (QML): the tabs as a bar widget (`TabsWidget.qml`), and the sidebar, which `Base.qml` draws under the windows. Their logic is in `Tabs.qml`, `Sidebar.qml`, `SidebarList.qml` and `Spaces.mjs`. Both read the spaces from one `space watch` that the plugin's service runs (`Watch.qml`) and restarts when it ends, as it does when Hyprland restarts or `space` is reinstalled. It reads Hyprland again only when Hyprland's events say something changed. A shell changing folder sends no event, so it reads the active space's terminals' folders five times a second, the rest every two seconds, and all of them when a window's title changes, as terminals retitle themselves on `cd`.
- `hypr/renspace.lua` holds the keybindings, the hook that moves on from a tab emptied by closing its last window (`space emptied`), and the blur behind a glassy sidebar and bar.
- `~/.local/state/renspace/spaces.json` holds what Hyprland can't about each live space: the name you gave it, the terminal it follows and that terminal's last folder. `~/.config/renspace/config.toml` can set `terminal`, the command that opens one (default: Omarchy's, `xdg-terminal-exec`), and `picker`, for `space go` with no space named.

## Development

```sh
cargo test
node --test omarchy/tests/*.test.mjs
omarchy-restart-shell   # after changing the plugin; the shell doesn't reload it by itself
```
