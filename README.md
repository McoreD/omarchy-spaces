# Omarchy Spaces

<img src="assets/screenshot.png" width="627" alt="A sidebar listing your spaces, beside the tabs of the open one">

Inspired by multiplexer apps like [Herdr](https://herdr.dev) and [Rex](https://superlogical.com), but for your whole desktop:

- Every project gets its own **space**
- Every space has its own **tabs** (Hyprland workspaces)
- Keeps track of agent status across projects
- Spaces are restored after a reboot

## Install

You need Omarchy 4.5 or newer.

**1. Add the plugin.** Choose "Left" (the default) when it asks where to put it. This replaces the built-in `omarchy.menu` and `omarchy.workspaces` components with `com.t4t5.spaces`.

```sh
omarchy plugin add https://github.com/t4t5/omarchy-spaces.git --enable
```

As it first starts, the plugin downloads the `space` command it runs on into `~/.local/bin`, from this repository's GitHub release. [What gets installed](docs/guide/install.md#what-gets-installed) lists everything it sets up.

**2. Add the basic key bindings** to your `~/.config/hypr/bindings.lua` file (at the end):

```lua
if package.searchpath("hypr.omarchy-spaces", package.path) then
  local spaces = require("hypr.omarchy-spaces")

  spaces.bind_tab_keys()
  o.rebind("SUPER + B", "Show or hide spaces", spaces.toggle_sidebar())
end
```

For advanced workflows, check out [all the key bindings you can add](docs/guide/keys.md).

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
