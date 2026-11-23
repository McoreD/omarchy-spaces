pragma ComponentBehavior: Bound

import QtQuick
import Quickshell.Hyprland
import "Spaces.mjs" as Spaces

// The active space's tabs, which are its workspaces, read live from Hyprland.
Item {
  id: root

  // Bumped after Hyprland has answered a refresh, for anything bindings miss.
  property int revision: 0

  readonly property var model: {
    root.revision
    var active = Hyprland.focusedWorkspace
    var workspaces = Hyprland.workspaces.values.map(function(ws) { return { id: ws.id, name: ws.name } })
    var windows = []
    var toplevels = Hyprland.toplevels.values
    for (var i = 0; i < toplevels.length; i++) {
      var ipc = toplevels[i].lastIpcObject || {}
      var workspace = toplevels[i].workspace
      windows.push({
        workspaceId: workspace ? workspace.id : (ipc.workspace ? ipc.workspace.id : 0),
        windowClass: ipc.class || "",
        focus: typeof ipc.focusHistoryID === "number" ? ipc.focusHistoryID : 1e9,
      })
    }
    return Spaces.tabsFor(workspaces, windows, active ? { id: active.id, name: active.name } : null)
  }

  // Window classes and focus order come from Hyprland's IPC, which Quickshell
  // only asks for on request. Ask after the events that change them, once a
  // burst has settled.
  Connections {
    target: Hyprland
    function onRawEvent(event) {
      switch (event.name) {
      case "openwindow": case "closewindow": case "movewindowv2": case "activewindowv2":
      case "workspacev2": case "createworkspacev2": case "destroyworkspacev2":
      case "renameworkspace": case "focusedmonv2":
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

  Component.onCompleted: refresh.restart()
}
