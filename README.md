# Spaces for Omarchy

A set of tabs for each thing you work on, for Hyprland on Omarchy.

A **space** holds one piece of work. Each space has its own **tabs**, and each tab is a Hyprland workspace (`s1:1`, `s1:2`, …) holding whatever windows you tile there. The plain numbered workspaces are the `home` space.

A space is called after the folder its first terminal is in: the repository's folder when it's in one, `~` for home. It follows that terminal as it changes folder. Name a space when you want a name that stays. A space lasts as long as its windows: close the last one, however it goes (`super+w`, `exit`, the app quitting), and the space ends, taking you back to the space you were in before. Closing the last window on one of a space's tabs takes you to its next tab, or the one before.

The spaces have their own column down the left of the screen, and the active space's tabs sit beside it on Omarchy's bar, in place of its workspaces. Collapsed, the column is a rail of badges, one per space: two letters for its name, as T3 Code marks projects, in a colour from the theme that stays with the name, with the space you're in outlined. Right-click a space to give its badge an icon instead, a Nerd Font glyph or an emoji: Tab to switch between them, or type to search both at once, and the badge shows the icon under the cursor until you pick one. Click the toggle at its top, or press a key you bind to it (`super+b` in [Keys](#keys)), to expand it into the spaces sidebar, with each space's name: your windows and the bar's tabs slide aside to reveal it, and the keyboard stays with your windows. Another key (`super+p`) searches it.

A tab is named after its most recently focused window: a terminal by what it's running (`claude`, `nvim`), or by the folder it's in when it's at its prompt, and any other app by its name. Name a tab yourself, by double-clicking it, by right-clicking it and choosing Rename, or with a key bound to `renameTab` (see Keys), and it keeps that name whatever runs in it, until you clear it with an empty name or close the tab. `home`'s tabs show just their numbers, as Omarchy's workspaces do, until you name one; each monitor's are named on their own.

A dot shows which spaces need you. An agent in a terminal says what it's doing, and its tab and its space's badge show it: red when it's waiting for you (a permission prompt, a question, an error), green when it finished while you were elsewhere, and a breathing dot in the text's colour while it works. A space shows whichever of its agents most needs you, and green clears once you focus the agent's window. A sound plays when an agent starts waiting for you, and when one finishes while you aren't looking at its window: Herdr's own (`assets/sounds/`), unless `[sounds]` in the config names others or sets `enabled = false`. Any native terminal works out of the box through `space status` (see Agent status below); if [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`) is installed, Spaces can also read OSC 7501 status from it automatically.

Click `+` at the end of the list to open a new space with a terminal in the folder you are in, or home if you aren't in one: you go there once the terminal is up, so you never see the space empty. A new tab, from the `+` after the tabs or `space ws new`, opens the same way, with a terminal in the space's folder. Right-click a space to rename it, change its icon or close it. Drag a space up or down to move it; a press has to move a good part of a row before it becomes a drag, so clicks stay clicks. `home` stays first. The sidebar takes the bar's look, which double-clicking the bar cycles between opaque, glassy and clear.

## Install

Spaces has two parts: the `space` command, in Rust, and an Omarchy shell plugin that draws the tabs and the sidebar. Each step below does only what it says, and [Undo](#undo) reverses each one. Nothing binds keys for you.

You need Omarchy 4.5 or newer, and Rust: check with `cargo --version`, and if it's missing, `sudo pacman -S rust`.

1. Get the code, and build `space` into `~/.local/bin`, which Omarchy has on its PATH. Keep the folder: the plugin is linked from it.

   ```sh
   git clone <repo-url> ~/.local/share/omarchy-spaces
   cd ~/.local/share/omarchy-spaces
   cargo build --release
   install -Dm755 target/release/space ~/.local/bin/space
   ```

2. Link the plugin into Omarchy's plugins and have the shell look for it:

   ```sh
   ln -sfn ~/.local/share/omarchy-spaces/omarchy ~/.config/omarchy/plugins/com.t4t5.spaces
   omarchy-shell shell rescanPlugins
   ```

3. Turn it on, with the tabs after Omarchy's menu, and turn off Omarchy's workspaces, which the tabs replace. If the first command says the plugin is unknown, the rescan hasn't finished: wait a second and run it again.

   ```sh
   omarchy plugin enable com.t4t5.spaces --after omarchy.menu
   omarchy plugin disable omarchy.workspaces
   ```

   The rail of spaces appears down the left of the screen and the tabs on the bar.

4. Link the Hyprland part. It ends a tab when you close its last window, and holds the tab actions for your keys:

   ```sh
   ln -sfn ~/.local/share/omarchy-spaces/hypr/omarchy-spaces.lua ~/.config/hypr/omarchy-spaces.lua
   ```

   Then add this line at the end of `~/.config/hypr/bindings.lua`, with the keys you want after it (see [Keys](#keys)):

   ```lua
   local spaces = require("hypr.omarchy-spaces")
   ```

### Update

```sh
cd ~/.local/share/omarchy-spaces
git pull
cargo build --release
install -Dm755 target/release/space ~/.local/bin/space
omarchy restart shell
```

### Undo

1. Bring back any windows you want to keep from your spaces: from each one, `space send home`. Once the plugin is off, Omarchy's workspace keys can't reach windows still in a space.

2. Turn the plugin off, which takes the tabs off the bar, and put Omarchy's workspaces back:

   ```sh
   omarchy plugin disable com.t4t5.spaces
   omarchy plugin enable omarchy.workspaces --after omarchy.menu
   rm ~/.config/omarchy/plugins/com.t4t5.spaces
   omarchy-shell shell rescanPlugins
   ```

   If you took `omarchy.menu` off the bar for the [toggle](#toggle-in-place-of-the-menu), put it back first with `omarchy plugin enable omarchy.menu --section left --index 0`.

3. Take the `require("hypr.omarchy-spaces")` line and the Spaces keys out of `~/.config/hypr/bindings.lua`, and save. Only then remove the file it loads, so Hyprland's config never points at a missing file:

   ```sh
   rm ~/.config/hypr/omarchy-spaces.lua
   ```

4. Remove the command, what it remembers, and the code. Take out any `space status` hooks you added to your agents too.

   ```sh
   rm ~/.local/bin/space
   rm -rf ~/.local/state/omarchy-spaces ~/.config/omarchy-spaces ~/.local/share/omarchy-spaces
   ```

Spaces opens plain Omarchy terminals (`xdg-terminal-exec`) by default. To get agent status dots and sounds in any native terminal, wire up your agent's hooks to `space status` (below). If you prefer [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`), install it on your `PATH` and Spaces uses it automatically (set `herdr_core = false` in `config.toml` to keep plain terminals even when `hc` is installed).

## Agent status

`space status <working|waiting|done|idle>` reports status for the terminal window it runs in (walking the process tree up to its Hyprland window). It exits quietly outside Hyprland, so hooks stay safe over SSH or in headless runs.

**Claude Code** (`~/.claude/settings.json`):

```json
{
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "space status working" }] }],
    "PermissionRequest": [{ "hooks": [{ "type": "command", "command": "space status waiting" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "space status done" }] }],
    "SessionEnd": [{ "hooks": [{ "type": "command", "command": "space status idle" }] }]
  }
}
```

**Codex** (`~/.codex/config.toml`):

```toml
notify = ["space", "status", "done"]
```

It also works for plain shell commands — e.g. `cargo test && space status done` lights up the tab green and rings when the build finishes while you're in another space. If an agent is killed (`Ctrl-C`) while `working` or `waiting`, Spaces notices its process exited and clears the dot automatically.

### Toggle in place of the menu

To save the rail a row, the sidebar's header can move up onto the bar, where Omarchy's menu is: the Omarchy logo sits over the rail and expands or collapses the sidebar, and when the sidebar expands, its title shows beside the logo with a toggle at its right edge. A right-click on the logo opens the Omarchy menu, as its key (`super+space` by default) still does. Take `omarchy.menu` off the bar and set `toggle` on the tabs' entry in `~/.config/omarchy/shell.json`:

```json
"left": [
  { "id": "com.t4t5.spaces", "toggle": true }
]
```

It needs the bar along the top or bottom of the screen, with the tabs first in its left section.

## Keys

Spaces binds no keys, so it never takes one of Omarchy's or yours. Bind the ones you want in `~/.config/hypr/bindings.lua`, after the `require` line from [Install](#install). These are the ones you'll most likely want:

```lua
local spaces = require("hypr.omarchy-spaces")

o.rebind("SUPER + B", "Show or hide spaces", "omarchy-shell shell call com.t4t5.spaces showOrHide ''")
o.rebind("SUPER + P", "Find a space", { panel = "com.t4t5.spaces" })
o.rebind("SUPER + N", "New space", "space new")
o.rebind("SUPER + SHIFT + J", "Next space", "space next")
o.rebind("SUPER + SHIFT + K", "Previous space", "space prev")
o.rebind("SUPER + T", "New tab", "space ws new")
for n = 1, 10 do
  local key = "code:" .. tostring(n + 9)
  o.rebind("SUPER + " .. key, "Go to tab " .. n, spaces.go_to_tab(n))
  o.rebind("SUPER + SHIFT + " .. key, "Move window to tab " .. n, spaces.move_window_to_tab(n))
end
```

`o.rebind` replaces whatever Omarchy binds to that key, so pick others if you use those; `omarchy menu keybindings --print` lists what's taken. The number keys do exactly what Omarchy's workspace keys do while you're in `home`. Everything else you can bind is below, as `"space …"` commands or `omarchy-shell shell call com.t4t5.spaces <action> ''`.

| Action | Does |
|---|---|
| `showOrHide` | Expand or collapse the spaces sidebar |
| `space next` / `space prev` | Next / previous space, down and up the sidebar. They wrap around, with `home` at the top |
| `space new` | New space, with a terminal in the folder you are in |
| `space send next` / `space send prev` | Send the window to the next / previous space, and go with it |
| `renameTab` | Name the tab you are on: type over its name on the bar, Enter to keep it, Escape to stop. An empty name has it called after its window again |
| `rename` | Rename the space you are in: type over its name in the sidebar, Enter to keep it, Escape to stop. An empty name has it called after its folder again |
| the `com.t4t5.spaces` panel | Find a space: type to filter, Enter to switch, Escape to stop. Expands the sidebar for the search if it's collapsed |
| `space terminal` | Terminal in the folder you are in; on an empty tab, the space's folder |
| `space ws new` | New tab, with a terminal in the space's folder |
| `spaces.go_to_tab(n)` | Go to tab N |
| `space ws next` / `space ws prev` | Next / previous tab |
| `spaces.move_window_to_tab(n)` | Move the window to tab N; with `false`, staying where you are |

In `home`, `go_to_tab` and `move_window_to_tab` behave exactly as Omarchy's workspace keys, so a new tab there opens empty.

## CLI

```sh
space new [dir]              # open a space with a terminal, in the folder you are in or dir
space go [space]             # go to a space where you left it
space next / space prev      # the space below or above the active one, wrapping around
space ls [--json]            # list spaces with window counts
space rename [space] <name>  # name a space, the active one by default; "" for its folder's name
space rename <space>:<n> <name>  # name a tab, as s1:2 or work:2; "" for its window's name
space icon [space] <icon>    # a glyph or emoji for a space's badge, the active one by default; "" for its letters
space reorder <space> <n>    # move a space to position n in the sidebar, 1 being the first below home
space close [space]          # close a space's windows, the active one by default
space restore                # open the spaces from before the last restart again
space restore <file>         # or those of an earlier session
space terminal               # a terminal in the folder you are in, or the space's
space status <working|waiting|done|idle> [--window <addr>] [--pid <pid>]  # report agent status for this terminal
space watch [--sounds]       # print the spaces as ls --json does, a line each time they change; --sounds also rings for agents
space send <space|next|prev> [--silent]  # the window to another space, on the tab you left it on
space ws <n|new|next|prev>
space move <n> [--silent]
```

A space can be given by what it's called or by its id (`s1`, as `space ls` shows). `space close` asks each app to close, as `super+w` does, so an app can still stop to save.

After you log out or restart, the plugin opens the spaces that were up again by itself (it runs `space restore`), `home` included: their names, icons and tab names, and their terminals, each in its folder and arranged as they were (splits and their sizes, groups, floating and fullscreen windows, monitors). A terminal that was running Claude Code picks its conversation up again (`claude --resume`), unless `resume_agents = false` in the config; with a `terminal` of your own, that takes one that runs `space shell`. Chromium's windows and web apps come back among them with their tabs, unless `restore_browser = false`: the tab you were on comes first, and the tabs load anew, without their history, groups or pins. Incognito windows don't, nor do windows of a browser on another data folder. If Chromium is set to continue where you left off, that's left to it. It then goes back to the space you were in, unless you have started on something else. Other programs in the terminals, what they had printed, and other apps' windows don't come back; the windows beside one take its room. Splits are restored in Hyprland's dwindle layout; in others, windows come back in the order they were opened.

`space watch` saves the session as it changes, in `session.json` beside `spaces.json`. What's new is saved within seconds, but what's gone only once it has been gone for ten seconds, and never while the computer shuts down, as that closes everything too. After a restart it waits in `session-previous.json` until it has been restored. The last 48 sessions are kept in `sessions/`, the one before each restart and at most one every 15 minutes besides: `space restore <file>` opens one of those. They have the addresses of the pages you had open, so only you can read them.

When `herdr-core` (`hc`) is installed and enabled (`herdr_core` unset or `true`), terminals Spaces opens run `space shell`: a shell in herdr-core, in the terminal's folder and with its environment, labelled with the window's process so Spaces can tell which window shows it, and attached to the window. The window is the terminal: closing it, or the shell exiting, ends both, and `ctrl+\` (`hc attach`'s detach) closes it too. Inside, `TERM` is core's (`xterm-256color`), not your terminal app's. Without `hc` (or with `herdr_core = false`), Spaces opens your native terminal directly with no wrapper.

## How it fits together

- `src/` is the `space` CLI, in Rust. It keeps no state about windows: it asks Hyprland over its socket, then dispatches. Terminal folders and foreground programs come from their shells in `/proc`, as Omarchy's `omarchy-cmd-terminal-cwd` does (or from herdr-core over its socket in `core.rs` for terminals running in it). Native agent statuses from `space status` live as per-window files under `$XDG_RUNTIME_DIR/omarchy-spaces/status/` and are pruned automatically as windows close or agents exit.
- `omarchy/` is the Omarchy shell plugin (QML): the tabs as a bar widget (`TabsWidget.qml`), and the sidebar, which `Base.qml` draws under the windows. Their logic is in `Tabs.qml`, `Sidebar.qml`, `SidebarList.qml` and `Spaces.mjs`. Both read the spaces from one `space watch` that the plugin's service runs (`Watch.qml`) and restarts when it ends, as it does when Hyprland restarts or `space` is reinstalled. It reads Hyprland again when Hyprland's events say something changed, and every two seconds, as nothing says when a window is resized. It follows herdr-core's events for its terminals. A shell outside core changing folder or starting a program sends no event, so it reads the active space's other terminals' shells five times a second, the rest every two seconds, and all of them when a window's title changes, as terminals retitle themselves on `cd` and as a command starts. A terminal's program is the leader of the foreground process group on its shell's tty. What Chromium's windows hold it reads from the session files Chromium keeps open in its profile, matching them to Hyprland's windows by title, or the order they were opened in.
- `hypr/omarchy-spaces.lua` holds the tab actions for your own keys, the hook that moves on from a tab emptied by closing its last window (`space emptied`), and the blur behind a glassy sidebar and bar.
- `~/.local/state/omarchy-spaces/spaces.json` holds what Hyprland can't about each live space: the name and icon you gave it, the terminal it follows and that terminal's last folder. `~/.config/omarchy-spaces/config.toml` can set `terminal`, the command that opens one (default: Omarchy's, `xdg-terminal-exec`), `herdr_core` (`true`/`false`, default: auto when `hc` is on `PATH`), `picker`, for `space go` with no space named, `resume_agents` and `restore_browser` (`true`/`false`, default: `true`), and `[sounds]`: `done` and `waiting` files, and `enabled`.

## Development

The icon picker's glyphs, `omarchy/glyphs.json`, come from Nerd Fonts' list of glyph names; `just glyphs` makes it again, for a newer Nerd Fonts. Its emojis are Omarchy's own emoji picker's.

```sh
cargo test
node --test omarchy/tests/*.test.mjs
just reload             # after changing the plugin; the shell doesn't reload it by itself
```

The justfile's `install` and `setup` do Install's steps for a clone you work in, building into `~/.cargo/bin` and adding the `require` line for you.
