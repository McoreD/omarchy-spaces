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
// clicked: rename it or close it. Renaming takes the keyboard as the search
// does, to type the name in place; an empty name has the space called after
// its folder again. Each runs `space`, which says why in a notification if it
// can't.
Item {
  id: root

  // Emitted when the search ends by itself (a pick, Escape, focus moving to a
  // window), so the service can tell the shell.
  signal searchEnded()

  // The spaces as `space watch` keeps them (Watch.qml).
  property var watch: null

  // Whether the sidebar is expanded, and whether you are searching it, which
  // holds the keyboard. `searchOnly` marks one expanded only for the search.
  property bool opened: false
  property bool searching: false
  property bool searchOnly: false
  // The id of the space being renamed, and the name typed so far.
  property string renaming: ""
  property string nameText: ""
  property double renamedAt: 0
  readonly property bool naming: renaming !== ""
  // Whatever holds the keyboard: the search or a name being typed.
  readonly property bool typing: searching || naming
  // The id of the space whose menu is open, its row, and where on the screen
  // it opens.
  property string menuFor: ""
  readonly property var menuRow: menuFor ? rows.find(function(row) { return row.id === menuFor }) || null : null
  property real menuX: 0
  property real menuY: 0
  property var menuScreen: null
  // The screen it expands on, or last expanded on.
  property var screen: null
  // Hyprland's window motion, read from `hyprctl animations -j`.
  property var motion: ({ duration: 379, bezier: [0.23, 1, 0.32, 1] })
  property string filterText: ""
  property int selectedIndex: 0
  property var rows: []
  property var shownRows: []
  property string error: ""
  property double searchedAt: 0
  // Whether you have moved the cursor since the search began, which a fresh
  // list then keeps where it is rather than placing it again.
  property bool touched: false
  // Enter pressed before there was a list, to act on once it arrives.
  property bool pending: false
  readonly property string home: Quickshell.env("HOME") || ""
  // The colours spaces' badges take, from the theme's colors.toml.
  property var palette: Spaces.DEFAULT_PALETTE

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
  readonly property real axis: Style.space(8) + Style.bar.iconSlot / 2
  readonly property int railWidth: Math.round(axis * 2)
  readonly property int inset: Style.spacing.sm
  readonly property int cell: railWidth - inset * 2
  readonly property real badgeScale: 0.7
  readonly property int badgeSize: Math.round(cell * badgeScale)
  readonly property int badgeRadius: Math.round(badgeSize / 4)
  readonly property int controlSize: Math.round((badgeSize + cell) / 2)
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
    var monitor = root.screen ? Hyprland.monitorFor(root.screen) : null
    var rate = monitor && monitor.lastIpcObject ? Number(monitor.lastIpcObject.refreshRate) : 0
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
    root.take()
  }

  function hide() {
    if (!root.opened) return
    var wasSearching = root.searching
    root.stopSearching()
    root.endNaming(false)
    root.closeMenu()
    root.leading = root.windowsToMove()
    root.opened = false
    if (wasSearching) root.searchEnded()
  }

  function showOrHide(screen) {
    if (root.opened) root.hide()
    else root.show(screen)
  }

  // Takes the keyboard to search the list, showing the sidebar for it if it
  // is hidden.
  function search() {
    if (root.searching) return
    root.endNaming(false)
    root.closeMenu()
    root.searchOnly = !root.opened
    root.show()
    root.filterText = ""
    root.error = ""
    root.searchedAt = Date.now()
    root.touched = false
    root.pending = false
    root.searching = true
    root.take()
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

  // The list `space watch` last printed, put in place as the sidebar shows:
  // the cursor starts on the space you left last. Switching only just now,
  // the watch may not have printed it yet, so the active space comes from
  // Hyprland: Enter can come first, and must not pick the space you're in.
  function take() {
    root.rows = root.watch ? root.watch.rows : []
    root.error = root.watch && root.rows.length === 0 ? root.watch.error : ""
    var workspace = Hyprland.focusedWorkspace
    if (workspace) root.rows = Spaces.freshen(root.rows, Spaces.locate(workspace.id, workspace.name).space, Date.now())
    root.applyFilter(true)
  }

  // A new list while it shows: keep the cursor on the space it was on if you
  // moved it, and act on an Enter pressed before there was a list.
  function update() {
    var selected = root.shownRows[root.selectedIndex]
    var keep = root.touched && selected ? selected.id : ""
    root.rows = root.watch.rows
    root.error = root.rows.length === 0 ? root.watch.error : ""
    root.applyFilter(!keep)
    if (keep) {
      var index = root.shownRows.findIndex(function(row) { return row.id === keep })
      if (index >= 0) root.selectedIndex = index
    }
    if (root.pending) {
      root.pending = false
      root.activate(root.selectedIndex)
    }
  }

  // The rail shows the spaces too, so the list keeps up while collapsed.
  Connections {
    target: root.watch
    function onRowsChanged() {
      if (root.opened) root.update()
      else root.take()
    }
    function onErrorChanged() { if (root.rows.length === 0) root.error = root.watch.error }
  }

  function applyFilter(resetCursor) {
    root.shownRows = Spaces.filterRows(root.rows, root.filterText)
    if (resetCursor) {
      root.selectedIndex = root.filterText ? 0 : Spaces.initialIndex(root.shownRows)
    }
    if (root.selectedIndex >= root.shownRows.length) root.selectedIndex = Math.max(0, root.shownRows.length - 1)
  }

  function setFilter(next) {
    root.filterText = next
    root.applyFilter(true)
  }

  function move(delta) {
    var count = root.shownRows.length
    if (count === 0) return
    root.touched = true
    root.selectedIndex = (root.selectedIndex + delta + count) % count
  }

  function select(index) {
    root.touched = true
    root.selectedIndex = index
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

  // The `+`: a new space with a terminal in the folder you are in.
  function newSpace() {
    root.closeMenu()
    root.endNaming(false)
    root.run(["space", "new"])
  }

  // Opens a space's menu at a point on `screen`. The menu takes the keyboard,
  // so a search ends, leaving the sidebar open.
  function openMenu(id, x, y, screen) {
    root.endNaming(false)
    if (root.searching) {
      root.searchOnly = false
      root.stopSearching()
      root.searchEnded()
    }
    root.menuX = x
    root.menuY = y
    root.menuScreen = screen || root.screen || root.focusedScreen()
    root.menuFor = id
  }

  function closeMenu() {
    root.menuFor = ""
  }

  // An action from a space's menu (Spaces.rowActions).
  function act(id, row) {
    root.closeMenu()
    if (id === "rename") root.startRename(row)
    else if (id === "close") root.run(["space", "close", row.id])
  }

  // Takes the keyboard to type a name over the one it has. A sidebar shown
  // just for a search stays, as you are now using it.
  function startRename(row) {
    root.endNaming(false)
    if (root.searching) {
      root.searchOnly = false
      root.stopSearching()
      root.searchEnded()
    }
    root.renamedAt = Date.now()
    root.renaming = row.id
    root.nameText = row.label
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  // Enter commits the name typed, Escape (`commit` false) drops it. Leaving
  // an unnamed space's folder name as it was changes nothing; an empty name
  // has a named space called after its folder again.
  function endNaming(commit) {
    if (!root.naming) return
    var id = root.renaming
    var row = root.rows.find(function(r) { return r.id === id })
    var to = root.nameText.trim()
    root.renaming = ""
    root.nameText = ""
    if (!commit || !row) return
    if (row.named ? to === row.label : (to === row.label || to === "")) return
    root.run(["space", "rename", id, to])
  }

  // Runs a `space` command that changes the spaces.
  function run(args) {
    if (actionProc.running) return
    actionProc.command = args
    actionProc.running = true
  }

  // Whether the focused workspace has tiled windows, which Hyprland moves
  // when the sidebar claims or returns its width. Floating ones stay put.
  function windowsToMove() {
    var workspace = Hyprland.focusedWorkspace
    var toplevels = workspace && workspace.toplevels ? workspace.toplevels.values : []
    for (var i = 0; i < toplevels.length; i++) {
      var ipc = toplevels[i].lastIpcObject || {}
      if (!ipc.floating) return true
    }
    return false
  }

  function focusedScreen() {
    var monitor = Hyprland.focusedMonitor
    var name = monitor ? String(monitor.name || "") : ""
    for (var i = 0; i < Quickshell.screens.length; i++) {
      if (Quickshell.screens[i].name === name) return Quickshell.screens[i]
    }
    return Quickshell.screens.length > 0 ? Quickshell.screens[0] : null
  }

  // `space watch` sees what these change, and prints the list again.
  Process {
    id: actionProc
    // Collected only so `space` has somewhere to print.
    stdout: StdioCollector {}
    stderr: StdioCollector {}
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
    onLoaded: root.palette = Spaces.themePalette(text())
    onLoadFailed: root.palette = Spaces.DEFAULT_PALETTE
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
    root.take()
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
      if (!root.opened) return
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
      WlrLayershell.namespace: "renspace-sidebar-room"
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
    WlrLayershell.namespace: "renspace-sidebar-keys"
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
          if (event.key === Qt.Key_Escape) root.endNaming(false)
          else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) root.endNaming(true)
          else if (Util.editsFilter(event, root.nameText)) root.nameText = Util.editedFilter(event, root.nameText)
          else if (printable) root.nameText = Spaces.nameInput(root.nameText, event.text)
          else return
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
