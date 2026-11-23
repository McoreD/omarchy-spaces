# renSpace

Spaces for Hyprland on Omarchy: switch projects, not workspaces.

A **space** is a project: a name, a folder, and its own Hyprland workspaces `name:1` to `name:10`. You switch spaces with `space go`, and `super+1..9` moves between the workspaces of the space you're in. The plain numbered workspaces are the `home` space, so nothing changes until you create a space.

This is the proof of concept from the renSpace plan (`~/Projects/ren/products/renspace/RENSPACE_PLAN.md`). It answers one question: does switching between spaces feel better than switching between workspaces?

## Install

```sh
just install   # build the space binary into ~/.cargo/bin, which is on Hyprland's PATH
just setup     # add the sidebar to the Omarchy shell and bind it to super+p
```

`just setup` links `omarchy/` into `~/.config/omarchy/plugins/` as `org.ren.renspace` and enables it (`omarchy plugin enable` asks you to confirm). It also links `hypr/renspace.lua` into `~/.config/hypr/` and loads it from `bindings.lua`. That adds one binding and changes none of Omarchy's. It's safe to run again.

Hyprland reloads the config by itself. To undo it, delete that line from `bindings.lua`. `super+p` replaces Omarchy's pseudo window toggle, which keeps a tiled window at its own size inside its tile.

### The sidebar

`super+p` opens a column on the left edge: `home` first, then every space by name, each with its folder and window count. The space you're in has a mark beside it. Rows keep their place from one opening to the next, so you can learn where each space is. The cursor starts on the space you left most recently, so `super+p` then Enter goes back to it.

Type to filter. A prefix match beats a match at a word start, which beats letters in order: `spc` finds `renspace`. Up/down, Tab or `ctrl+n`/`ctrl+p` move the cursor. Enter or a click jumps there. Esc clears the filter, then closes. A click outside also closes.

The sidebar only reads `space ls --json` and runs `space go <name>`, so the `space` binary stays the only thing that knows about spaces. `space go` with no name still opens the centered vicinae or walker picker from a terminal.

With only the picker, `super+1..0` are still Omarchy's, so pressing one inside a space takes you back to a plain workspace. Each space then lives on its first workspace, `name:1`.

### Optional: workspaces inside a space

To give each space workspaces of its own, also load `renspace-workspaces.lua`:

```sh
ln -s ~/dev/ren/renspace/hypr/renspace-workspaces.lua ~/.config/hypr/renspace-workspaces.lua
echo 'require("hypr.renspace-workspaces")' >> ~/.config/hypr/bindings.lua
```

| Keys | In a space | In `home` |
|---|---|---|
| `super+1..0` | Workspace N of the space | Omarchy's workspace N |
| `super+shift+1..0` | Move the window there and follow it | Omarchy's move |
| `super+shift+alt+1..0` | Move it there silently | Omarchy's silent move |

These keys are plain Lua running inside Hyprland, not calls to `space`, so they can't leave you stuck. Outside a space they call Omarchy's own functions, the ones Omarchy binds to the same keys, and they keep working even if `space` is missing or broken. Inside a space, `super+p` or `space go home` leads back to the plain workspaces. The `space ws` and `space move` commands do the same from a shell or a script.

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

- With the optional workspace keys, `super+tab` and `super+mouse_wheel` still cycle through every workspace on the monitor (`m+1`), so they leave the space.
- Windows opened while the wrong workspace is focused land in the wrong space. `space move` fixes them by hand.
- Anything that renames a workspace breaks the mapping. Keep the `name:n` format strict: anything else counts as `home`.
- The bar doesn't show a space's workspaces (see above).
- Not yet tried live: the sidebar in the running Omarchy shell (it passes qmllint, and its logic runs in both Node and Qt's JS engine), the reload workaround in `hypr/renspace.lua`, and the vicinae picker.
- One monitor. A second monitor's numbered workspaces count as `home` too, but a space's workspaces don't follow monitors.

## What to note while living in it

- Whether you reach for `space go` or fall back to plain workspaces.
- How many spaces you keep, and whether `home` stays empty or fills up.
- Which windows end up in the wrong space, and why.
- What you miss most: agent status, reopening a space's apps, browser profiles, a visible switcher. That decides what gets built next.
