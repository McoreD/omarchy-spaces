# Spaces for Omarchy

<p align="center">
  <a href="#install">install</a> · <a href="#getting-started">getting started</a> · <a href="docs/README.md">docs</a>
</p>

<p align="center">
  <b>A set of tabs for each thing you work on.</b><br>
  Built for Hyprland on Omarchy.
</p>

<p align="center">
  <img src="assets/screenshot.png" width="627" alt="The Spaces sidebar, listing each space with a status dot on one of them, beside the tabs of the open space">
</p>

---

A **space** holds one piece of work. Each space has its own **tabs**, and each tab is a workspace for whatever windows you like. Switch spaces, and all of it changes at once.

## Features

- **A sidebar for your spaces** — Collapse it to a rail of badges, or expand it to see their names.
- **Tabs on the bar** — They take the place of Omarchy's workspaces, and name themselves after what's in them.
- **Agent status** — See at a glance when Claude Code is working, done or waiting for you.
- **Picks up where you left off** — After a restart, your terminals, Claude Code conversations and browser windows come back.
- **Multiple monitors** — They all show the same space, each with its own tabs.
- **Your keys** — Nothing is bound until you bind it.
- **Themes** — Follows your Omarchy theme and the look of your bar.

## Install

You need Omarchy 4.5 or newer.

**1. Add the plugin.** When it asks whether to enable it, say no: the next step does that.

```sh
omarchy plugin add https://github.com/t4t5/omarchy-spaces.git
```

**2. Install the `space` command and turn the plugin on.** Spaces replaces Omarchy's menu and workspaces on the bar, so this takes those off. Nothing is lost: the menu still opens with its key, or a right-click on the logo.

```sh
~/.config/omarchy/plugins/com.t4t5.spaces/install.sh &&
omarchy plugin enable com.t4t5.spaces --section left --index 0 &&
omarchy plugin disable omarchy.menu &&
omarchy plugin disable omarchy.workspaces &&
ln -sfn ~/.config/omarchy/plugins/com.t4t5.spaces/hypr/omarchy-spaces.lua ~/.config/hypr/omarchy-spaces.lua
```

**3. Choose your keys.** Add this at the end of `~/.config/hypr/bindings.lua`. The first line is required; change or drop any of the others. They replace whatever Omarchy has on those keys.

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

To update, uninstall or build from source, see [Installation](docs/guide/install.md).

## Getting started

With the keys above:

1. Press <kbd>Super</kbd> <kbd>N</kbd> to open a space. It starts with a terminal in the folder you're in, and takes that folder's name.
2. Press <kbd>Super</kbd> <kbd>T</kbd> to open another tab in it, and <kbd>Super</kbd> <kbd>1</kbd>–<kbd>0</kbd> to go between tabs.
3. Press <kbd>Super</kbd> <kbd>P</kbd> to find another space, or click one in the sidebar. Your plain workspaces are the `home` space, at the top.
4. Right-click a space or a tab to rename or close it, and drag to reorder.
5. Close a space's last window, and the space is gone.

A dot shows which spaces need you: **yellow** while an agent works, **red** when it's waiting for you, **green** when it's done.

## Docs

- [Spaces and tabs](docs/guide/spaces-and-tabs.md)
- [Keys](docs/guide/keys.md)
- [Configuration](docs/guide/configuration.md)
- [Command line](docs/guide/cli.md)

See [all the docs](docs/README.md) for the rest, including [how it works](docs/internals/architecture.md) and [developing Spaces](docs/internals/development.md).

## License

[MIT](LICENSE), except the sounds in `assets/sounds/`, which are Herdr's, under the [Apache License 2.0](assets/sounds/LICENSE).
