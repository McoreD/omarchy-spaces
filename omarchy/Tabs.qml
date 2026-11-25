pragma ComponentBehavior: Bound

import QtQuick
import Quickshell.Hyprland
import "Spaces.mjs" as Spaces

// The active space's tabs on a monitor, which are its workspaces there, read
// live from Hyprland, with what terminals run and their agents' statuses from `space watch`.
Item {
  id: root

  // Bumped after Hyprland has answered a refresh, for anything bindings miss.
  property int revision: 0
  // The spaces from `space watch`, which name terminals' tabs by what they
  // run, or their folder, and say what the agents in them are doing.
  property var rows: []

  // The monitor, by name: the one you are on unless given.
  property string monitorName: ""

  // The workspace it shows, `{ id, name, monitor }`. From what Hyprland says
  // of its monitors: Quickshell takes any change of workspace to be on the
  // monitor you are on, but going to another space changes every one's.
  readonly property var shown: {
    var monitors = Hyprland.monitors.values
    var monitor = Hyprland.focusedMonitor
    if (root.monitorName) {
      for (var i = 0; i < monitors.length; i++) {
        if (monitors[i].name === root.monitorName) monitor = monitors[i]
      }
    }
    var ipc = monitor ? monitor.lastIpcObject : null
    var workspace = ipc ? ipc.activeWorkspace : null
    return workspace ? { id: workspace.id, name: workspace.name, monitor: monitor.name } : null
  }

  readonly property var model: {
    root.revision
    var active = root.shown
    var workspaces = Hyprland.workspaces.values.map(function(ws) {
      return { id: ws.id, name: ws.name, monitor: ws.monitor ? ws.monitor.name : "" }
    })
    var windows = []
    var toplevels = Hyprland.toplevels.values
    for (var i = 0; i < toplevels.length; i++) {
      var ipc = toplevels[i].lastIpcObject || {}
      var workspace = toplevels[i].workspace
      windows.push({
        workspaceId: workspace ? workspace.id : (ipc.workspace ? ipc.workspace.id : 0),
        address: ipc.address || "",
        windowClass: ipc.class || "",
        focus: typeof ipc.focusHistoryID === "number" ? ipc.focusHistoryID : 1e9,
      })
    }
    var terminals = Spaces.terminalsByAddress(root.rows)
    var here = active ? Spaces.locate(active.id, active.name).space : ""
    var row = (root.rows || []).find(function(r) { return r.id === here })
    return Spaces.tabsFor(workspaces, windows, active, terminals, row ? row.tabs : null)
  }

  // Window classes and focus order come from Hyprland's IPC, which Quickshell
  // only asks for on request. Ask after the events that change them, once a
  // burst has settled.
  Connections {
    target: Hyprland
    function onRawEvent(event) {
      switch (event.name) {
      case "workspacev2": case "focusedmonv2": case "moveworkspacev2":
      case "monitoraddedv2": case "monitorremovedv2": case "renameworkspace":
        Hyprland.refreshMonitors()
      }
      switch (event.name) {
      case "openwindow": case "closewindow": case "movewindowv2": case "activewindowv2":
      case "workspacev2": case "createworkspacev2": case "destroyworkspacev2":
      case "renameworkspace": case "focusedmonv2": case "moveworkspacev2":
      case "monitoraddedv2": case "monitorremovedv2":
        refresh.restart()
      }
    }
  }

  Timer {
    id: refresh
    interval: 30
    onTriggered: {
      Hyprland.refreshWorkspaces()
      Hyprland.refreshToplevels()
      settle.restart()
    }
  }

  Timer {
    id: settle
    interval: 120
    onTriggered: root.revision++
  }

  Component.onCompleted: {
    Hyprland.refreshMonitors()
    refresh.restart()
  }
}
