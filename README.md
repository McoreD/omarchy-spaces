# renSpace

Spaces for Hyprland on Omarchy: a set of tabs for each thing you work on.

A **space** holds one piece of work. Each space has its own **tabs**, and each tab is a Hyprland workspace (`s1:1`, `s1:2`, …) holding whatever windows you tile there. The plain numbered workspaces are the `home` space.

A space is called after the folder its first terminal is in: the repository's folder when it's in one, `~` for home. It follows that terminal as it changes folder. Name a space when you want a name that stays. A space lasts as long as its windows: close the last one, however it goes (`super+w`, `exit`, the app quitting), and the space ends, taking you back to the space you were in before. Closing the last window on one of a space's tabs takes you to its next tab, or the one before.

The spaces have their own column down the left of the screen, and the active space's tabs sit beside it on Omarchy's bar, in place of its workspaces. Collapsed, the column is a rail of badges, one per space: two letters for its name, as T3 Code marks projects, in a colour from the theme that stays with the name, with the space you're in outlined. Press `super+b`, or click the toggle at its top, to expand it into the spaces sidebar, with each space's name: your windows and the bar's tabs slide aside to reveal it, and the keyboard stays with your windows. Press `super+p` to search it.

A tab is named after its most recently focused window: a terminal by what it's running (`claude`, `nvim`), or by the folder it's in when it's at its prompt, and any other app by its name. Name a tab yourself, by double-clicking it or with `super+,`, and it keeps that name whatever runs in it, until you clear it with an empty name or close the tab.

A dot shows which spaces need you. An agent in a terminal says what it's doing, and its tab and its space's badge show it: red when it's waiting for you (a permission prompt, a question, an error), green when it finished while you were elsewhere, and a breathing dot in the text's colour while it works. A space shows whichever of its agents most needs you, and green clears once you focus the agent's window. A sound plays when an agent starts waiting for you, and when one finishes while you aren't looking at its window: Herdr's own (`assets/sounds/`), unless `[sounds]` in the config names others or sets `enabled = false`. This needs [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`): the terminals renSpace opens then run in it, and any agent that reports its status through OSC 7501, as Claude Code, opencode, pi and Kimi do, shows here. Without it, terminals are plain and have no dots.

Click `+` at the end of the list to open a new space with a terminal in the folder you are in, or home if you aren't in one: you go there once the terminal is up, so you never see the space empty. A new tab, from `super+t` or the `+` after the tabs, opens the same way, with a terminal in the space's folder. Right-click a space to rename or close it, or press `super+shift+r` to rename the one you are in. The sidebar takes the bar's look, which double-clicking the bar cycles between opaque, glassy and clear.

## Install

```sh
just install   # build the space CLI into ~/.cargo/bin
just setup     # add the tabs to the bar, the sidebar and the keys
```

For agents' status, install herdr-core as its README says, so `hc` is on your PATH. renSpace starts its core when it isn't running.

### Toggle in place of the menu

To save the rail a row, the sidebar's header can move up onto the bar, where Omarchy's menu is: the Omarchy logo sits over the rail and expands or collapses the sidebar, and when the sidebar expands, its title shows beside the logo with a toggle at its right edge. A right-click on the logo opens the Omarchy menu, as its key (`super+space` by default) still does. Take `omarchy.menu` off the bar and set `toggle` on the tabs' entry in `~/.config/omarchy/shell.json`:

```json
"left": [
  { "id": "org.ren.renspace", "toggle": true }
]
```

It needs the bar along the top or bottom of the screen, with the tabs first in its left section.

## Keys

| Keys | Does |
|---|---|
| `super+b` | Expand or collapse the spaces sidebar |
| `super+j` / `super+k` | Next / previous space, down and up the sidebar. They wrap around, with `home` at the top |
| `super+n` | New space, with a terminal in the folder you are in |
| `super+shift+j` / `super+shift+k` | Send the window to the next / previous space, and go with it |
| `super+,` | Name the tab you are on: type over its name on the bar, Enter to keep it, Escape to stop. An empty name has it called after its window again |
| `super+shift+r` | Rename the space you are in: type over its name in the sidebar, Enter to keep it, Escape to stop. An empty name has it called after its folder again |
| `super+p` | Find a space: type to filter, Enter to switch, Escape to stop. Expands the sidebar for the search if it's collapsed |
| `super+enter` | Terminal in the folder you are in; on an empty tab, the space's folder |
| `super+t` | New tab, with a terminal in the space's folder |
| `super+1..0` | Go to tab N |
| `super+tab` / `super+shift+tab` | Next / previous tab |
| `super+shift+1..0` | Move the window to tab N |
| `super+shift+alt+1..0` | Move the window to tab N, staying where you are |

In `home`, the tab keys behave exactly as Omarchy's workspace keys, so a new tab there opens empty.

## CLI

```sh
space new [dir]              # open a space with a terminal, in the folder you are in or dir
space go [space]             # go to a space where you left it
space next / space prev      # the space below or above the active one, wrapping around
space ls [--json]            # list spaces with window counts
space rename [space] <name>  # name a space, the active one by default; "" for its folder's name
space rename <space>:<n> <name>  # name a tab, as s1:2 or ren:2; "" for its window's name
space close [space]          # close a space's windows, the active one by default
space terminal               # a terminal in the folder you are in, or the space's
space watch [--sounds]       # print the spaces as ls --json does, a line each time they change; --sounds also rings for agents
space send <space|next|prev> [--silent]  # the window to another space, on the tab you left it on
space ws <n|new|next|prev>
space move <n> [--silent]
```

A space can be given by what it's called or by its id (`s1`, as `space ls` shows). `space close` asks each app to close, as `super+w` does, so an app can still stop to save.

A terminal renSpace opens runs `space shell`: a shell in herdr-core, in the terminal's folder and with its environment, labelled with the window's process so renSpace can tell which window shows it, and attached to the window. The window is the terminal: closing it, or the shell exiting, ends both, and `ctrl+\` (`hc attach`'s detach) closes it too. Inside, `TERM` is core's (`xterm-256color`), not your terminal app's. Without core, `space shell` runs your own shell.

## How it fits together

- `src/` is the `space` CLI, in Rust. It keeps no state about windows: it asks Hyprland over its socket, then dispatches. A terminal's folder, what it runs and its agent's status come from herdr-core, over its socket (`core.rs`), for terminals running in it; other terminals' folders come from their shells, in `/proc`, as Omarchy's `omarchy-cmd-terminal-cwd` does.
- `omarchy/` is the Omarchy shell plugin (QML): the tabs as a bar widget (`TabsWidget.qml`), and the sidebar, which `Base.qml` draws under the windows. Their logic is in `Tabs.qml`, `Sidebar.qml`, `SidebarList.qml` and `Spaces.mjs`. Both read the spaces from one `space watch` that the plugin's service runs (`Watch.qml`) and restarts when it ends, as it does when Hyprland restarts or `space` is reinstalled. It reads Hyprland again only when Hyprland's events say something changed. It follows herdr-core's events for its terminals. A shell outside core changing folder or starting a program sends no event, so it reads the active space's other terminals' shells five times a second, the rest every two seconds, and all of them when a window's title changes, as terminals retitle themselves on `cd` and as a command starts. A terminal's program is the leader of the foreground process group on its shell's tty.
- `hypr/renspace.lua` holds the keybindings, the hook that moves on from a tab emptied by closing its last window (`space emptied`), and the blur behind a glassy sidebar and bar.
- `~/.local/state/renspace/spaces.json` holds what Hyprland can't about each live space: the name you gave it, the terminal it follows and that terminal's last folder. `~/.config/renspace/config.toml` can set `terminal`, the command that opens one (default: Omarchy's, `xdg-terminal-exec`, running `space shell`; have yours run `space shell` too for agents' status), `picker`, for `space go` with no space named, and `[sounds]`: `done` and `waiting` files, and `enabled`.

## Development

```sh
cargo test
node --test omarchy/tests/*.test.mjs
omarchy-restart-shell   # after changing the plugin; the shell doesn't reload it by itself
```
