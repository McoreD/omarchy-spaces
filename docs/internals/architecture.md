# Architecture

Spaces has three parts: a command in Rust, an Omarchy shell plugin in QML, and a piece of Hyprland config in Lua.

## `src/`: the `space` command

It keeps no state about windows: it asks Hyprland over its socket, then dispatches.

- **Terminals**: their folders and foreground programs come from their shells in `/proc`, as Omarchy's `omarchy-cmd-terminal-cwd` does, plus directly launched agent sessions (like `omarchy agent`). A shell terminal's program is the leader of the foreground process group on its shell's tty.
- **Agents**: what an agent is doing, and the conversation it's in, come from its adapter in `src/agent/`, which reads what the agent itself keeps on disk, so none needs hooks.
- **Browsers**: what browser windows hold comes from an adapter in `src/browser/`. Chromium's reads the session files that it and those made from it keep open in their profile, matching them to Hyprland's windows by title, or the order they were opened in.

Nothing else knows of any one agent or browser, so another is a new file there.

Windows of one process (Ghostty's that you open yourself, `foot --server`'s) are told apart by the order they and their shells were opened in, as long as each has one shell. With tabs or splits in them, some show the newest shell's folder.

## `omarchy/`: the shell plugin

The tabs are a bar widget (`TabsWidget.qml`), and `Base.qml` draws the sidebar under the windows. Their logic is in `Controller.qml`, `Sidebar.qml`, `SidebarList.qml` and `Spaces.mjs`.

Both draw what one `space watch` says (`src/view.rs`). The plugin's service runs it (`Watch.qml`) and restarts it when it ends, as it does when Hyprland restarts or `space` is reinstalled. Each line is all of it as of one moment: where you are, each monitor's tabs and the spaces. The plugin binds to the last one, and neither copies it nor asks Hyprland where you are as well, as the two would come apart.

The same watch serves `space events` (`src/events.rs`), on `$XDG_RUNTIME_DIR/omarchy-spaces/events.sock`. It sends each subscriber a snapshot when it joins, then the changes between one moment and the next. It's the only one that follows which agents are done and seen, so the events and the sidebar can't disagree. A subscriber that falls far behind is dropped rather than holding the watch up. The watch that serves holds `.events.lock` beside the socket. Another one, run beside the plugin's or started by the plugin before the last has ended, tries for the lock every two seconds and takes over once it gets it.

### When `space watch` reads again

| What | When |
|---|---|
| Hyprland | At once when you go somewhere, when Hyprland's events say something changed, and every two seconds, as nothing says when a window is resized |
| The active space's terminals' shells | Five times a second, as a shell changing folder or starting a program sends no event |
| The other spaces' shells | Every two seconds |
| All shells | When a window's title changes, as terminals retitle themselves on `cd` and as a command starts |
| Windows' titles, for the search | With Hyprland's state, so a new title shows within two seconds; the line is printed again only if it differs |

## `hypr/omarchy-spaces.lua`: the Hyprland part

It holds:

- the keybind helpers (tab actions and plugin calls),
- the hook that takes the other monitors along when you go to another space,
- the hook that moves on from a tab emptied by closing its last window (`space emptied`),
- the hook that numbers each space's tabs 1, 2, 3, … again as a workspace comes or goes (`space renumber`); the commands that can end a tab hold `.tabs.lock` while they run, so it waits for them. With `tabs.style = "omarchy"` it only forgets the names of tabs that ended (see below),
- the blur behind a glassy sidebar and bar.

A tab is found by its name, as the bar shows it, never by a `name:` selector made from it. Hyprland (0.57) gives a workspace the name it was made with as its address, which `name:` matches, and a rename (`space renumber`) changes the name only, so after one `name:s1:1` can mean another tab, or none. `src/hypr.rs`'s `selector_for` and the Lua part's `target` look the name up and aim at the workspace that has it. A new tab never takes a number that is a renamed tab's address, for the same reason: `space renumber` numbers it as the next tab.

## Tab styles

`tabs.style` (`browser` or `omarchy`) counts at once, and every part goes by one value, so that none can disagree about it:

- **Rust** (`config::tab_style()`) is the one that decides. It reads `[tabs] style` from `config.toml` as plain TOML, again only when the file's modified time or size changes (the tabs ask many times a second in `space watch`), so another setting that is wrong doesn't matter. A file that can't be read as TOML, as it may be halfway through an edit, leaves the style as it last was (here, else what was last noted, else `browser`). It then notes the style, if it changed, in `~/.local/state/omarchy-spaces/tab-style` (`paths::tab_style_file()`). `space watch` calls it as it starts and when the config changes, and `space config set` and `unset` after they write. After a hand edit with no `space watch` running, the note catches up on the next `space` run (the workspace hook's `space renumber` is one, soon after), and going back to `browser` closes the gaps through that renumber. The style rides on `monitors::Blocks` (`Blocks::style`), which every place that works out a space's tabs already has, so `Strip` knows it: with `omarchy`, `Strip::reach` is exact, as it is in `home`, and `Strip::new_tab` is the lowest free slot.
- **The watch line** says it per monitor, as `tab_style` (an additive field, so no `VERSION` bump), which `omarchy/Spaces.mjs` reads as `monitor.style`: `tabCells` draws 1-5 always with it, and the tenth as `0`. When the style changes, `space watch` reads Hyprland again at once, and, if it went from `omarchy` to `browser`, runs `space renumber` once to close the gaps (`space config set` does so too).
- **Lua** reads that note (`omarchy_style()` in `hypr/omarchy-spaces.lua`), not the config: `go_to_tab` and `move_window_to_tab` are plain Lua so they can't leave you stuck, and a call to `space` couldn't answer in time. Without the note it's `browser`.

With `omarchy`, `renumber_tabs` renames nothing and only forgets the names of tabs that have ended (so a name doesn't come back on the next tab with that number); it runs on every workspace event for that. Nothing else renames a tab either: `send_tab` and `reorder_tab` move the windows to the workspace of the other number (`move_tab`, `reorder_by_moving`, the way `home`'s tabs are moved) instead of renaming the workspace. A renamed workspace keeps the address it was made with, which `name:` still matches, so with tabs keeping their numbers a number would stay unreachable (or land on the tab after it) as long as that tab lived. Tabs that a browser's numbering renamed before the style changed are the one thing left that is addressed by another number, and nothing puts them right: moving the user's windows while Hyprland drops and makes workspaces and the Lua hooks answer was judged riskier than the limitation. They stop blocking when those tabs end, and a restart clears them, as `restore` opens each tab by `name:` on a Hyprland that has no workspaces yet, so a new one is named and addressed alike. Meanwhile `tab reorder` refuses, before it moves anything, in a space that has one (`is_mismatched`), and `reorder_by_moving` otherwise finds each target as the workspaces are now, focuses the tab's own monitor first so that a workspace made meanwhile is made there, and sets `cursor.no_warps` and restores it, with the focused monitor and window. `fresh_key` goes by the same rule for those: the number asked for goes to the tab that has it, and if it can't be made because of one's address, to the next number that is free, never to a tab that is there. They stop blocking as those tabs end.

## An empty space

A space is the workspaces and windows Hyprland has with its name (`src/live.rs`), so one with no window exists only while Hyprland keeps a workspace of it: that of a monitor that shows it. `space new` with nothing to open (`src/newspace.rs`) just goes to the new space's first tab, which Hyprland makes as you arrive, and drops the workspace when you leave it empty. Nothing else keeps it or ends it: its folder is in `spaces.json` from the start, which names it, and `space watch` prunes the entry once the workspace is gone, as for any space that ends. `space emptied` only runs as a window closes, and `space renumber` only closes gaps between a space's tabs, so neither touches it while you are on it. `switch` forgets where you left a space that ended so.

What a new space opens is read from the config as it opens (`[new_space]`), and Omarchy's agents from `omarchy-default-agent`'s own list (`src/newspace.rs`). `space config` and the Settings screen work from one list of settings, `SETTINGS` in `src/settings.rs`; `space config set` and `unset` edit the one key with `toml_edit`, so the rest of the file stays as it was.

## Adding a setting

A setting is one entry in `SETTINGS` (`src/settings.rs`) and the field in `Config` (`src/config.rs`) that it controls. The entry is its `key` (`section.name`, as in `config.toml`), `label`, one line of `help`, its `kind`, its `default`, and `get`, how to read it from `Config`:

- `Kind::Bool`: a switch.
- `Kind::Choice(&[Choice { value, label }, ...])`: one of a fixed list.
- `Kind::Multi { options, none, unknown }`: any of a list that a function gives (`newspace::options`), `none` having the empty list its own exclusive word.

A setting of the kind `Multi` can group its options: an `Opt` names its `group`, and the setting's `groups` (`Group { name, grid }`) say how the panel draws it. A group with `grid: true` is a grid of icons with short names under a subheading, the cursor moving through its cells; without, it is rows under the subheading. `new_space.open` groups its agents as a grid. Ungrouped options are rows.

A new section also needs:

- a line in `SECTIONS`, with its title;
- a field in `Config` (`src/config.rs`) of its own struct, with the settings' defaults, `#[serde(default)]`, and `#[serde(flatten)] unknown` like the others;
- a line in `Config::unknown()`, so a typo in that section warns.

A test checks that every entry's `default` is `Config`'s own, that its section and key are well formed, and that writing a value of each setting to an empty file reads back as that value (so a key that doesn't match its field fails).

That is all: `space config get`, `set`, `unset` and `schema` know it, with its value checked and written by the same `toml_edit` path as the others, and the Settings screen (`omarchy/SettingsPanel.qml`) draws it from `space config schema --json`, with no QML to write: the section title, then a tick box for a switch, radio rows for a choice, or tick rows (or a grid) for a list, one keyboard cursor through all of them, each change run as `space config set`. Only a setting of a new kind needs the panel taught (`Spaces.settingsItems`, `Spaces.toggled`, `Spaces.settingArg` and `Spaces.moveCursor` in `omarchy/Spaces.mjs`, and its row in `SettingsPanel.qml`). A setting whose value in the file is invalid still shows, with its default and a `problem` in the schema; `set` replaces the bad value. The plugin's QML must stay to syntax the shell's QML engine reads: no object rest or spread (`{ a, ...rest }`), which made the whole plugin go dark.

Paths and free strings aren't kinds yet, so `sounds.done` and `sounds.waiting` stay in the file.

## State

`~/.local/state/omarchy-spaces/spaces.json` holds what Hyprland can't about each live space: the name, icon and metadata you gave it, the terminal it follows and that terminal's last folder. The saved sessions are beside it (see [Restoring your spaces](../guide/restore.md#whats-saved)); `space watch` saves them as they change, and after a restart the plugin runs `space restore`.

All of these say their `version`. One that can't be read, or that a newer `space` made, is put aside as `<name>.broken-<time>` beside it rather than saved over.
