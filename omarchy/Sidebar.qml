pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import "Spaces.mjs" as Spaces

// The spaces sidebar: its rows, search and cursor, and when it shows. The base
// (Base.qml) draws it by the left edge, under the windows. Showing it claims
// its width, so Hyprland slides the windows aside and they uncover it; hiding
// it slides them back over it. With no windows to uncover it, it slides in and
// out by itself. Picking a space runs `space go`, which stays the only thing
// that knows how spaces map to workspaces.
//
// Showing it (`super+b`, or the space's name on the bar) is only visual: the
// keyboard stays with the windows, and the list keeps up with the spaces until
// you hide it. Searching it (`super+p`) takes the keyboard to filter the list;
// a pick, Escape, or focus moving to a window ends the search. A sidebar shown
// just for the search hides again with it. A search no space matches offers to
// make a space of that name.
//
// Right-clicking a space opens its menu (SpaceMenu.qml), floating where you
// clicked: rename it, close its windows, or remove it. Renaming takes the
// keyboard as the search does, to type the new name in place. Each runs `space`, which says why in a notification if it
// can't.
Item {
  id: root

  // Emitted when the search ends by itself (a pick, Escape, focus moving to a
  // window), so the service can tell the shell.
  signal searchEnded()

  // Whether the sidebar shows, and whether you are searching it, which holds
  // the keyboard. `searchOnly` marks a sidebar shown only for the search.
  property bool opened: false
  property bool searching: false
  property bool searchOnly: false
  // The space being renamed, and its new name so far.
  property string renaming: ""
  property string renameText: ""
  property double renamedAt: 0
  // Whatever holds the keyboard: the search or a rename.
  readonly property bool typing: searching || renaming !== ""
  // The space whose menu is open, its row, and where on the screen it opens.
  property string menuFor: ""
  readonly property var menuRow: menuFor ? rows.find(function(row) { return row.name === menuFor }) || null : null
  property real menuX: 0
  property real menuY: 0
  // `zoned` claims the width, which makes Hyprland move the windows; `shown`
  // draws the list, which stays drawn until the windows have covered it.
  property bool zoned: false
  property bool shown: false
  // How far the list sits left of its place, while it slides by itself.
  property real offset: -panelWidth
  // The screen it opened on.
  property var screen: null
  // Hyprland's window motion, read from `hyprctl animations -j`.
  property var motion: ({ duration: 379, bezier: [0.23, 1, 0.32, 1] })
  readonly property var curve: [motion.bezier[0], motion.bezier[1], motion.bezier[2], motion.bezier[3], 1, 1]
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

  readonly property int panelWidth: Style.space(260)

  function show() {
    if (root.opened) return
    root.screen = root.focusedScreen()
    root.opened = true
    root.zoned = true
    root.shown = true
    hideTimer.stop()
    slideOut.stop()
    if (root.windowsToMove()) {
      slideIn.stop()
      root.offset = 0
    } else {
      slideIn.restart()
    }
    root.freshen()
    root.refresh()
  }

  function hide() {
    if (!root.opened) return
    var wasSearching = root.searching
    root.stopSearching()
    root.endRename(false)
    root.closeMenu()
    root.opened = false
    root.zoned = false
    slideIn.stop()
    if (root.windowsToMove()) hideTimer.restart()
    else slideOut.restart()
    if (wasSearching) root.searchEnded()
  }

  function showOrHide() {
    if (root.opened) root.hide()
    else root.show()
  }

  // Takes the keyboard to search the list, showing the sidebar for it if it
  // is hidden.
  function search() {
    if (root.searching) return
    root.endRename(false)
    root.closeMenu()
    root.searchOnly = !root.opened
    root.show()
    root.filterText = ""
    root.error = ""
    root.searchedAt = Date.now()
    root.touched = false
    root.pending = false
    root.searching = true
    root.freshen()
    root.refresh()
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

  // Until `space ls` answers, show the last list, updated for the space you
  // are in now: Enter can come first, and must not pick that space.
  function freshen() {
    var workspace = Hyprland.focusedWorkspace
    if (!workspace) return
    root.rows = Spaces.freshen(root.rows, Spaces.locate(workspace.id, workspace.name).space, Date.now())
    root.applyFilter(true)
  }

  function refresh() {
    if (lsProc.running) {
      lsProc.again = true
      return
    }
    lsProc.running = true
  }

  function loaded(text, exitCode, stderr) {
    var selected = root.shownRows[root.selectedIndex]
    var keep = root.touched && selected ? selected.name : ""
    try {
      if (exitCode !== 0) throw new Error(stderr.trim() || "space ls exited with " + exitCode)
      root.rows = Spaces.parseRows(text)
      root.error = ""
    } catch (e) {
      root.rows = []
      root.error = String(e.message || e)
    }
    root.applyFilter(!keep)
    if (keep) {
      var index = root.shownRows.findIndex(function(row) { return row.name === keep })
      if (index >= 0) root.selectedIndex = index
    }
    if (root.pending) {
      root.pending = false
      root.activate(root.selectedIndex)
    }
  }

  function applyFilter(resetCursor) {
    root.shownRows = Spaces.withNewRow(Spaces.filterRows(root.rows, root.filterText), root.rows, root.filterText)
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
    if (root.searching && root.shownRows.length === 0 && lsProc.running) {
      root.pending = true
      return
    }
    if (index < 0 || index >= root.shownRows.length) return
    var row = root.shownRows[index]
    root.closeMenu()
    root.endSearch()
    // It says why in a notification if it can't.
    if (row.create) Quickshell.execDetached(["space", "new", row.name, "--in-roots"])
    else Quickshell.execDetached(["space", "go", row.name])
  }

  function subtitle(row) {
    if (row.name === root.renaming) return "Enter to rename, Esc to cancel"
    if (row.create) return root.searching ? "Enter to make it" : "Click to make it"
    if (row.dir) return Spaces.displayDir(row.dir, root.home)
    return row.name === "home" ? "Plain workspaces" : "Folder not found"
  }

  // Opens a space's menu at a point on the sidebar's screen. The menu takes
  // the keyboard, so a search ends, leaving the sidebar open.
  function openMenu(name, x, y) {
    root.endRename(false)
    if (root.searching) {
      root.searchOnly = false
      root.stopSearching()
      root.searchEnded()
    }
    root.menuX = x
    root.menuY = y
    root.menuFor = name
  }

  function closeMenu() {
    root.menuFor = ""
  }

  // An action from a space's menu (Spaces.rowActions).
  function act(id, row) {
    root.closeMenu()
    if (id === "rename") root.startRename(row.name)
    else if (id === "close") root.run(["space", "close", row.name])
    else if (id === "remove") root.run(["space", "rm", row.name])
  }

  // Takes the keyboard to type a new name over the old one. A sidebar shown
  // just for a search stays, as you are now using it.
  function startRename(name) {
    if (root.searching) {
      root.searchOnly = false
      root.stopSearching()
      root.searchEnded()
    }
    root.renaming = name
    root.renameText = name
    root.renamedAt = Date.now()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function endRename(commit) {
    if (!root.renaming) return
    var from = root.renaming
    var to = root.renameText.replace(/-+$/, "")
    root.renaming = ""
    root.renameText = ""
    if (commit && to && to !== from) root.run(["space", "rename", from, to])
  }

  // Runs a `space` command that changes the spaces, then lists them again:
  // a renamed or removed space changes no window, so nothing else would.
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

  Process {
    id: lsProc
    command: ["space", "ls", "--json"]
    property string out: ""
    property string err: ""
    // Asked again while running, for a change it may have missed.
    property bool again: false
    stdout: StdioCollector { onStreamFinished: lsProc.out = text }
    stderr: StdioCollector { onStreamFinished: lsProc.err = text }
    onExited: function(exitCode) {
      // The collectors can finish just after the exit is reported.
      Qt.callLater(function() {
        root.loaded(lsProc.out, exitCode, lsProc.err)
        if (lsProc.again) {
          lsProc.again = false
          root.refresh()
        }
      })
    }
  }

  Process {
    id: actionProc
    // Collected only so `space` has somewhere to print.
    stdout: StdioCollector {}
    stderr: StdioCollector {}
    onExited: Qt.callLater(root.refresh)
  }

  // Hide the list once the windows have slid back over it.
  Timer {
    id: hideTimer
    interval: root.motion.duration + 40
    onTriggered: if (!root.opened) root.shown = false
  }

  // On an empty workspace nothing uncovers the list, so it slides itself, on
  // the same curve the windows would move on.
  NumberAnimation {
    id: slideIn
    target: root
    property: "offset"
    from: -root.panelWidth
    to: 0
    duration: root.motion.duration
    easing.type: Easing.BezierSpline
    easing.bezierCurve: root.curve
  }

  NumberAnimation {
    id: slideOut
    target: root
    property: "offset"
    to: -root.panelWidth
    duration: root.motion.duration
    easing.type: Easing.BezierSpline
    easing.bezierCurve: root.curve
    onFinished: if (!root.opened) root.shown = false
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

  Component.onCompleted: motionProc.running = true

  // While it shows, the list follows what changes: the active space and the
  // window counts. A click on a window or a switch of tab during a search
  // moves the keyboard there, which ends the search.
  Connections {
    target: Hyprland
    function onRawEvent(event) {
      if (event.name === "configreloaded" && !motionProc.running) motionProc.running = true
      if (!root.opened) return
      switch (event.name) {
      case "workspacev2": case "openwindow": case "closewindow": case "movewindowv2":
        relist.restart()
      }
      if (event.name !== "activewindowv2" && event.name !== "workspacev2") return
      if (root.renaming && Date.now() - root.renamedAt >= 300) root.endRename(false)
      if (root.searching && Date.now() - root.searchedAt >= 300) root.endSearch()
    }
  }

  Timer {
    id: relist
    interval: 150
    onTriggered: if (root.opened) root.refresh()
  }

  // A one-pixel-wide panel on a top layer, drawing nothing and taking no
  // clicks. During a search it holds the keyboard, which the bottom
  // layer can't do the way the top layers can, and reserves the sidebar's
  // width, which makes Hyprland move the windows. The shell makes the bar
  // first, so this reserves room below a top bar and the bar never moves.
  PanelWindow {
    id: keys
    screen: root.screen || root.focusedScreen()
    anchors { left: true; top: true; bottom: true }
    implicitWidth: 1
    exclusionMode: ExclusionMode.Normal
    exclusiveZone: root.zoned ? root.panelWidth : 0
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
        if (root.renaming) {
          if (event.key === Qt.Key_Escape) root.endRename(false)
          else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) root.endRename(true)
          else if (Util.editsFilter(event, root.renameText)) root.renameText = Util.editedFilter(event, root.renameText)
          else if (printable) root.renameText = Spaces.renameInput(root.renameText, event.text)
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
