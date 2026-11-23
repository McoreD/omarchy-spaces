pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import "Spaces.mjs" as Spaces

// The spaces sidebar: its rows, filter and cursor, and when it shows. The base
// (Base.qml) draws it by the left edge, under the windows. Opening it claims
// its width, so Hyprland slides the windows aside and they uncover it; closing
// slides them back over it. With no windows to uncover it, it slides in and
// out by itself. Picking a space runs `space go`, which stays the only thing
// that knows how spaces map to workspaces.
Item {
  id: root

  // Emitted when the sidebar closes itself (a pick, Escape, focus moving to a
  // window), so the service can tell the shell.
  signal closed()

  // Whether the sidebar is open, as the shell's toggle sees it.
  property bool opened: false
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
  property double openedAt: 0
  readonly property string home: Quickshell.env("HOME") || ""

  readonly property int panelWidth: Style.space(260)

  function open() {
    if (root.opened) return
    root.filterText = ""
    root.error = ""
    root.openedAt = Date.now()
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
    root.refresh()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function close() {
    if (!root.opened) return
    root.opened = false
    root.zoned = false
    slideIn.stop()
    if (root.windowsToMove()) hideTimer.restart()
    else slideOut.restart()
    root.closed()
  }

  function refresh() {
    if (lsProc.running) return
    lsProc.running = true
  }

  function loaded(text, exitCode, stderr) {
    try {
      if (exitCode !== 0) throw new Error(stderr.trim() || "space ls exited with " + exitCode)
      root.rows = Spaces.parseRows(text)
      root.error = ""
    } catch (e) {
      root.rows = []
      root.error = String(e.message || e)
    }
    root.applyFilter(true)
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
    root.selectedIndex = (root.selectedIndex + delta + count) % count
  }

  function activate(index) {
    if (index < 0 || index >= root.shownRows.length) return
    var name = root.shownRows[index].name
    root.close()
    Quickshell.execDetached(["space", "go", name])
  }

  function subtitle(row) {
    if (row.dir) return Spaces.displayDir(row.dir, root.home)
    return row.name === "home" ? "Plain workspaces" : "Folder not found"
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
    stdout: StdioCollector { onStreamFinished: lsProc.out = text }
    stderr: StdioCollector { onStreamFinished: lsProc.err = text }
    onExited: function(exitCode) {
      // The collectors can finish just after the exit is reported.
      Qt.callLater(function() { root.loaded(lsProc.out, exitCode, lsProc.err) })
    }
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

  // A click on a window moves focus there; take that as done with the list.
  Connections {
    target: Hyprland
    function onRawEvent(event) {
      if (event.name === "configreloaded" && !motionProc.running) motionProc.running = true
      if (!root.opened || Date.now() - root.openedAt < 300) return
      if (event.name === "activewindowv2" || event.name === "workspacev2") root.close()
    }
  }

  // A one-pixel-wide panel on a top layer, drawing nothing and taking no
  // clicks. While the sidebar is open it holds the keyboard, which the bottom
  // layer can't do the way the top layers can, and reserves the sidebar's
  // width, which makes Hyprland move the windows. Created after the base, so
  // it reserves room below the strip and the strip never moves.
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
    WlrLayershell.keyboardFocus: root.opened ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    Item {
      id: keyCatcher
      anchors.fill: parent
      focus: true

      Keys.priority: Keys.BeforeItem
      Keys.onPressed: function(event) {
        var ctrl = event.modifiers & Qt.ControlModifier
        if (event.key === Qt.Key_Escape) {
          if (root.filterText) root.setFilter("")
          else root.close()
        } else if (Util.editsFilter(event, root.filterText)) {
          root.setFilter(Util.editedFilter(event, root.filterText))
        } else if (event.key === Qt.Key_Up || (ctrl && event.key === Qt.Key_P) || event.key === Qt.Key_Backtab) {
          root.move(-1)
        } else if (event.key === Qt.Key_Down || (ctrl && event.key === Qt.Key_N) || event.key === Qt.Key_Tab) {
          root.move(1)
        } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
          root.activate(root.selectedIndex)
        } else if (event.text && event.text.length === 1 && !ctrl
            && event.text.charCodeAt(0) >= 32 && event.text.charCodeAt(0) !== 127) {
          root.setFilter(root.filterText + event.text)
        } else {
          return
        }
        event.accepted = true
      }
    }
  }
}
