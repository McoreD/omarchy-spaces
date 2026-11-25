# Development

The [justfile](../../justfile) has the commands for a clone you work in:

```sh
just install   # build `space` into ~/.cargo/bin
just setup     # link and enable the plugin, and load the Hyprland part
just reload    # after changing the plugin; the shell doesn't reload it by itself
just stress    # go between your spaces quickly, and check that what shows keeps up
just glyphs    # make the icon picker's glyphs again, for a newer Nerd Fonts
```

`just install` and `just setup` do the [README's install steps](../../README.md#install), adding the `require` line to your bindings for you and replacing Omarchy's menu and workspaces on the bar without asking. The plugin's own setup (`install.sh --setup`) leaves a linked clone alone.

## Tests

```sh
cargo test
node --test omarchy/tests/*.test.mjs
lua hypr/tests/keys.lua
```

CI (`.github/workflows/ci.yml`) runs on pull requests and on `main`: the tests above, `cargo fmt --check`, `cargo clippy` with warnings as errors, a syntax check of the Lua, and a check that there are no symlinks, which `omarchy plugin add` refuses.

`hypr/tests/keys.lua` also checks that [Keys](../guide/keys.md) has every action you can bind.

## Changing what the parts say to each other

The plugin and `space` go together. After changing what `space watch` prints, do both `just install` and `just reload`, and raise `VERSION` in `src/view.rs` and `omarchy/Spaces.mjs` if the one can't read the other.

What `space ls --json` prints is apart from it, in `src/list.rs`, with a `VERSION` of its own. Scripts read that one, so add to it rather than change it.

## Icons

The icon picker's glyphs, `omarchy/glyphs.json`, come from Nerd Fonts' list of glyph names. Its emojis are Omarchy's own emoji picker's.
