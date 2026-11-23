# renSpace

Spaces for Hyprland on Omarchy: switch projects, not workspaces.

A **space** is a project: a name, a folder, and its own tabs. Each tab is a Hyprland workspace, `name:1` to `name:10`, holding whatever windows you tile there. A strip along the top of the screen shows the current space's tabs. You switch spaces with `super+p`, and switch tabs within a space as in a browser. The plain numbered workspaces are the `home` space, whose tabs work like Omarchy's workspaces.

This is the proof of concept from the renSpace plan (`~/Projects/ren/products/renspace/RENSPACE_PLAN.md`). It answers one question: does switching between spaces feel better than switching between workspaces?

## Install

```sh
just install   # build the space binary into ~/.cargo/bin, which is on Hyprland's PATH
just setup     # add the tab strip and sidebar to the Omarchy shell, and the keys
```

`just setup` links `omarchy/` into `~/.config/omarchy/plugins/` as `org.ren.renspace` and enables it (`omarchy plugin enable` asks you to confirm). It also links `hypr/renspace.lua` into `~/.config/hypr/` and loads it from `bindings.lua`. It's safe to run again. Hyprland reloads the config by itself; to undo the keys, delete that line from `bindings.lua`.

The strip takes the top edge, so the layout is meant for Omarchy's bar on the right: `omarchy bar position right`.

A file added to the plugin while the shell is running doesn't load until the shell restarts (`omarchy-restart-shell`). The shell reports it as "File name case mismatch". Edits to existing files reload by themselves.

| Keys | Does |
|---|---|
| `super+p` | Switch space (the sidebar) |
| `super+t` | New tab, after the last one |
| `super+1..0` | Go to tab N |
| `super+tab` / `super+shift+tab` | Next / previous tab |
| `super+shift+1..0` | Move the focused window to tab N and follow it |
| `super+shift+alt+1..0` | Move it there without following |

Closing a tab means closing its windows: Hyprland removes an empty workspace once you leave it.

These keys replace Omarchy's: `super+p` was the pseudo window toggle, `super+t` the floating toggle, and `super+tab` cycled every workspace on the monitor. `super+1..0` and `super+shift+1..0` are plain Lua inside Hyprland, not calls to `space`, so they can't leave you stuck. In `home` they run the very functions Omarchy binds to those keys, and they keep working even if `space` is missing or broken. `super+t` and `super+tab` run `space ws new|next|prev`.

### The tab strip

The strip shows the space's name, then a tab for each of its workspaces: its number, the app you used last on it, and `+N` for any other windows there. A new tab reads "New tab" until something opens on it. The current tab is underlined. Click a tab to go there, `+` for a new one, or the space's name to open the sidebar. The strip follows Hyprland live, and clicks run `space ws`.

### The sidebar

`super+p` opens a column on the left edge: `home` first, then every space by name, each with its folder and window count. The space you're in has a mark beside it. Rows keep their place from one opening to the next, so you can learn where each space is. The cursor starts on the space you left most recently, so `super+p` then Enter goes back to it.

Type to filter. A prefix match beats a match at a word start, which beats letters in order: `spc` finds `renspace`. Up/down, Tab or `ctrl+n`/`ctrl+p` move the cursor. Enter or a click jumps there. Esc clears the filter, then closes. A click outside also closes.

The sidebar only reads `space ls --json` and runs `space go <name>`, so the `space` binary stays the only thing that knows about spaces. `space go` with no name still opens the centered vicinae or walker picker from a terminal.

## Use

```sh
cd ~/Projects/ren && space new      # this folder is now the space 'ren'; go to ren:1
space new tax-2026 ~/Spaces/tax     # name and folder (created if missing)
space go ren                        # back to where you left ren
space go home                       # back to the plain workspaces
space go                            # pick one
space ls                            # list spaces, active one marked, window counts
space ls --json                     # the same for scripts
space ws 3                          # tab 3 of the active space
space ws new|next|prev              # a new tab after the last, or the next / previous one
space move 2 [--silent]             # focused window to tab 2 of the active space
```

```
$ space ls
  home       4 windows
* ren        3 windows    ~/Projects/ren
  tax-2026   0 windows    ~/Spaces/tax
```

`space go rn` fails with `no space called 'rn'. Did you mean ren?` instead of creating a space. The picker lists the space you left most recently first and the current space last, so Enter goes back to the previous space.

Space names are lowercase letters, digits and `-`. There's no `:`, since that separates the space from the workspace number.

## Files

The project folder is the source of truth. A folder is a space when it has a `.renspace.toml`, written by `space new` and editable by hand or by an agent working in the folder:

```toml
# ~/Projects/ren/.renspace.toml
name = "ren"   # optional, defaults to the folder name
```

Global config, all optional:

```toml
# ~/.config/renspace/config.toml
roots = ["~/Projects", "~/Spaces"]   # scanned for .renspace.toml (these are the defaults)
scan_depth = 3                        # levels below each root
picker = "vicinae dmenu"              # any dmenu-style command: lines in, chosen line out
```

Without a `picker`, `space go` uses `vicinae dmenu` when vicinae is installed and `walker --dmenu` otherwise. Walker 2 needs its `elephant` service running, which current Omarchy no longer starts.

Everything under `~/.local/state/renspace/` is disposable cache and can be deleted at any time:

- `index` lists known space folders: those found under `roots`, plus spaces created elsewhere. Entries for folders that moved or lost their `.renspace.toml` are pruned whenever it's read. `space go` rescans the roots when it doesn't know a name, so a `.renspace.toml` written by hand works straight away.
- `last` records each space's last-focused workspace and when you left it, so `space go` returns you to where you were.

## Design

**No daemon.** `space` keeps no state about windows. Every command asks Hyprland, then dispatches, so nothing can drift out of sync and there's nothing to recover after a crash.

**The workspace name is the data.** A window belongs to the space whose name prefixes its workspace (`ren:3`). The active space is read from the focused workspace on every command, never stored. New windows open on the focused workspace, so they land in the active space with no extra handling. A space whose folder is gone still shows in `space ls` and `space go` while its workspaces have windows.

**Fast keypresses.** `space` talks to Hyprland's IPC socket directly rather than shelling out to `hyprctl`. Hyprland 0.56 takes Lua for `dispatch`, so for example `space ws 2` in `ren` sends `dispatch hl.dsp.focus({ workspace = "name:ren:2" })`. On the target laptop `space ws` takes about 0.5 ms from spawn to exit, against 3 ms for a single `hyprctl -j activeworkspace`. `space ws` and `space move` don't read config or scan roots, and the optional workspace keys skip the binary altogether. Scanning `~/Projects` for `space ls` takes about 2 ms.

The code is small. The sidebar is `omarchy/Sidebar.qml` plus `omarchy/Spaces.mjs`. On the Rust side: `name.rs` (names, the `name:n` format, typo suggestions), `hypr.rs` (socket client), `spaces.rs` (`.renspace.toml`, scanning, index), `state.rs` (`last`), `commands.rs` (the five commands), `main.rs` (CLI).

## Spike findings (step 0)

Checked on Hyprland 0.56.2 and the Omarchy of 2026-11-23:

- **Named workspaces work** and get negative ids (`renspike:1` was `-1337`). An empty one disappears when you leave it, like a numbered one.
- **Omarchy's bar hides them.** `Workspaces.qml` skips ids `<= 0`, so inside a space the bar still shows home's numbers with none highlighted. It's not broken, just not informative. A bar integration is in the plan's later steps.
- **Omarchy's per-monitor logic leaves named workspaces' windows alone.** `monitor-workspaces.lua` only renumbers or moves "regular" workspaces (`id > 0`). The numeric fallback in the plan (space k owns `k*100+1..k*100+10`) would be worse: Omarchy treats those ids as guests from an unplugged monitor and renumbers them into 1–10. Named workspaces are the right choice.
- **But a config reload drops you out of your space.** Verified: on `renspike:2`, `hyprctl reload` landed on `1`. Omarchy's `config.reloaded` handler puts every monitor back on one of its numbered workspaces. Reloads happen whenever a Hypr config file is saved or the theme changes. `hypr/renspace.lua` works around it by noting the workspace while the config is re-read and going back to it after Omarchy's handler. That workaround is not yet verified live; see below.
- **The optional workspace keys override Omarchy's.** `o.rebind` (unbind then bind) on the same keys, loaded from `bindings.lua` after Omarchy's defaults. Tested through `hyprctl eval`: in `home`, `super+2` runs Omarchy's own focus; in a space, `super+3` goes to `name:3`; and a move with no window does nothing.

## Testing

```sh
cargo test
node --test omarchy/tests/*.test.mjs
```

The unit tests cover names, parsing, scanning, the index and the state files. `tests/cli.rs` runs the real `space` binary against a fake Hyprland that speaks the socket protocol, with its own home, config and state directories. It covers every command, the Omarchy fallback outside Omarchy, the picker ordering, and running without Hyprland. The sidebar's filtering and cursor logic lives in `omarchy/Spaces.mjs`, outside QML, so Node can test it.

## Known gaps

- `super+mouse_wheel` still cycles through every workspace on the monitor (`m+1`), so it leaves the space.
- Windows opened while the wrong workspace is focused land in the wrong space. `space move` fixes them by hand.
- Anything that renames a workspace breaks the mapping. Keep the `name:n` format strict: anything else counts as `home`.
- Omarchy's workspace widget in the bar doesn't show a space's workspaces (see above); the tab strip does. Drop the widget from the bar if it's redundant.
- Tabs have numbers and app names but no names of their own, and can't be reordered. The strip shows the focused monitor's tabs on every monitor.
- Not yet tried live: the reload workaround in `hypr/renspace.lua`, and the vicinae picker.
- One monitor. A second monitor's numbered workspaces count as `home` too, but a space's workspaces don't follow monitors.

## What to note while living in it

- Whether you reach for `space go` or fall back to plain workspaces.
- How many spaces you keep, and whether `home` stays empty or fills up.
- Which windows end up in the wrong space, and why.
- What you miss most: agent status, reopening a space's apps, browser profiles, a visible switcher. That decides what gets built next.
