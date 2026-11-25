# Omarchy Spaces

A plugin that organizes [Omarchy](https://omarchy.org) by project.

<p align="center">
  <img src="assets/screenshot.png" width="627" alt="A sidebar listing your spaces, beside the tabs of the open one">
</p>

- Every project gets its own **space**.
- Every space has its own **tabs**.
- Every tab is a regular workspace.
- Agent status in the sidebar, like [Herdr](https://herdr.dev).
- Spaces are restored after a restart.

## Install

You need Omarchy 4.5 or newer.

**1. Add the plugin.** Press Enter when it asks where to put it, then say yes to the notification that asks to replace the menu and workspaces on your bar.

```sh
omarchy plugin add https://github.com/t4t5/omarchy-spaces.git --enable
```

**2. Add the keys** to the end of `~/.config/hypr/bindings.lua`:

```lua
if package.searchpath("hypr.omarchy-spaces", package.path) then
  local spaces = require("hypr.omarchy-spaces")

  spaces.bind_tab_keys()
  o.rebind("SUPER + B", "Show or hide spaces", spaces.toggle_sidebar())
end
```

There are [more keys you can add](docs/guide/keys.md).

## Use

- <kbd>Super</kbd> <kbd>B</kbd> shows your spaces. Click **New space** to open one in the folder you're in.
- <kbd>Super</kbd> <kbd>1</kbd>–<kbd>0</kbd> goes to a tab. Click **+** to open a new one.
- Right-click a space or a tab to rename or close it.

Your regular workspaces are still there, in the `home` space.

For everything else, check out the [docs](docs/README.md).

## Uninstall

```sh
omarchy plugin remove com.t4t5.spaces
```

Your windows, bar and keys go back to how they were.

## Credits

Inspired by [Herdr](https://herdr.dev) and [Rex](https://superlogical.com). The [sounds](assets/sounds) are Herdr's.
