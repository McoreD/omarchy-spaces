pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import "Spaces.mjs" as Spaces

// The spaces sidebar: its rows, search and cursor, and how wide it is. The
// base (Base.qml) draws it by the left edge, under the windows. Collapsed, it
// is a rail down every screen, as wide as the badges each row starts with, so
// the spaces always have their column and the windows and the bar's tabs sit
// beside it. Expanding it claims its whole width on one screen, so Hyprland
// slides the windows aside and they uncover the rest of each row; collapsing
// slides them back over it. Where nothing covers it, it is cut off where the
// windows would be. Picking a space runs `space go`, which stays the only
// thing that knows how spaces map to workspaces.
//
// Expanding it (`super+b`, or the toggle at its top) is only visual: the
// keyboard stays with the windows. Searching it (`super+p`) takes the keyboard
// to filter the list; a pick, Escape, or focus moving to a window ends the
// search. A sidebar expanded just for the search collapses again with it.
//
// Each space is called after the folder its first terminal is in, unless you
// name it, so the list follows your terminals as they change folder. The `+`
// at the end of the list opens a new space with a terminal in the folder you
// are in.
// Right-clicking a space opens its menu (SpaceMenu.qml), floating where you
// clicked: rename it, pick an icon for its badge, or close it. The icon
// picker (IconPicker.qml) opens where the menu was, and its badge shows the
// icon under the cursor until you pick one or stop. Renaming takes the keyboard as the search
// does, to type the name in place; an empty name has the space called after
// its folder again. `super+shift+r` renames the space you are in the same way,
// expanding the sidebar for it if it's collapsed, as the search does.
// `super+,`, double-clicking a tab on the bar, or its right-click menu, names
// the tab the same way, typed on the bar (TabsWidget.qml) with the keyboard held here; an empty
// name has it called after its window again. Each runs `space`, which says why in a notification if it
// can't.
Item {
  id: root

  // Emitted when the search ends by itself (a pick, Escape, focus moving to a
  // window), so the service can tell the shell.
  signal searchEnded()

  // What `space watch` says (Watch.qml), which what shows is bound to, and
  // the monitor you are on in it, with the space and tab it shows.
  property var watch: null
  readonly property var now: root.watch ? root.watch.now : Spaces.EMPTY
  readonly property var here: Spaces.focusedMonitor(root.now)

  // Whether the sidebar is expanded, and whether you are searching it, which
  // holds the keyboard. `searchOnly` marks one expanded only for the search.
  property bool opened: false
  property bool searching: false
  property bool searchOnly: false
  // What is being renamed: a space's id, as `space rename` takes it, or a
  // tab's, as `space tab rename` does. The name typed so
  // far, and whether all of it is selected, as it starts, for typing to
  // replace. The name it had, and whether you had given it that name.
  property string renaming: ""
  property string nameText: ""
  property bool nameSelected: false
  property string nameFrom: ""
  property bool nameWasSet: false
  property double renamedAt: 0
  // Whether it was expanded only for the rename, to collapse once it ends.
  property bool renameOnly: false
  readonly property bool naming: renaming !== ""
  // Whatever holds the keyboard: the search or a name being typed.
  readonly property bool typing: searching || naming
  // The id of the space whose menu is open, its row, and where on the screen
  // it opens; or, opened on a tab, its id and the tab, which
  // is only there while you are in its space.
  property string menuFor: ""
  readonly property var menuRow: menuFor ? rows.find(function(row) { return row.id === menuFor }) || null : null
  property string menuTabFor: ""
  readonly property var menuTabAt: root.menuTabFor ? Spaces.findTab(root.now, root.menuTabFor) : null
  readonly property var menuTab: root.menuTabAt ? root.menuTabAt.tab : null
  // Gone to another space, or the tab ended: the menu doesn't come back.
  onMenuTabChanged: if (!root.menuTab && root.menuTabFor) root.closeMenu()
  readonly property var menuActions: root.menuTabFor ? Spaces.tabActions(root.menuTab, root.menuTabAt ? root.menuTabAt.monitor : root.here, root.moveTargets.length > 0) : Spaces.rowActions(root.menuRow)
  // Whether the tab's menu has given way to asking whether to close it, and
  // what it asks.
  property bool confirmingClose: false
  readonly property string closeWarning: Spaces.closeTabWarning(root.menuTab)
  // Whether it has given way to picking a space to move the tab to
  // (MovePicker.qml), or a key opened that by itself, in the middle of the
  // screen; and the spaces to pick from.
  property bool movingTab: false
  property bool moveCentered: false
  readonly property var moveTargets: root.rows.filter(function(row) { return row.id !== root.here.space })
  property real menuX: 0
  property real menuY: 0
  property var menuScreen: null
  // The id of the space whose icon is being picked, where the picker opens
  // (the menu's place), and the icon its badge shows meanwhile: the one
  // under the picker's cursor, or "" for the one it has.
  property string iconFor: ""
  readonly property var iconRow: iconFor ? rows.find(function(row) { return row.id === iconFor }) || null : null
  property string iconPreview: ""
  // The screen it expands on, or last expanded on.
  property var screen: null
  // Hyprland's window motion, read from `hyprctl animations -j`.
  property var motion: ({ duration: 379, bezier: [0.23, 1, 0.32, 1] })
  property string filterText: ""
  // The order the spaces were dragged into, by their ids, until `space watch`
  // has them so.
  property var pendingOrder: null
  readonly property var rows: Spaces.ordered(root.now.spaces, root.pendingOrder)
  readonly property var shownRows: Spaces.filterRows(root.rows, root.filterText)
  // The space you moved the cursor to, by its id, which a fresh list then
  // keeps it on; "" while it's where it starts.
  property string picked: ""
  readonly property int selectedIndex: Spaces.cursorIndex(root.shownRows, root.filterText, root.picked)
  readonly property string error: root.watch && root.rows.length === 0 ? root.watch.error : ""
  property double searchedAt: 0
  // Enter pressed before there was a list, to act on once it arrives.
  property bool pending: false
  readonly property string home: Quickshell.env("HOME") || ""
  // The colours spaces' badges take, from the theme's colors.toml.
  property var palette: Spaces.DEFAULT_PALETTE
  // The colours of agents' statuses, from it too (StatusDot.qml).
  property var statusColors: null
  // Whether its header, the toggle and title, is on the bar, the tabs being
  // first there (TabsWidget.qml), so the list starts without one.
  property bool headerOnBar: false

  readonly property int panelWidth: Style.space(200)

  // The rail's geometry, from which the list's follows (SidebarList.qml).
  // Whatever `inset` and `badgeScale` are set to, these hold:
  //  1. The rail's icons share one centre line, `axis`, with the menu's logo
  //     on a top or bottom bar above or below them. Omarchy's bar sets the
  //     logo in from the screen's edge by its left section's margin (8, in
  //     its Bar.qml) and half the menu's slot, so the rail is twice that
  //     wide and its icons centre in it.
  //  2. Collapsed, every icon's box is a square `cell`: the toggle, each
  //     space's row and its highlight, the `+` and the search.
  //  3. One padding, `inset`, sets the cell in from the rail's edges, the
  //     list in from the sidebar's, and a row's highlight short of the cut.
  //  4. A space's row highlights its whole cell; a control (the toggle, the
  //     `+`, the search) highlights a smaller box, `controlSize`, between a
  //     badge and a cell, centred in its cell. The search's box stretches
  //     from there with the sidebar, as far short of the cut as it starts in.
  //  5. On the rail, the space you're in has its badge bordered in the
  //     theme's foreground.
  readonly property real axis: Style.space(8) + Style.bar.iconSlot / 2
  readonly property int railWidth: Math.round(axis * 2)
  readonly property int inset: Style.spacing.sm
  readonly property int cell: railWidth - inset * 2
  readonly property real badgeScale: 0.7
  readonly property int badgeSize: Math.round(cell * badgeScale)
  readonly property int badgeRadius: Math.round(badgeSize / 4)
  readonly property int controlSize: Math.round((badgeSize + cell) / 2)
  // From the icon column to a row's text: the spaces' names, and the title
  // in the header. None: the badge's own margin in its cell is the gap.
  readonly property int textGap: 0

  // The search's magnifier, drawn larger than the rail's other icons.
  readonly property int searchIconSize: Math.round(badgeSize * 0.72)
  // Hyprland's outer gap on the left, from `hyprctl getoption`.
  property int gapLeft: 0
  // How far from its screen's left edge the windows begin by the rail.
  readonly property int railEdge: railWidth + gapLeft
  // How much of it the windows leave uncovered on its screen, from the rail's
  // width to its whole width, on the curve Hyprland slides them on, starting
  // as they start. It is worked out from the clock each frame, rather than by
  // a NumberAnimation, so it can run `lead` ahead: Hyprland draws the windows
  // where they are in the frame it shows, while what the shell draws reaches
  // the screen a frame after, so the bar is told where they will be by then.
  // Measured on a recording, the bar lagged the windows by one frame of the
  // screen, either way.
  property real uncovered: railWidth
  // How much of it to draw: never less than the windows uncover, so no gap
  // opens between its edge and theirs. Ahead of them as they uncover more,
  // and behind them, under them, as they cover it again.
  property real revealed: railWidth
  readonly property real slideTo: opened ? panelWidth : railWidth
  property real slideFrom: railWidth
  property double slideStart: 0
  // With no windows to move, nothing to keep up with but itself, so the bar
  // and the sidebar keep to each other frame for frame.
  property bool leading: true
  readonly property int lead: {
    if (!root.leading) return 0
    var rate = root.screen ? Spaces.monitorNamed(root.now, root.screen.name).refresh : 0
    return rate > 0 ? Math.round(1000 / rate) : 8
  }
  // Turned around partway, Hyprland slides the windows from where they are,
  // taking the whole time again.
  onSlideToChanged: {
    root.slideFrom = root.uncovered
    root.slideStart = Date.now()
    slide.start()
    root.step()
  }

  // How much is uncovered `elapsed` milliseconds into the slide.
  function uncoveredAt(elapsed) {
    var duration = root.motion.duration
    var progress = duration > 0 ? elapsed / duration : 1
    return root.slideFrom + (root.slideTo - root.slideFrom) * Spaces.ease(root.motion.bezier, progress)
  }

  function step() {
    var elapsed = Date.now() - root.slideStart
    root.uncovered = root.uncoveredAt(elapsed + root.lead)
    root.revealed = root.slideTo > root.slideFrom ? root.uncovered : root.uncoveredAt(elapsed)
    if (elapsed >= root.motion.duration) slide.stop()
  }

  FrameAnimation {
    id: slide
    onTriggered: root.step()
  }
  // Where the windows' left edge is on its screen as they slide: the bar
  // shows the background up to it and keeps the space's tabs past it
  // (TabsWidget.qml), so the bar parts along the same line as the screen.
  // Every other screen parts at `railEdge`.
  readonly property real edge: uncovered + gapLeft

  // Expands it on `screen`, or the focused one.
  function show(screen) {
    if (root.opened) return
    root.screen = screen || root.focusedScreen()
    root.leading = root.windowsToMove()
    root.opened = true
    root.picked = ""
  }

  function hide() {
    if (!root.opened) return
    var wasSearching = root.searching
    root.stopSearching()
    root.endNaming(false)
    root.closeMenu()
    root.closeIconPicker()
    root.leading = root.windowsToMove()
    root.opened = false
    if (wasSearching) root.searchEnded()
  }

  function toggleSidebar(screen) {
    if (root.opened) root.hide()
    else root.show(screen)
  }

  // Takes the keyboard to search the list, showing the sidebar for it if it
  // is hidden.
  function search() {
    if (root.searching) return
    root.endNaming(false)
    root.closeMenu()
    root.closeIconPicker()
    root.searchOnly = !root.opened
    root.show()
    root.filterText = ""
    root.searchedAt = Date.now()
    root.picked = ""
    root.pending = false
    root.searching = true
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  // Hands the keyboard back to the windows, and hides a sidebar that was
  // shown just for the search.
  function endSearch() {
    if (!root.searching) return
    if (root.searchOnly) {
      root.hide()
      return
    }
    root.stopSearching()
    root.searchEnded()
  }

  function stopSearching() {
    root.searching = false
    root.searchOnly = false
    root.pending = false
    if (root.filterText) root.setFilter("")
  }

  onNowChanged: {
    if (root.pendingOrder && Spaces.orderSettled(root.now.spaces, root.pendingOrder)) root.settleOrder()
    // An Enter pressed before there was a list, once all that follows from it
    // is in place.
    if (root.pending) Qt.callLater(function() {
      if (!root.pending || root.shownRows.length === 0) return
      root.pending = false
      root.activate(root.selectedIndex)
    })
  }

  function setFilter(next) {
    root.filterText = next
    root.picked = ""
  }

  function move(delta) {
    var count = root.shownRows.length
    if (count === 0) return
    root.picked = root.shownRows[(root.selectedIndex + delta + count) % count].id
  }

  function select(index) {
    if (index >= 0 && index < root.shownRows.length) root.picked = root.shownRows[index].id
  }

  function activate(index) {
    if (root.searching && root.shownRows.length === 0 && !(root.watch && root.watch.ready)) {
      root.pending = true
      return
    }
    if (index < 0 || index >= root.shownRows.length) return
    var row = root.shownRows[index]
    root.closeMenu()
    root.endSearch()
    // It says why in a notification if it can't.
    Quickshell.execDetached(["space", "go", row.id])
  }

  // Moves the space at `from` in the list to `to`, where a drag dropped it.
  // The list shows the new order at once rather than waiting for `space
  // watch` to print it, so the row doesn't jump back first. Only the whole
  // list is dragged, never a search's, whose order is the search's own.
  function reorder(from, to) {
    if (from === to || root.filterText !== "") return
    var row = root.shownRows[from]
    if (!row || row.id === "home" || to < 1) return
    root.pendingOrder = Spaces.moveRow(root.rows, from, to).map(function(r) { return r.id })
    root.picked = row.id
    ordering.restart()
    // Positions count from the first space below home.
    root.run(["space", "reorder", row.id, String(to)])
  }

  function settleOrder() {
    ordering.stop()
    root.pendingOrder = null
  }

  // `space watch` might never have it, if something else changed meanwhile or
  // the space couldn't move.
  Timer {
    id: ordering
    interval: 1000
    onTriggered: root.settleOrder()
  }

  // The `+`: a new space with a terminal in the folder you are in.
  function newSpace() {
    root.closeMenu()
    root.endNaming(false)
    root.run(["space", "new"])
  }

  // Opens a space's menu at a point on `screen`. The menu takes the keyboard,
  // so a search ends, leaving the sidebar open.
  function openMenu(id, x, y, screen) {
    root.placeMenu(x, y, screen)
    root.menuFor = id
  }

  // Opens the menu of a tab of the space you are in, as it does a space's.
  function openTabMenu(id, x, y, screen) {
    root.placeMenu(x, y, screen)
    root.menuTabFor = id
  }

  function placeMenu(x, y, screen) {
    root.closeMenu()
    root.endNaming(false)
    root.closeIconPicker()
    if (root.searching) {
      root.searchOnly = false
      root.stopSearching()
      root.searchEnded()
    }
    root.menuX = x
    root.menuY = y
    root.menuScreen = screen || root.screen || root.focusedScreen()
  }

  function closeMenu() {
    root.menuFor = ""
    root.menuTabFor = ""
    root.confirmingClose = false
    root.movingTab = false
    root.moveCentered = false
  }

  // Acts on whatever the menu is open on. A tab with windows asks first.
  function pickAction(id) {
    if (id === "close-tab" && root.closeWarning !== "") root.confirmingClose = true
    else if (id === "close-tab") root.closeTab()
    else if (id === "move-tab") root.movingTab = true
    else root.act(id, root.menuTabFor ? root.menuTab : root.menuRow)
  }

  // Picks a space to send the tab you are on to, from the keyboard; again,
  // it stops.
  function sendTab() {
    if (root.movingTab) {
      root.closeMenu()
      return
    }
    var here = root.here
    if (!here.tab) return
    root.placeMenu(0, 0, root.focusedScreen())
    // Before the tab, so its menu doesn't flash up first.
    root.moveCentered = true
    root.movingTab = true
    root.menuTabFor = here.tab
  }

  // Sends the tab to a space, and you with it.
  function sendTabTo(id) {
    var target = root.menuTabFor
    root.closeMenu()
    if (target) root.run(["space", "tab", "send", target, id])
  }

  // Closes the tab whose menu is open, and its windows.
  function closeTab() {
    var target = root.menuTabFor
    root.closeMenu()
    if (target) root.run(["space", "tab", "close", target])
  }

  // An action from a space's menu (Spaces.rowActions), or a tab's
  // (Spaces.tabActions).
  function act(id, row) {
    root.closeMenu()
    if (id === "rename") root.startRename(row)
    else if (id === "rename-tab") root.renameTab(row.id)
    else if (id === "icon") root.openIconPicker(row)
    else if (id === "clear-icon") root.run(["space", "icon", row.id, ""])
    else if (id === "close") root.run(["space", "close", row.id])
  }

  // Opens the icon picker for a space, where its menu was.
  function openIconPicker(row) {
    root.endNaming(false)
    root.iconPreview = ""
    root.iconFor = row.id
  }

  function closeIconPicker() {
    root.iconFor = ""
    root.iconPreview = ""
  }

  // Gives the space the icon picked in the picker.
  function pickIcon(icon) {
    var id = root.iconFor
    root.closeIconPicker()
    if (id && icon) root.run(["space", "icon", id, icon])
  }

  // Takes the keyboard to type a name over the one a space has. A sidebar
  // shown just for a search stays, as you are now using it.
  function startRename(row) {
    root.startNaming(row.id, row.label, row.named)
  }

  function startNaming(target, label, named) {
    root.endNaming(false)
    root.closeIconPicker()
    if (root.searching) {
      root.searchOnly = false
      root.stopSearching()
      root.searchEnded()
    }
    root.renamedAt = Date.now()
    root.renameOnly = false
    root.renaming = target
    root.nameText = label
    root.nameFrom = label
    root.nameWasSet = named
    root.nameSelected = true
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  // Names a tab of the space you are in, by its id, or ("") the one you are on: the
  // bar shows the name as you type it. A sidebar shown just for a search goes.
  function renameTab(id) {
    var target = id || root.here.tab
    var found = Spaces.findTab(root.now, target)
    if (!found) return
    var tab = found.tab
    if (root.renaming === target) return
    if (root.searching && root.searchOnly) root.hide()
    root.closeMenu()
    root.startNaming(target, tab.label, tab.named)
  }

  // Renames the space you are in, from the keyboard. `home` has no name to
  // change. A sidebar collapsed, or expanded just for a search, is expanded
  // just for the rename.
  function renameSpace() {
    var id = root.here.space
    if (!id || id === "home" || root.renaming === id) return
    var only = !root.opened || (root.searching && root.searchOnly)
    if (!root.opened) root.show()
    var row = root.rows.find(function(r) { return r.id === id })
    if (!row) {
      if (only) root.hide()
      return
    }
    root.closeMenu()
    root.startRename(row)
    root.renameOnly = only
  }

  // Enter commits the name typed, Escape (`commit` false) drops it. Leaving
  // an unnamed space's folder name as it was changes nothing; an empty name
  // has a named space called after its folder again. A sidebar expanded just
  // for the rename collapses again.
  function endNaming(commit) {
    if (!root.naming) return
    var target = root.renaming
    var to = root.nameText.trim()
    var unchanged = root.nameWasSet ? to === root.nameFrom : (to === root.nameFrom || to === "")
    root.renaming = ""
    root.nameText = ""
    root.nameSelected = false
    if (commit && !unchanged) root.run(Spaces.renameCommand(target, to))
    if (root.renameOnly) {
      root.renameOnly = false
      root.hide()
    }
  }

  // Runs a `space` command that changes the spaces, beside any still running:
  // `space new` takes until its terminal is up. `space watch` sees what it
  // changes, and prints the list again.
  function run(args) {
    Quickshell.execDetached(args)
  }

  // Whether there are tiled windows on its screen, which Hyprland moves when
  // the sidebar claims or returns its width. Floating ones stay put.
  function windowsToMove() {
    return !!root.screen && Spaces.monitorNamed(root.now, root.screen.name).tiled
  }

  function focusedScreen() {
    var monitor = Hyprland.focusedMonitor
    var name = monitor ? String(monitor.name || "") : ""
    for (var i = 0; i < Quickshell.screens.length; i++) {
      if (Quickshell.screens[i].name === name) return Quickshell.screens[i]
    }
    return Quickshell.screens.length > 0 ? Quickshell.screens[0] : null
  }

  Process {
    id: motionProc
    command: ["hyprctl", "animations", "-j"]
    stdout: StdioCollector {
      onStreamFinished: {
        var motion = Spaces.windowMotion(text)
        if (motion) root.motion = motion
      }
    }
  }

  // A new theme changes current/; the watch on a file inside it fires for the
  // whole directory, as it does for the bar.
  FileView {
    path: (Quickshell.env("XDG_STATE_HOME") || (root.home + "/.local/state")) + "/omarchy/current/theme/colors.toml"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      root.palette = Spaces.themePalette(text())
      root.statusColors = Spaces.statusColors(text())
    }
    onLoadFailed: {
      root.palette = Spaces.DEFAULT_PALETTE
      root.statusColors = null
    }
  }

  Process {
    id: gapsProc
    command: ["hyprctl", "getoption", "general:gaps_out", "-j"]
    stdout: StdioCollector {
      onStreamFinished: {
        var gap = Spaces.gapLeft(text)
        if (gap !== null) root.gapLeft = gap
      }
    }
  }

  Component.onCompleted: {
    motionProc.running = true
    gapsProc.running = true
  }

  // A click on a window or a switch of tab during a search or a rename moves
  // the keyboard there, which ends it.
  Connections {
    target: Hyprland
    function onRawEvent(event) {
      if (event.name === "configreloaded") {
        if (!motionProc.running) motionProc.running = true
        if (!gapsProc.running) gapsProc.running = true
      }
      if (!root.opened && !root.naming) return
      if (event.name !== "activewindowv2" && event.name !== "workspacev2") return
      if (root.naming && Date.now() - root.renamedAt >= 300) root.endNaming(false)
      if (root.searching && Date.now() - root.searchedAt >= 300) root.endSearch()
    }
  }

  // On each screen, a one-pixel-wide panel on a top layer, drawing nothing
  // and taking no clicks, that reserves the sidebar's width, the rail's or the
  // whole of it, which makes Hyprland move the windows. The shell makes the
  // bar first, so this reserves room below a top bar and the bar never moves.
  Variants {
    model: Quickshell.screens

    PanelWindow {
      required property var modelData
      readonly property bool expanded: root.opened && !!root.screen && root.screen.name === modelData.name
      screen: modelData
      anchors { left: true; top: true; bottom: true }
      implicitWidth: 1
      exclusionMode: ExclusionMode.Normal
      exclusiveZone: expanded ? root.panelWidth : root.railWidth
      mask: Region {}
      color: "transparent"
      WlrLayershell.namespace: "omarchy-spaces-sidebar-room"
      WlrLayershell.layer: WlrLayer.Top
      WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    }
  }

  // Like it, on the screen it expands on, holding the keyboard during a
  // search or a rename, which the bottom layer can't do the way the top
  // layers can. It reserves nothing.
  PanelWindow {
    id: keys
    screen: root.screen || root.focusedScreen()
    anchors { left: true; top: true; bottom: true }
    implicitWidth: 1
    exclusionMode: ExclusionMode.Ignore
    mask: Region {}
    color: "transparent"
    WlrLayershell.namespace: "omarchy-spaces-sidebar-keys"
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.keyboardFocus: root.typing ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    Item {
      id: keyCatcher
      anchors.fill: parent
      focus: true

      Keys.priority: Keys.BeforeItem
      Keys.onPressed: function(event) {
        var ctrl = event.modifiers & Qt.ControlModifier
        var printable = event.text && event.text.length === 1 && !ctrl
            && event.text.charCodeAt(0) >= 32 && event.text.charCodeAt(0) !== 127
        if (root.naming) {
          // With the whole name selected, typing replaces it and deleting
          // clears it; Right or End puts the caret after it, to add to it.
          // Ctrl+A selects it again.
          var selected = root.nameSelected
          var deletes = event.key === Qt.Key_Backspace || event.key === Qt.Key_Delete
              || (ctrl && event.key === Qt.Key_U)
          if (event.key === Qt.Key_Escape) root.endNaming(false)
          else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) root.endNaming(true)
          else if (ctrl && event.key === Qt.Key_A) root.nameSelected = root.nameText !== ""
          else if (selected && deletes) root.nameText = ""
          else if (selected && (event.key === Qt.Key_Right || event.key === Qt.Key_End)) {}
          else if (selected && printable) root.nameText = Spaces.nameInput("", event.text)
          else if (Util.editsFilter(event, root.nameText)) root.nameText = Util.editedFilter(event, root.nameText)
          else if (printable) root.nameText = Spaces.nameInput(root.nameText, event.text)
          else return
          if (!(ctrl && event.key === Qt.Key_A)) root.nameSelected = false
          event.accepted = true
          return
        }
        if (event.key === Qt.Key_Escape) {
          if (root.filterText) root.setFilter("")
          else root.endSearch()
        } else if (Util.editsFilter(event, root.filterText)) {
          root.setFilter(Util.editedFilter(event, root.filterText))
        } else if (event.key === Qt.Key_Up || (ctrl && event.key === Qt.Key_P) || event.key === Qt.Key_Backtab) {
          root.move(-1)
        } else if (event.key === Qt.Key_Down || (ctrl && event.key === Qt.Key_N) || event.key === Qt.Key_Tab) {
          root.move(1)
        } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
          root.activate(root.selectedIndex)
        } else if (printable) {
          root.setFilter(root.filterText + event.text)
        } else {
          return
        }
        event.accepted = true
      }
    }
  }
}
