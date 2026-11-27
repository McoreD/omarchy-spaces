# Configuration

Spaces works without a config. To change something, make `~/.config/omarchy-spaces/config.toml`, with only the settings you want. These are all of them, as they are by default:

```toml
[restore]
auto = true       # open your spaces again after you log out or restart; `space restore` always does
agents = true     # with the agents' conversations that were in their terminals
browsers = true   # with your browser's windows; false also stops saving the pages you have open

[new_space]
open = ["terminal"]  # what a new space opens, side by side on its first tab
# open = ["files", "claude"]
# open = []          # nothing: an empty space

[sounds]
enabled = false   # play a sound when an agent starts waiting for you, or finishes while you aren't looking
# done = "~/ding.oga"      # your own sounds, in place of the ones it comes with;
# waiting = "~/dong.oga"   # a relative path is from the config's folder
```

Changes count at once. A setting Spaces doesn't know gets a warning, and the others still count.

See [Restoring your spaces](restore.md) and [Agent status](agent-status.md) for what these do.

## What a new space opens

`[new_space] open` lists what `space new` (and the sidebar's `+`) opens in the new space's folder, all on its first tab, tiled side by side in the order you give:

- `terminal`: Omarchy's terminal, as it is by default.
- `files`: Omarchy's file manager (Folio), at the folder.
- an Omarchy agent's id (`claude`, `codex`, `agy`, `opencode`, …; `omarchy default agent --help` lists them): the agent in a terminal of its own, in the folder. Spaces runs `omarchy-agent --inline --agent <id>` in that terminal, so the agent starts as Omarchy starts it, including the flag that has it skip asking before it acts (`--permission-mode auto` for Claude Code, `--approve-for-me` for Codex, `--dangerously-skip-permissions` for Antigravity). Claude Code, Codex and Antigravity show their [status](agent-status.md) and come back with [restore](restore.md) as any terminal's does. An agent started in your home folder starts in `~/Work` instead, if you have one: Omarchy does that, as agents won't remember trusting `$HOME`.
- nothing, `open = []`: an empty space, see [Spaces and tabs](spaces-and-tabs.md#spaces).

The setting is for a space you open yourself: the `+`, a key bound to `spaces.new_space()`. A script's `space new --silent`, or `space new -- <cmd>`, opens just a terminal, as it always has, unless it says `--open`. A value Spaces doesn't know gets a warning, and is skipped, as is an agent or Files that isn't installed; if that leaves nothing of a list that had something in it, a terminal opens instead. Only `open = []` opens nothing. Pick what opens from the sidebar's [settings panel](sidebar.md#what-a-new-space-opens), or with `space config set new_space.open files,claude`; either changes only that setting and leaves the rest of the file, comments included, as it is. `space new --open` [overrides it for one space](cli.md#spaces).

## Terminal

Spaces opens the terminal you chose in Omarchy (`omarchy default terminal`).

Each Ghostty window gets a process of its own: from one process, Hyprland couldn't open it on the tab it's for. Other terminals have to open a process per window too, so use `foot` rather than `footclient`.
