# renSpace

Spaces for Hyprland on Omarchy: switch projects, not workspaces.

A **space** is a project: a name, a folder, and its own Hyprland workspaces `name:1` to `name:10`. You switch spaces with `space go`, and `super+1..9` moves between the workspaces of the space you're in. The plain numbered workspaces are the `home` space, so nothing changes until you create a space.

This is the proof of concept from the renSpace plan (`~/Projects/ren/products/renspace/RENSPACE_PLAN.md`). It answers one question: does switching between spaces feel better than switching between workspaces?

## Install

Build and install the `space` binary into `~/.cargo/bin`, which is on Hyprland's `PATH`:

```sh
cargo install --path ~/dev/ren/renspace
```

Link the keybinds into your Hyprland config and load them after Omarchy's defaults:

```sh
ln -s ~/dev/ren/renspace/hypr/renspace.lua ~/.config/hypr/renspace.lua
echo 'require("hypr.renspace")' >> ~/.config/hypr/bindings.lua
```

Hyprland reloads the config by itself. To undo it, delete that line from `bindings.lua`.

| Keys | Does |
|---|---|
| `super+1..0` | Workspace 1–10 of the active space (`space ws N`) |
| `super+shift+1..0` | Move the focused window there and follow it (`space move N`) |
| `super+shift+alt+1..0` | Move it there silently (`space move --silent N`) |
| `super+alt+p` | Switch space with a picker (`space go`) |

In `home` the workspace keys behave exactly as Omarchy's, because `space` hands them to Omarchy's own `o.workspaces.focus` and `o.workspaces.move_window`.

`super+p` (the plan's placeholder) is Omarchy's pseudo window and `super+shift+p` its Google Photos web app, so the picker is on `super+alt+p`.

## Use

```sh
cd ~/Projects/ren && space new      # this folder is now the space 'ren'; go to ren:1
space new tax-2026 ~/Spaces/tax     # name and folder (created if missing)
space go ren                        # back to where you left ren
space go home                       # back to the plain workspaces
space go                            # pick one
space ls                            # list spaces, active one marked, window counts
space ls --json                     # the same for scripts
space ws 3                          # workspace 3 of the active space
space move 2 [--silent]             # focused window to workspace 2 of the active space
```

```
$ space ls
  home       4 windows
* ren        3 windows    ~/Projects/ren
  tax-2026   0 windows    ~/Spaces/tax
```

`space go rn` fails with `no space called 'rn'. Did you mean ren?` instead of creating a space. The picker lists the space you left most recently first and the current space last, so `super+alt+p` then Enter goes back to the previous space.

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

**Fast keypresses.** `space` talks to Hyprland's IPC socket directly rather than shelling out to `hyprctl`. Hyprland 0.56 takes Lua for `dispatch`, so for example `space ws 2` in `ren` sends `dispatch hl.dsp.focus({ workspace = "name:ren:2" })`. On the target laptop `space ws` takes about 0.5 ms from spawn to exit, against 3 ms for a single `hyprctl -j activeworkspace`. The workspace keys don't read config or scan roots. Scanning `~/Projects` for `space ls` takes about 2 ms.

The code is small: `name.rs` (names, the `name:n` format, typo suggestions), `hypr.rs` (socket client), `spaces.rs` (`.renspace.toml`, scanning, index), `state.rs` (`last`), `commands.rs` (the five commands), `main.rs` (CLI).

## Spike findings (step 0)

Checked on Hyprland 0.56.2 and the Omarchy of 2026-11-23:

- **Named workspaces work** and get negative ids (`renspike:1` was `-1337`). An empty one disappears when you leave it, like a numbered one.
- **Omarchy's bar hides them.** `Workspaces.qml` skips ids `<= 0`, so inside a space the bar still shows home's numbers with none highlighted. It's not broken, just not informative. A bar integration is in the plan's later steps.
- **Omarchy's per-monitor logic leaves named workspaces' windows alone.** `monitor-workspaces.lua` only renumbers or moves "regular" workspaces (`id > 0`). The numeric fallback in the plan (space k owns `k*100+1..k*100+10`) would be worse: Omarchy treats those ids as guests from an unplugged monitor and renumbers them into 1–10. Named workspaces are the right choice.
- **But a config reload drops you out of your space.** Verified: on `renspike:2`, `hyprctl reload` landed on `1`. Omarchy's `config.reloaded` handler puts every monitor back on one of its numbered workspaces. Reloads happen whenever a Hypr config file is saved or the theme changes. `hypr/renspace.lua` works around it by noting the workspace while the config is re-read and going back to it after Omarchy's handler. That workaround is not yet verified live; see below.
- **The binds override Omarchy's.** `o.rebind` (unbind then bind) on the same keys, loaded from `bindings.lua` after Omarchy's defaults.

## Testing

```sh
cargo test
```

The unit tests cover names, parsing, scanning, the index and the state files. `tests/cli.rs` runs the real `space` binary against a fake Hyprland that speaks the socket protocol, with its own home, config and state directories. It covers every command, the Omarchy fallback outside Omarchy, the picker ordering, and running without Hyprland.

## Known gaps

- `super+tab` and `super+mouse_wheel` cycle through every workspace on the monitor (`m+1`), so they leave the space.
- Windows opened while the wrong workspace is focused land in the wrong space. `space move` fixes them by hand.
- Anything that renames a workspace breaks the mapping. Keep the `name:n` format strict: anything else counts as `home`.
- The bar doesn't show a space's workspaces (see above).
- Not yet tried live: the reload workaround in `hypr/renspace.lua`, and the vicinae picker, which needs a person to pick something.
- One monitor. A second monitor's numbered workspaces count as `home` too, but a space's workspaces don't follow monitors.

## What to note while living in it

- Whether you reach for `space go` or fall back to plain workspaces.
- How many spaces you keep, and whether `home` stays empty or fills up.
- Which windows end up in the wrong space, and why.
- What you miss most: agent status, reopening a space's apps, browser profiles, a visible switcher. That decides what gets built next.
