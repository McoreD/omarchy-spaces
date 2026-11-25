# Spaces for Omarchy

<p align="center">
  <img src="assets/screenshot.png" width="627" alt="The Spaces sidebar, listing each space with a status dot on one of them, beside the tabs of the open space">
</p>

A **space** holds one piece of work. Each space has its own **tabs**, and each tab is a workspace for whatever windows you like.

## Install

You need Omarchy 4.5 or newer.

**1. Add the plugin.** Press Enter when it asks where on the bar to put it.

```sh
omarchy plugin add https://github.com/t4t5/omarchy-spaces.git --enable
```

This also downloads the `space` command and links a file into your Hyprland config. A notification then asks whether Spaces should replace Omarchy's menu and workspaces on the bar. Nothing is lost if you say yes: the menu still opens with its key, or a right-click on the logo.

**2. Choose your keys.** Add this at the end of `~/.config/hypr/bindings.lua`. Keep the first two lines and the last, and change or drop any of the others. They replace whatever Omarchy has on those keys, until you remove Spaces.

```lua
if package.searchpath("hypr.omarchy-spaces", package.path) then
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
end
```

To update or build from source, see [Installation](docs/guide/install.md). To remove it, see [Uninstall](#uninstall).

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

## Uninstall

```sh
omarchy plugin remove com.t4t5.spaces
```

This moves your windows back to the plain workspaces, gives you back Omarchy's keys and bar, and removes the `space` command and your saved sessions. The keys you added to `bindings.lua` do nothing from then on, so you can delete them whenever you like.

## License

[MIT](LICENSE), except the sounds in `assets/sounds/`, which are Herdr's, under the [Apache License 2.0](assets/sounds/LICENSE).
