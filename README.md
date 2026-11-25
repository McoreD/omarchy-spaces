# Spaces for Omarchy

A set of tabs for each thing you work on, for Hyprland on Omarchy.

A **space** holds one piece of work. Each space has its own **tabs**, and each tab is a Hyprland workspace (`s1:1`, `s1:2`, …) holding whatever windows you tile there. The plain numbered workspaces are the `home` space.

With more than one monitor, they all show the same space, and each has tabs of its own in it. Going to another space takes every monitor along, each back to the tab you last used on it, or to an empty one if you haven't used the space there. Moving between monitors only changes whose tabs your keys act on, never the space. Unplug a monitor and its tabs join those of the one that's left, until it's back: each takes the lowest number free there, so with tabs 1 to 3 the other monitor's first is tab 4, for the keys too. Drag one among them, or one past it, and it stays there for good.

A space is called after the folder its first terminal is in: the repository's folder when it's in one, `~` for home. It follows that terminal as it changes folder. Name a space when you want a name that stays. A space lasts as long as its windows: close the last one, however it goes (`super+w`, `exit`, the app quitting), and the space ends, taking you back to the space you were in before. Closing the last window on one of a space's tabs takes you to its next tab on that monitor, or the one before; with none there, it stays, empty, as long as the space has windows on another.

The spaces have their own column down the left of the screen, and the active space's tabs sit beside it on Omarchy's bar, in place of its workspaces. Collapsed, the column is a rail of badges, one per space: two letters for its name, as T3 Code marks projects, in a colour from the theme that stays with the name, with the space you're in outlined. Right-click a space to give its badge an icon instead, a Nerd Font glyph or an emoji: Tab to switch between them, or type to search both at once, and the badge shows the icon under the cursor until you pick one. Click the logo above it, or press a key you bind to it (`super+b` in [Keys](#keys)), to expand it into the spaces sidebar, with each space's name: your windows and the bar's tabs slide aside to reveal it, and the keyboard stays with your windows. Another key (`super+p`) searches it.

A tab is named after its most recently focused window: a terminal by what it's running (`claude`, `nvim`), or by the folder it's in when it's at its prompt, and any other app by its name. Name a tab yourself, by double-clicking it, by right-clicking it and choosing Rename, or with a key bound to `spaces.rename_tab()` (see [Keys](#keys)), and it keeps that name whatever runs in it, until you clear it with an empty name or close the tab. Close a tab from the same menu: if it has windows it asks first, as they close with it. Or move it to another space, where it comes after the tabs there, with its windows arranged as they are and the name you gave it, and you go with it: pick the space from the menu, or with a key bound to `spaces.send_tab()`. Drag a tab along the bar to move it among its space's tabs; the space keeps the numbers its tabs have, what's on them moves along, and you stay on the tab you're on. (Dragging the tabs' `+` still moves the whole widget on the bar.) `home`'s tabs show just their numbers, as Omarchy's workspaces do, until you name one; each monitor's are named on their own.

A dot shows which spaces need you. An agent in a terminal says what it's doing, and its tab and its space's badge show it: red when it's waiting for you (a permission prompt, a question, an error), green when it finished while you were elsewhere, and a breathing yellow dot while it works. A space shows whichever of its agents most needs you, and green clears once you focus the agent's window. A sound plays when an agent starts waiting for you, and when one finishes while you aren't looking at its window: Herdr's own (`assets/sounds/`), unless `[sounds]` in the config names others or sets `enabled = false`. Any native terminal works out of the box through `space status` (see Agent status below); if [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`) is installed, Spaces can also read OSC 7501 status from it automatically.

Click `+` at the end of the list to open a new space with a terminal in the folder you are in, or home if you aren't in one: you go there once the terminal is up, so you never see the space empty. A new tab, from the `+` after the tabs or `space tab new`, opens the same way, with a terminal in the space's folder. Right-click a space to rename it, change its icon or close it. Drag a space up or down to move it; a press has to move a good part of a row before it becomes a drag, so clicks stay clicks. `home` stays first. The sidebar takes the bar's look, which double-clicking the bar cycles between opaque, glassy and clear.

## Install

Spaces has two parts: the `space` command, in Rust, and an Omarchy shell plugin that draws the tabs and the sidebar. Each step below does only what it says, and [Undo](#undo) reverses each one. Nothing binds keys for you.

You need Omarchy 4.5 or newer, on x86-64 or ARM64.

1. Add the plugin. When it asks whether to enable it, say no: step 3 does, in the right place on the bar.

   ```sh
   omarchy plugin add https://github.com/t4t5/omarchy-spaces.git
   ```

2. Install `space` into `~/.local/bin`, which Omarchy has on its PATH. The script downloads the build for the plugin's version from its GitHub release and checks it against the release's checksum. To build it yourself instead, see [From source](#from-source).

   ```sh
   ~/.config/omarchy/plugins/com.t4t5.spaces/install.sh
   ```

3. Turn it on, first on the bar, and take off Omarchy's menu and workspaces, which its logo and tabs replace. The menu still opens with a right-click on the logo, or its key (`super+space` by default).

   ```sh
   omarchy plugin enable com.t4t5.spaces --section left --index 0
   omarchy plugin disable omarchy.menu
   omarchy plugin disable omarchy.workspaces
   ```

   The rail of spaces appears down the left of the screen, and the logo and tabs on the bar. To keep the menu on the bar, see [The logo](#the-logo).

4. Link the Hyprland part. It keeps every monitor on the same space, ends a tab when you close its last window, and holds the tab actions for your keys:

   ```sh
   ln -sfn ~/.config/omarchy/plugins/com.t4t5.spaces/hypr/omarchy-spaces.lua ~/.config/hypr/omarchy-spaces.lua
   ```

   Then add this line at the end of `~/.config/hypr/bindings.lua`, with the keys you want after it (see [Keys](#keys)):

   ```lua
   local spaces = require("hypr.omarchy-spaces")
   ```

### Update

The plugin and `space` go together, so update both:

```sh
omarchy plugin update com.t4t5.spaces && ~/.config/omarchy/plugins/com.t4t5.spaces/install.sh && omarchy restart shell
```

### From source

With Rust (`sudo pacman -S rust` if `cargo --version` fails), in place of `install.sh`, when you install and when you update:

```sh
cargo install --locked --force --root ~/.local --target-dir ~/.cache/omarchy-spaces --path ~/.config/omarchy/plugins/com.t4t5.spaces
```

It builds outside the plugin's folder, as the shell watches that for changes.

### Undo

1. Bring back any windows you want to keep from your spaces: from each one, `space window send home`. Once the plugin is off, Omarchy's workspace keys can't reach windows still in a space.

2. Turn the plugin off, which takes the logo and tabs off the bar, and put Omarchy's menu and workspaces back:

   ```sh
   omarchy plugin disable com.t4t5.spaces
   omarchy plugin enable omarchy.menu --section left --index 0
   omarchy plugin enable omarchy.workspaces --after omarchy.menu
   ```

3. Take the `require("hypr.omarchy-spaces")` line and the Spaces keys out of `~/.config/hypr/bindings.lua`, and save. Only then remove the file it loads, so Hyprland's config never points at a missing file:

   ```sh
   rm ~/.config/hypr/omarchy-spaces.lua
   ```

4. Remove the plugin, the command and what it remembers. Take out any `space status` hooks you added to your agents too.

   ```sh
   omarchy plugin remove com.t4t5.spaces --yes
   rm ~/.local/bin/space
   rm -rf ~/.local/state/omarchy-spaces ~/.config/omarchy-spaces ~/.cache/omarchy-spaces
   ```

   If you built it from source, run `cargo uninstall --root ~/.local omarchy-spaces` in place of `rm ~/.local/bin/space`.

Spaces opens plain Omarchy terminals (`xdg-terminal-exec`) by default, except that each Ghostty window gets a process of its own: from one process, Hyprland couldn't open it on the tab it's for. A `command` of your own under `[terminal]` has to do that too, so `foot` rather than `footclient`, and `kitty` without `-1`. To get agent status dots and sounds in any native terminal, wire up your agent's hooks to `space status` (below). If you prefer [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`), install it on your `PATH` and Spaces uses it automatically (set `herdr_core = false` under `[terminal]` in `config.toml` to keep plain terminals even when `hc` is installed).

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

### The logo

With the tabs first in the left section of a bar along the top or bottom of the screen, the sidebar's header is on the bar, over the rail: the Omarchy logo expands or collapses the sidebar, and when the sidebar expands, its title shows beside the logo with a toggle at its right edge. A right-click on the logo opens the Omarchy menu.

Anywhere else, the sidebar has its header, with the toggle, at its own top. So to keep Omarchy's menu on the bar, put it before the tabs:

```sh
omarchy plugin enable omarchy.menu --section left --index 0
```

## Keys

Spaces binds no keys, so it never takes one of Omarchy's or yours. Bind the ones you want in `~/.config/hypr/bindings.lua`, after the `require` line from [Install](#install). These are the ones you'll most likely want:

```lua
local spaces = require("hypr.omarchy-spaces")

o.rebind("SUPER + B", "Show or hide spaces", spaces.toggle_sidebar())
o.rebind("SUPER + P", "Find a space", spaces.find_space())
o.rebind("SUPER + N", "New space", spaces.new_space())
o.rebind("SUPER + SHIFT + J", "Next space", spaces.next_space())
o.rebind("SUPER + SHIFT + K", "Previous space", spaces.prev_space())
o.rebind("SUPER + T", "New tab", spaces.new_tab())
for n = 1, 10 do
  local key = "code:" .. tostring(n + 9)
  o.rebind("SUPER + " .. key, "Go to tab " .. n, spaces.go_to_tab(n))
  o.rebind("SUPER + SHIFT + " .. key, "Move window to tab " .. n, spaces.move_window_to_tab(n))
end
```

`o.rebind` replaces whatever Omarchy binds to that key, so pick others if you use those; `omarchy menu keybindings --print` lists what's taken. The number keys do exactly what Omarchy's workspace keys do while you're in `home`. Everything else you can bind is below. Bind these rather than `space` commands: they stay the same if the commands change.

| Action | Does |
|---|---|
| `spaces.toggle_sidebar()` | Expand or collapse the spaces sidebar |
| `spaces.next_space()` / `spaces.prev_space()` | Next / previous space, down and up the sidebar. They wrap around, with `home` at the top |
| `spaces.new_space()` | New space, with a terminal in the folder you are in |
| `spaces.send_window("next")` / `spaces.send_window("prev")` | Send the window to the next / previous space, or to one by name, and go with it; with `false` after it, staying where you are |
| `spaces.rename_tab()` | Name the tab you are on: type over its name on the bar, Enter to keep it, Escape to stop. An empty name has it called after its window again |
| `spaces.send_tab()` | Move the tab you are on to another space, and go with it: type to filter the spaces, Enter to pick, Escape to stop. It starts on the space you left last |
| `spaces.send_tab("next")` / `spaces.send_tab("prev")` | Move the tab you are on to the next / previous space, or to one by name, and go with it |
| `spaces.rename_space()` | Rename the space you are in: type over its name in the sidebar, Enter to keep it, Escape to stop. An empty name has it called after its folder again |
| `spaces.find_space()` | Find a space: type to filter, Enter to switch, Escape to stop. Expands the sidebar for the search if it's collapsed |
| `spaces.terminal()` | Terminal in the folder you are in; on an empty tab, the space's folder. If that fails, Omarchy's own |
| `spaces.new_tab()` | New tab, with a terminal in the space's folder |
| `spaces.go_to_tab(n)` | Go to tab N |
| `spaces.next_tab()` / `spaces.prev_tab()` | Next / previous tab |
| `spaces.move_window_to_tab(n)` | Move the window to tab N; with `false`, staying where you are |

In `home`, `go_to_tab` and `move_window_to_tab` behave exactly as Omarchy's workspace keys, so a new tab there opens empty.

## CLI

```sh
# Spaces
space new [dir]              # open a space with a terminal, in the folder you are in or dir
space go [space]             # go to a space where you left it
space next / space prev      # the space below or above the active one, wrapping around
space rename [space] <name>  # name a space, the active one by default; "" for its folder's name
space icon [space] <icon>    # a glyph or emoji for a space's badge, the active one by default; "" for its letters
space reorder <space> <n>    # move a space to position n in the sidebar, 1 being the first below home
space close [space]          # close a space's windows, the active one by default

# Tabs
space tab new                # open a tab in the active space, with a terminal in its folder
space tab go <n>             # go to tab n of the active space on this monitor, or to a tab as s1:2
space tab next / space tab prev  # its next or previous tab, wrapping around
space tab rename [tab] <name>  # name a tab, the one you are on by default; "" for its window's name
space tab reorder <tab> <n>  # move a tab to position n among its space's tabs
space tab close [tab]        # close a tab's windows, the one you are on by default, and leave it if you're on it
space tab send [tab] <space|next|prev>  # a tab, the one you are on by default, to another space

# Windows
space window send <space|next|prev> [--silent]  # the window to another space, on the tab you left it on
space window tab <n> [--silent]  # the window to tab n of the active space on this monitor

# The rest
space ls [--json]            # list spaces with window counts; --json as { version, spaces }
space restore                # open the spaces from before the last restart again
space restore <file>         # or those of an earlier session
space terminal               # a terminal in the folder you are in, or the space's
space status <working|waiting|done|idle> [--window <addr>] [--pid <pid>]  # report agent status for this terminal
space watch [--sounds]       # print the monitors, their tabs and the spaces, as a line of JSON each time they change; --sounds also rings for agents
```

A space can be given by what it's called or by its id (`s1`, as `space ls` shows), and a tab by its space and what its workspace is numbered (`s1:2`, `work:2`). Each monitor's are numbered as Omarchy numbers workspaces, 1-10 on the first and 11-20 on the next, each shown as 1-10: the second monitor's tab 2 is `s1:12`. In what `space` prints as JSON, a `tab` or a `key` is what its workspace is numbered, 12, and a `slot` what it shows as, 2. `space tab go 2` goes to tab 2 of the monitor you are on. `space close` asks each app to close, as `super+w` does, so an app can still stop to save.

After you log out or restart, the plugin opens the spaces that were up again by itself (it runs `space restore`, unless `[restore]` in the config sets `enabled = false`), `home` included: their names, icons and tab names, and their terminals, each in its folder and arranged as they were (splits and their sizes, groups, floating and fullscreen windows, monitors). A terminal that was running Claude Code picks its conversation up again (`claude --resume`), unless `[restore]` sets `agents = false`; with a terminal `command` of your own, that takes one that runs `space shell`. Chromium's windows and web apps come back among them with their tabs, unless `[restore]` sets `browser = false`: the tab you were on comes first, and the tabs load anew, without their history, groups or pins. Incognito windows don't, nor do windows of a browser on another data folder. If Chromium is set to continue where you left off, that's left to it. It then goes back to the space you were in, unless you have started on something else. Other programs in the terminals, what they had printed, and other apps' windows don't come back; the windows beside one take its room. Splits are restored in Hyprland's dwindle layout; in others, windows come back in the order they were opened.

`space watch` saves the session as it changes, in `session.json` beside `spaces.json`. What's new is saved within seconds, but what's gone only once it has been gone for ten seconds, and never while the computer shuts down, as that closes everything too. After a restart it waits in `session-previous.json` until it has been restored. The last 48 sessions are kept in `sessions/`, the one before each restart and at most one every 15 minutes besides: `space restore <file>` opens one of those. They have the addresses of the pages you had open, so only you can read them.

When `herdr-core` (`hc`) is installed and enabled (`herdr_core` under `[terminal]` unset or `true`), terminals Spaces opens run `space shell`: a shell in herdr-core, in the terminal's folder and with its environment, labelled with the window's process so Spaces can tell which window shows it, and attached to the window. The window is the terminal: closing it, or the shell exiting, ends both, and `ctrl+\` (`hc attach`'s detach) closes it too. Inside, `TERM` is core's (`xterm-256color`), not your terminal app's. Without `hc` (or with `herdr_core = false`), Spaces opens your native terminal directly with no wrapper.

## How it fits together

- `src/` is the `space` CLI, in Rust. It keeps no state about windows: it asks Hyprland over its socket, then dispatches. Terminal folders and foreground programs come from their shells in `/proc`, as Omarchy's `omarchy-cmd-terminal-cwd` does (or from herdr-core over its socket in `core.rs` for terminals running in it). Native agent statuses from `space status` live as per-window files under `$XDG_RUNTIME_DIR/omarchy-spaces/status/` and are pruned automatically as windows close or agents exit.
- `omarchy/` is the Omarchy shell plugin (QML): the tabs as a bar widget (`TabsWidget.qml`), and the sidebar, which `Base.qml` draws under the windows. Their logic is in `Sidebar.qml`, `SidebarList.qml` and `Spaces.mjs`. Both draw what one `space watch` says (`src/view.rs`), which the plugin's service runs (`Watch.qml`) and restarts when it ends, as it does when Hyprland restarts or `space` is reinstalled. Each line is all of it as of one moment: where you are, each monitor's tabs and the spaces. The plugin binds to the last one, and neither copies it nor asks Hyprland where you are as well, as the two would come apart. It reads Hyprland again, at once when you go somewhere, when Hyprland's events say something changed, and every two seconds, as nothing says when a window is resized. It follows herdr-core's events for its terminals. A shell outside core changing folder or starting a program sends no event, so it reads the active space's other terminals' shells five times a second, the rest every two seconds, and all of them when a window's title changes, as terminals retitle themselves on `cd` and as a command starts. A terminal's program is the leader of the foreground process group on its shell's tty. Windows of one process (Ghostty's that you open yourself, `foot --server`'s) are told apart by the order they and their shells were opened in, as long as each has one shell; with tabs or splits in them, some show the newest shell's folder. What Chromium's windows hold it reads from the session files Chromium keeps open in its profile, matching them to Hyprland's windows by title, or the order they were opened in.
- `hypr/omarchy-spaces.lua` holds the keybind helpers (tab actions and plugin calls), the hook that takes the other monitors along when you go to another space, the hook that moves on from a tab emptied by closing its last window (`space emptied`), and the blur behind a glassy sidebar and bar.
- `~/.local/state/omarchy-spaces/spaces.json` holds what Hyprland can't about each live space: the name and icon you gave it, the terminal it follows and that terminal's last folder. It and the saved sessions say their `version`; one that can't be read, or that a newer `space` made, is put aside as `<name>.broken-<time>` beside it rather than saved over. `~/.config/omarchy-spaces/config.toml` can set `picker`, for `space go` with no space named, `[terminal]`: `command`, which opens one (default: Omarchy's, `xdg-terminal-exec`), and `herdr_core` (`true`/`false`, default: auto when herdr-core's `hc` is on `PATH`), `[restore]`: `enabled`, `agents` and `browser` (all `true` by default), and `[sounds]`: `enabled`, `done` and `waiting`. A setting it doesn't know gets a warning, and the others still count.

## Development

The icon picker's glyphs, `omarchy/glyphs.json`, come from Nerd Fonts' list of glyph names; `just glyphs` makes it again, for a newer Nerd Fonts. Its emojis are Omarchy's own emoji picker's.

```sh
cargo test
node --test omarchy/tests/*.test.mjs
lua hypr/tests/keys.lua
just reload             # after changing the plugin; the shell doesn't reload it by itself
just stress             # goes between your spaces quickly, and checks that what shows keeps up
```

The justfile's `install` and `setup` do Install's steps for a clone you work in, building into `~/.cargo/bin` and adding the `require` line for you. The plugin and `space` go together: after changing what `space watch` prints, do both `just install` and `just reload`, and raise `VERSION` in `src/view.rs` and `omarchy/Spaces.mjs` if the one can't read the other.

`space` and the plugin share one version, in `Cargo.toml` and `manifest.json`; the tests fail if they differ. CI (`.github/workflows/ci.yml`) runs on pull requests and on `main`: the tests above, `cargo fmt --check`, `cargo clippy` with warnings as errors, a syntax check of the Lua, and a check that there are no symlinks, which `omarchy plugin add` refuses.

### Releasing

`omarchy plugin add` and `update` take whatever is on `main`, and `install.sh` takes the release of the version in its `manifest.json`. So `main` has to go with the latest release: keep work that changes both the plugin and `space` on a branch until it's released. To release, raise the version in `Cargo.toml`, `Cargo.lock` and `manifest.json`, commit, and push `main` together with the version's tag, `v0.2.0`. The tag has `.github/workflows/release.yml` run CI, build `space` for x86-64 and ARM64 and make the GitHub release. Until it's done, a few minutes later, `install.sh` says there's no such release and changes nothing.

## License

[MIT](LICENSE), except the sounds in `assets/sounds/`, which are Herdr's, under the [Apache License 2.0](assets/sounds/LICENSE).
