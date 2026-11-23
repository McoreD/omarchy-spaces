pragma ComponentBehavior: Bound

import QtQuick
import Quickshell.Hyprland
import Quickshell.Io
import "Spaces.mjs" as Spaces

// The active space's tabs, which are its workspaces, read live from Hyprland,
// and what the space is called. Its workspaces are named after its id (`s1`);
// what to call it, the name you gave it or its terminal's folder, comes from
// `space ls`, asked after workspace changes and every couple of seconds, as a
// terminal changing folder tells Hyprland nothing.
Item {
  id: root

  // Space ids to what they're called.
  property var labels: ({})
  readonly property string label: root.labels[root.model.space] || root.model.space

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
    onTriggered: {
      root.revision++
      root.relabel()
    }
  }

  function relabel() {
    if (!lsProc.running) lsProc.running = true
  }

  Process {
    id: lsProc
    command: ["space", "ls", "--json"]
    stdout: StdioCollector {
      onStreamFinished: {
        var labels = Spaces.labels(text)
        if (Object.keys(labels).length > 0) root.labels = labels
      }
    }
  }

  Timer {
    interval: 2000
    repeat: true
    running: true
    onTriggered: root.relabel()
  }

  Component.onCompleted: refresh.restart()
}
