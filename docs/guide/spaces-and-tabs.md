# Spaces and tabs

A **space** holds one piece of work. Each space has its own **tabs**, and each tab is a Hyprland workspace (`s1:1`, `s1:2`, …) holding whatever windows you tile there. The plain numbered workspaces are the `home` space.

## Spaces

**Opening.** Click `+` at the end of the list. The space opens in the folder you're in, or in your home folder if you aren't in one, with what you've chosen a new space opens: a terminal, by default, or Files, any of Omarchy's agents, or several, side by side on its first tab, in that order ([choose them](sidebar.md#settings), or in the [config](configuration.md#what-a-new-space-opens)). You go there once the first is up, so you never see the space empty. Scripts that open a space with `space new --silent` or `-- <cmd>` get a terminal, unless they pass `--open`.

**Opening nothing.** With nothing chosen, the space opens empty and you go there. It's like a browser's new tab: it stays while you're on it, and you can open things in it (`space terminal`, a new tab, a window sent there), in its folder. If you leave it while it's still empty, it ends and leaves the sidebar. Once it has a window, it's a space like any other.

**Naming.** A space is called after the folder its first terminal is in: the repository's folder when it's in one, `~` for your home folder. The name follows that terminal as it changes folder. One opened without a terminal is called after the folder it was opened in. Rename the space when you want a name that stays; an empty name has it called after its folder again.

**Moving.** Drag a space up or down the sidebar. `home` stays first.

**Closing.** A space lasts as long as its windows. Close the last one, however it goes (`super+w`, `exit`, the app quitting), and the space ends, taking you back to the space you were in before. To close all of its windows at once, right-click it. Each app is asked to close, as `super+w` does, so it can still stop to save.

## Tabs

**Opening.** Click `+` after the tabs. The tab opens with a terminal in the space's folder; in `home`, it opens empty.

**Naming.** A tab is named after its most recently focused window: a terminal by what it's running (`claude`, `nvim`), or by its folder when it's at its prompt, and any other app by its name. Double-click a tab, or right-click it and choose Rename, to name it yourself. It keeps that name whatever runs in it, until you clear it with an empty name or close the tab. `home`'s tabs show just their numbers, as Omarchy's workspaces do, until you name one.

**Moving.** Drag a tab along the bar to move it among its space's tabs. The space keeps the numbers its tabs have: what's on them moves along, and you stay on the tab you're on. (Dragging the tabs' `+` moves the whole widget on the bar.)

**Sending to another space.** Right-click a tab and pick the space. It comes after the tabs there, with its windows arranged as they are and the name you gave it, and you go with it.

**Closing.** Right-click a tab to close it. If it has windows it asks first, as they close with it. Closing the last window on a tab takes you to the next tab on that monitor, or the one before. With none there, the tab stays, empty, as long as the space has windows on another monitor.

**Numbers.** By default (`tabs.style` `browser`, "Like a browser" in [Settings](sidebar.md#settings)), a space's tabs are always numbered 1, 2, 3, … on each monitor, as a browser's are. When a tab ends, however it goes (closed, its last window closing or moving away, sent to another space), the tabs after it move up a number, with the names you gave them, and `super+1`–`0` follow. Going to a number past the last tab goes to a new, empty tab after it. `home` is different: its tabs are Omarchy's workspaces, which keep their numbers, so a closed tab with tabs after it leaves its number on the bar, faded, as Omarchy shows its empty workspaces. Click it to go there. Two or more empty numbers in a row show as one `…`, which goes to the first.

**Numbers like Omarchy's.** Set `tabs.style` to `omarchy` ("Like Omarchy") and every space's tabs, `home`'s too, are numbered as Omarchy's workspaces are, on each monitor:

- The bar always shows 1–5, an empty one faded, and any of 6–10 that has a tab, in number order. The tenth shows as `0`. There is no `…`. Click a faded number to go to that empty tab.
- Numbers are fixed: nothing is numbered again when a tab ends, so a closed tab leaves its number, faded if it's one of 1–5, and gone from the bar if it's past them.
- `super+N`, `space tab go N` and `space window tab N` go to exactly tab N, making it if it's empty, never "the next after the last".
- A new tab (`+`, `space tab new`, `spaces.new_tab()`) takes the lowest number with no tab, as Omarchy's next free workspace does. A tab sent to another space does the same there.
- `super+tab`, `space tab next` and `prev` go through the tabs there are, in number order, wrapping around, which is what Omarchy does with its workspaces in use. Empty numbers are skipped.
- A space still ends when its last window closes, going to a space still takes every monitor along, and each monitor still has its own ten tabs (the second monitor's 11–20 show as 1–10).

Changing the style counts at once, with no restart, in the bar, the keys and the commands. Sending a tab to another space, or dragging one to reorder, moves its windows rather than renaming it, so a number is never left blocked. One limit, right after switching from `browser`: a tab that a browser's numbering had moved up a number keeps its old workspace address, which can make one number skip to the next free one for `super+N`, and reordering the tabs of that space is refused until that tab ends: `space tab reorder` says why, while dragging on the bar just has the tab snap back. A restart clears it. Going back to `browser` numbers every space's tabs again, once, closing the gaps; going to `omarchy` changes nothing until you use it.

All of these have [keys](keys.md) you can bind, [commands](cli.md), or both.

## Multiple monitors

All monitors show the same space, and each has tabs of its own in it.

- Going to another space takes every monitor along, each back to the tab you last used on it there, or to an empty one if you haven't used the space on it.
- Moving between monitors only changes whose tabs your keys act on, never the space.
- `home`'s tabs are named on each monitor separately.

**Unplugging a monitor.** Its tabs join those of the one that's left until it's back. Each takes the lowest number free there, for the keys too: with tabs 1 to 3 already there, the other monitor's first is tab 4. Drag one among the other monitor's own tabs, or one of those past it, and it stays there for good.
